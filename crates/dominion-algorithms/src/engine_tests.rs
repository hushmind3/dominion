use super::*;
use dominion_core::hosted::{self, Action, DispatchResult, Dispatcher};
use dominion_core::runtime_port::AgentLoop;
use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

struct Memory {
    saved: Mutex<Snapshot>,
    cancelled: AtomicBool,
    fail_model_intent: bool,
    fail_receipt: bool,
}
impl Journal for Memory {
    fn save(&self, snapshot: &Snapshot) -> Result<(), String> {
        if (self.fail_model_intent && snapshot.model_in_flight)
            || (self.fail_receipt && !snapshot.receipts.is_empty())
        {
            return Err("storage unavailable".into());
        }
        *self.saved.lock().unwrap() = snapshot.clone();
        Ok(())
    }
    fn cancelled(&self, _: &str) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}
struct Write(Arc<AtomicUsize>);
impl Dispatcher for Write {
    type Output = ();
    fn execute<'a>(&'a self, _: &'a Action) -> BoxFuture<'a, Result<DispatchResult<()>, String>> {
        Box::pin(async move {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(DispatchResult {
                output: (),
                observation: json!({"written":true}),
                success: true,
            })
        })
    }
}
struct Fixture {
    store: Arc<Memory>,
    replies: VecDeque<ModelDecision>,
    verdicts: VecDeque<Flow>,
    trace: Vec<&'static str>,
    models: usize,
    effects: Arc<AtomicUsize>,
    plan: bool,
    shortcut: bool,
    cancel_after_model: bool,
    fail_during_prepare: bool,
    confirmations: VecDeque<Flow>,
    reconcile_on_receipt: bool,
    reconcile_calls: usize,
}
impl Fixture {
    fn new() -> Self {
        Self {
            store: Arc::new(Memory {
                saved: Mutex::new(Snapshot::new("fixture", "test execution", 8).unwrap()),
                cancelled: AtomicBool::new(false),
                fail_model_intent: false,
                fail_receipt: false,
            }),
            replies: VecDeque::new(),
            verdicts: VecDeque::new(),
            trace: vec![],
            models: 0,
            effects: Arc::new(AtomicUsize::new(0)),
            plan: false,
            shortcut: false,
            cancel_after_model: false,
            fail_during_prepare: false,
            confirmations: VecDeque::new(),
            reconcile_on_receipt: false,
            reconcile_calls: 0,
        }
    }
    async fn drive(&mut self, rounds: usize) -> Result<(), String> {
        let store = self.store.clone();
        let snapshot = store.saved.lock().unwrap().clone();
        AgentLoop::run(
            &Engine,
            self,
            Context {
                snapshot,
                journal: store.as_ref(),
                round: 0,
                budget: BudgetLedger::new(rounds, 8, Duration::from_secs(10)),
            },
        )
        .await
    }
}
impl Host for Fixture {
    type Reply = ModelDecision;
    type ToolResult = ();
    fn reload(&self) -> Result<Snapshot, String> {
        Ok(self.store.saved.lock().unwrap().clone())
    }
    fn initialize<'a>(&'a mut self, _: &'a mut Context<'_>) -> BoxFuture<'a, Result<Flow, String>> {
        Box::pin(async move {
            self.trace.push("initialize");
            Ok(if self.shortcut {
                Flow::Complete
            } else {
                Flow::Continue
            })
        })
    }
    fn wants_plan(&self) -> bool {
        self.plan
    }
    fn plan<'a>(
        &'a mut self,
        _: &'a mut Context<'_>,
    ) -> BoxFuture<'a, Result<Option<String>, String>> {
        Box::pin(async move {
            self.trace.push("plan");
            Ok(Some("inspect, write, verify".into()))
        })
    }
    fn prepare_model<'a>(
        &'a mut self,
        _: &'a mut Context<'_>,
    ) -> BoxFuture<'a, Result<Vec<Value>, String>> {
        Box::pin(async move {
            if self.fail_during_prepare {
                self.store.saved.lock().unwrap().native.status = hosted::Status::Failed;
            }
            Ok(vec![])
        })
    }
    fn model<'a>(
        &'a mut self,
        _: &'a mut Context<'_>,
    ) -> BoxFuture<'a, Result<Self::Reply, String>> {
        Box::pin(async move {
            self.trace.push("model");
            self.models += 1;
            Ok(self.replies.pop_front().unwrap_or(ModelDecision::Verify))
        })
    }
    fn observe_model<'a>(
        &'a mut self,
        _: &'a mut Context<'_>,
        reply: Self::Reply,
    ) -> BoxFuture<'a, Result<ModelDecision, String>> {
        Box::pin(async move {
            if self.cancel_after_model {
                self.store.cancelled.store(true, Ordering::SeqCst);
            }
            Ok(reply)
        })
    }
    fn verify<'a>(&'a mut self, _: &'a mut Context<'_>) -> BoxFuture<'a, Result<Flow, String>> {
        Box::pin(async move {
            self.trace.push("verify");
            Ok(self.verdicts.pop_front().unwrap_or(Flow::Continue))
        })
    }
    fn begin_tools<'a>(
        &'a mut self,
        _: &'a mut Context<'_>,
    ) -> BoxFuture<'a, Result<usize, String>> {
        Box::pin(async { Ok(1) })
    }
    fn prepare_tool<'a>(
        &'a mut self,
        _: &'a mut Context<'_>,
        _: usize,
    ) -> BoxFuture<'a, Result<bool, String>> {
        Box::pin(async { Ok(true) })
    }
    fn dispatch_tool<'a>(
        &'a mut self,
        ctx: &'a mut Context<'_>,
        _: usize,
    ) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            self.trace.push("tool");
            hosted::dispatch_batch(
                &mut ctx.snapshot,
                &[Action {
                    id: format!("{}:write", ctx.round),
                    tool: "workspace_write".into(),
                    arguments: json!({"path":"memo.txt","content":""}),
                    depends_on: vec![],
                }],
                &Write(self.effects.clone()),
                ctx.journal,
            )
            .await?
            .pop()
            .unwrap()
        })
    }
    fn observe_tool<'a>(
        &'a mut self,
        _: &'a mut Context<'_>,
        _: usize,
        _: (),
    ) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async { Ok(()) })
    }
    fn verify_tools<'a>(
        &'a mut self,
        _: &'a mut Context<'_>,
    ) -> BoxFuture<'a, Result<Flow, String>> {
        Box::pin(async move {
            self.trace.push("verify_tools");
            Ok(Flow::Continue)
        })
    }
    fn reconcile_verified<'a>(
        &'a mut self,
        ctx: &'a mut Context<'_>,
    ) -> BoxFuture<'a, Result<Flow, String>> {
        Box::pin(async move {
            self.reconcile_calls += 1;
            let accepted = self.reconcile_on_receipt
                && ctx.snapshot.receipts.iter().any(|receipt| receipt.success);
            if accepted {
                self.trace.push("reconcile");
            }
            Ok(if accepted { Flow::Complete } else { Flow::Continue })
        })
    }
    fn exhausted(&mut self) -> Result<(), String> {
        Err("round budget exhausted".into())
    }
    fn confirm_completion(&mut self) -> Result<Flow, String> {
        Ok(self.confirmations.pop_front().unwrap_or(Flow::Complete))
    }
    fn completion(&self) -> Result<(String, usize, u64), String> {
        Ok((
            "verified result".into(),
            self.models + usize::from(self.plan && !self.shortcut),
            20,
        ))
    }
}

