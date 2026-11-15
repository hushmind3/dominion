//! Dominion-owned Rust SDK execution kernel.
//! Provider, tool, verifier and policy are replaceable; no external agent
//! harness decides the control flow. The snapshot is the public resume format.
use crate::context::ContextBudget;
use crate::policy::{Decision, Policy};
use crate::{Limits, Message, ModelReply, Provider, Tool, ToolCall};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{atomic::{AtomicBool, Ordering}, Arc};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", content = "detail", rename_all = "snake_case")]
pub enum RunStatus {
    Ready,
    Running,
    AwaitingApproval { call_id: String, tool: String },
    InDoubt { call_id: String, tool: String },
    Completed,
    Cancelled,
    Denied { tool: String },
    Exhausted { reason: String },
    Failed { error: String },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SdkEvent {
    pub kind: String,
    pub detail: Value,
}

/// Stable result ledger prevents a provider replay from rerunning a tool.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CompletedCall {
    pub tool: String,
    pub arguments: Value,
    pub result: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub instruction: String,
    pub messages: Vec<Message>,
    pub pending_calls: Vec<ToolCall>,
    #[serde(default)]
    pub completed_calls: BTreeMap<String, CompletedCall>,
    /// Recorded BEFORE an effectful tool call. A crash never silently replays it.
    pub in_flight: Option<ToolCall>,
    pub events: Vec<SdkEvent>,
    pub rounds: usize,
    pub tool_calls: usize,
    pub status: RunStatus,
}

impl Session {
    pub fn new(id: impl Into<String>, system: impl Into<String>, instruction: impl Into<String>) -> Self {
        let instruction = instruction.into();
        Self {
            id: id.into(),
            instruction: instruction.clone(),
            messages: vec![Message::system(system), Message::user(instruction)],
            pending_calls: Vec::new(),
            completed_calls: BTreeMap::new(),
            in_flight: None,
            events: Vec::new(),
            rounds: 0,
            tool_calls: 0,
            status: RunStatus::Ready,
        }
    }

    pub fn final_text(&self) -> Option<&str> {
        if self.status != RunStatus::Completed { return None; }
        self.messages.iter().rev()
            .find(|m| m.role == crate::Role::Assistant)
            .map(|m| m.content.as_str())
    }

    fn event(&mut self, kind: &str, detail: Value) {
        self.events.push(SdkEvent { kind: kind.into(), detail });
    }

    /// Only an operator can resolve a potentially executed tool after a crash.
    /// This never reruns a side effect; the recorded observation becomes the
    /// corresponding tool result before the next model turn.
    pub fn reconcile_observation(&mut self, observation: Value) -> Result<(), String> {
        if !matches!(self.status, RunStatus::InDoubt { .. }) {
            return Err("session is not awaiting tool reconciliation".into());
        }
        let call = self.in_flight.take().ok_or("no uncertain tool call")?;
        let result = json!({"manuallyReconciled":true,"observation":observation}).to_string();
        self.completed_calls.insert(call.id.clone(), CompletedCall {
            tool:call.name.clone(), arguments:call.arguments.clone(), result:result.clone()
        });
        self.messages.push(Message::tool_result(call.id.clone(), result));
        self.event("tool_reconciled", json!({"id":call.id,"tool":call.name}));
        self.status = RunStatus::Ready;
        Ok(())
    }
}

pub enum Verification {
    Passed,
    Retry(String),
    Rejected(String),
}

/// A real, independently implemented verifier may execute tests, inspect
/// artifacts, or compare expected output before the engine accepts completion.
pub trait Verifier: Send {
    fn verify(&mut self, snapshot: &Session) -> Verification;
}

/// Verify a coding task by running the operator's real test command.
pub struct CommandVerifier {
    pub shell: crate::tools::Shell,
    pub command: String,
}
impl Verifier for CommandVerifier {
    fn verify(&mut self, _: &Session) -> Verification {
        use crate::Tool;
        match self.shell.invoke(&json!({"command":self.command})) {
            Ok(result) if result["exit"] == 0 => Verification::Passed,
            Ok(result) => Verification::Retry(result.to_string()),
            Err(error) => Verification::Retry(error),
        }
    }
}

