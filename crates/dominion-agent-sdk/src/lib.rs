//! DOMINION's model-independent Rust agent SDK.
//!
//! The kernel owns planning, tool dispatch order, limits, independent evidence,
//! bounded replanning, progress, and durable write-ahead checkpoints.
//! The host supplies providers, tools, and storage; there is no imported harness.

pub mod runtime;
mod dispatch;
pub use dispatch::dispatch_batch;

use futures_util::future::join_all;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::HashMap, future::Future, pin::Pin};
mod scheduler;
mod acceptance;
mod context;
mod tool_discovery;
mod schema;
pub use schema::validate_arguments;
pub mod replay;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Strategy { #[default] Balanced, VerifyFirst, LowCost }

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
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub schema: Value,
    /// Declared by the host, not the LLM. Only truly read-only operations may parallelize.
    pub read_only: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Action {
    pub id: String,
    pub tool: String,
    #[serde(default)] pub arguments: Value,
    #[serde(default)] pub depends_on: Vec<String>,
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
pub trait Journal: Send + Sync {
    /// Must durably persist before an action, or the action must not execute.
    fn save(&self, snapshot: &Snapshot) -> Result<(), String>;
    fn cancelled(&self, _id: &str) -> bool { false }
    /// The owner may redirect a running task; new instructions override stale plans.
    fn directive(&self, _id: &str) -> Result<Option<String>, String> { Ok(None) }
}

/// Native tool protocols may retain additional host data (stream events,
/// receipts and message framing) without forcing the model through JSON prose.
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
pub struct Receipt {
    pub id: String,
    pub tool: String,
    pub arguments: Value,
    pub success: bool,
    pub result: Value,
    pub step: usize,
}
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
            instruction: request.instruction.clone(), plan: vec![], step: 0,
            model_calls: 0, charged_tokens: 0, receipts: vec![], checks: vec![],
            answer: None, error: None, in_flight: vec![], model_in_flight: false,
            decision_tape: vec![] }
    }
    fn stop(&mut self, status: Status, reason: impl Into<String>) {
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

pub struct Kernel<'a> {
    pub model: &'a dyn Model,
    pub tools: &'a dyn Tools,
    pub evidence: &'a dyn Evidence,
    pub journal: &'a dyn Journal,
}
fn estimate_tokens(text: &str) -> u64 {
    // A 4-chars-per-token rule seriously undercounts Korean/CJK prompts.
    let ascii=text.chars().filter(char::is_ascii).count();
    let unicode=text.chars().count().saturating_sub(ascii);
    ascii.div_ceil(4) as u64 + unicode as u64
}
fn decode(content: &str) -> Result<Decision, String> {
    let raw = content.trim();
    // Some compatible model providers wrap structured output in a fenced
    // block. Decode only the outermost fence; never "repair" arbitrary text.
    let json_text = if let Some(fenced) = raw.strip_prefix("```") {
        let header = fenced.find('\n').ok_or("Missing fenced JSON body")?;
        fenced[header + 1..].trim().strip_suffix("```")
            .ok_or("Unterminated JSON fence")?.trim()
    } else { raw };
    let decision: Decision = serde_json::from_str(json_text)
        .map_err(|error| format!("Invalid model decision JSON: {error}"))?;
    validate_decision_plan(decision)
}
/// Native provider function calls and text-only JSON planners must satisfy
/// the identical bounded plan contract before any decision is journaled.
fn validate_decision_plan(decision: Decision) -> Result<Decision, String> {
    if decision.plan.len() > 32 || decision.plan.iter().any(|item| item.len() > 1000) {
        return Err("Plan exceeds maximum structure size".into());
    }
    Ok(decision)
}

/// Distinguish inspection from external effects so existing files or a
/// plausible model claim cannot satisfy an unperformed edit.
/// File changes and generic external actions have different evidence.
/// Running a command, opening a browser or deploying a service must not be
/// rejected just because it did not write a workspace file.
fn owner_requires_file_mutation(text: &str) -> bool {
    let text=text.to_lowercase();
    let edit=["수정","편집","작성","만들","생성","삭제","제거","고쳐","구현","개발",
        "리팩토링","패치","write","edit","delete","remove","modify",
        "fix","implement","refactor","patch","create"];
    let artifact=["파일","코드","소스","저장소","프로젝트","버그","구현","개발",
        "리팩토링","readme","source","repository","code","file","script",
        "unit test",".rs",".py",".ts",".js",".md",".txt","artifact"];
    edit.iter().any(|word|text.contains(word))
        && artifact.iter().any(|word|text.contains(word))
}

