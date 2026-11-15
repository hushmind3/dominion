//! Regression coverage for the public native dispatcher: journal first, no
//! concurrent effects, no stale writes after failure, and host-owned read lanes.
use dominion_agent_sdk::{
    dispatch_batch, Action, BoxFuture, DispatchResult, Dispatcher, Journal,
    Limits, RunRequest, Snapshot, Strategy,
};
use serde_json::json;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Mutex,
};

#[derive(Default)]
struct JournalLog {
    checkpoints: Mutex<Vec<Snapshot>>,
    reject_intents: bool,
}
impl Journal for JournalLog {
    fn save(&self, run: &Snapshot) -> Result<(), String> {
        if self.reject_intents && !run.in_flight.is_empty() {
            return Err("durable intent storage unavailable".into());
        }
        self.checkpoints.lock().unwrap().push(run.clone());
        Ok(())
    }
}
struct Host {
    calls: Mutex<Vec<String>>,
    active: AtomicUsize,
    peak: AtomicUsize,
    fail_id: Option<&'static str>,
    report_failure_id: Option<&'static str>,
    parallel_reads: bool,
}
impl Host {
    fn new() -> Self {
        Self { calls: Mutex::new(Vec::new()), active: AtomicUsize::new(0),
            peak: AtomicUsize::new(0), fail_id: None,
            report_failure_id: None, parallel_reads: true }
    }
    fn calls(&self) -> Vec<String> { self.calls.lock().unwrap().clone() }
}
impl Dispatcher for Host {
    type Output = String;
    fn execute<'a>(&'a self, action: &'a Action)
        -> BoxFuture<'a, Result<DispatchResult<Self::Output>, String>> {
        Box::pin(async move {
            self.calls.lock().unwrap().push(action.id.clone());
            let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.peak.fetch_max(active, Ordering::SeqCst);
            // Both independent reads must be polled before either settles.
            tokio::task::yield_now().await;
            self.active.fetch_sub(1, Ordering::SeqCst);
            if self.fail_id == Some(action.id.as_str()) {
                return Err("simulated host failure".into());
            }
            Ok(DispatchResult {
                output: action.id.clone(),
                observation: json!({"result":action.id}),
                success: self.report_failure_id != Some(action.id.as_str()),
            })
        })
    }
    fn is_read_only(&self, action: &Action) -> bool {
        self.parallel_reads && action.tool == "read"
    }
}
fn run() -> Snapshot {
    Snapshot::from_request(&RunRequest {
        id: "dispatch-regression".into(),
        instruction: "Perform ordered tool batch".into(),
        owner_instructions: String::new(),
        strategy: Strategy::Balanced,
        limits: Limits::default(),
        allowed_tools: vec![],
        checks: vec![],
        require_evidence: true,
    })
}
fn action(id: &str, tool: &str) -> Action {
    Action { id:id.into(), tool:tool.into(), arguments:json!({}), depends_on:vec![] }
}
#[tokio::test]
async fn independent_reads_parallelize_but_writes_are_hard_ordering_barriers() {
    let host = Host::new();
    let journal = JournalLog::default();
    let mut state = run();
    let actions = [
        action("r1","read"), action("r2","read"),
        action("effect","write"), action("after","read")
    ];
    let result = dispatch_batch(&mut state, &actions, &host, &journal).await.unwrap();
    assert!(result.into_iter().all(|item|item.is_ok()));
    assert_eq!(host.calls(), vec!["r1","r2","effect","after"]);
    assert_eq!(host.peak.load(Ordering::SeqCst),2);
    assert_eq!(state.receipts.len(),4);
    assert!(state.in_flight.is_empty());
    let intents = journal.checkpoints.lock().unwrap().iter()
        .filter(|snapshot|!snapshot.in_flight.is_empty())
        .map(|snapshot|snapshot.in_flight.iter().map(|action|action.id.clone())
            .collect::<Vec<_>>()).collect::<Vec<_>>();
    assert_eq!(intents,vec![vec!["r1","r2"],vec!["effect"],vec!["after"]]);
}
#[tokio::test]
async fn failed_effect_blocks_subsequent_mutation_and_preserves_failure_receipt() {
    let mut host = Host::new();
    host.fail_id = Some("first");
    let journal = JournalLog::default();
    let mut state = run();
    let actions = [action("first","write"),action("must_not_run","write")];
    let result = dispatch_batch(&mut state,&actions,&host,&journal).await.unwrap();
    assert_eq!(host.calls(),vec!["first"]);
    assert_eq!(result.len(),2);
    assert!(result.iter().all(Result::is_err));
    assert_eq!(state.receipts.len(),1);
    assert_eq!(state.receipts[0].id,"first");
    assert!(!state.receipts[0].success);
    assert!(state.receipts[0].result["error"].as_str().unwrap().contains("simulated"));
}
#[tokio::test]
async fn reported_failure_cannot_be_mistaken_for_a_successful_result() {
    let mut host = Host::new();
    host.report_failure_id = Some("false_result");
    let journal = JournalLog::default();
    let mut state = run();
    let actions = [action("false_result","write"),action("later","write")];
    let result = dispatch_batch(&mut state,&actions,&host,&journal).await.unwrap();
    assert_eq!(host.calls(),vec!["false_result"]);
    assert!(result.iter().all(Result::is_err));
    assert!(!state.receipts[0].success);
    assert_eq!(state.receipts[0].result["result"],"false_result");
}
#[tokio::test]
async fn failed_write_ahead_persistence_executes_nothing() {
    let host = Host::new();
    let journal = JournalLog{reject_intents:true,..JournalLog::default()};
    let mut state = run();
    assert!(dispatch_batch(&mut state,&[action("write","write")],&host,&journal)
        .await.is_err());
    assert!(host.calls().is_empty());
    assert!(state.receipts.is_empty());
}

