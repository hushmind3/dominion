//! Core execution ports and checkpoint types. No planner, policy or database.
use crate::{Action,Receipt,Strategy,ToolSpec};
use serde::{Serialize,Deserialize};
use serde_json::{json,Value};
use std::{future::Future,pin::Pin};

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Limits {
    pub max_steps: usize,
    pub max_model_calls: usize,
    /// Hard operation ceiling across every checkpoint, including verification tools.
    pub max_tool_calls: usize,
    pub max_estimated_tokens: u64,
    pub max_parallel_reads: usize,
    pub max_result_chars: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self { max_steps: 24, max_model_calls: 32, max_tool_calls: 96,
            max_estimated_tokens: 100_000,
            max_parallel_reads: 4, max_result_chars: 3000 }
    }
}
impl Limits {
    pub fn validate(&self) -> Result<(), String> {
        if !(1..=200).contains(&self.max_steps)
            || !(1..=250).contains(&self.max_model_calls)
            || !(1..=4096).contains(&self.max_tool_calls)
            || !(1..=20_000_000).contains(&self.max_estimated_tokens)
            || !(1..=16).contains(&self.max_parallel_reads)
            || !(128..=16_384).contains(&self.max_result_chars) {
            return Err("Invalid agent budgets or parallelism".into());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CriterionKind { FileExists, FileAbsent, FileContains, FileSha256, ToolSucceeded }
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Criterion {
    #[serde(rename = "type")]
    pub kind: CriterionKind,
    #[serde(default)] pub path: Option<String>,
    #[serde(default)] pub contains: Option<String>,
    #[serde(default)] pub tool: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunRequest {
    pub id: String,
    pub instruction: String,
    #[serde(default)] pub owner_instructions: String,
    #[serde(default)] pub strategy: Strategy,
    #[serde(default)] pub limits: Limits,
    #[serde(default)] pub allowed_tools: Vec<String>,
    #[serde(default)] pub checks: Vec<Criterion>,
    /// A model claim by itself does not prove a file-editing task succeeded.
    #[serde(default = "default_true")] pub require_evidence: bool,
}
fn default_true() -> bool { true }
impl RunRequest {
    pub fn validate(&self) -> Result<(), String> {
        self.limits.validate()?;
        if self.id.trim().is_empty() || self.id.len() > 128
            || self.instruction.trim().is_empty() || self.instruction.len() > 64_000
            || self.owner_instructions.len() > 32_000
            || self.allowed_tools.len() > 1024 || self.checks.len() > 32
            || self.allowed_tools.iter().map(String::len).sum::<usize>() > 65_536 {
            return Err("Invalid agent request size or identity".into());
        }
        for tool in &self.allowed_tools {
            if tool.is_empty() || tool.len() > 128 || !tool.chars().all(|c|
                c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.')) {
                return Err(format!("Invalid tool name: {tool}"));
            }
        }
        for check in &self.checks {
            let path = check.path.as_deref().unwrap_or("");
            match check.kind {
                CriterionKind::FileExists | CriterionKind::FileAbsent if path.is_empty() =>
                    return Err("File check requires a path".into()),
                CriterionKind::FileContains if path.is_empty() || check.contains.as_deref().unwrap_or("").is_empty() =>
                    return Err("File contains check requires a path and text".into()),
                CriterionKind::FileSha256 if path.is_empty() || !check.contains.as_deref().is_some_and(|hash|
                    hash.len()==64 && hash.bytes().all(|b|b.is_ascii_hexdigit())) =>
                    return Err("File SHA-256 check requires a path and hexadecimal digest".into()),
                CriterionKind::ToolSucceeded if check.tool.as_deref().unwrap_or("").is_empty() =>
                    return Err("Tool check requires a tool name".into()),
                _ => {}
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Decision {
    #[serde(default)] pub plan: Vec<String>,
    #[serde(default)] pub actions: Vec<Action>,
    #[serde(default)] pub answer: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelReply {
    pub content: String,
    #[serde(default)] pub input_tokens: Option<u64>,
    #[serde(default)] pub output_tokens: Option<u64>,
}

/// One provider-native assistant turn. A structured tool request is already a
/// typed Decision; it is never decoded from an invented JSON text wrapper.
/// `decision: None` retains compatibility with local/legacy text-only models.
#[derive(Clone, Debug)]
pub struct ModelTurn {
    pub reply: ModelReply,
    pub decision: Option<Decision>,
}

pub trait Model: Send + Sync {
    fn complete<'a>(&'a self, system: &'a str, prompt: &'a str)
        -> BoxFuture<'a, Result<ModelReply, String>>;

    /// A provider that accepts typed tool schemas need not receive a
    /// duplicated megabyte-scale JSON catalogue in the system prompt.
    fn supports_native_tools(&self) -> bool {false}

    /// Native function/tool-call lane. Providers receive real host tool
    /// schemas and the durable receipt transcript; the default retains
    /// backwards-compatible JSON planning for text-only local routes.
    /// Every actual tool still runs through the same Rust kernel scheduler.
    fn complete_with_tools<'a>(
        &'a self,
        system: &'a str,
        prompt: &'a str,
        _tools: &'a [ToolSpec],
        _receipts: &'a [Receipt],
    ) -> BoxFuture<'a, Result<ModelTurn, String>> {
        Box::pin(async move {
            let reply=self.complete(system,prompt).await?;
            Ok(ModelTurn{reply,decision:None})
        })
    }
}
pub trait Tools: Send + Sync {
    fn definitions(&self) -> Vec<ToolSpec>;
    /// Host checks and performs user approvals; the kernel never bypasses them.
    fn execute<'a>(&'a self, action: &'a Action) -> BoxFuture<'a, Result<Value, String>>;
}
pub trait Evidence: Send + Sync {
    fn check<'a>(&'a self, criterion: &'a Criterion) -> BoxFuture<'a, Result<bool, String>>;
}
/// The single post-write-ahead admission check used by both the streaming
/// conversation transport and the native AgentLoop. This code cannot issue
/// model calls or effects on its own.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum DispatchFence {Proceed,Cancelled,OwnerChanged}
pub fn dispatch_fence(journal:&dyn Journal,run_id:&str,
    directive_before:Option<&str>)->Result<DispatchFence,String>{
    if journal.cancelled(run_id){return Ok(DispatchFence::Cancelled);}
    if journal.directive(run_id)?.as_deref()!=directive_before{
        return Ok(DispatchFence::OwnerChanged);
    }
    Ok(DispatchFence::Proceed)
}
pub trait Journal: Send + Sync {
    /// Must durably persist before an action, or the action must not execute.
    fn save(&self, snapshot: &Snapshot) -> Result<(), String>;
    fn cancelled(&self, _id: &str) -> bool { false }
    /// The owner may redirect a running task; new instructions override stale plans.
    fn directive(&self, _id: &str) -> Result<Option<String>, String> { Ok(None) }
}

#[cfg(test)]
mod dispatch_fence_tests {
    use super::*;
    use std::sync::atomic::{AtomicBool,Ordering};
    struct JournalStub {cancelled:AtomicBool,directive:Option<String>}
    impl Journal for JournalStub {
        fn save(&self,_:&Snapshot)->Result<(),String>{Ok(())}
        fn cancelled(&self,_:&str)->bool{self.cancelled.load(Ordering::SeqCst)}
        fn directive(&self,_:&str)->Result<Option<String>,String>{Ok(self.directive.clone())}
    }
    #[test] fn one_post_intent_fence_guards_cancellation_and_owner_changes(){
        let j=JournalStub{cancelled:AtomicBool::new(false),directive:Some("corrected".into())};
        assert_eq!(dispatch_fence(&j,"run",None).unwrap(),DispatchFence::OwnerChanged);
        assert_eq!(dispatch_fence(&j,"run",Some("corrected")).unwrap(),DispatchFence::Proceed);
        j.cancelled.store(true,Ordering::SeqCst);
        assert_eq!(dispatch_fence(&j,"run",Some("corrected")).unwrap(),DispatchFence::Cancelled);
    }
}
/// Native tool protocols may retain additional host data (stream events,
/// receipts and message framing) without forcing the model through JSON prose.

/// Minimal host ports exposed to a compiled agent-loop implementation.
/// No concrete DB, channels, provider SDK or UI types cross this boundary.
#[derive(Clone,Copy)]
pub struct AgentPorts<'a> {
    pub model: &'a dyn Model,
    pub tools: &'a dyn Tools,
    pub evidence: &'a dyn Evidence,
    pub journal: &'a dyn Journal,
}

/// Replaceable agent-loop strategy; independent of the runtime crate.
///
/// Rust loop implementations are trusted components of the host binary:
/// they must run tool effects through the host Tools authority interface.
/// Third-party/untrusted loops require an additional sandbox boundary.
pub trait AgentLoop: Send + Sync {
    fn id(&self)->&'static str;
    fn execute<'a>(&'a self,ports:AgentPorts<'a>,request:RunRequest,
        checkpoint:Option<Snapshot>)->BoxFuture<'a,Result<Snapshot,String>>;
}

pub struct DispatchResult<T> {
    pub output: T,
    pub observation: Value,
    pub success: bool,
}

pub trait Dispatcher: Send + Sync {
    type Output: Send;
    fn execute<'a>(&'a self, action: &'a Action)
        -> BoxFuture<'a, Result<DispatchResult<Self::Output>, String>>;

