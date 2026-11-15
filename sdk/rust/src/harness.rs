//! Reusable native DOMINION orchestration core.
//! No model vendor, HTTP transport, server app, GUI, or external agent SDK.
//! A Driver supplies host IO; DOMINION controls plans, retries, proofs and checkpoints.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{future::Future, pin::Pin};

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, String>> + Send + 'a>>;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    /// Original owner instruction; never replaced by a model-created goal.
    pub instruction: String,
    /// Deterministic checks frozen before the first effect.
    #[serde(default)]
    pub acceptance: Value,
    #[serde(default)]
    pub session_id: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Policy {
    pub max_attempts_per_step: usize,
    pub plan_enabled: bool,
    pub max_steps: usize,
}
impl Default for Policy {
    fn default() -> Self { Self { max_attempts_per_step: 3, plan_enabled: true, max_steps: 5 } }
}
impl Policy {
    pub fn bounded(mut self) -> Self {
        self.max_attempts_per_step = self.max_attempts_per_step.clamp(1, 6);
        self.max_steps = self.max_steps.clamp(1, 12);
        self
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Action {
    pub step: usize,
    pub attempt: usize,
    pub instruction: String,
    pub prompt: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Observation {
    pub answer: String,
    pub message_id: Option<String>,
    #[serde(default)] pub evidence: Vec<Value>,
    #[serde(default)] pub tool_errors: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Verification {
    pub passed: bool,
    #[serde(default)] pub gaps: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attempt {
    pub step: usize,
    pub attempt: usize,
    pub message_id: Option<String>,
    pub tool_errors: Vec<String>,
    pub verification_gaps: Vec<String>,
    pub passed: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Phase { Planning, Executing, Verifying, Repairing, Completed, NeedsReview, Cancelled }
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Checkpoint {
    pub phase: Phase,
    pub step_index: usize,
    pub step_total: usize,
    pub attempt: usize,
    /// A crash during external effects must never silently replay a tool.
    pub in_flight: bool,
    pub plan: Vec<String>,
    pub history: Vec<Attempt>,
    pub evidence: Vec<Value>,
    pub last_message_id: Option<String>,
    #[serde(default)] pub gaps: Vec<String>,
}
impl Checkpoint {
    pub fn initial() -> Self {
        Self { phase: Phase::Planning, step_index: 0, step_total: 0,
            attempt: 0, in_flight: false, plan: Vec::new(), history: Vec::new(),
            evidence: Vec::new(), last_message_id: None, gaps: Vec::new() }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Outcome {
    pub status: String,
    pub checkpoint: Checkpoint,
    pub gaps: Vec<String>,
}

/// The only IO boundary. The host may use existing Rust model routing,
/// authorized tools, SQLite, MCP and RSI. It must not implement another
/// completion/repair loop; this core owns the orchestration.
pub trait Driver: Send {
    fn plan<'a>(&'a mut self, task: &'a Task, max_steps: usize) -> BoxFuture<'a, Vec<String>>;
    fn execute<'a>(&'a mut self, action: &'a Action) -> BoxFuture<'a, Observation>;
    fn verify<'a>(&'a mut self, task: &'a Task, evidence: &'a [Value]) -> BoxFuture<'a, Verification>;
    fn checkpoint<'a>(&'a mut self, snapshot: &'a Checkpoint) -> BoxFuture<'a, ()>;
    fn cancelled(&self) -> bool;
    fn replan<'a>(&'a mut self, _task: &'a Task, _snapshot: &'a Checkpoint) -> BoxFuture<'a, Vec<String>> {
        Box::pin(async { Ok(Vec::new()) })
    }
}
fn normalize(steps: Vec<String>, limit: usize) -> Vec<String> {
    let mut accepted = Vec::new();
    for step in steps {
        let step = step.trim();
        if step.is_empty() { continue; }
        let step = step.chars().take(1000).collect::<String>();
        if accepted.contains(&step) { continue; }
        accepted.push(step);
        if accepted.len() >= limit { break; }
    }
    accepted
}
fn outcome(status: &str, cp: Checkpoint) -> Outcome {
    Outcome { status: status.into(), gaps: cp.gaps.clone(), checkpoint: cp }
}
fn make_action(task: &Task, cp: &Checkpoint) -> Action {
    let mut prompt = format!(
        "Original user instruction (authoritative):\n{}\n\nCurrent step {}/{}:\n{}\n\n\
         Work only toward the owner's original outcome. Execute real tools when needed; \
         do not claim completion without evidence.",
        task.instruction, cp.step_index+1, cp.plan.len(),
        cp.plan.get(cp.step_index).map(String::as_str).unwrap_or(&task.instruction)
    );
    if !cp.gaps.is_empty() {
        prompt.push_str("\n\nRust verifier reported remaining gaps:\n");
        for gap in cp.gaps.iter().take(12) {
            prompt.push_str("- "); prompt.push_str(&gap.chars().take(400).collect::<String>()); prompt.push('\n');
        }
        prompt.push_str("Correct these exact gaps. Do not repeat a side effect with unknown outcome.");
    }
    Action { step: cp.step_index, attempt: cp.attempt,
        instruction: task.instruction.clone(), prompt }
}

/// Restarts from the last safe checkpoint. A checkpoint written before an
/// effect is a barrier: interrupted effects require owner review, not replay.
pub async fn run<D: Driver>(
    driver: &mut D, task: &Task, policy: Policy, previous: Option<Checkpoint>
) -> Result<Outcome, String> {
    if task.instruction.trim().is_empty() { return Err("Empty user instruction".into()); }
    let policy = policy.bounded();
    let mut cp = previous.unwrap_or_else(Checkpoint::initial);
    if cp.in_flight {
        cp.phase = Phase::NeedsReview;
        cp.gaps = vec!["Interrupted external effect: inspect tool evidence before replay".into()];
        driver.checkpoint(&cp).await?;
        return Ok(outcome("needs_review", cp));
    }
    if matches!(cp.phase, Phase::Completed | Phase::Cancelled | Phase::NeedsReview) {
        let status = match cp.phase {
            Phase::Completed => "completed", Phase::Cancelled => "cancelled", _ => "needs_review"
        };
        return Ok(outcome(status, cp));
    }
    if cp.plan.is_empty() {
        cp.phase = Phase::Planning;
        driver.checkpoint(&cp).await?;
        let proposed = if policy.plan_enabled {
            driver.plan(task, policy.max_steps).await.unwrap_or_default()
        } else { Vec::new() };
        cp.plan = normalize(proposed, policy.max_steps);
        if cp.plan.is_empty() { cp.plan.push(task.instruction.clone()); }
        cp.step_total = cp.plan.len();
        cp.step_index = 0;
        cp.attempt = 0;
        driver.checkpoint(&cp).await?;
    }
    if cp.step_index > cp.plan.len() { return Err("Corrupted step cursor".into()); }
    while cp.step_index < cp.plan.len() {
        if driver.cancelled() {
            cp.phase = Phase::Cancelled;
            driver.checkpoint(&cp).await?;
            return Ok(outcome("cancelled", cp));
        }
        if cp.attempt >= policy.max_attempts_per_step {
            cp.phase = Phase::NeedsReview;
            cp.gaps = vec!["Retry limit reached for this step".into()];
            driver.checkpoint(&cp).await?;
            return Ok(outcome("needs_review", cp));
        }
        let action = make_action(task, &cp);
        // Persist the in-flight barrier BEFORE any tool or model effect.
        cp.phase = if cp.attempt == 0 { Phase::Executing } else { Phase::Repairing };
        cp.in_flight = true;
        driver.checkpoint(&cp).await?;
        let observed = match driver.execute(&action).await {
            Ok(value) => value,
            Err(error) => {
                cp.phase = Phase::NeedsReview;
                cp.gaps = vec![format!("Execution outcome uncertain: {error}")];
                // Keep in_flight=true. This is not an automatic retry.
                driver.checkpoint(&cp).await?;
                return Ok(outcome("needs_review", cp));
            }
        };
        cp.in_flight = false;
        cp.last_message_id = observed.message_id.clone().or(cp.last_message_id);
        cp.evidence.extend(observed.evidence.into_iter().take(100));
        if cp.evidence.len() > 400 { cp.evidence.drain(..cp.evidence.len() - 400); }
        cp.phase = Phase::Verifying;
        driver.checkpoint(&cp).await?;
        // Intermediate steps are not complete merely because an answer exists.
        // Final verification considers all steps' accumulated tool evidence.
        let is_last = cp.step_index + 1 == cp.plan.len();
        let mut gaps = observed.tool_errors.clone();
        if is_last {
            match driver.verify(task, &cp.evidence).await {
                Ok(verified) => {
                    if !verified.passed || !verified.gaps.is_empty() {
                        gaps.extend(verified.gaps);
                        if gaps.is_empty() { gaps.push("Verifier did not approve completion".into()); }
                    }
                }
                Err(error) => gaps.push(format!("Verifier unavailable: {error}")),
            };
        }
        let passed = gaps.is_empty();
        cp.history.push(Attempt {
            step: cp.step_index, attempt: cp.attempt,
            message_id: observed.message_id,
            tool_errors: observed.tool_errors,
            verification_gaps: gaps.clone(), passed
        });
        cp.gaps = gaps;
        cp.attempt += 1;
        driver.checkpoint(&cp).await?;
        if passed {
            cp.step_index += 1;
            cp.attempt = 0;
            cp.gaps.clear();
            driver.checkpoint(&cp).await?;
            continue;
        }
        if cp.attempt >= policy.max_attempts_per_step {
            if cp.plan.len() < policy.max_steps {
                let additional = driver.replan(task, &cp).await.unwrap_or_default();
                let additional = normalize(additional, policy.max_steps - cp.plan.len());
                if !additional.is_empty() {
                    cp.plan.extend(additional);
                    cp.step_total = cp.plan.len();
                    cp.step_index += 1;
                    cp.attempt = 0;
                    driver.checkpoint(&cp).await?;
                    continue;
                }
            }
            cp.phase = Phase::NeedsReview;
            driver.checkpoint(&cp).await?;
            return Ok(outcome("needs_review", cp));
        }
    }
    cp.phase = Phase::Completed;
    driver.checkpoint(&cp).await?;
    Ok(outcome("completed", cp))
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Stub {
        checkpoints: Vec<Checkpoint>, executions: usize, fail_first: bool, interrupted: bool,
    }
    impl Driver for Stub {
        fn plan<'a>(&'a mut self, task: &'a Task, _: usize) -> BoxFuture<'a, Vec<String>> {
            Box::pin(async move { Ok(vec![task.instruction.clone()]) })
        }
        fn execute<'a>(&'a mut self, _: &'a Action) -> BoxFuture<'a, Observation> {
            self.executions += 1;
            let failed = self.fail_first && self.executions == 1;
            Box::pin(async move {
                Ok(Observation { answer: "done".into(), message_id: Some("m".into()),
                    evidence: if failed { vec![] } else { vec![serde_json::json!({"tool":"workspace_write","result":{"ok":true}})] },
                    tool_errors: if failed { vec!["write failed".into()] } else { vec![] } })
            })
        }
        fn verify<'a>(&'a mut self, _: &'a Task, evidence: &'a [Value]) -> BoxFuture<'a, Verification> {
            Box::pin(async move { Ok(Verification { passed: !evidence.is_empty(),
                gaps: if evidence.is_empty() { vec!["No verified write".into()] } else { vec![] } }) })
        }
        fn checkpoint<'a>(&'a mut self, cp: &'a Checkpoint) -> BoxFuture<'a, ()> {
            self.checkpoints.push(cp.clone()); Box::pin(async { Ok(()) })
        }
        fn cancelled(&self) -> bool { self.interrupted }
    }
    fn task() -> Task {
        Task { instruction: "Create report.txt".into(), acceptance: Value::Null, session_id: "test".into() }
    }
    #[tokio::test]
    async fn real_checkpoint_before_effect_and_verifier_guided_repair() {
        let mut driver = Stub { checkpoints: Vec::new(), executions: 0, fail_first: true, interrupted: false };
        let outcome = run(&mut driver, &task(), Policy::default(), None).await.unwrap();
        assert_eq!(outcome.status, "completed");
        assert_eq!(driver.executions, 2);
        assert!(driver.checkpoints.iter().any(|cp| cp.in_flight));
        assert_eq!(outcome.checkpoint.history.len(), 2);
        assert!(!outcome.checkpoint.history[0].passed);
        assert!(outcome.checkpoint.history[1].passed);
    }
    #[tokio::test]
    async fn unknown_side_effect_is_never_repeated_on_restart() {
        let mut driver = Stub { checkpoints: Vec::new(), executions: 0, fail_first: false, interrupted: false };
        let mut cp = Checkpoint::initial();
        cp.plan.push("write file".into()); cp.in_flight = true; cp.phase = Phase::Executing;
        let outcome = run(&mut driver, &task(), Policy::default(), Some(cp)).await.unwrap();
        assert_eq!(outcome.status, "needs_review");
        assert_eq!(driver.executions, 0);
    }
    #[tokio::test]
    async fn cancelled_run_never_calls_tools() {
        let mut driver = Stub { checkpoints: Vec::new(), executions: 0, fail_first: false, interrupted: true };
        let outcome = run(&mut driver, &task(), Policy::default(), None).await.unwrap();
        assert_eq!(outcome.status, "cancelled");
        assert_eq!(driver.executions, 0);
    }
}
