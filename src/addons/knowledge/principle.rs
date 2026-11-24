//! Model-agnostic principles extracted from verified adaptations.
//!
//! A task-specific rule says "after `github_api` failed with 409, call
//! `github_api` read". That is useful but does not transfer. A principle says
//! "before mutating an external resource, observe its current state/version",
//! which applies to GitHub, databases, HTTP APIs and filesystem writes.
//!
//! Principles are global, carry no provider/model gate, and are only created
//! from a verified successful adaptation. Policy promotion is intentionally
//! opt-in and is not performed by this module.
use crate::{engine::experience, state::AppState};
use serde_json::{json, Value};

use dominion_algorithms::principle::{domain,is_mutation,is_observation,observed_success,principle_text,principle_id,synthesize};

pub fn compose(principle:&Value)->Option<Value>{
    dominion_algorithms::principle::compose(principle,&chrono::Utc::now().to_rfc3339())
}

fn union_strings(record: &mut Value, key: &str, value: &str, limit: usize) {
    if value.is_empty() {
        return;
    }
    let mut items = record[key].as_array().cloned().unwrap_or_default();
    if !items.iter().any(|item| item.as_str() == Some(value)) {
        items.push(json!(value));
        items.truncate(limit);
    }
    record[key] = json!(items);
}

/// A model may recover in its ordinary tool loop without invoking the separate
/// adaptation planner. Require all three operations on one typed resource.
fn observed_recoveries(state: &AppState, execution_id: &str) -> Vec<Value> {
    let Ok(operations) = state.store.execution_operations(execution_id) else {
        return Vec::new();
    };
    fn resource(operation: &Value) -> Option<(&str, &str, &str)> {
        let tool = operation["tool"].as_str()?;
        let arguments = &operation["arguments"];
        let (scope, keys): (_, &[&str]) = match tool {
            "github_api" => ("github_api", &["path"]),
            "workspace_read" | "workspace_write" => ("workspace", &["path"]),
            "file_read" | "file_write" => ("file", &["path"]),
            tool if tool.starts_with("mcp__") => {
                (tool.rsplit_once("__")?.0, &["key", "id", "path"])
            }
            _ => return None,
        };
        keys.iter()
            .find_map(|key| arguments[*key].as_str().map(|value| (scope, *key, value)))
    }
    let mut recoveries = Vec::new();
    for (index, failed) in operations.iter().enumerate() {
        let tool = failed["tool"].as_str().unwrap_or_default();
        if failed["status"] != "failed" || !is_mutation(tool, &failed["arguments"]) {
            continue;
        }
        let error = failed["result"]["error"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| failed["result"].to_string());
        if failed["result"]["status"] != 409
            && experience::failure_class(&error) != Some("stale_state")
        {
            continue;
        }
        let Some(target) = resource(failed) else {
            continue;
        };
        let mut observation = None;
        for operation in &operations[index + 1..] {
            if operation["status"] != "completed" || resource(operation) != Some(target) {
                continue;
            }
            let name = operation["tool"].as_str().unwrap_or_default();
            if is_observation(name, &operation["arguments"])
                && observed_success(&operation["result"])
            {
                observation = Some(operation);
            } else if name == tool && is_mutation(name, &operation["arguments"]) {
                if let Some(observation) = observation {
                    recoveries.push(json!({
                        "observation":{"tool":tool,"arguments":failed["arguments"],"actual":format!("stale state: {error}")},
                        "selected":{"tool":observation["tool"],"inputs":observation["arguments"]},
                        "result":observation["result"],
                    }));
                }
                break;
            }
        }
    }
    recoveries
}

