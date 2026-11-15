//! Adapter from DOMINION's reusable Rust SDK harness to real Rust engine IO.
//! It reuses model routing, memory, tool/approval execution and SQLite; no
//! vendor SDK or shadow executor owns workflow state.
use crate::{engine::{self, chat::TurnRequest, jobs::JobContext, llm, verification}, state::AppState};
use dominion_sdk::harness::{self, Action, BoxFuture, Checkpoint, Driver, Observation, Policy, Task, Verification};
use serde_json::{json, Value};

struct NativeDriver {
    context: JobContext,
    input: Value,
    session_id: String,
    first_turn: bool,
    owner_instruction: String,
}
impl NativeDriver {
    fn route(&self) -> (&str, &str) {
        (self.input["providerId"].as_str().unwrap_or(""),
         self.input["model"].as_str().unwrap_or(""))
    }
    fn request(&self, directive: &str) -> TurnRequest {
        TurnRequest {
            session_id: Some(self.session_id.clone()),
            input: self.owner_instruction.clone(),
            attachment_ids: Vec::new(),
            agent_id: self.input["agentId"].as_str().map(str::to_owned),
            provider_id: self.input["providerId"].as_str().map(str::to_owned),
            model: self.input["model"].as_str().map(str::to_owned),
            tools_enabled: self.input["toolsEnabled"].as_bool().unwrap_or(true),
            tools_execution_enabled: self.input["toolsExecutionEnabled"].as_bool().unwrap_or(true),
            thinking: self.input["thinking"].as_bool(),
            reasoning_effort: self.input["reasoningEffort"].as_str().map(str::to_owned),
            reasoning_language: None,
            auto_learning: false,
            // Appends to the normal agent/system instructions; does not replace them.
            system_override: Some(directive.to_string()),
            task_id: None,
            parent_task_id: None,
            task_revision: None,
            // Rust SDK performs final deterministic verification, rather than
            // spending tokens on a second independent chat reviewer.
            verification_owner: Some("parent".into()),
        }
    }
}
impl Driver for NativeDriver {
    fn plan<'a>(&'a mut self, task: &'a Task, max_steps: usize) -> BoxFuture<'a, Vec<String>> {
        Box::pin(async move {
            let owner = task.instruction.trim();
            let complex = owner.len() > 240 || owner.contains('\n')
                || ["그리고", "다음", "후에", "then", "after", ";", "단계"]
                    .iter().any(|word| owner.contains(word));
            if !complex { return Ok(vec![]); }
            let (provider, model) = self.route();
            let system = format!(
                "You are a task planner, not an executor. Preserve the user's exact intent, \
                 constraints and requested output. Return ONLY JSON {{\"steps\":[\"...\",...]}} \
                 with at most {max_steps} concrete ordered execution phases. \
                 Prefer a short plan. Include verification of artifacts in the final phase. \
                 Do not invent additional user goals or approvals."
            );
            let raw = llm::complete_for_turn(&self.context.state, provider, model,
                &system, owner, Some(&self.session_id)).await.unwrap_or_default();
            let parsed = raw.find('{').zip(raw.rfind('}'))
                .and_then(|(start, end)| (start <= end).then_some((start,end)))
                .and_then(|(start,end)| serde_json::from_str::<Value>(&raw[start..=end]).ok());
            Ok(parsed.as_ref().and_then(|v| v["steps"].as_array())
                .map(|items| items.iter().filter_map(Value::as_str)
                    .take(max_steps).map(str::to_owned).collect()).unwrap_or_default())
        })
    }
    fn execute<'a>(&'a mut self, action: &'a Action) -> BoxFuture<'a, Observation> {
        Box::pin(async move {
            let request = self.request(&action.prompt);
            // One and only one owner-authored user message. Later iterations
            // resume the same transcript without synthesizing user messages.
            let (message, stats) = if self.first_turn {
                self.first_turn = false;
                engine::run_quiet_outcome(&self.context.state, &request).await
            } else {
                engine::run_quiet_resume_outcome(&self.context.state, &request).await
            };
            let message = message?.ok_or("Rust turn returned no final message")?;
            Ok(Observation {
                answer: message["content"].as_str().unwrap_or("").to_string(),
                message_id: message["id"].as_str().map(str::to_owned),
                evidence: stats.evidence, tool_errors: stats.tool_errors
            })
        })
    }
    fn verify<'a>(&'a mut self, task: &'a Task, evidence: &'a [Value]) -> BoxFuture<'a, Verification> {
        Box::pin(async move {
            let gaps = verification::check_acceptance(
                &self.context.state, &task.instruction, &task.acceptance, evidence
            );
            Ok(Verification { passed: gaps.is_empty(), gaps })
        })
    }
    fn checkpoint<'a>(&'a mut self, cp: &'a Checkpoint) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            self.context.checkpoint(json!({"nativeHarness":cp,"sessionId":self.session_id}))?;
            self.context.emit(json!({
                "type":"native_harness_progress",
                "phase":cp.phase,
                "stepIndex":cp.step_index,
                "stepTotal":cp.step_total,
                "attempt":cp.attempt,
                "inFlight":cp.in_flight
            }));
            Ok(())
        })
    }
    fn cancelled(&self) -> bool { self.context.state.is_cancelled(&self.session_id) }
    fn replan<'a>(&'a mut self, task: &'a Task, cp: &'a Checkpoint) -> BoxFuture<'a, Vec<String>> {
        Box::pin(async move {
            // Only the frozen verifier's remaining gaps may cause a replan.
            if cp.gaps.is_empty() { return Ok(Vec::new()); }
            let (provider, model) = self.route();
            let raw = llm::complete_for_turn(
                &self.context.state, provider, model,
                "You are a repair planner. Return JSON {\"steps\":[\"...\"]} with 1 short \
                 action to address the exact verification gaps. Do not add new goals.",
                &format!("Owner request: {}\n\nVerified gaps: {}", task.instruction,
                    cp.gaps.iter().take(8).cloned().collect::<Vec<_>>().join("; ")),
                Some(&self.session_id),
            ).await.unwrap_or_default();
            let plan = raw.find('{').zip(raw.rfind('}'))
                .and_then(|(start,end)| (start <= end).then_some((start,end)))
                .and_then(|(start,end)| serde_json::from_str::<Value>(&raw[start..=end]).ok());
            Ok(plan.as_ref().and_then(|v| v["steps"].as_array())
                .and_then(|items| items.first()).and_then(Value::as_str)
                .map(|step| vec![step.to_string()]).unwrap_or_default())
        })
    }
}

