use dominion_agent_sdk::*;
use serde_json::{json, Value};
use std::{collections::VecDeque, path::PathBuf, sync::{Mutex, atomic::{AtomicUsize, Ordering}}, time::Duration};

struct Store(Mutex<Snapshot>);
impl Journal for Store {
    fn save(&self, snapshot: &Snapshot) -> Result<(), String> {
        *self.0.lock().unwrap() = snapshot.clone();
        Ok(())
    }
}
impl CheckpointStore for Store {
    fn load(&self) -> Result<Snapshot, String> {
        Ok(self.0.lock().unwrap().clone())
    }
}
struct Script(VecDeque<TurnDecision>);
impl ModelPort for Script {
    fn decide<'a>(&'a mut self, snapshot: &'a Snapshot) -> BoxFuture<'a, Result<ModelTurn, String>> {
        Box::pin(async move {
            assert!(snapshot.model_in_flight);
            let decision = self.0.pop_front().ok_or("unexpected model call")?;
            Ok(ModelTurn { decision, charged_tokens: 10 })
        })
    }
}
struct FileTool {
    path: PathBuf,
    effects: AtomicUsize,
    unknown: bool,
}
impl Drop for FileTool {
    fn drop(&mut self) { let _ = std::fs::remove_file(&self.path); }
}
impl Dispatcher for FileTool {
    type Output = ();
    fn definitions(&self) -> Vec<Value> {
        vec![json!({"type":"function","function":{"name":"workspace_write","description":"write test artifact",
            "parameters":{"type":"object","properties":{},"additionalProperties":false}}})]
    }
    fn execute<'a>(&'a self, _: &'a Action) -> BoxFuture<'a, Result<DispatchResult<()>, String>> {
        Box::pin(async move {
            self.effects.fetch_add(1, Ordering::SeqCst);
            std::fs::write(&self.path, "verified artifact").map_err(|e| e.to_string())?;
            if self.unknown { return Err("connection lost after effect".into()); }
            Ok(DispatchResult { output: (), observation: json!({"written":true}), success: true })
        })
    }
}
struct FileVerifier(PathBuf);
impl Verifier for FileVerifier {
    fn verify<'a>(&'a mut self, _: &'a Snapshot, _: &'a str) -> BoxFuture<'a, Result<Verification, String>> {
        Box::pin(async move {
            Ok(if std::fs::read_to_string(&self.0).ok().as_deref() == Some("verified artifact") {
                Verification::Accept
            } else {
                Verification::Retry("artifact missing".into())
            })
        })
    }
}
fn fixture(name: &str) -> (Store, FileTool, FileVerifier) {
    let snapshot = Snapshot::new(name, "create verified artifact", 4).unwrap();
    let path = std::env::temp_dir().join(format!("dominion-embedded-{}-{}", std::process::id(), snapshot.native.id));
    let tool = FileTool { path: path.clone(), effects: AtomicUsize::new(0), unknown: false };
    (Store(Mutex::new(snapshot)), tool, FileVerifier(path))
}
fn write() -> TurnDecision {
    TurnDecision::Tools(vec![Action { id:"write-1".into(), tool:"workspace_write".into(),
        arguments:json!({}), depends_on:vec![] }])
}
fn limits() -> EmbeddedLimits {
    EmbeddedLimits { max_rounds:4, max_tools:2, max_elapsed:Duration::from_secs(10) }
}

#[tokio::test]
async fn independent_host_repairs_failed_verification_and_writes_real_artifact() {
    let (store, tool, mut verifier) = fixture("repair");
    let mut model = Script([TurnDecision::Answer("premature".into()), write(),
        TurnDecision::Answer("done".into())].into());
    let saved = run_embedded(&store, &mut model, &tool, &mut verifier, limits()).await.unwrap();
    assert_eq!(saved.native.status, RunStatus::Completed);
    assert_eq!(saved.native.answer.as_deref(), Some("done"));
    assert_eq!(saved.model_calls, 3);
    assert_eq!(saved.charged_tokens, 30);
    assert_eq!(saved.receipts.len(), 1);
    assert!(saved.receipts[0].success);
    assert!(saved.native.events.iter().any(|e| e.kind == "embedded_retry"));
    assert_eq!(tool.effects.load(Ordering::SeqCst), 1);
    assert_eq!(std::fs::read_to_string(&tool.path).unwrap(), "verified artifact");
}

#[tokio::test]
async fn unknown_effect_is_not_replayed_by_convenience_sdk() {
    let (store, mut tool, mut verifier) = fixture("uncertain-effect");
    tool.unknown = true;
    let mut model = Script([write()].into());
    assert!(run_embedded(&store, &mut model, &tool, &mut verifier, limits()).await.is_err());
    assert!(store.load().unwrap().native.in_flight.is_some());
    let mut retry = Script([write()].into());
    assert!(run_embedded(&store, &mut retry, &tool, &mut verifier, limits()).await.is_err());
    assert_eq!(retry.0.len(), 1);
    assert_eq!(tool.effects.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn model_only_frame_cannot_use_dispatcher_fallback_catalog() {
    let (store, tool, mut verifier) = fixture("model-only");
    store.0.lock().unwrap().native.model_only = true;
    let mut model = Script([write()].into());
    assert!(run_embedded(&store, &mut model, &tool, &mut verifier, limits()).await.is_err());
    assert_eq!(tool.effects.load(Ordering::SeqCst), 0);
    assert!(store.load().unwrap().receipts.is_empty());
}

#[tokio::test]
async fn previous_rounds_are_not_refunded_on_resume() {
    let (store, tool, mut verifier) = fixture("budget");
    {
        let mut saved = store.0.lock().unwrap();
        saved.model_calls = 4;
        saved.step = 4;
        saved.native.steps = 4;
    }
    let mut model = Script([write()].into());
    assert!(run_embedded(&store, &mut model, &tool, &mut verifier, limits()).await.is_err());
    assert_eq!(model.0.len(), 1);
    assert_eq!(tool.effects.load(Ordering::SeqCst), 0);
}