    /// The host is the only authority for read-only classification.
    /// Unknown tools default to effects: never parallelize them by guessing.
    fn is_read_only(&self, _action: &Action) -> bool { false }
    fn max_parallel_reads(&self) -> usize { 4 }
}

/// A shared write-ahead barrier for native streaming and structured planners.
/// Independent, host-declared reads can run together. Every effect is a strict
/// ordering barrier; a failed group prevents later effects from executing.
/// Each group is independently journaled so a crash cannot replay prior effects.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status { Running, Completed, NeedsReview, Failed, Cancelled }

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckResult {
    pub criterion: Criterion,
    pub satisfied: bool,
    pub explanation: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub id: String,
    pub status: Status,
    pub instruction: String,
    /// Pinned algorithm across crash recovery. Older snapshots deserialize as None.
    #[serde(default,skip_serializing_if="Option::is_none")]
    pub algorithm_id: Option<String>,
    pub plan: Vec<String>,
    pub step: usize,
    pub model_calls: usize,
    pub charged_tokens: u64,
    pub receipts: Vec<Receipt>,
    pub checks: Vec<CheckResult>,
    pub answer: Option<String>,
    pub error: Option<String>,
    /// Write-ahead marker. If the host crashes at this point, do not replay.
    pub in_flight: Vec<Action>,
    /// A crash during paid generation must not silently repeat the model call.
    #[serde(default)] pub model_in_flight: bool,
    /// Exact typed model decisions before dispatch, persisted for replay.
    /// Prior checkpoint JSON remains loadable; legacy runs have an empty tape.
    #[serde(default,skip_serializing_if="Vec::is_empty")]
    pub decision_tape: Vec<Decision>,
}
impl Snapshot {
    pub fn from_request(request: &RunRequest) -> Self {
        Self { id: request.id.clone(), status: Status::Running,
            instruction: request.instruction.clone(), algorithm_id:None,
            plan: vec![], step: 0,
            model_calls: 0, charged_tokens: 0, receipts: vec![], checks: vec![],
            answer: None, error: None, in_flight: vec![], model_in_flight: false,
            decision_tape: vec![] }
    }
    pub fn stop(&mut self, status: Status, reason: impl Into<String>) {
        self.status = status;
        self.error = Some(reason.into());
    }
    /// The operator may resolve exactly ONE uncertain tool effect. A human
    /// observation is never a successful tool receipt, an independent
    /// acceptance proof, or permission to replay a previous paid call.
    ///
    /// This returns the reconciled action so the host can atomically attach
    /// its own durable audit metadata before allowing queue continuation.
    pub fn reconcile_tool_observation(
        &mut self, applied:bool, details:&str
    )->Result<Action,String>{
        let details=details.trim();
        if details.is_empty()||details.len()>8192 {
            return Err("A concrete operator observation (1..8192 bytes) is required".into());
        }
        if !matches!(self.status,Status::NeedsReview)
            ||self.model_in_flight||self.in_flight.len()!=1 {
            return Err("Only one uncertain tool effect may be reconciled; uncertain model calls cannot be replayed".into());
        }
        let action=self.in_flight[0].clone();
        if self.receipts.iter().any(|r|r.id==action.id) {
            return Err("An existing tool receipt already owns this action ID".into());
        }
        self.receipts.push(Receipt{
            id:action.id.clone(),tool:action.tool.clone(),
            arguments:action.arguments.clone(),success:false,
            result:json!({
                "operatorObservedApplied":applied,
                "observation":details,
                "independentlyVerified":false,
                "replayed":false,
                "reason":"uncertain tool outcome manually reconciled; verify before trusting"
            }),
            step:self.step,
        });
        self.checks.push(CheckResult{
            criterion:Criterion{kind:CriterionKind::ToolSucceeded,path:None,
                contains:None,tool:Some("manual_reconciliation".into())},
            satisfied:false,
            explanation:format!(
                "Operator observed uncertain {} result for {}. Independent inspection and replanning required; never treat this as a successful tool receipt.",
                if applied{"applied"}else{"not applied"},action.tool),
        });
        self.in_flight.clear();
        self.status=Status::Running;
        self.error=None;
        Ok(action)
    }
}


#[cfg(test)]
mod legacy_snapshot_tests {
    use super::*;