fn owner_requires_effect(text: &str) -> bool {
    let t = text.to_lowercase();
    ["만들","생성","작성","수정","편집","삭제","제거","고쳐","구현",
     "개발","리팩토링","배포","설치","실행","저장","커밋","복사","이동",
     "create","write","edit","delete","remove","modify","fix","implement",
     "build","deploy","install","execute","save","commit","refactor"]
        .iter().any(|word| t.contains(word))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AcceptanceRun {
    Passed,
    NewFailure,
    ExistingFailure,
    OwnerChanged,
    Cancelled,
    ToolUnavailable,
    ToolBudgetExceeded,
}

impl Kernel<'_> {
    pub async fn run(&self, request: RunRequest) -> Result<Snapshot, String> {
        self.run_from(request, None).await
    }

    /// Continue only from a committed, exact-job checkpoint. Nothing already
    /// recorded is replayed, including completed read calls and paid model
    /// generations. Unknown external effects require explicit reconciliation.
    pub async fn resume(&self, request: RunRequest, saved: Snapshot) -> Result<Snapshot, String> {
        self.run_from(request, Some(saved)).await
    }

    async fn run_from(&self, request: RunRequest, saved: Option<Snapshot>)
        -> Result<Snapshot, String> {
        request.validate()?;
        let mut run = if let Some(saved) = saved {
            if saved.id != request.id || saved.instruction != request.instruction {
                return Err("Checkpoint does not match the requested task identity".into());
            }
            if !saved.in_flight.is_empty() || saved.model_in_flight {
                return Err("Checkpoint has an uncertain tool or model outcome; manual review required".into());
            }
            if saved.step > request.limits.max_steps ||
                saved.model_calls > request.limits.max_model_calls ||
                saved.receipts.len() > request.limits.max_tool_calls {
                return Err("Checkpoint exceeds the current execution limits".into());
            }
            if !matches!(saved.status, Status::Running) {
                return Ok(saved);
            }
            saved
        } else { Snapshot::from_request(&request) };
        let specs = self.tools.definitions();
        let mut tools = HashMap::new();
        for spec in specs {
            if !request.allowed_tools.contains(&spec.name) {
                continue;
            }
            if tools.contains_key(&spec.name) {
                return Err(format!("Ambiguous duplicate authorized tool: {}", spec.name));
            }
            tools.insert(spec.name.clone(), spec);
        }
        // Internal observation/discovery tools are reserved Rust capabilities.
        // Host declarations must not silently shadow them or be shadowed by
        // them; a collision can change routing or the effective permissions.
        for spec in context::internal_specs() {
            if tools.contains_key(&spec.name) {
                return Err(format!("Reserved kernel tool name: {}", spec.name));
            }
            tools.insert(spec.name.clone(),spec);
        }
        if tool_discovery::needed(&tools) {
            for spec in [tool_discovery::spec(),tool_discovery::call_spec()] {
                if tools.contains_key(&spec.name) {
                    return Err(format!("Reserved kernel tool name: {}", spec.name));
                }
                tools.insert(spec.name.clone(),spec);
            }
        }
        let native_wire=self.model.supports_native_tools();
        let response_contract=if native_wire {
            "For each action use a provider-native function/tool call with valid JSON object arguments. \
             Never emit a JSON decision wrapper or impersonate a tool result in text. \
             When no more actions are needed, respond with the actual final answer as normal text."
        }else{
            "Reply with exactly one JSON object: \
             {\"plan\":[\"concise ordered steps\"],\"actions\":[{\"id\":\"unique-id\", \
             \"tool\":\"name\",\"arguments\":{},\"dependsOn\":[]}],\"answer\":null}. \
             On task completion return actions=[] and answer=\"actual result\"."
        };
        let discovery_hint=if tool_discovery::needed(&tools) {
            "If a capability is not advertised, call dominion_tool_search to read \
             its authorized JSON schema, then dominion_tool_call with the exact \
             discovered name and arguments. The provider function declaration \
             list stays fixed; discovery never increases host permissions."
        }else{
            "Every currently permitted host tool is directly available."
        };
        let system=format!(
            "You are DOMINION's model planner. The Rust kernel owns action dispatch, \
             limits, permission checks and verification. Respect the owner's instructions \
             and never claim an unobserved task is done. {} \
             Owner-required checks are executed by the Rust kernel on proposed completion. \
             You may run diagnostic tests while working, but do not waste a model turn \
             rerunning an unchanged acceptance command. After edits, propose completion; \
             Rust will execute outstanding exact tests. If a test fails, inspect the recorded \
             diagnostic, modify the source and propose completion again. A requested source \
             edit requires a workspace tool receipt with written=true. \
             Previous tool outputs remain in the durable journal. Use dominion_receipt_search \
             and dominion_receipt_read for prior details; never replay external effects merely \
             to retrieve an observation. {} Strategy: {:?}. Owner instructions: {}.",
            response_contract,discovery_hint,request.strategy,request.owner_instructions
        );
        let mut invalid = 0usize;
        self.journal.save(&run)?;
        'steps: for step in run.step..request.limits.max_steps {
            if self.journal.cancelled(&request.id) {
                run.stop(Status::Cancelled, "Owner cancelled execution");
                self.journal.save(&run)?;
                return Ok(run);
            }
            if run.model_calls >= request.limits.max_model_calls {
                break;
            }
            run.step = step + 1;
            // Compact only the model's view. Full receipts remain durable
            // and can be searched/retrieved by internal read-only tools.
            let projected=context::project(&run.receipts,
                request.strategy,request.limits.max_result_chars);
            let current_directive=self.journal.directive(&request.id)?;
            // Tool authorization remains the full host-owned catalogue. The
            // model sees a small, deterministic view, promoted by its own
            // durable discovery receipts rather than by an extra model call.
            let visible_tools=tool_discovery::select(
                &tools,&request.instruction,&request.owner_instructions,
                current_directive.as_deref(),&run.plan,&run.receipts);
            let catalog_for_prompt=if native_wire {
                format!("The provider-native tool catalogue exposes {} authorized schemas this turn.",
                    visible_tools.len())
            } else {
                let definitions=visible_tools.iter().map(|spec|json!({
                    "name":spec.name,
                    "description":if matches!(request.strategy,Strategy::LowCost) {
                        spec.description.chars().take(120).collect::<String>()
                    } else {spec.description.clone()},
                    "parameters":spec.schema,"readOnly":spec.read_only
                })).collect::<Vec<_>>();
                format!("Advertised authorized tools for this turn: {}",
                    serde_json::to_string(&definitions).map_err(|e|e.to_string())?)
            };
            let prompt = json!({
                "userTask":request.instruction,
                "latestOwnerDirective":current_directive, "ownerInstructions":request.owner_instructions,
                "currentPlan":run.plan, "recentObservations":projected.recent,
                "importantPastFailures":projected.important_failures,
                "historicalReceiptIndex":projected.history_index,
                "executionDigest":projected.digest,
                "requiredAcceptanceChecks":request.checks,
                "verificationGaps":run.checks.iter().filter(|v| !v.satisfied)
                    .map(|v| &v.explanation).collect::<Vec<_>>(),
                "remainingSteps":request.limits.max_steps-step,
                "modelBudgetRemaining":request.limits.max_model_calls-run.model_calls,
                "toolBudgetRemaining":request.limits.max_tool_calls.saturating_sub(run.receipts.len()),
                "authorizedToolCount":tools.len(),
                "advertisedToolCount":visible_tools.len()
            }).to_string();
            // Latest owner instruction is reloaded as a higher-priority
            // instruction on every turn; stale models cannot hide it in a
            // lower-priority tool observation.
            let active_system=if let Some(directive)=current_directive.as_deref() {
                format!("{system}\n{catalog_for_prompt}\nLATEST OWNER DIRECTIVE (overrides earlier task plans): {directive}")
            }else{format!("{system}\n{catalog_for_prompt}")};
            // Model response generation is bounded by the user budget, too.
            let estimated = estimate_tokens(&active_system) + estimate_tokens(&prompt) + 1024;
            if run.charged_tokens.saturating_add(estimated) > request.limits.max_estimated_tokens {
                run.stop(Status::NeedsReview, "Model token budget exhausted");
                self.journal.save(&run)?;
                return Ok(run);
            }
            run.model_in_flight = true;
            self.journal.save(&run)?;
            let turn = match self.model.complete_with_tools(
                &active_system, &prompt, &visible_tools, &run.receipts,
            ).await {
                Ok(turn) => turn,
                Err(error) => {
                    if self.journal.cancelled(&request.id) {
                        // Cancellation before the provider's terminal model
                        // event is not permission to replay that paid call.
                        // The cancelled job itself is terminal.
                        run.model_in_flight=false;
                        run.stop(Status::Cancelled,"Owner cancelled native model generation");
                    } else {
                        // Unknown partial model output remains explicitly
                        // in-flight, prohibiting automatic paid replay.
                        run.stop(Status::NeedsReview,format!("Model unavailable: {error}"));
                    }
                    self.journal.save(&run)?;
                    return Ok(run);
                }
            };
            // The model's native function calls are validated by the same
            // kernel-owned scheduler, authorization and durable checkpoints.
            let native_decision=turn.decision;
            let reply=turn.reply;
            run.model_calls += 1;
            // Persist the fact that generation ended before deciding next actions.
            // If this checkpoint fails, no actions are dispatched.
            run.model_in_flight = false;
            run.charged_tokens = run.charged_tokens.saturating_add(
                reply.input_tokens.unwrap_or_else(|| estimate_tokens(&active_system) + estimate_tokens(&prompt))
                    .saturating_add(reply.output_tokens.unwrap_or_else(|| estimate_tokens(&reply.content)))
            );
            self.journal.save(&run)?;
            let decision = match native_decision.map_or_else(||decode(&reply.content),validate_decision_plan) {
                Ok(d) => d,
                Err(error) => {
                    invalid += 1;
                    run.checks = vec![CheckResult {
                        criterion: Criterion {kind:CriterionKind::ToolSucceeded,path:None,contains:None,tool:Some("decision_parse".into())},
                        satisfied:false, explanation:error
                    }];
                    self.journal.save(&run)?;
                    if invalid >= 3 { break; }
                    continue;
                }
            };
            // Store the MODEL decision, before any host action or acceptance
            // outcome, so historical failures are reproducible offline.
            // This is durable evidence, never authority to re-execute effects.
            run.decision_tape.push(decision.clone());
            self.journal.save(&run)?;
            if decision.plan.len() <= 32 {
                run.plan = decision.plan.into_iter().map(|s| s.chars().take(400).collect()).collect();
            }
            // An owner correction arriving during generation invalidates the
            // stale decision before any new tool call or final claim.
            if self.journal.directive(&request.id)? != current_directive {
                run.checks = vec![CheckResult {
                    criterion: Criterion {kind:CriterionKind::ToolSucceeded,path:None,
                        contains:None,tool:Some("owner_directive".into())},
                    satisfied:false,
                    explanation:"Owner changed the instruction; replan before acting".into()
                }];
                self.journal.save(&run)?;
                continue;
            }
            // A final answer never overrides an independent acceptance failure.
            if decision.actions.is_empty() {
                if let Some(answer) = decision.answer.filter(|s| !s.trim().is_empty()) {
                    // Don't waste user-approved test executions on an edit task
                    // for which the host has not recorded a single real write.
                    let requires_file_mutation = owner_requires_file_mutation(&request.instruction);
                    let requires_external_effect = owner_requires_effect(&request.instruction);
                    let has_nonread_effect = run.receipts.iter().any(|r|r.success &&
                        tools.get(&r.tool).is_some_and(|tool| !tool.read_only));
                    let has_confirmed_write = run.receipts.iter().any(|r|r.success &&
                        r.result["written"] == true &&
                        tools.get(&r.tool).is_some_and(|tool| !tool.read_only));
                    if requires_file_mutation && !has_confirmed_write {
                        run.checks = vec![CheckResult {
                            criterion:Criterion{kind:CriterionKind::ToolSucceeded,
                                path:None,contains:None,tool:Some("confirmed_write".into())},
                            satisfied:false,
                            explanation:"No host-confirmed file modification. Use a workspace editing tool and verify its written=true receipt before finishing".into(),
                        }];
                        invalid += 1;
                        self.journal.save(&run)?;
                        if invalid >= 3 {break;}
                        continue;
                    }
                    // Verify cheap file conditions before running shell tests.
                    // No compiler invocation is needed when a required file
                    // is missing or does not contain the requested change.
                    let cheap_checks=self.verify(&request,&run,&tools).await;
                    if cheap_checks.iter().any(|check| !check.satisfied
                        && check.criterion.kind != CriterionKind::ToolSucceeded) {
                        run.checks=cheap_checks;
                        self.journal.save(&run)?;
                        continue;
                    }
                    // The model may propose the final answer, but only the
                    // Rust kernel decides when required tests run and pass.
                    // Run all missing tests within this same model step,
                    // without paying for a separate planning call per test.
                    match self.run_required_commands(
                        &request, &mut run, &tools, current_directive.as_deref(),
                    ).await? {
                        AcceptanceRun::Passed => {}
                        AcceptanceRun::NewFailure => {
                            run.checks = self.verify(&request, &run, &tools).await;
                            self.journal.save(&run)?;
                            // Hand the real failure receipt to the model for
                            // source repair; do not blindly retry the test.
                            continue;
                        }
                        AcceptanceRun::ExistingFailure => {
                            run.stop(Status::NeedsReview,
                                "Required verification previously failed; no intervening repair was observed");
                            run.checks = self.verify(&request, &run, &tools).await;
                            self.journal.save(&run)?;
                            return Ok(run);
                        }
                        AcceptanceRun::ToolBudgetExceeded => {
                            run.stop(Status::NeedsReview,
                                "Owner-required verification would exceed the durable tool-call budget");
                            self.journal.save(&run)?;
                            return Ok(run);
                        }
                        AcceptanceRun::ToolUnavailable => {
                            run.stop(Status::NeedsReview,
                                "Required shell verification is not allowed by this run's tool permissions");
                            self.journal.save(&run)?;
                            return Ok(run);
                        }
                        AcceptanceRun::OwnerChanged => {
                            self.journal.save(&run)?;
                            continue;
                        }
                        AcceptanceRun::Cancelled => {
                            run.stop(Status::Cancelled,"Owner cancelled verification");
                            self.journal.save(&run)?;
                            return Ok(run);
                        }
                    }
                    run.checks = self.verify(&request, &run, &tools).await;
                    // A lengthy real test can finish after the owner has
                    // changed the goal. Never accept a stale completion.
                    if self.journal.directive(&request.id)?.as_deref()
                        != current_directive.as_deref() {
                        self.journal.save(&run)?;
                        continue;
                    }
                    if self.journal.cancelled(&request.id) {
                        run.stop(Status::Cancelled,"Owner cancelled execution");
                        self.journal.save(&run)?;
                        return Ok(run);
                    }
                    // Both evidence sources were checked before running tests;
                    // tests still cannot serve as proof of a file edit.
                    // A preexisting file can satisfy file_exists but cannot
                    // prove a requested change. Require a genuine successful
                    // non-read tool effect for action tasks unless the owner
                    // explicitly disabled evidence checking.
                    let observed = run.receipts.iter().any(|r| r.success);
                    let all_checks = run.checks.iter().all(|c| c.satisfied);
                    let evidence_sufficient = !request.require_evidence
                        || (all_checks && (!requires_external_effect || has_nonread_effect)
                            && (!requires_file_mutation || has_confirmed_write)
                            && (observed || !request.checks.is_empty()));
                    if evidence_sufficient && run.checks.iter().all(|c| c.satisfied) {
                        run.answer = Some(answer);
                        run.status = Status::Completed;
                        self.journal.save(&run)?;
                        return Ok(run);
                    }
                    if !evidence_sufficient {
                        run.checks.push(CheckResult {
                            criterion: Criterion {kind:CriterionKind::ToolSucceeded,path:None,contains:None,tool:Some("evidence".into())},
                            satisfied: false, explanation: "At least one real tool observation or explicit acceptance check is required".into()
                        });
                    }
                }
                self.journal.save(&run)?;
                continue;
            }
            if decision.actions.len() > 12 {
                invalid += 1;
                run.checks = vec![CheckResult {
                    criterion: Criterion {kind:CriterionKind::ToolSucceeded,path:None,
                        contains:None,tool:Some("dispatch_plan".into())},
                    satisfied:false,
                    explanation:"At most 12 tool actions may be proposed in one model step".into(),
                }];
                self.journal.save(&run)?;
                if invalid >= 3 { break; }
                continue;
            }
            // Reserve the entire proposed batch by checking the persisted
            // operation count first. Over-budget plans dispatch zero tools.
            // This also guards against slow no-progress loops with cheap reads.
            if decision.actions.len() > request.limits.max_tool_calls.saturating_sub(run.receipts.len()) {
                run.stop(Status::NeedsReview,"Tool-call budget exhausted before dispatch");
                self.journal.save(&run)?;
                return Ok(run);
            }
            // The Rust SDK, not the model, owns the dependency graph and
            // read-only concurrency. Invalid batches are sent back for
            // replanning without dispatching partial side effects.
            let groups = match scheduler::arrange(
                decision.actions, &tools, &run.receipts,
                request.limits.max_parallel_reads,
            ) {
                Ok(groups) => groups,
                Err(error) => {
                    invalid += 1;
                    run.checks = vec![CheckResult {
                        criterion: Criterion {kind:CriterionKind::ToolSucceeded,path:None,
                            contains:None,tool:Some("dispatch_plan".into())},
                        satisfied:false, explanation:error,
                    }];
                    self.journal.save(&run)?;
                    if invalid >= 3 { break; }
                    continue;
                }
            };
            invalid = 0;
            for group in groups {
                if self.journal.directive(&request.id)? != current_directive {
                    run.checks.push(CheckResult {
                        criterion: Criterion {kind:CriterionKind::ToolSucceeded,
                            path:None,contains:None,tool:Some("owner_directive".into())},
                        satisfied:false,
                        explanation:"Owner changed the task while tools were executing".into()
                    });
                    self.journal.save(&run)?;
                    continue 'steps;
                }
                if self.journal.cancelled(&request.id) {
                    run.stop(Status::Cancelled, "Owner cancelled execution");
                    self.journal.save(&run)?;
                    return Ok(run);
                }
                let mut ready = Vec::new();
                let mut skipped_any = false;
                for action in group {
                    let failed_dependency = action.depends_on.iter().find(|dependency| {
                        !run.receipts.iter().any(|receipt|
                            &receipt.id == *dependency && receipt.success)
                    });
                    if let Some(dependency) = failed_dependency {
                        skipped_any = true;
                        run.receipts.push(Receipt {
                            id:action.id.clone(),tool:action.tool.clone(),
                            arguments:action.arguments.clone(),success:false,
                            result:json!({"skipped":true,
                                "reason":format!("Dependency {dependency} was not successful")}),
                            step:run.step,
                        });
                    } else { ready.push(action); }
                }
                self.journal.save(&run)?;
                if !ready.is_empty() {
                    run.in_flight = ready.clone();
                    // Durable write-ahead marker: fail closed if storage fails.
                    self.journal.save(&run)?;
                    // Kernel-local observability never reaches the host. Every
                    // external effect still passes through normal permissions.
                    let history=&run.receipts;
                    let tool_catalog=&tools;
                    let results=join_all(ready.iter().map(|action| async move {
                        let actual=match tool_discovery::resolve_bridge(action,tool_catalog,history) {
                            Ok(Some(actual))=>actual,
                            Ok(None)=>action.clone(),
                            Err(error)=>return (action.clone(),Err(error)),
                        };
                        // Every real and synthetic operation is validated against
                        // the authoritative Rust catalog before any host effect.
                        let outcome=if let Some(spec)=tool_catalog.get(&actual.tool){
                            match schema::validate_arguments(&spec.schema,&actual.arguments){
                                Err(error)=>Err(error),
                                Ok(())=>if let Some(result)=
                                    tool_discovery::execute_local(&actual,tool_catalog)
                                    .or_else(||context::execute_local(&actual,history)) {
                                    result
                                } else {self.tools.execute(&actual).await}
                            }
                        }else{Err("Tool missing from authorized Rust catalog".into())};
                        (actual,outcome)
                    })).await;
                    for (action, (actual,outcome)) in ready.iter().zip(results) {
                        let (success, result) = match outcome {
                            Ok(data) => (true, data),
                            Err(error) => (false, json!({"error":error}))
                        };
                        skipped_any |= !success;
                        run.receipts.push(Receipt {
                            id:action.id.clone(), tool:actual.tool,
                            arguments:actual.arguments,
                            success, result, step:run.step,
                        });
                    }
                    run.in_flight.clear();
                    self.journal.save(&run)?;
                }
                if matches!(request.strategy,Strategy::VerifyFirst) && !request.checks.is_empty() {
                    run.checks=self.verify(&request,&run,&tools).await;
                    self.journal.save(&run)?;
                }
                if skipped_any {
                    // Replan against observed failures; never continue with
                    // writes derived from a stale or partially failed plan.
                    run.checks.push(CheckResult {
                        criterion: Criterion {kind:CriterionKind::ToolSucceeded,path:None,
                            contains:None,tool:Some("failed_tool_batch".into())},
                        satisfied:false,
                        explanation:"A tool failed or a dependency was skipped. Inspect receipts and replan".into(),
                    });
                    self.journal.save(&run)?;
                    continue 'steps;
                }
            }
        }
        run.stop(Status::NeedsReview, "Step, model or decision-repair limit reached");
        self.journal.save(&run)?;
        Ok(run)
    }

    /// Run exactly the owner's unobserved postconditions, never LLM-created
    /// test substitutes. Each call passes through the host's ordinary tool
    /// approval and durable write-ahead operation record.
    async fn run_required_commands(&self, request: &RunRequest, run: &mut Snapshot,
        tools: &HashMap<String, ToolSpec>, directive: Option<&str>,
    ) -> Result<AcceptanceRun, String> {
        for (index, check) in request.checks.iter().enumerate() {
            let Some(command) = acceptance::required_command(check) else {continue;};
            match acceptance::command_state(&run.receipts,&request.checks,tools,command) {
                acceptance::CommandState::Passed => continue,
                acceptance::CommandState::Failed => return Ok(AcceptanceRun::ExistingFailure),
                acceptance::CommandState::Pending => {}
            }
            if !tools.contains_key("shell_exec") {
                return Ok(AcceptanceRun::ToolUnavailable);
            }
            if run.receipts.len() >= request.limits.max_tool_calls {
                return Ok(AcceptanceRun::ToolBudgetExceeded);
            }
            if self.journal.cancelled(&request.id) {
                return Ok(AcceptanceRun::Cancelled);
            }
            if self.journal.directive(&request.id)?.as_deref() != directive {
                return Ok(AcceptanceRun::OwnerChanged);
            }
            let mut id = format!("kernel-verify-{}-{index}",run.step);
            while run.receipts.iter().any(|receipt|receipt.id==id) {
                id.push('x');
            }
            let action=Action {id:id.clone(),tool:"shell_exec".into(),
                arguments:json!({"command":command}),depends_on:Vec::new()};
            run.in_flight = vec![action.clone()];
            self.journal.save(run)?;
            let result=self.tools.execute(&action).await;
            let (success, result) = match result {
                Ok(output) => (true,output),
                Err(error) => (false,json!({"error":error})),
            };
            run.receipts.push(Receipt {id,tool:action.tool,
                arguments:action.arguments,success,result,step:run.step});
            run.in_flight.clear();
            // A checkpoint failure here leaves a durable unresolved
            // write-ahead marker; a restarted job cannot replay the command.
            self.journal.save(run)?;
            if !success {return Ok(AcceptanceRun::NewFailure);}
        }
        Ok(AcceptanceRun::Passed)
    }

    async fn verify(&self, request: &RunRequest, run: &Snapshot,
        tools: &HashMap<String, ToolSpec>) -> Vec<CheckResult> {
        // Independent final-byte witnesses are derived from trusted write
        // receipts and verified through the host, never from model prose.
        let artifact_witnesses=acceptance::artifact_witnesses(&run.receipts);
        if artifact_witnesses.len()>64 {
            return vec![CheckResult {
                criterion:Criterion{kind:CriterionKind::ToolSucceeded,
                    path:None,contains:None,tool:Some("artifact_witness_budget".into())},
                satisfied:false,
                explanation:format!("{} changed files exceed the 64-file final-byte verification budget; split the task or verify with an owner check",artifact_witnesses.len()),
            }];
        }
        let mut checks = Vec::with_capacity(request.checks.len()+artifact_witnesses.len());
        for criterion in request.checks.iter().chain(artifact_witnesses.iter()) {
            let observation = if criterion.kind == CriterionKind::ToolSucceeded {
                let name = criterion.tool.as_deref().unwrap_or("");
                if let Some(command)=acceptance::required_command(criterion) {
                    Ok(acceptance::command_state(&run.receipts,&request.checks,
                        tools,command)==acceptance::CommandState::Passed)
                } else {
                    Ok(run.receipts.iter().any(|r| r.tool == name && r.success))
                }
            } else {
                self.evidence.check(criterion).await
            };
            let (satisfied, explanation) = match observation {
                Ok(true) => (true, "Observed independent acceptance evidence".into()),
                Ok(false) => (false, format!("Unmet criterion: {:?}", criterion.kind)),
                Err(error) => (false, format!("Verification failed: {error}")),
            };
            checks.push(CheckResult {criterion:criterion.clone(),satisfied,explanation});
        }
        checks
    }
}