#[tokio::test]
async fn driver_retries_failed_verification_and_completes_only_after_evidence() {
    let mut host = Fixture::new();
    host.plan = true;
    host.replies = [
        ModelDecision::Verify,
        ModelDecision::Tools,
        ModelDecision::Verify,
    ]
    .into();
    host.verdicts = [Flow::Continue, Flow::Complete].into();
    host.drive(4).await.unwrap();
    assert_eq!(
        host.trace,
        [
            "initialize",
            "plan",
            "model",
            "verify",
            "model",
            "tool",
            "verify_tools",
            "model",
            "verify"
        ]
    );
    let saved = host.store.saved.lock().unwrap();
    assert_eq!(saved.native.status, hosted::Status::Completed);
    assert_eq!(saved.native.plan.as_deref(), Some("inspect, write, verify"));
    assert_eq!(saved.model_calls, 4);
    assert_eq!(saved.native.steps, 3);
    assert_eq!(saved.receipts[0].step, 2);
    assert_eq!(saved.receipts.len(), 1);
    assert!(saved.receipts[0].success);
    assert!(!saved.model_in_flight);
}

#[tokio::test]
async fn kernel_settles_verified_receipt_without_reserving_final_model_round() {
    let mut host = Fixture::new();
    host.reconcile_on_receipt = true;
    host.replies.push_back(ModelDecision::Tools);
    host.drive(1).await.expect("verified tool result should finish in one round");
    assert_eq!(host.models, 1);
    assert_eq!(host.effects.load(Ordering::SeqCst), 1);
    assert!(host.trace.contains(&"reconcile"));
    let saved = host.store.saved.lock().unwrap();
    assert_eq!(saved.native.status, hosted::Status::Completed);
    assert_eq!(saved.native.steps, 1);
    assert_eq!(saved.receipts.len(), 1);
}

#[tokio::test]
async fn kernel_never_reconciles_an_uncertain_tool_intent() {
    let mut host = Fixture::new();
    host.reconcile_on_receipt = true;
    {
        let mut stored = host.store.saved.lock().unwrap();
        stored.native.in_flight = Some(dominion_core::domain::ToolCall {
            id: "uncertain".into(),
            name: "workspace_write".into(),
            arguments: json!({"path":"x"}),
        });
        stored.receipts.push(dominion_core::hosted::Receipt {
            id: "earlier".into(),
            tool: "workspace_write".into(),
            arguments: json!({"path":"x"}),
            success: true,
            result: json!({"ok":true}),
            step: 1,
        });
    }
    assert!(host.drive(1).await.is_err());
    assert_eq!(host.reconcile_calls, 0);
    assert_eq!(host.models, 0);
    assert_ne!(host.store.saved.lock().unwrap().native.status, hosted::Status::Completed);
}