/// Stores a structured composition once the principle has independent cases.
/// The composition is never activated here; it only becomes an experiment
/// candidate that the existing promotion path may later evaluate.
fn ensure_composition(state: &AppState, principle: &mut Value) -> bool {
    if principle["strategyCompositionId"].as_str().is_some() {
        return false;
    }
    let Some(composition) = compose(principle) else {
        return false;
    };
    let Some(saved) = state.store.put("strategy_composition", &composition) else {
        return false;
    };
    principle["strategyCompositionId"] = saved["id"].clone();
    principle["strategyComposedAt"] = json!(chrono::Utc::now().to_rfc3339());
    if let Some(candidate_id) = principle["strategyCandidateId"].as_str() {
        if let Some(mut candidate) = state.store.get("improvement", candidate_id) {
            candidate["compositionId"] = saved["id"].clone();
            candidate["compositionRevision"] = saved["parentRevision"].clone();
            state.store.put("improvement", &candidate);
        }
    }
    true
}

/// Learns principles only from a verified successful adaptation. The source
/// tool/domain is evidence, never an applicability requirement.
pub fn learn_from_execution(state: &AppState, execution: &Value) -> usize {
    if execution["taskOutcome"].as_str() != Some("succeeded") {
        return 0;
    }
    if execution["deterministicShortcut"].is_object() {
        return 0;
    }
    // A succeeded task can still contain an unresolved external effect. It is
    // evidence for the task result, not for a reusable positive rule.
    if execution["effectSummary"]["unverifiedEffects"]
        .as_u64()
        .unwrap_or(0)
        > 0
    {
        return 0;
    }
    let Some(execution_id) = execution["id"].as_str() else {
        return 0;
    };
    let _guard = state.principle_learning.lock().unwrap();
    let model = execution["effectiveModel"]
        .as_str()
        .or_else(|| execution["model"].as_str())
        .unwrap_or("");
    let agent = execution["agentId"].as_str().unwrap_or("");
    let mut learned = 0usize;
    let recoveries = observed_recoveries(state, execution_id);
    for adaptation in execution["adaptations"].as_array().into_iter().flatten().take(20)
        .chain(&recoveries)
    {
        let failed_tool = adaptation["observation"]["tool"].as_str().unwrap_or("");
        let selected_tool = adaptation["selected"]["tool"].as_str().unwrap_or("");
        if !is_mutation(failed_tool, &adaptation["observation"]["arguments"])
            || !is_observation(selected_tool, &adaptation["selected"]["inputs"])
        {
            continue;
        }
        if !observed_success(&adaptation["result"]) {
            continue;
        }
        let error = adaptation["observation"]["actual"]
            .as_str()
            .or_else(|| adaptation["observation"]["error"].as_str())
            .unwrap_or("");
        let class = experience::failure_class(error).unwrap_or("unknown");
        let kind = "verify-before-mutate";
        let id = principle_id(kind, class);
        let mut principle = state.store.get("experience_principle", &id).unwrap_or_else(|| {
            json!({
                "id": id,
                "kind": kind,
                "scope": "global",
                "status": "active",
                "failureClasses": [class],
                "sourceDomains": [],
                "principle": principle_text(kind, class),
                "suggestedActions": ["read", "list", "get", "status", "head", "diff"],
                "evidenceGrade": "E1_observed",
                "confidence": 0.5,
                "evidenceExecutionIds": [],
                "supportingCases": [],
                "supportingModels": [],
                "supportingAgents": [],
                "counterexamples": [],
                "createdAt": chrono::Utc::now().to_rfc3339(),
            })
        });
        // Structured conditions, exclusions, typed procedure, expected
        // evidence and cost. The counterexample list stays owned by usage.
        let synthesized = synthesize(kind, class, failed_tool, selected_tool);
        if let Some(fields) = synthesized.as_object() {
            for (key, value) in fields {
                if key != "counterexamples" {
                    principle[key] = value.clone();
                }
            }
        }
        union_strings(&mut principle, "evidenceExecutionIds", execution_id, 64);
        union_strings(&mut principle, "supportingModels", model, 16);
        union_strings(&mut principle, "supportingAgents", agent, 16);
        union_strings(
            &mut principle,
            "sourceDomains",
            domain(failed_tool),
            16,
        );
        union_strings(
            &mut principle,
            "sourceDomains",
            domain(selected_tool),
            16,
        );
        let cases = principle["evidenceExecutionIds"]
            .as_array()
            .map(Vec::len)
            .unwrap_or(0);
        principle["confidence"] = json!((0.5 + 0.1 * cases as f64).min(0.95));
        principle["updatedAt"] = json!(chrono::Utc::now().to_rfc3339());
        // The growth cycle does not stop at memory. Once the same principle is
        // backed by two verified executions it becomes a strategy candidate in
        // the existing experiment/promotion pipeline.
        if cases >= 2 && principle["strategyCandidateId"].is_null() {
            let evidence = principle["evidenceExecutionIds"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter_map(|value| value.as_str().map(str::to_string))
                .collect::<Vec<_>>();
            let scope = crate::engine::memory::MemoryScope::for_session(
                &state.store,
                execution["sessionId"].as_str().unwrap_or(""),
                execution["agentId"].as_str(),
            );
            let text = format!(
                "{} 적용: 변경/외부 작업 전에 현재 상태와 버전을 관찰하고 기대 조건과 비교한다.",
                principle["principle"].as_str().unwrap_or("")
            );
            if let Some(candidate) = crate::engine::improve::propose(
                state,
                &scope,
                "strategy",
                &text,
                "검증된 실행에서 추출한 일반 원리를 실행 전 검증 전략 후보로 승격",
                &evidence,
                None,
                "global",
            ) {
                principle["strategyCandidateId"] = candidate["id"].clone();
                principle["strategyCandidateAt"] = json!(chrono::Utc::now().to_rfc3339());
            }
            ensure_composition(state, &mut principle);
        }
        if state.store.put("experience_principle", &principle).is_some() {
            learned += 1;
        }
    }
    learned
}

/// Returns model-agnostic guidance for a mutation failure. The match is on the
/// structural failure class and the fact that the failed tool mutates; the tool
/// name and provider model are deliberately not part of applicability.
pub fn context_block(
    state: &AppState,
    agent_id: Option<&str>,
    model: &str,
    failed_tool: &str,
    arguments: &Value,
    error: &str,
    execution_id: Option<&str>,
) -> Option<String> {
    if !is_mutation(failed_tool, arguments) {
        return None;
    }
    let class = experience::failure_class(error).unwrap_or("unknown");
    let mut applicable = state
        .store
        .all("experience_principle")
        .into_iter()
        .filter(|principle| {
            principle["status"].as_str() == Some("active")
                && principle["failureClasses"].as_array().is_some_and(|classes| {
                    classes.is_empty()
                        || classes.iter().any(|value| {
                            value.as_str() == Some(class) || value.as_str() == Some("unknown")
                        })
                })
        })
        .collect::<Vec<_>>();
    applicable.sort_by(|a, b| {
        b["supportingCases"]
            .as_array()
            .map(Vec::len)
            .unwrap_or(0)
            .cmp(&a["supportingCases"].as_array().map(Vec::len).unwrap_or(0))
            .then_with(|| a["id"].as_str().cmp(&b["id"].as_str()))
    });
    if applicable.is_empty() {
        return None;
    }
    let mut lines = Vec::new();
    for principle in applicable.iter().take(3) {
        if let (Some(execution_id), Some(principle_id)) =
            (execution_id, principle["id"].as_str())
        {
            let usage_id = format!(
                "principle-use-{}",
                crate::engine::memory::stable_id(&[execution_id, principle_id])
            );
            if state.store.get("principle_usage", &usage_id).is_none() {
                let usage = json!({
                    "id": usage_id,
                    "executionId": execution_id,
                    "principleId": principle_id,
                    "agentId": agent_id,
                    "model": model,
                    "failureClass": class,
                    "status": "pending",
                    "createdAt": chrono::Utc::now().to_rfc3339(),
                });
                let _ = state.store.put("principle_usage", &usage);
            }
        }
        lines.push(format!(
            "- {} (검증 사례 {}건, 모델 {}개, 출처 도메인: {})",
            principle["principle"].as_str().unwrap_or(""),
            principle["supportingCases"]
                .as_array()
                .map(Vec::len)
                .unwrap_or(0),
            principle["supportingModels"]
                .as_array()
                .map(Vec::len)
                .unwrap_or(0),
            principle["sourceDomains"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    Some(format!(
        "Generalized principles from VERIFIED past runs (model-agnostic; apply to the current resource, never copy old arguments):\n{}",
        lines.join("\n")
    ))
}

/// Closes the loop: a principle used in a successful run gains a supporting
/// case; a failed run is kept as a counterexample. This is what lets the same
/// principle improve across models and agents.
pub fn finalize_usage(state: &AppState, execution: &Value, succeeded: bool) -> usize {
    let Some(execution_id) = execution["id"].as_str() else {
        return 0;
    };
    let _guard = state.principle_learning.lock().unwrap();
    let mut finalized = 0usize;
    for mut usage in state.store.all("principle_usage") {
        if usage["executionId"].as_str() != Some(execution_id)
            || usage["status"].as_str() != Some("pending")
        {
            continue;
        }
        if matches!(execution["taskOutcome"].as_str(), Some("answered_unverified" | "cancelled")) {
            usage["status"] = json!("unverified");
            usage["finalizedAt"] = json!(chrono::Utc::now().to_rfc3339());
            state.store.put("principle_usage", &usage);
            finalized += 1;
            continue;
        }
        let Some(principle_id) = usage["principleId"].as_str() else {
            continue;
        };
        let Some(mut principle) = state.store.get("experience_principle", principle_id) else {
            continue;
        };
        if succeeded {
            union_strings(&mut principle, "supportingCases", execution_id, 64);
            union_strings(
                &mut principle,
                "supportingModels",
                usage["model"].as_str().unwrap_or(""),
                16,
            );
            union_strings(
                &mut principle,
                "supportingAgents",
                usage["agentId"].as_str().unwrap_or(""),
                16,
            );
            let cases = principle["supportingCases"]
                .as_array()
                .map(Vec::len)
                .unwrap_or(0);
            principle["confidence"] = json!((0.5 + 0.1 * cases as f64).min(0.95));
            usage["status"] = json!("supported");
        } else {
            let mut counterexamples = principle["counterexamples"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            let counterexample = json!({
                "executionId": execution_id,
                "failureClass": usage["failureClass"],
                "model": usage["model"],
                "agentId": usage["agentId"],
            });
            if !counterexamples.iter().any(|item| item == &counterexample) {
                counterexamples.push(counterexample);
                counterexamples.truncate(16);
            }
            principle["counterexamples"] = json!(counterexamples);
            principle["confidence"] =
                json!((principle["confidence"].as_f64().unwrap_or(0.5) - 0.1).max(0.1));
            usage["status"] = json!("counterexample");
        }
        principle["lastUsedAt"] = json!(chrono::Utc::now().to_rfc3339());
        ensure_composition(state, &mut principle);
        state.store.put("experience_principle", &principle);
        usage["finalizedAt"] = json!(chrono::Utc::now().to_rfc3339());
        state.store.put("principle_usage", &usage);
        finalized += 1;
    }
    finalized
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::Config, db::Store, vault::Vault};

    fn state() -> (AppState, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "dominion-principle-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let config = Config {
            port: 0, data_dir: dir.clone(), web_dir: dir.join("web"), workspace: dir.clone(),
        };
        (
            AppState::new(
                config.clone(),
                Store::open(&config.db_path()).unwrap(),
                Vault::open(&config.key_path()),
            ),
            dir,
        )
    }

    fn execution(model: &str, agent: &str, failed: &str, selected: &str, error: &str) -> Value {
        json!({
            "id": format!("run-{agent}"),
            "taskOutcome": "succeeded",
            "model": model,
            "agentId": agent,
            "adaptations": [{
                "observation": {"tool": failed, "actual": error},
                "selected": {"tool": selected, "inputs": {"path": "safe"}},
                "result": {"success": true}
            }]
        })
    }

    #[test]
    fn verified_github_failure_creates_a_global_principle() {
        let (state, dir) = state();
        assert_eq!(
            learn_from_execution(
                &state,
                &execution(
                    "model-a",
                    "agent-a",
                    "github_api_update",
                    "github_api_get",
                    "409 conflict: sha mismatch"
                )
            ),
            1
        );
        let principles = state.store.all("experience_principle");
        assert_eq!(principles.len(), 1);
        assert_eq!(principles[0]["kind"], "verify-before-mutate");
        assert_eq!(principles[0]["scope"], "global");
        assert!(principles[0]["sourceDomains"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "source_control"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn direct_recovery_requires_the_same_resource_to_be_read_and_retried() {
        let (state, dir) = state();
        let execution = json!({"id":"direct","taskOutcome":"succeeded","model":"real-tool-loop"});
        for (seq, tool, method, path, status, result) in [
            (1, "github_api", "PUT", "/target", "failed", json!({"error":"409 conflict"})),
            (2, "github_api", "GET", "/other", "completed", json!({"status":200})),
            (4, "github_api", "PUT", "/target", "completed", json!({"status":200})),
        ] {
            state.store.put("execution_operation", &json!({
                "id":format!("operation-{seq}"),"runId":"direct","opSeq":seq,"tool":tool,
                "arguments":{"method":method,"path":path},"status":status,"result":result,
            })).unwrap();
        }
        assert_eq!(learn_from_execution(&state, &execution), 0);
        state.store.put("execution_operation", &json!({
            "id":"operation-3","runId":"direct","opSeq":3,"tool":"github_api",
            "arguments":{"method":"GET","path":"/target"},"status":"completed","result":{"status":200},
        })).unwrap();
        assert_eq!(learn_from_execution(&state, &execution), 1);
        assert_eq!(state.store.all("experience_principle")[0]["supportingModels"], json!(["real-tool-loop"]));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn principle_transfers_to_another_domain_and_model() {
        let (state, dir) = state();
        learn_from_execution(
            &state,
            &execution(
                "model-a",
                "agent-a",
                "github_api_update",
                "github_api_get",
                "409 conflict: sha mismatch",
            ),
        );
        for (index, (agent, model, tool, run)) in [
            ("agent-b", "model-b", "sql_update", "run-b"),
            ("agent-c", "model-c", "http_post", "run-c"),
            ("agent-d", "model-d", "workspace_write", "run-d"),
        ]
        .into_iter()
        .enumerate()
        {
            let guidance = context_block(
                &state,
                Some(agent),
                model,
                tool,
                &json!({}),
                "409 conflict: stale state",
                Some(run),
            )
            .unwrap_or_else(|| panic!("cross-domain principle missing for {tool}"));
            assert!(guidance.contains("현재 상태와 버전"), "{guidance}");
            let execution = json!({"id":run,"model":model,"agentId":agent});
            assert_eq!(finalize_usage(&state, &execution, true), 1);
            assert_eq!(state.store.all("principle_usage").len(), index + 1);
        }
        // A second verified source run promotes the principle into the
        // existing strategy-candidate pipeline.
        learn_from_execution(
            &state,
            &json!({
                "id":"run-source-2",
                "taskOutcome":"succeeded",
                "model":"model-a2",
                "agentId":"agent-a2",
                "sessionId":"s-source-2",
                "adaptations":[{
                    "observation":{"tool":"github_api_update","actual":"409 conflict"},
                    "selected":{"tool":"github_api_get","inputs":{}},
                    "result":{"success":true}
                }]
            }),
        );
        let candidate = state.store.all("improvement").into_iter()
            .find(|record| record["kind"].as_str() == Some("strategy"))
            .expect("principle strategy candidate");
        assert_eq!(candidate["scope"], "global", "{candidate}");
        assert!(candidate["text"].as_str().unwrap_or("").contains("현재 상태와 버전"), "{candidate}");
        let principle = &state.store.all("experience_principle")[0];
        assert_eq!(principle["strategyCandidateId"], candidate["id"], "{principle}");
        for model in ["model-b", "model-c", "model-d"] {
            assert!(principle["supportingModels"]
                .as_array()
                .unwrap()
                .iter()
                .any(|value| value == model), "{principle}");
        }
        assert_eq!(principle["supportingCases"].as_array().unwrap().len(), 3);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn synthesized_principle_is_typed_not_a_prompt_sentence() {
        let (state, dir) = state();
        learn_from_execution(
            &state,
            &execution(
                "model-a",
                "agent-a",
                "github_api_update",
                "github_api_get",
                "409 conflict: sha mismatch",
            ),
        );
        let principle = &state.store.all("experience_principle")[0];
        assert_eq!(principle["schemaVersion"], 2);
        assert!(principle["conditions"].as_array().unwrap().len() >= 2);
        assert!(principle["exclusions"].as_array().unwrap().len() >= 5);
        assert_eq!(principle["failureLayer"], "state");
        assert_eq!(principle["procedure"][0]["operator"], "observe");
        assert_eq!(principle["procedure"][3]["operator"], "verify");
        assert_eq!(principle["verification"]["modelClaimAccepted"], false);
        assert_eq!(principle["autoApply"], false);
        assert_eq!(principle["cost"]["modelCalls"], 0);
        assert!(principle["provenance"]["capabilities"].is_object());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn composition_requires_independent_cases_and_stays_unpromoted() {
        let (state, dir) = state();
        learn_from_execution(
            &state,
            &execution(
                "model-a",
                "agent-a",
                "github_api_update",
                "github_api_get",
                "409 conflict: sha mismatch",
            ),
        );
        assert!(context_block(
            &state,
            Some("agent-b"),
            "model-b",
            "workspace_write",
            &json!({}),
            "409 conflict",
            Some("run-b"),
        )
        .is_some());
        finalize_usage(&state, &json!({"id":"run-b"}), true);
        assert!(
            state.store.all("strategy_composition").is_empty(),
            "one source and one supporting case are not enough"
        );

        learn_from_execution(
            &state,
            &json!({
                "id":"run-source-2","taskOutcome":"succeeded","model":"model-a2",
                "agentId":"agent-a2","sessionId":"s-2",
                "adaptations":[{
                    "observation":{"tool":"github_api_update","actual":"409 conflict"},
                    "selected":{"tool":"github_api_get","inputs":{}},
                    "result":{"success":true}
                }]
            }),
        );
        assert!(context_block(
            &state,
            Some("agent-c"),
            "model-c",
            "file_write",
            &json!({}),
            "409 conflict: stale state",
            Some("run-c"),
        )
        .is_some());
        finalize_usage(&state, &json!({"id":"run-c"}), true);

        let composition = state
            .store
            .all("strategy_composition")
            .into_iter()
            .next()
            .expect("composition from independent evidence");
        assert_eq!(composition["promotionEligible"], false);
        assert_eq!(composition["experiment"]["status"], "not_run");
        assert_eq!(composition["experiment"]["gates"]["tokenRatioMax"], 1.25);
        let domains = composition["supportedDomains"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|value| value["domain"].as_str())
            .collect::<Vec<_>>();
        assert!(domains.contains(&"filesystem"));
        assert!(domains.contains(&"source_control"));
        assert!(domains.contains(&"system") == false);
        let principle = state.store.all("experience_principle")[0].clone();
        assert_eq!(principle["strategyCompositionId"], composition["id"]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn unresolved_effects_are_not_learned_as_success() {
        let (state, dir) = state();
        let mut execution = execution(
            "model-a",
            "agent-a",
            "github_api_update",
            "github_api_get",
            "409 conflict: sha mismatch",
        );
        execution["effectSummary"] = json!({"unverifiedEffects": 1});
        assert_eq!(learn_from_execution(&state, &execution), 0);
        assert!(state.store.all("experience_principle").is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn observations_do_not_receive_mutation_principles() {
        let (state, dir) = state();
        learn_from_execution(
            &state,
            &execution(
                "model-a",
                "agent-a",
                "workspace_write",
                "workspace_read",
                "no such file",
            ),
        );
        assert!(context_block(
            &state,
            None,
            "model-b",
            "workspace_read",
            &json!({}),
            "no such file",
            Some("run-c"),
        )
        .is_none());
        let _ = std::fs::remove_dir_all(dir);
    }
}