struct RedirectingJournal {
    snapshots: Mutex<Vec<Snapshot>>,
    redirected: std::sync::atomic::AtomicBool,
    redirect_after_intent: bool,
}
impl RedirectingJournal {
    fn new(redirect_after_intent: bool) -> Self {
        Self {snapshots:Mutex::new(vec![]),
            redirected:std::sync::atomic::AtomicBool::new(false),
            redirect_after_intent}
    }
}
impl Journal for RedirectingJournal {
    fn save(&self, run: &Snapshot) -> Result<(), String> {
        self.snapshots.lock().unwrap().push(run.clone());
        if (self.redirect_after_intent && !run.in_flight.is_empty())
            || (!self.redirect_after_intent && !run.receipts.is_empty()) {
            self.redirected.store(true, Ordering::SeqCst);
        }
        Ok(())
    }
    fn directive(&self, _id: &str) -> Result<Option<String>, String> {
        Ok(self.redirected.load(Ordering::SeqCst)
            .then(|| "Owner changed the task".into()))
    }
}
#[tokio::test]
async fn owner_redirection_after_a_read_prevents_a_stale_write() {
    let host=Host::new();
    let journal=RedirectingJournal::new(false);
    let mut state=run();
    let outcome=dispatch_batch(&mut state,
        &[action("inspect","read"),action("stale_mutation","write")],&host,&journal).await;
    assert!(outcome.is_err());
    assert_eq!(host.calls(),vec!["inspect"]);
    assert_eq!(state.receipts.len(),1);
    assert_eq!(state.receipts[0].id,"inspect");
    assert!(state.in_flight.is_empty());
}
#[tokio::test]
async fn owner_redirection_after_intent_fence_dispatches_no_tool() {
    let host=Host::new();
    let journal=RedirectingJournal::new(true);
    let mut state=run();
    let result=dispatch_batch(&mut state,&[action("stale_mutation","write")],&host,&journal).await;
    assert!(result.is_err());
    assert!(host.calls().is_empty());
    assert!(state.receipts.is_empty());
    assert!(state.in_flight.is_empty());
    let saved=journal.snapshots.lock().unwrap();
    assert_eq!(saved.len(),2);
    assert_eq!(saved[0].in_flight.len(),1);
    assert!(saved[1].in_flight.is_empty());
}
