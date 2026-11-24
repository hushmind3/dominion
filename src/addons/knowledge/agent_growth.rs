//! Per-bot growth. The model is chosen by the user and never changed here.
//! What accumulates is the agent's own execution history: success/failure
//! counts, failing tools, recurring failure classes and concrete guidance
//! derived from verified runs. That guidance is injected into the same agent's
//! next turn, so using one bot repeatedly makes that bot better.
use crate::state::AppState;
use serde_json::{json, Value};

fn agent_of(state: &AppState, execution: &Value) -> Option<String> {
    execution["agentId"]
        .as_str()
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| {
            execution["sessionId"].as_str().and_then(|session_id| {
                state
                    .store
                    .get("session", session_id)
                    .and_then(|session| session["agentId"].as_str().map(str::to_string))
            })
        })
}

/// Updates the agent's own growth record after an execution. Never writes to
/// another agent's record and never changes the model.
pub fn record(state: &AppState, execution: &Value) -> Option<Value> {
    let agent_id = agent_of(state, execution)?;
    let mut growth = state
        .store
        .get("agent_growth", &agent_id)
        .unwrap_or_else(|| {
            json!({
                "id": agent_id,
                "agentId": agent_id,
                "runs": 0,
                "succeeded": 0,
                "failed": 0,
                "answeredUnverified": 0,
                "unrated": 0,
                "ratedRuns": 0,
                "toolUses": {},
                "toolFailures": {},
                "failureClasses": {},
                "guidance": [],
                "createdAt": chrono::Utc::now().to_rfc3339(),
            })
        });
    let operations=state.store.execution_operations(execution["id"].as_str().unwrap_or("")).ok()?;
    dominion_algorithms::growth::update(&mut growth,&agent_id,execution,&operations,&chrono::Utc::now().to_rfc3339());
    let saved = state.store.put("agent_growth", &growth)?;
    state.bus.record("agent_growth", &saved);
    Some(saved)
}

/// Offline sweep: analyzes terminal executions that were not finalized (for
/// example after a crash) using only stored records. No chat model is called.
pub fn sweep(state: &AppState) -> usize {
    let mut processed = 0;
    for execution in state.store.all("execution") {
        if execution["growthFinalized"].as_bool() == Some(true) {
            continue;
        }
        if !matches!(
            execution["status"].as_str(),
            Some("completed" | "failed" | "interrupted" | "cancelled")
        ) {
            continue;
        }
        let Some(id) = execution["id"].as_str() else {
            continue;
        };
        let mut growth_record = execution.clone();
        if let Some(agent_id) = agent_of(state, &growth_record) {
            growth_record["agentId"] = json!(agent_id);
            let _ = record(state, &growth_record);
        }
        crate::engine::algorithms::finalize_execution(state, &growth_record);
        if let Some(mut saved) = state.store.get("execution", id) {
            saved["growthFinalized"] = json!(true);
            state.store.put("execution", &saved);
        }
        processed += 1;
    }
    processed
}

pub fn growth(state: &AppState, agent_id: &str) -> Value {
    let mut growth = state.store.get("agent_growth", agent_id).unwrap_or_else(|| {
        json!({
            "agentId": agent_id,
            "runs": 0,
            "succeeded": 0,
            "failed": 0,
            "answeredUnverified": 0,
            "unrated": 0,
            "ratedRuns": 0,
            "successRate": 0.0,
            "guidance": [],
        })
    });
    // Older records counted every terminal run as a failure denominator.
    // Recompute from explicit outcomes so unverified chat never reads as 0%.
    let rated_runs = growth["succeeded"].as_u64().unwrap_or(0)
        + growth["failed"].as_u64().unwrap_or(0);
    growth["ratedRuns"] = json!(rated_runs);
    growth["successRate"] = json!(if rated_runs > 0 {
        growth["succeeded"].as_u64().unwrap_or(0) as f64 / rated_runs as f64
    } else {
        0.0
    });
    growth
}