/// Durable file checkpoint, using a same-directory temporary file and rename.
#[derive(Clone, Debug)]
pub struct FileJournal {
    path: PathBuf,
}

impl FileJournal {
    pub fn new(path: impl AsRef<Path>) -> Self { Self { path: path.as_ref().to_owned() } }

    pub fn load(&self) -> Result<Option<Session>, String> {
        if !self.path.exists() { return Ok(None); }
        let bytes = fs::read(&self.path).map_err(|e| format!("checkpoint read: {e}"))?;
        serde_json::from_slice(&bytes).map(Some).map_err(|e| format!("checkpoint decode: {e}"))
    }

    pub fn save(&self, snapshot: &Session) -> Result<(), String> {
        let parent = self.path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
        fs::create_dir_all(parent).map_err(|e| format!("checkpoint directory: {e}"))?;
        let basename = self.path.file_name().ok_or("checkpoint path must name a file")?;
        let name = format!(".{}.{}.tmp", basename.to_string_lossy(), std::process::id());
        let temporary = parent.join(name);
        let bytes = serde_json::to_vec_pretty(snapshot).map_err(|e| e.to_string())?;
        let result = (|| -> Result<(), String> {
            let mut file = fs::OpenOptions::new().create(true).write(true).truncate(true)
                .open(&temporary).map_err(|e| format!("checkpoint create: {e}"))?;
            use std::io::Write;
            file.write_all(&bytes).map_err(|e| format!("checkpoint write: {e}"))?;
            file.sync_all().map_err(|e| format!("checkpoint sync: {e}"))?;
            fs::rename(&temporary, &self.path).map_err(|e| format!("checkpoint commit: {e}"))
        })();
        if result.is_err() { let _ = fs::remove_file(temporary); }
        result
    }
}

pub struct AgentSdk {
    provider: Box<dyn Provider + Send>,
    tools: BTreeMap<String, Box<dyn Tool + Send>>,
    policy: Box<dyn Policy + Send>,
    limits: Limits,
    journal: Option<FileJournal>,
    verifier: Option<Box<dyn Verifier>>,
    cancellation: Arc<AtomicBool>,
    approved_once: Option<String>,
}

impl AgentSdk {
    pub fn new(
        provider: Box<dyn Provider + Send>,
        policy: Box<dyn Policy + Send>,
        limits: Limits,
    ) -> Self {
        Self {
            provider, tools: BTreeMap::new(), policy, limits,
            journal: None, verifier: None,
            cancellation: Arc::new(AtomicBool::new(false)),
            approved_once: None,
        }
    }

    pub fn register(&mut self, tool: Box<dyn Tool + Send>) {
        self.tools.insert(tool.spec().name, tool);
    }

    pub fn tool_specs(&self) -> Vec<crate::ToolSpec> {
        self.tools.values().map(|tool| tool.spec()).collect()
    }

    pub fn set_journal(&mut self, journal: FileJournal) { self.journal = Some(journal); }
    pub fn set_verifier(&mut self, verifier: Box<dyn Verifier>) { self.verifier = Some(verifier); }
    pub fn cancel_handle(&self) -> Arc<AtomicBool> { Arc::clone(&self.cancellation) }
    pub fn set_cancel_handle(&mut self, handle: Arc<AtomicBool>) { self.cancellation = handle; }

    /// Approval is scoped to ONE pending call id, not the whole permission class.
    pub fn approve_once(&mut self, call_id: impl Into<String>) {
        self.approved_once = Some(call_id.into());
    }

    /// Explicit recovery after an uncertain side effect. An operator must
    /// inspect the workspace; neither choice is silently inferred.
    pub fn reconcile(&mut self, state: &mut Session, result: Value) -> Result<(), String> {
        state.reconcile_observation(result)?;
        self.persist(state)
    }

    fn persist(&self, state: &Session) -> Result<(), String> {
        if let Some(journal) = &self.journal { journal.save(state) } else { Ok(()) }
    }

