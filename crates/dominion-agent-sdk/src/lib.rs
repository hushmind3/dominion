//! DOMINION's model-independent Rust agent SDK.
//!
//! The kernel owns planning, tool dispatch order, limits, independent evidence,
//! bounded replanning, progress, and durable write-ahead checkpoints.
//! The host supplies providers, tools, and storage; there is no imported harness.

use futures_util::future::join_all;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::{HashMap, HashSet}, future::Future, pin::Pin};

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Strategy { #[default] Balanced, VerifyFirst, LowCost }

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Limits {
    pub max_steps: usize,
    pub max_model_calls: usize,
    pub max_estimated_tokens: u64,
    pub max_parallel_reads: usize,
    pub max_result_chars: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self { max_steps: 24, max_model_calls: 32, max_estimated_tokens: 100_000,
            max_parallel_reads: 4, max_result_chars: 3000 }
    }
}
impl Limits {
    pub fn validate(&self) -> Result<(), String> {
        if !(1..=200).contains(&self.max_steps)
            || !(1..=250).contains(&self.max_model_calls)
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
pub enum CriterionKind { FileExists, FileAbsent, FileContains, ToolSucceeded }
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
            || self.instruction.trim().is_empty() || self.instruction.len() > 32_000
            || self.owner_instructions.len() > 32_000
            || self.allowed_tools.len() > 64 || self.checks.len() > 32 {
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

pub trait Model: Send + Sync {
    fn complete<'a>(&'a self, system: &'a str, prompt: &'a str)
        -> BoxFuture<'a, Result<ModelReply, String>>;
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
}
impl Snapshot {
    pub fn from_request(request: &RunRequest) -> Self {
        Self { id: request.id.clone(), status: Status::Running,
            instruction: request.instruction.clone(), plan: vec![], step: 0,
            model_calls: 0, charged_tokens: 0, receipts: vec![], checks: vec![],
            answer: None, error: None, in_flight: vec![], model_in_flight: false }
    }
    fn stop(&mut self, status: Status, reason: impl Into<String>) {
        self.status = status;
        self.error = Some(reason.into());
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
fn shortened(value: &Value, max_chars: usize) -> String {
    let content = value.to_string();
    let total = content.chars().count();
    if total <= max_chars { return content; }
    let head: String = content.chars().take(max_chars).collect();
    format!("{head} ... [truncated {total} chars; full receipt preserved]")
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
    if decision.plan.len() > 32 || decision.plan.iter().any(|item| item.len() > 1000) {
        return Err("Plan exceeds maximum structure size".into());
    }
    Ok(decision)
}

impl Kernel<'_> {
    pub async fn run(&self, request: RunRequest) -> Result<Snapshot, String> {
        request.validate()?;
        let mut run = Snapshot::from_request(&request);
        let specs = self.tools.definitions();
        let mut tools = HashMap::new();
        for spec in specs {
            if !request.allowed_tools.contains(&spec.name) {
                continue;
            }
            tools.insert(spec.name.clone(), spec);
        }
        let mut sorted_tools = tools.values().collect::<Vec<_>>();
        sorted_tools.sort_by(|a,b|a.name.cmp(&b.name));
        let definitions: Vec<Value> = sorted_tools.into_iter().map(|spec| json!({
            "name":spec.name,
            "description":if matches!(request.strategy,Strategy::LowCost) {
                spec.description.chars().take(120).collect::<String>()
            } else {spec.description.clone()},
            "parameters":spec.schema, "readOnly":spec.read_only
        })).collect();
        let system = format!(
            "You are DOMINION's model planner. The Rust kernel owns action dispatch, \
            limits, permission checks, and verification. Respect the owner's instruction \
            and never claim an unobserved task is done. Reply with exactly one JSON object: \
            {{\"plan\":[\"concise ordered steps\"],\"actions\":[{{\"id\":\"unique-id\",\"tool\":\"name\",\
            \"arguments\":{{}},\"dependsOn\":[]}}],\"answer\":null}}. \
            On task completion return actions=[] and answer=\"actual result\". \
            Only call tools from this allowed catalog: {}. \
            Strategy: {:?}. Owner instructions: {}.",
            serde_json::to_string(&definitions).map_err(|e| e.to_string())?,
            request.strategy, request.owner_instructions
        );
        let mut seen = HashSet::<String>::new();
        let mut invalid = 0usize;
        self.journal.save(&run)?;
        'steps: for step in 0..request.limits.max_steps {
            if self.journal.cancelled(&request.id) {
                run.stop(Status::Cancelled, "Owner cancelled execution");
                self.journal.save(&run)?;
                return Ok(run);
            }
            if run.model_calls >= request.limits.max_model_calls {
                break;
            }
            run.step = step + 1;
            // Route policy adjusts actual prompt and observation cost, not
            // just a strategy label. LowCost reduces repeated observations.
            let (tail_count,detail_chars)=match request.strategy {
                Strategy::LowCost=>(6,request.limits.max_result_chars.min(1000)),
                Strategy::VerifyFirst=>(16,request.limits.max_result_chars),
                Strategy::Balanced=>(12,request.limits.max_result_chars),
            };
            let tail: Vec<Value> = run.receipts.iter().rev().take(tail_count).rev()
                .map(|r| json!({"id":r.id,"tool":r.tool,"success":r.success,
                    "result":shortened(&r.result,detail_chars)}))
                .collect();
            let current_directive=self.journal.directive(&request.id)?;
            let prompt = json!({
                "userTask":request.instruction,
                "latestOwnerDirective":current_directive, "ownerInstructions":request.owner_instructions,
                "currentPlan":run.plan, "recentObservations":tail,
                "verificationGaps":run.checks.iter().filter(|v| !v.satisfied)
                    .map(|v| &v.explanation).collect::<Vec<_>>(),
                "remainingSteps":request.limits.max_steps-step,
                "modelBudgetRemaining":request.limits.max_model_calls-run.model_calls
            }).to_string();
            // Latest owner instruction is reloaded as a higher-priority
            // instruction on every turn; stale models cannot hide it in a
            // lower-priority tool observation.
            let active_system=if let Some(directive)=current_directive.as_deref() {
                format!("{system}\nLATEST OWNER DIRECTIVE (overrides earlier task plans): {directive}")
            }else{system.clone()};
            // Model response generation is bounded by the user budget, too.
            let estimated = estimate_tokens(&active_system) + estimate_tokens(&prompt) + 1024;
            if run.charged_tokens.saturating_add(estimated) > request.limits.max_estimated_tokens {
                run.stop(Status::NeedsReview, "Model token budget exhausted");
                self.journal.save(&run)?;
                return Ok(run);
            }
            run.model_in_flight = true;
            self.journal.save(&run)?;
            let reply = match self.model.complete(&active_system, &prompt).await {
                Ok(reply) => reply,
                Err(error) => {
                    run.stop(Status::NeedsReview, format!("Model unavailable: {error}"));
                    self.journal.save(&run)?;
                    return Ok(run);
                }
            };
            run.model_calls += 1;
            // Persist the fact that generation ended before deciding next actions.
            // If this checkpoint fails, no actions are dispatched.
            run.model_in_flight = false;
            run.charged_tokens = run.charged_tokens.saturating_add(
                reply.input_tokens.unwrap_or_else(|| estimate_tokens(&active_system) + estimate_tokens(&prompt))
                    .saturating_add(reply.output_tokens.unwrap_or_else(|| estimate_tokens(&reply.content)))
            );
            self.journal.save(&run)?;
            let decision = match decode(&reply.content) {
                Ok(d) => { invalid = 0; d },
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
                    run.checks = self.verify(&request, &run).await;
                    let requires_file_mutation = request.instruction.to_ascii_lowercase()
                        .split_whitespace().any(|word| matches!(word, "edit" | "write" | "create" | "modify" | "fix" | "implement" | "delete"));
                    let has_nonread_effect = run.receipts.iter().any(|r| r.success &&
                        tools.get(&r.tool).is_some_and(|tool| !tool.read_only));
                    let evidence_sufficient = !request.require_evidence
                        || (!request.checks.is_empty() && run.checks.iter().all(|c| c.satisfied))
                        || (run.receipts.iter().any(|r| r.success)
                            && (!requires_file_mutation || has_nonread_effect));
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
                run.stop(Status::NeedsReview, "Model requested too many tool actions in one step");
                self.journal.save(&run)?;
                return Ok(run);
            }
            let mut batch: Vec<Action> = Vec::new();
            for action in decision.actions {
                if action.id.trim().is_empty() || action.id.len() > 128
                    || !seen.insert(action.id.clone()) {
                    run.stop(Status::NeedsReview, "Invalid or duplicate tool operation id");
                    self.journal.save(&run)?;
                    return Ok(run);
                }
                if !tools.contains_key(&action.tool) {
                    run.stop(Status::NeedsReview, format!("Tool not authorized: {}", action.tool));
                    self.journal.save(&run)?;
                    return Ok(run);
                }
                if action.depends_on.iter().any(|dep| !run.receipts.iter().any(|r| &r.id == dep && r.success)) {
                    run.stop(Status::NeedsReview, format!("Missing successful dependency for {}", action.id));
                    self.journal.save(&run)?;
                    return Ok(run);
                }
                batch.push(action);
            }
            // Do not run a later operation while a prior write's outcome is unknown.
            let mut cursor = 0;
            while cursor < batch.len() {
                // Check again just before each tool batch. A correction made
                // after planning cannot authorize stale commands, even if an
                // earlier read batch has already completed.
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
                let mut end = cursor + 1;
                if tools[&batch[cursor].tool].read_only {
                    while end < batch.len() && end-cursor < request.limits.max_parallel_reads
                        && tools[&batch[end].tool].read_only {
                        end += 1;
                    }
                }
                let selected = batch[cursor..end].to_vec();
                run.in_flight = selected.clone();
                // Write-ahead receipt: fail closed if SQLite cannot commit.
                self.journal.save(&run)?;
                let results = join_all(selected.iter().map(|action| self.tools.execute(action))).await;
                for (action, outcome) in selected.iter().zip(results) {
                    let (success, result) = match outcome {
                        Ok(data) => (true, data),
                        Err(error) => (false, json!({"error":error}))
                    };
                    run.receipts.push(Receipt {
                        id:action.id.clone(), tool:action.tool.clone(), arguments:action.arguments.clone(),
                        success, result, step:run.step
                    });
                }
                run.in_flight.clear();
                self.journal.save(&run)?;
                if matches!(request.strategy,Strategy::VerifyFirst) && !request.checks.is_empty() {
                    // Feedback is based on observed filesystem/tool receipts,
                    // not the model grading its own optimistic answer.
                    run.checks=self.verify(&request,&run).await;
                    self.journal.save(&run)?;
                }
                cursor = end;
            }
        }
        run.stop(Status::NeedsReview, "Step, model or decision-repair limit reached");
        self.journal.save(&run)?;
        Ok(run)
    }

    async fn verify(&self, request: &RunRequest, run: &Snapshot) -> Vec<CheckResult> {
        let mut checks = Vec::with_capacity(request.checks.len());
        for criterion in &request.checks {
            let observation = if criterion.kind == CriterionKind::ToolSucceeded {
                let name = criterion.tool.as_deref().unwrap_or("");
                Ok(run.receipts.iter().any(|r| r.tool == name && r.success))
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