/// Guidance for this agent only. It is injected into the same agent's prompt,
/// never into another bot's.
pub fn context_block(state: &AppState, agent_id: &str) -> Option<(String, Vec<Value>)> {
    let growth = state.store.get("agent_growth", agent_id)?;
    dominion_algorithms::growth::context_block(&growth)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::db::Store;
    use crate::vault::Vault;

    fn state() -> (AppState, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("dominion-growth-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let store = Store::open(&config.db_path()).unwrap();
        let vault = Vault::open(&config.key_path());
        (AppState::new(config, store, vault), dir)
    }

    fn failed_execution(id: &str, agent: &str) -> Value {
        json!({
            "id": id,
            "sessionId": "s",
            "agentId": agent,
            "taskOutcome": "failed",
            "failureClass": "tool_failed",
            "recoveryHint": "다른 인자를 사용하세요"
        })
    }

    #[test]
    fn repeated_failures_become_guidance_for_that_agent_only() {
        let (state, dir) = state();
        for run in ["r1", "r2"] {
            state.store.put(
                "execution_operation",
                &json!({"id":format!("{run}:1"),"runId":run,"tool":"shell_exec","status":"failed"}),
            );
            record(&state, &failed_execution(run, "agent-a")).unwrap();
        }
        let growth = growth(&state, "agent-a");
        assert_eq!(growth["runs"], 2);
        assert_eq!(growth["failed"], 2);
        assert!(!growth["guidance"].as_array().unwrap().is_empty());
        let (block, items) = context_block(&state, "agent-a").unwrap();
        assert!(block.contains("Agent growth"));
        assert!(block.contains("tool_failed"));
        assert!(!items.is_empty());
        // Another bot never inherits agent-a's private growth.
        assert!(context_block(&state, "agent-b").is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn offline_sweep_grows_a_bot_without_any_model_call() {
        let (state, dir) = state();
        state.store.put(
            "execution",
            &json!({
                "id":"offline-run","sessionId":"s","agentId":"agent-a",
                "status":"completed","taskOutcome":"failed","failureClass":"tool_failed"
            }),
        );
        let processed = sweep(&state);
        assert_eq!(processed, 1);
        let growth = growth(&state, "agent-a");
        assert_eq!(growth["runs"], 1);
        assert_eq!(growth["failed"], 1);
        let execution = state.store.get("execution", "offline-run").unwrap();
        assert_eq!(execution["growthFinalized"], true);
        assert_eq!(sweep(&state), 0, "sweep is idempotent");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn unverified_chat_is_unrated_and_does_not_report_zero_percent() {
        let (state, dir) = state();
        for index in 0..10 {
            record(
                &state,
                &json!({
                    "id": format!("chat-{index}"),
                    "sessionId": "s",
                    "agentId": "agent-a",
                    "taskOutcome": "answered_unverified"
                }),
            )
            .unwrap();
        }
        let growth = growth(&state, "agent-a");
        assert_eq!(growth["runs"], 10);
        assert_eq!(growth["failed"], 0, "{growth}");
        assert_eq!(growth["unrated"], 10, "{growth}");
        assert_eq!(growth["ratedRuns"], 0, "{growth}");
        assert!(context_block(&state,"agent-a").is_none(),
            "unverified chat counters must not be injected into the next model prompt");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn stale_failure_details_on_nonfailed_turns_never_become_guidance() {
        let (state, dir) = state();
        for (index, outcome) in [
            "answered_unverified", "cancelled", "answered_unverified", "cancelled"
        ].into_iter().enumerate() {
            record(&state, &json!({
                "id":format!("legacy-{index}"),"sessionId":"s","agentId":"agent-a",
                "taskOutcome":outcome,"failureClass":"legacy_false_failure",
                "recoveryHint":"do not repeat a normal conversation"
            })).unwrap();
        }
        let result=growth(&state,"agent-a");
        assert_eq!(result["failed"],0);
        assert!(result["failureClasses"].get("legacy_false_failure").is_none(),"{result}");
        assert!(result["guidance"].as_array().unwrap().is_empty(),"{result}");
        assert!(context_block(&state,"agent-a").is_none());
        let _=std::fs::remove_dir_all(dir);
    }

    #[test]
    fn preexisting_unproven_failure_guidance_is_quarantined_without_deletion() {
        let (state,dir)=state();
        state.store.put("agent_growth",&json!({
            "id":"agent-a","agentId":"agent-a","runs":9,
            "guidance":[
                {"id":"failure:agent-a:execution_error","revision":"legacy",
                    "text":"STALE_FALSE_ERROR_ADVICE","confidence":0.9,"evidenceCount":9},
                {"id":"tool:agent-a:workspace_read","revision":"legacy-tool",
                    "text":"READ_TOOL_RECEIPT_ADVICE","confidence":0.8,"evidenceCount":3}
            ]
        })).unwrap();
        let (block,_items)=context_block(&state,"agent-a").unwrap();
        assert!(!block.contains("STALE_FALSE_ERROR_ADVICE"),"{block}");
        assert!(block.contains("READ_TOOL_RECEIPT_ADVICE"),"{block}");
        let durable=state.store.get("agent_growth","agent-a").unwrap();
        assert_eq!(durable["guidance"].as_array().unwrap().len(),2,
            "read-time quarantine must not delete historical records");
        let _=std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_single_success_still_records_the_bot_run() {
        let (state, dir) = state();
        record(
            &state,
            &json!({"id":"ok","sessionId":"s","agentId":"agent-a","taskOutcome":"succeeded"}),
        )
        .unwrap();
        let growth = growth(&state, "agent-a");
        assert_eq!(growth["succeeded"], 1);
        assert_eq!(growth["successRate"], 1.0);
        let _ = std::fs::remove_dir_all(dir);
    }
}
