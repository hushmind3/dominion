//! Section 11-1 of design 2.4: observation -> adaptation inside the running
//! loop. A failing tool call is recorded as an observation with a hypothesis
//! and a changed-action intervention. The runtime also blocks an unchanged
//! repeat of that failed call so the model has to pick a different action.
use crate::state::AppState;
use serde_json::{json, Value};

/// Records a tool failure and the adaptation it implies. Bounded so a long run
/// cannot grow the execution record without limit.
pub fn record_tool_failure(
    state: &AppState,
    execution_id: &str,
    round: usize,
    call_id: &str,
    tool: &str,
    arguments: &Value,
    error: &str,
) -> Option<Value> {
    let mut execution = state.store.get("execution", execution_id)?;
    let digest = crate::engine::memory::stable_id(&[tool, &arguments.to_string()]);
    let adaptation = json!({
        "id": format!(
            "adaptation:{}",
            crate::engine::memory::stable_id(&[execution_id, call_id, &digest])
        ),
        "runId": execution_id,
        "stepId": round,
        "operationId": call_id,
        "observation": {
            "kind": "tool_failure",
            "tool": tool,
            "argsDigest": digest,
            "expected": "success",
            "actual": error.chars().take(600).collect::<String>(),
        },
        "hypothesis": {
            "claim": "the same call will repeat the same failure",
            "evidenceIds": [call_id],
        },
        "intervention": {
            "change": "different tool, arguments, or approach",
            "instruction": "do not repeat the identical failed call; choose a changed action or explain the blocker",
        },
        "createdAt": chrono::Utc::now().to_rfc3339(),
    });
    let mut adaptations = execution["adaptations"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    adaptations.push(adaptation.clone());
    if adaptations.len() > 20 {
        adaptations = adaptations.split_off(adaptations.len() - 20);
    }
    execution["adaptations"] = json!(adaptations);
    execution["lastAdaptation"] = adaptation.clone();
    let saved = state.store.put("execution", &execution)?;
    state.bus.record("execution", &saved);
    Some(adaptation)
}

pub use dominion_algorithms::adaptation::{
    repeated_failure_message,adaptation_prompt,parse_adaptation,validate_adaptation,
    choose_intervention_with_context,evaluate_hypotheses_phased,evaluate_hypotheses,
};

#[allow(clippy::too_many_arguments)]
pub fn record_adaptation(
    state: &AppState,
    execution_id: &str,
    round: usize,
    failed_tool: &str,
    failed_arguments: &Value,
    error: &str,
    adaptation: &Value,
    selected: &Value,
    environment_version: u64,
    result: Option<&Value>,
    hypothesis_results: &[Value],
) -> Option<Value> {
    let mut execution = state.store.get("execution", execution_id)?;
    let failure_signature = crate::engine::memory::stable_id(&[
        failed_tool,
        &failed_arguments.to_string(),
    ]);
    let record = json!({
        "id": format!(
            "adaptation:{}",
            crate::engine::memory::stable_id(&[execution_id, &failure_signature, &selected["id"].as_str().unwrap_or("")])
        ),
        "runId": execution_id,
        "stepId": round,
        "failureSignature": failure_signature,
        "environmentVersion": environment_version,
        "observation": {
            "kind": "tool_failure",
            "tool": failed_tool,
            "arguments": failed_arguments,
            "expected": "success",
            "actual": error.chars().take(600).collect::<String>(),
        },
        "beforeObservation": adaptation["beforeObservation"],
        "hypotheses": adaptation["hypotheses"],
        "interventions": adaptation["interventions"],
        "diagnostics": adaptation["diagnostics"],
        "diagnosticEvidence": adaptation["diagnosticEvidence"],
        "acceptanceDecision": adaptation["acceptanceDecision"],
        "environmentFingerprint": adaptation["environmentFingerprint"],
        "consultedExperienceRules": adaptation["consultedExperienceRules"],
        "selected": selected,
        "result": result,
        "hypothesisResults": hypothesis_results,
        "createdAt": chrono::Utc::now().to_rfc3339(),
    });
    let mut adaptations = execution["adaptations"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    adaptations.push(record.clone());
    if adaptations.len() > 20 {
        adaptations = adaptations.split_off(adaptations.len() - 20);
    }
    execution["adaptations"] = json!(adaptations);
    execution["lastAdaptation"] = record.clone();
    execution["adaptationCheckpoint"] = json!({
        "environmentVersion": environment_version,
        "failureSignature": record["failureSignature"],
        "selectedIntervention": selected["id"],
        "selectedEvaluation": selected["evaluation"],
        "environmentFingerprint": adaptation["environmentFingerprint"],
        "adaptationRevision": execution["adaptationCheckpoint"]["adaptationRevision"]
            .as_u64().unwrap_or(0) + 1,
        "beforeObservation": adaptation["beforeObservation"],
        "hypothesisResults": hypothesis_results,
        // Full evidence remains in the bounded adaptation history and tool
        // receipts; the checkpoint needs only the selection cursor.
    });
    let saved = state.store.put("execution", &execution)?;
    state.bus.record("execution", &saved);
    Some(record)
}

#[cfg(test)]
mod selection_tests {
    use super::*;

    fn adaptation() -> Value {
        json!({
            "hypotheses":[
                {"id":"h1","claim":"permission","predictions":[{"predicate":{"type":"equals","from":"result","path":"/exitCode","value":0}}]},
                {"id":"h2","claim":"wrong path","predictions":[{"predicate":{"type":"exists","from":"result","path":"/missing"}}]}
            ],
            "interventions":[
                {"id":"same","hypothesisIds":["h1"],"tool":"shell_exec","inputs":{"command":"exit 7"},"expectedChecks":["x"],"estimatedCost":0.1},
                {"id":"change","hypothesisIds":["h1","h2"],"tool":"workspace_write","inputs":{"path":"a.txt","content":"ok"},"expectedChecks":["a","b"],"estimatedCost":0.2}
            ]
        })
    }

    #[test]
    fn identical_failed_call_is_filtered_until_the_environment_changes() {
        let failed = json!({"command":"exit 7"});
        let chosen = choose_intervention_with_context(&adaptation(), "shell_exec", &failed, false, &json!({}), &[]).unwrap();
        assert_eq!(chosen["id"], "change");
        let retry = choose_intervention_with_context(&adaptation(), "shell_exec", &failed, true, &json!({}), &[]).unwrap();
        assert_eq!(retry["id"], "change"); // still higher score than identical retry
        let only_same = json!({"hypotheses":[],"interventions":[adaptation()["interventions"][0].clone()]});
        assert!(choose_intervention_with_context(&only_same, "shell_exec", &failed, false, &json!({}), &[]).is_none());
        assert!(choose_intervention_with_context(&only_same, "shell_exec", &failed, true, &json!({}), &[]).is_some());
    }

    #[test]
    fn nondominated_repair_selection_does_not_trust_model_progress_claims() {
        let mut scenario=adaptation();
        scenario["interventions"]=json!([
            {"id":"hype","tool":"workspace_write",
             "inputs":{"path":"hype.txt","content":"x"},
             "hypothesisIds":["h1"],"expectedProgress":999999,
             "estimatedCost":3.0},
            {"id":"grounded","tool":"workspace_write",
             "inputs":{"path":"real.txt","content":"x"},
             "hypothesisIds":["h1","h2"],"expectedProgress":0,
             "estimatedCost":0.5}
        ]);
        let result=choose_intervention_with_context(
            &scenario,"shell_exec",&json!({"command":"failed"}),false,
            &json!({}),&[]
        ).unwrap();
        assert_eq!(result["id"],"grounded");
        assert_eq!(result["evaluation"]["selectionPolicy"],"constrained_pareto_2026");
        assert_eq!(result["evaluation"]["paretoFrontierCount"],1);
        assert!(result.get("score").is_none());
    }

    #[test]
    fn false_precondition_excludes_a_mutating_candidate() {
        let mut scenario=adaptation();
        scenario["interventions"]=json!([
            {"id":"bad","tool":"workspace_write","inputs":{"path":"bad"},
             "hypothesisIds":["h1","h2"],"estimatedCost":0,
             "preconditions":[{"type":"equals","from":"result",
                 "path":"/ready","value":true}]},
            {"id":"good","tool":"workspace_list","inputs":{"path":"."},
             "hypothesisIds":["h1"],"estimatedCost":1}
        ]);
        let result=choose_intervention_with_context(
            &scenario,"shell_exec",&json!({}),false,
            &json!({"result":{"ready":false}}),&[],
        ).unwrap();
        assert_eq!(result["id"],"good");
    }

    #[test]
    fn noisy_model_output_is_parsed_and_invalid_schema_is_rejected() {
        let raw = "Here is the plan {not json} then:\n```json\n{\"hypotheses\":[{\"id\":\"h1\",\"claim\":\"missing file\",\"predictions\":[]}],\"interventions\":[{\"id\":\"i1\",\"hypothesisIds\":[\"h1\"],\"tool\":\"workspace_list\",\"inputs\":{},\"expectedChecks\":[\"entries\"],\"estimatedCost\":0.1}]}\n```\nDone.";
        let parsed = parse_adaptation(raw).expect("prose-wrapped JSON");
        assert_eq!(parsed["interventions"][0]["tool"], "workspace_list");
        assert!(validate_adaptation(&parsed).is_ok());
        let invalid = json!({
            "hypotheses":[{"id":"h1","claim":"x","predictions":[]}],
            "interventions":[{"id":"i1","hypothesisIds":["h1"],"tool":"workspace_list","inputs":"not-an-object"}]
        });
        assert!(validate_adaptation(&invalid).is_err());
    }

    #[test]
    fn predictions_refute_the_wrong_hypothesis() {
        let results = evaluate_hypotheses(&adaptation(), &json!({"exitCode": 7}));
        assert_eq!(results[0]["status"], "refuted");
        assert_eq!(results[1]["status"], "refuted");
        let success = evaluate_hypotheses(&adaptation(), &json!({"exitCode": 0, "missing": 1}));
        assert_eq!(success[0]["status"], "supported");
        assert_eq!(success[1]["status"], "supported");
    }
}
