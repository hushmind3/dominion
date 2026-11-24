use dominion_agent_sdk::*;
use serde_json::Value;
use std::{sync::Mutex, time::Duration};

struct Store(Mutex<Snapshot>);
impl Journal for Store {
    fn save(&self, snapshot: &Snapshot) -> Result<(), String> {
        *self.0.lock().unwrap() = snapshot.clone();
        Ok(())
    }
}
struct Embedded<'a> {
    store: &'a Store,
    calls: usize,
}
impl Host for Embedded<'_> {
    type Reply = String;
    type ToolResult = ();
    fn reload(&self) -> Result<Snapshot, String> {
        Ok(self.store.0.lock().unwrap().clone())
    }
    fn initialize<'a>(&'a mut self, _: &'a mut Context<'_>) -> BoxFuture<'a, Result<Flow, String>> {
        Box::pin(async { Ok(Flow::Continue) })
    }
    fn wants_plan(&self) -> bool { false }
    fn plan<'a>(&'a mut self, _: &'a mut Context<'_>) -> BoxFuture<'a, Result<Option<String>, String>> {
        Box::pin(async { Err("unexpected plan".into()) })
    }
    fn prepare_model<'a>(&'a mut self, _: &'a mut Context<'_>) -> BoxFuture<'a, Result<Vec<Value>, String>> {
        Box::pin(async { Ok(vec![]) })
    }
    fn model<'a>(&'a mut self, ctx: &'a mut Context<'_>) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(async move {
            assert!(ctx.snapshot.model_in_flight);
            assert!(self.reload()?.model_in_flight);
            self.calls += 1;
            Ok("answer".into())
        })
    }
    fn observe_model<'a>(&'a mut self, _: &'a mut Context<'_>, reply: String) -> BoxFuture<'a, Result<ModelDecision, String>> {
        Box::pin(async move {
            assert_eq!(reply, "answer");
            Ok(ModelDecision::Verify)
        })
    }
    fn verify<'a>(&'a mut self, _: &'a mut Context<'_>) -> BoxFuture<'a, Result<Flow, String>> {
        Box::pin(async { Ok(Flow::Complete) })
    }
    fn begin_tools<'a>(&'a mut self, _: &'a mut Context<'_>) -> BoxFuture<'a, Result<usize, String>> {
        Box::pin(async { Err("unexpected tools".into()) })
    }
    fn prepare_tool<'a>(&'a mut self, _: &'a mut Context<'_>, _: usize) -> BoxFuture<'a, Result<bool, String>> {
        Box::pin(async { Err("unexpected tool".into()) })
    }
    fn dispatch_tool<'a>(&'a mut self, _: &'a mut Context<'_>, _: usize) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async { Err("unexpected dispatch".into()) })
    }
    fn observe_tool<'a>(&'a mut self, _: &'a mut Context<'_>, _: usize, _: ()) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async { Err("unexpected observation".into()) })
    }
    fn verify_tools<'a>(&'a mut self, _: &'a mut Context<'_>) -> BoxFuture<'a, Result<Flow, String>> {
        Box::pin(async { Err("unexpected tool verification".into()) })
    }
    fn exhausted(&mut self) -> Result<(), String> { Err("budget exhausted".into()) }
    fn completion(&self) -> Result<(String, usize, u64), String> {
        Ok(("answer".into(), self.calls, 12))
    }
}

#[tokio::test]
async fn external_host_runs_and_persists_through_the_public_sdk() {
    let snapshot = Snapshot::new("embedding-test", "answer", 1).unwrap();
    let store = Store(Mutex::new(snapshot.clone()));
    let mut host = Embedded { store: &store, calls: 0 };
    run(&mut host, Context { snapshot, budget: BudgetLedger::new(1, 1, Duration::from_secs(10)),
        round: 0, journal: &store }).await.unwrap();
    let saved = store.0.lock().unwrap();
    assert_eq!(saved.native.status, RunStatus::Completed);
    assert_eq!(saved.native.answer.as_deref(), Some("answer"));
    assert_eq!(saved.model_calls, 1);
    assert_eq!(saved.charged_tokens, 12);
    assert!(!saved.model_in_flight);
    assert_eq!(host.calls, 1);
}

#[tokio::test]
async fn external_host_cannot_replay_uncertain_paid_intent() {
    let mut snapshot = Snapshot::new("uncertain", "answer", 1).unwrap();
    snapshot.model_in_flight = true;
    let store = Store(Mutex::new(snapshot.clone()));
    let mut host = Embedded { store: &store, calls: 0 };
    assert!(run(&mut host, Context { snapshot, budget: BudgetLedger::new(1, 1, Duration::from_secs(10)),
        round: 0, journal: &store }).await.is_err());
    assert_eq!(host.calls, 0);
    assert!(store.0.lock().unwrap().model_in_flight);
}