#[cfg(test)]
mod owner_effect_tests {
    use super::*;
    #[test]
    fn native_and_text_only_model_decisions_share_bounded_plans() {
        let oversized_plan=Decision{plan:vec!["step".into();33],
            actions:vec![],answer:None};
        assert!(validate_decision_plan(oversized_plan).is_err());
        let oversized_step=Decision{plan:vec!["x".repeat(1001)],
            actions:vec![],answer:None};
        assert!(validate_decision_plan(oversized_step).is_err());
        let text=serde_json::json!({"plan":vec!["step";33],
            "actions":[],"answer":null}).to_string();
        assert!(decode(&text).is_err());
        let accepted=Decision{plan:vec!["inspect".into(),"verify".into()],
            actions:vec![],answer:Some("complete".into())};
        assert!(validate_decision_plan(accepted).is_ok());
    }
    #[test]
    fn manual_uncertain_tool_observation_is_not_a_forged_success_receipt(){
        let request=RunRequest{
            id:"manual-one".into(),instruction:"write a file".into(),
            owner_instructions:String::new(),strategy:Strategy::Balanced,
            limits:Limits::default(),allowed_tools:vec!["workspace_write".into()],
            checks:vec![],require_evidence:true
        };
        let mut snapshot=Snapshot::from_request(&request);
        snapshot.status=Status::NeedsReview;
        snapshot.step=3;
        snapshot.in_flight=vec![Action{
            id:"operation-1".into(),tool:"workspace_write".into(),
            arguments:json!({"path":"result.txt","content":"written"}),
            depends_on:vec![]
        }];
        let action=snapshot.reconcile_tool_observation(true,"result.txt exists").unwrap();
        assert_eq!(action.id,"operation-1");
        assert!(matches!(snapshot.status,Status::Running));
        assert!(!snapshot.model_in_flight);
        assert!(snapshot.in_flight.is_empty());
        assert_eq!(snapshot.receipts.len(),1);
        assert_eq!(snapshot.receipts[0].success,false);
        assert_eq!(snapshot.receipts[0].result["independentlyVerified"],false);
        assert_eq!(snapshot.receipts[0].result["operatorObservedApplied"],true);
        assert!(!snapshot.checks.last().unwrap().satisfied);
        assert!(snapshot.reconcile_tool_observation(true,"again").is_err(),
            "cannot reconcile or replay an action twice");
    }
    #[test]
    fn uncertain_paid_model_and_multiple_tools_are_not_operator_resumable(){
        let request=RunRequest{
            id:"manual-two".into(),instruction:"run a shell command".into(),
            owner_instructions:String::new(),strategy:Strategy::Balanced,
            limits:Limits::default(),allowed_tools:vec![],checks:vec![],
            require_evidence:false
        };
        let mut snapshot=Snapshot::from_request(&request);
        snapshot.status=Status::NeedsReview;
        snapshot.model_in_flight=true;
        snapshot.in_flight=vec![Action{id:"x".into(),tool:"shell_exec".into(),
            arguments:json!({"command":"echo test"}),depends_on:vec![]}];
        assert!(snapshot.reconcile_tool_observation(false,"no effect").is_err());
        assert!(snapshot.in_flight.len()==1);
        snapshot.model_in_flight=false;
        snapshot.in_flight.push(snapshot.in_flight[0].clone());
        assert!(snapshot.reconcile_tool_observation(true,"could be applied").is_err());
        assert_eq!(snapshot.receipts.len(),0);
    }
    #[test]
    fn edit_intent_requires_actual_writes_in_any_supported_language() {
        for text in ["README 수정해라", "파일 작성", "코드 리팩토링", "fix the unit test", "implement tool routing"] {
            assert!(owner_requires_effect(text), "{text}");
        }
        for text in ["인사를 해줘", "현재 상황을 설명해줘", "summarize the known context"] {
            assert!(!owner_requires_effect(text), "{text}");
        }
        for text in ["README 수정해라", "코드를 구현해라", "fix the unit test"] {
            assert!(owner_requires_file_mutation(text), "{text}");
        }
        for text in ["실행해라", "배포해라", "Run cargo test", "open the browser",
            "기존 파일 읽어줘", "create a calendar event"] {
            assert!(!owner_requires_file_mutation(text), "{text}");
        }
    }
}