#[tokio::test]
async fn unsuccessful_verification_cannot_escape_shared_round_limit() {
    let mut host = Fixture::new();
    assert!(host
        .drive(2)
        .await
        .unwrap_err()
        .contains("budget exhausted"));
    assert_eq!(host.models, 2);
    assert_ne!(
        host.store.saved.lock().unwrap().native.status,
        hosted::Status::Completed
    );
}

#[tokio::test]
async fn cancellation_between_model_and_tool_prevents_mutation() {
    let mut host = Fixture::new();
    host.replies.push_back(ModelDecision::Tools);
    host.cancel_after_model = true;
    assert!(host.drive(4).await.is_err());
    assert_eq!(host.effects.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn failed_model_intent_storage_prevents_paid_call() {
    let mut host = Fixture::new();
    Arc::get_mut(&mut host.store).unwrap().fail_model_intent = true;
    assert!(host
        .drive(4)
        .await
        .unwrap_err()
        .contains("storage unavailable"));
    assert_eq!(host.models, 0);
}

#[tokio::test]
async fn failed_receipt_storage_stops_next_model_and_leaves_durable_intent() {
    let mut host = Fixture::new();
    Arc::get_mut(&mut host.store).unwrap().fail_receipt = true;
    host.replies.push_back(ModelDecision::Tools);
    assert!(host
        .drive(4)
        .await
        .unwrap_err()
        .contains("storage unavailable"));
    assert_eq!(host.models, 1);
    assert_eq!(host.effects.load(Ordering::SeqCst), 1);
    assert!(host.store.saved.lock().unwrap().native.in_flight.is_some());
}

#[tokio::test]
async fn verified_shortcut_completes_without_model_or_round() {
    let mut host = Fixture::new();
    host.shortcut = true;
    host.plan = true;
    host.drive(0).await.unwrap();
    assert_eq!(host.models, 0);
    assert_eq!(host.store.saved.lock().unwrap().model_calls, 0);
}

#[tokio::test]
async fn dropping_auxiliary_call_preserves_uncertain_paid_intent() {
    let host = Fixture::new();
    let mut snapshot = host.reload().unwrap();
    let pending = model_call(
        &mut snapshot,
        host.store.as_ref(),
        "review",
        std::future::pending::<()>(),
    );
    assert!(tokio::time::timeout(Duration::from_millis(5), pending)
        .await
        .is_err());
    assert!(host.store.saved.lock().unwrap().model_in_flight);
    let mut budget = BudgetLedger::new(4, 4, Duration::from_secs(10));
    assert!(snapshot
        .next_round(&mut budget, host.store.as_ref())
        .is_err());
}

#[tokio::test]
async fn cancelled_execution_never_enters_product_initialization() {
    let mut host = Fixture::new();
    host.store.cancelled.store(true, Ordering::SeqCst);
    assert!(host.drive(4).await.is_err());
    assert!(host.trace.is_empty());
}

#[tokio::test]
async fn uncertain_paid_call_cannot_restart_product_initialization() {
    let mut host = Fixture::new();
    host.store.saved.lock().unwrap().model_in_flight = true;
    assert!(host.drive(4).await.is_err());
    assert!(host.trace.is_empty());
    assert!(host.store.saved.lock().unwrap().model_in_flight);
}

#[tokio::test]
async fn superseded_model_and_retracted_completion_stay_in_the_bounded_driver() {
    let mut host = Fixture::new();
    host.replies = [
        ModelDecision::Continue,
        ModelDecision::Verify,
        ModelDecision::Verify,
    ]
    .into();
    host.verdicts = [Flow::Complete, Flow::Complete].into();
    host.confirmations = [Flow::Continue, Flow::Complete].into();
    host.drive(3).await.unwrap();
    assert_eq!(host.models, 3);
    assert_eq!(host.effects.load(Ordering::SeqCst), 0);
    assert_eq!(
        host.trace
            .iter()
            .filter(|phase| **phase == "verify")
            .count(),
        2
    );
    assert_eq!(
        host.store.saved.lock().unwrap().native.status,
        hosted::Status::Completed
    );
}


#[tokio::test]
async fn failed_during_preparation_never_buys_a_model_call() {
    let mut host = Fixture::new();
    host.fail_during_prepare = true;
    assert!(host.drive(4).await.is_err());
    assert_eq!(host.models, 0);
    let saved = host.store.saved.lock().unwrap();
    assert_eq!(saved.native.status, hosted::Status::Failed);
    assert_eq!(saved.model_calls, 0);
    assert!(!saved.model_in_flight);
}