    #[test]
    fn legacy_native_run_without_model_fence_and_decision_tape_is_readable(){
        let request=RunRequest{id:"old-run".into(),instruction:"inspect".into(),
            owner_instructions:String::new(),strategy:Strategy::Balanced,
            limits:Limits::default(),allowed_tools:Vec::new(),checks:Vec::new(),
            require_evidence:true};
        let mut value=serde_json::to_value(Snapshot::from_request(&request)).unwrap();
        let object=value.as_object_mut().unwrap();
        object.remove("modelInFlight");
        object.remove("decisionTape");
        object.remove("algorithmId");
        let decoded:Snapshot=serde_json::from_value(value).unwrap();
        assert_eq!(decoded.id,"old-run");
        assert!(decoded.decision_tape.is_empty());
        assert!(!decoded.model_in_flight);
        assert!(decoded.algorithm_id.is_none());
        assert!(matches!(decoded.status,Status::Running));
    }

    #[test]
    fn uncertain_legacy_tool_checkpoint_requires_manual_reconciliation(){
        let request=RunRequest{id:"old-effect".into(),instruction:"write".into(),
            owner_instructions:String::new(),strategy:Strategy::Balanced,
            limits:Limits::default(),allowed_tools:Vec::new(),checks:Vec::new(),
            require_evidence:true};
        let mut snapshot=Snapshot::from_request(&request);
        snapshot.status=Status::NeedsReview;
        snapshot.in_flight.push(Action{id:"effect-a".into(),
            tool:"workspace_write".into(),arguments:json!({"path":"test.txt"}),
            depends_on:Vec::new()});
        let mut value=serde_json::to_value(&snapshot).unwrap();
        value.as_object_mut().unwrap().remove("decisionTape");
        value.as_object_mut().unwrap().remove("modelInFlight");
        let mut recovered:Snapshot=serde_json::from_value(value).unwrap();
        assert_eq!(recovered.in_flight.len(),1);
        assert!(recovered.receipts.is_empty());
        recovered.model_in_flight=true;
        assert!(recovered.reconcile_tool_observation(false,"no file").is_err(),
            "unknown paid model and an uncertain effect cannot be reconciled as a tool-only event");
        assert!(recovered.receipts.is_empty());
    }
}