/// Called by the existing durable Rust job worker. Successful work passes
/// through the same native SDK core used by other embedders.
pub async fn run(context: &JobContext, job: &Value) -> Result<Value, String> {
    let input = job["input"].clone();
    let instruction = input["prompt"].as_str().unwrap_or("").trim().to_string();
    if instruction.is_empty() { return Err("작업 입력(prompt)이 비어 있습니다.".into()); }
    let existing_session = job["sessionId"].as_str()
        .or_else(|| input["sessionId"].as_str())
        .filter(|id| context.state.store.get("session", id).is_some())
        .map(str::to_owned);
    let session_id = match existing_session {
        Some(id) => id,
        None => context.state.store.put("session", &json!({
            "title": instruction.chars().take(60).collect::<String>(),
            "agentId":input.get("agentId"),
            "kind":"native_harness"
        })).and_then(|v| v["id"].as_str().map(str::to_owned))
            .ok_or("Rust 하네스 세션을 저장하지 못했습니다.")?
    };
    let saved_contract = job.get("nativeHarnessContract").filter(|v| !v.is_null());
    let contract = if let Some(contract) = saved_contract { contract.clone() }
        else if let Some(explicit) = input.get("acceptanceChecks").filter(|v| !v.is_null()) {
            verification::freeze_criteria(&context.state, explicit)?
        } else {
            verification::prepare_acceptance_checks(&context.state, &instruction)
        };
    // Capture baseline, session and owner intent before tool side effects.
    context.checkpoint(json!({"sessionId":session_id,"nativeHarnessContract":contract}))?;
    let restored: Option<Checkpoint> = job.get("nativeHarness")
        .filter(|v| v.is_object()).and_then(|v| serde_json::from_value(v.clone()).ok());
    let owner_message_exists = context.state.store.messages(&session_id).iter().any(|message| {
        message["role"] == "user" && message["metadata"]["authorKind"] == "owner"
    });
    let policy = Policy {
        max_attempts_per_step: input["maxAttempts"].as_u64().unwrap_or(3) as usize,
        max_steps: input["maxSteps"].as_u64().unwrap_or(5) as usize,
        plan_enabled: input["planning"].as_bool().unwrap_or(true)
    };
    let task = Task { instruction: instruction.clone(),
        acceptance: contract, session_id: session_id.clone() };
    let mut driver = NativeDriver {
        context: context.clone(), input, session_id: session_id.clone(),
        first_turn: !owner_message_exists, owner_instruction: instruction
    };
    let result = harness::run(&mut driver, &task, policy, restored).await?;
    Ok(json!({
        "status":result.status, "sessionId":session_id,
        "steps":result.checkpoint.step_total,
        "attempts":result.checkpoint.history.len(),
        "messageId":result.checkpoint.last_message_id,
        "verificationGaps":result.gaps
    }))
}