    fn text(&self, text: String) -> String {
        if text.chars().count() <= self.limits.max_tool_output_chars { return text; }
        let mut value: String = text.chars().take(self.limits.max_tool_output_chars).collect();
        value.push_str("...[truncated]");
        value
    }

    pub fn run(&mut self, state: &mut Session) -> Result<(), String> {
        if state.messages.len() < 2 || state.messages[0].role != crate::Role::System
            || state.messages[1].role != crate::Role::User
            || state.messages[1].content != state.instruction {
            return Err("invalid session: original user instruction is missing".into());
        }
        if let Some(call) = &state.in_flight {
            state.status = RunStatus::InDoubt { call_id: call.id.clone(), tool: call.name.clone() };
            self.persist(state)?;
            return Ok(());
        }
        if matches!(state.status, RunStatus::Completed | RunStatus::Cancelled | RunStatus::Exhausted { .. }) {
            return Ok(());
        }
        state.status = RunStatus::Running;
        self.persist(state)?;
        loop {
            if self.cancellation.load(Ordering::SeqCst) {
                state.status = RunStatus::Cancelled;
                state.event("cancelled", json!({}));
                self.persist(state)?;
                return Ok(());
            }
            if !state.pending_calls.is_empty() {
                let call = state.pending_calls[0].clone();
                if let Some(previous) = state.completed_calls.get(&call.id).cloned() {
                    state.pending_calls.remove(0);
                    if previous.tool != call.name || previous.arguments != call.arguments {
                        state.status = RunStatus::Failed {
                            error:format!("provider reused tool call id {} with changed arguments",call.id)
                        };
                        state.event("tool_replay_conflict",json!({"id":call.id,"tool":call.name}));
                        self.persist(state)?;
                        return Ok(());
                    }
                    state.messages.push(Message::tool_result(call.id.clone(), previous.result));
                    state.event("tool_replayed", json!({"id":call.id,"tool":call.name}));
                    self.persist(state)?;
                    continue;
                }
                if state.tool_calls >= self.limits.max_tool_calls {
                    state.status = RunStatus::Exhausted { reason: "max_tool_calls".into() };
                    self.persist(state)?;
                    return Ok(());
                }
                let Some(spec) = self.tools.get(&call.name).map(|tool| tool.spec()) else {
                    state.pending_calls.remove(0);
                    state.messages.push(Message::tool_result(call.id.clone(),
                        json!({"error":format!("unknown tool: {}", call.name)}).to_string()));
                    state.event("tool_unknown", json!({"id":call.id,"tool":call.name}));
                    self.persist(state)?;
                    continue;
                };
                let decision = self.policy.decide(&spec);
                if decision == Decision::Deny {
                    state.status = RunStatus::Denied { tool: spec.name };
                    state.event("denied", json!({"id":call.id,"tool":call.name}));
                    self.persist(state)?;
                    return Ok(());
                }
                if decision == Decision::Ask && self.approved_once.as_deref() != Some(call.id.as_str()) {
                    state.status = RunStatus::AwaitingApproval { call_id: call.id, tool: call.name };
                    self.persist(state)?;
                    return Ok(());
                }
                self.approved_once = None;
                state.tool_calls += 1;
                state.pending_calls.remove(0);
                state.in_flight = Some(call.clone());
                state.event("tool_started", json!({"id":call.id,"tool":call.name}));
                // Durability barrier BEFORE any possible external side effect.
                self.persist(state)?;
                let result = self.tools.get_mut(&call.name)
                    .expect("registered tool must exist until dispatch")
                    .invoke(&call.arguments);
                let (ok, text) = match result {
                    Ok(value) => (true, value.to_string()),
                    Err(error) => (false, json!({"error": error}).to_string()),
                };
                let text = self.text(text);
                state.completed_calls.insert(call.id.clone(), CompletedCall {
                    tool:call.name.clone(), arguments:call.arguments.clone(), result:text.clone()
                });
                state.messages.push(Message::tool_result(call.id.clone(), text));
                state.in_flight = None;
                state.event("tool_finished", json!({"id":call.id,"tool":call.name,"ok":ok}));
                self.persist(state)?;
                continue;
            }
            if state.rounds >= self.limits.max_rounds {
                state.status = RunStatus::Exhausted { reason: "max_rounds".into() };
                self.persist(state)?;
                return Ok(());
            }
            let removed = ContextBudget::new(self.limits.max_context_chars).trim(&mut state.messages);
            if removed > 0 { state.event("context_trimmed", json!({"removed":removed})); }
            state.rounds += 1;
            state.event("model_started", json!({"round":state.rounds}));
            self.persist(state)?;
            let reply = match self.provider.complete(&state.messages) {
                Ok(reply) => reply,
                Err(error) => {
                    state.status = RunStatus::Failed { error };
                    self.persist(state)?;
                    return Ok(());
                }
            };
            let (content, calls) = match reply {
                ModelReply::Assistant { content, tool_calls } => (content, tool_calls),
                ModelReply::Final { content } => (content, Vec::new()),
            };
            state.messages.push(Message::assistant_calls(content, calls.clone()));
            state.pending_calls = calls;
            state.event("model_finished", json!({"round":state.rounds,"calls":state.pending_calls.len()}));
            self.persist(state)?;
            if !state.pending_calls.is_empty() { continue; }
            match self.verifier.as_mut().map(|v| v.verify(state)) {
                None => {
                    // Final prose is not evidence that a coding task passed.
                    state.status = RunStatus::Completed;
                    state.event("completed", json!({"independentlyVerified":false}));
                }
                Some(Verification::Passed) => {
                    state.status = RunStatus::Completed;
                    state.event("verified", json!({"passed":true,"independentlyVerified":true}));
                }
                Some(Verification::Rejected(reason)) => {
                    state.status = RunStatus::Failed { error: reason };
                    state.event("verified", json!({"passed":false,"retry":false}));
                }
                Some(Verification::Retry(reason)) => {
                    state.event("verified", json!({"passed":false,"retry":true}));
                    state.messages.push(Message::user(format!(
                        "Independent verification failed. Fix the result and verify again. Details: {}", self.text(reason)
                    )));
                }
            }
            self.persist(state)?;
            if state.status != RunStatus::Running { return Ok(()); }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Permission, ToolSpec};
    use std::collections::VecDeque;

    struct Scripted(VecDeque<ModelReply>);
    impl Provider for Scripted {
        fn complete(&mut self, _: &[Message]) -> Result<ModelReply, String> {
            self.0.pop_front().ok_or_else(|| "no model response".into())
        }
    }
    struct Echo;
    impl Tool for Echo {
        fn spec(&self) -> ToolSpec {
            ToolSpec { name: "echo".into(), description: "echo".into(),
                parameters: json!({"type":"object"}), permission: Permission::Write }
        }
        fn invoke(&mut self, v: &Value) -> Result<Value, String> { Ok(v.clone()) }
    }
    struct Ask;
    impl Policy for Ask { fn decide(&self, _: &ToolSpec) -> Decision { Decision::Ask } }
    fn calls() -> ModelReply {
        ModelReply::Assistant { content: String::new(), tool_calls: vec![
            ToolCall { id: "c1".into(), name: "echo".into(), arguments: json!({"x":1}) }
        ]}
    }
    #[test]
    fn approval_pauses_and_resumes_exactly_once() {
        let provider = Scripted(VecDeque::from([calls(), ModelReply::Final { content: "done".into() }]));
        let mut engine = AgentSdk::new(Box::new(provider), Box::new(Ask), Limits::default());
        engine.register(Box::new(Echo));
        let mut run = Session::new("s", "system", "user's exact goal");
        engine.run(&mut run).unwrap();
        assert!(matches!(run.status, RunStatus::AwaitingApproval { .. }));
        assert_eq!(run.tool_calls, 0);
        engine.approve_once("c1");
        engine.run(&mut run).unwrap();
        assert_eq!(run.status, RunStatus::Completed);
        assert_eq!(run.tool_calls, 1);
        assert_eq!(run.messages[1].content, "user's exact goal");
        assert_eq!(run.messages[3].tool_call_id.as_deref(), Some("c1"));
    }
    #[test]
    fn duplicate_provider_call_is_not_executed_twice() {
        let provider = Scripted(VecDeque::from([
            calls(), calls(), ModelReply::Final { content: "done".into() }
        ]));
        let mut engine = AgentSdk::new(Box::new(provider), Box::new(crate::AllowAll), Limits::default());
        engine.register(Box::new(Echo));
        let mut run = Session::new("s", "s", "u");
        engine.run(&mut run).unwrap();
        assert_eq!(run.status, RunStatus::Completed);
        assert_eq!(run.tool_calls, 1);
        assert_eq!(run.completed_calls.len(), 1);
        assert!(run.events.iter().any(|event| event.kind=="tool_replayed"));
    }
    #[test]
    fn modified_arguments_with_reused_call_id_fail_closed() {
        let mut changed = calls();
        if let ModelReply::Assistant { ref mut tool_calls, .. } = changed {
            tool_calls[0].arguments = json!({"x":99});
        }
        let provider = Scripted(VecDeque::from([calls(), changed]));
        let mut engine = AgentSdk::new(Box::new(provider), Box::new(crate::AllowAll), Limits::default());
        engine.register(Box::new(Echo));
        let mut run = Session::new("s", "s", "u");
        engine.run(&mut run).unwrap();
        assert!(matches!(run.status, RunStatus::Failed { .. }));
        assert_eq!(run.tool_calls, 1);
    }

    #[test]
    fn uncertain_external_effect_requires_manual_reconciliation() {
        let provider = Scripted(VecDeque::new());
        let mut engine = AgentSdk::new(Box::new(provider), Box::new(crate::AllowAll), Limits::default());
        let mut run = Session::new("s", "s", "u");
        run.in_flight = Some(ToolCall { id:"risk".into(), name:"shell".into(), arguments:json!({}) });
        engine.run(&mut run).unwrap();
        assert!(matches!(run.status, RunStatus::InDoubt { .. }));
        assert_eq!(run.tool_calls, 0);
        engine.reconcile(&mut run, json!({"operator":"reconciled"})).unwrap();
        assert!(run.in_flight.is_none());
    }
    #[test]
    fn final_answer_without_an_independent_verifier_is_not_claimed_as_verified() {
        let provider = Scripted(VecDeque::from([ModelReply::Final { content: "done".into() }]));
        let mut engine = AgentSdk::new(Box::new(provider), Box::new(crate::AllowAll), Limits::default());
        let mut run = Session::new("s", "rules", "goal");
        engine.run(&mut run).unwrap();
        assert_eq!(run.status, RunStatus::Completed);
        assert!(run.events.iter().any(|event| event.kind == "completed"
            && event.detail["independentlyVerified"] == false));
        assert!(!run.events.iter().any(|event| event.kind == "verified"));
    }
    struct FailOnce(bool);
    impl Verifier for FailOnce {
        fn verify(&mut self, _: &Session) -> Verification {
            if self.0 { self.0=false; Verification::Retry("test failed".into()) }
            else { Verification::Passed }
        }
    }
    #[test]
    fn failed_verification_requests_a_real_revision() {
        let provider = Scripted(VecDeque::from([
            ModelReply::Final { content:"incorrect".into() },
            ModelReply::Final { content:"corrected".into() },
        ]));
        let mut engine = AgentSdk::new(Box::new(provider), Box::new(crate::AllowAll), Limits::default());
        engine.set_verifier(Box::new(FailOnce(true)));
        let mut run = Session::new("s", "s", "original goal");
        engine.run(&mut run).unwrap();
        assert_eq!(run.status, RunStatus::Completed);
        assert_eq!(run.rounds, 2);
        assert_eq!(run.final_text(), Some("corrected"));
        assert_eq!(run.messages[1].content, "original goal");
    }
}
