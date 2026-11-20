//! H9 versioned policy slots.
//!
//! A policy slot names a replaceable algorithm/parameter set (for example the
//! planning strategy). New revisions are candidates; only an isolated paired
//! experiment can activate them, and activation supersedes the previous active
//! revision. This is deliberately separate from one-off strategy text: the
//! artifact is versioned and rollbackable.
use crate::state::AppState;
use serde_json::{json, Value};

pub const SLOTS: &[&str] = &["planning", "recovery", "execution", "mutation.preflight"];
use crate::engine::preflight_fixture as preflight;

pub fn is_preflight_kernel(value: &Value) -> bool {
    matches!(value.as_str(), Some("same-file-revision-v1" | preflight::GUARD_KERNEL | preflight::EXACT_KERNEL))
}

pub const MIN_EXPERIMENT_UNITS: u64 = 8;

pub fn frozen_manifest(
    request_id: &str,
    fixture: &str,
    candidate_revision: &str,
    units: u64,
) -> (Value, String) {
    use sha2::Digest;
    let manifest = json!({
        "experimentId": request_id,
        "candidateRevision": candidate_revision,
        "baselineRevision": "baseline",
        "modelRevision": "runner-reported",
        "toolRevisions": [],
        "fixtures": [fixture],
        "environmentSeeds": [],
        "pairAssignments": (0..units).collect::<Vec<_>>(),
        "evaluationRules": {
            "primary": "task success (ITT)",
            "secondary": ["plan quality", "cost", "latency"],
            "missingOutcome": "failure",
        },
        "totalBudget": {"pairs": units},
        "stoppingRules": {"minIndependentUnits": units},
    });
    let hash = format!("{:x}", sha2::Sha256::digest(manifest.to_string().as_bytes()));
    (manifest, hash)
}

/// Creates one durable experiment request per policy revision. Idempotent.
pub fn enqueue_policy_request(
    state: &AppState,
    policy_id: &str,
    fixture: &str,
    search_id: Option<&str>,
) -> Option<(Value, Value, bool)> {
    enqueue_policy_evaluation(state, policy_id, fixture, search_id, None)
}

pub(crate) fn enqueue_policy_evaluation(
    state: &AppState,
    policy_id: &str,
    fixture: &str,
    search_id: Option<&str>,
    model: Option<&str>,
) -> Option<(Value, Value, bool)> {
    let policy = state.store.get("adaptation_policy", policy_id)?;
    let revision = policy["revision"].as_str().unwrap_or("");
    let scope = model.map(|model| crate::engine::preflight_fixture::scope(
        &std::env::var("DOMINION_EXPERIMENT_PROVIDER_BASE")
            .unwrap_or_else(|_| "http://127.0.0.1:11434/v1".into()),
        model, "native",
    ));
    let mut key = vec![policy_id, revision];
    key.extend(scope.as_deref());
    let request_id = format!("experiment-request-policy-{}",
        crate::engine::memory::stable_id(&key));

    if let Some(existing) = state.store.get("experiment_request", &request_id) {
        return Some((existing, policy, false));
    }
    let units = if policy["slot"] == "mutation.preflight" { 10 } else { MIN_EXPERIMENT_UNITS };
    let (manifest, manifest_hash) = frozen_manifest(&request_id, fixture, revision, units);
    let request = json!({
        "id": request_id,
        "kind": "policy",
        "policyId": policy_id,
        "slot": policy["slot"],
        "revision": revision,
        "status": "queued",
        "fixture": fixture,
        "evaluationModel": model,
        "evaluationScope": scope,
        "searchId": search_id,
        "minIndependentUnits": units,
        "frozenManifest": manifest,
        "frozenManifestHash": manifest_hash,
        "externalRunner": true,
        "createdAt": chrono::Utc::now().to_rfc3339(),
    });
    let saved = state.store.put_if_absent("experiment_request", &request);
    let Some(saved) = saved else {
        return state.store.get("experiment_request", &request_id)
            .map(|existing| (existing, policy, false));
    };
    state.bus.record("experiment_request", &saved);
    state.store.audit(
        "policy.candidate_queued",
        json!({"policyId": policy_id, "requestId": saved["id"], "searchId": search_id}),
    );
    Some((saved, policy, true))
}

/// Recovery retry budgets from the `recovery` slot. Values are clamped so a
/// policy can tune, but not remove, execution safety limits.
pub fn recovery_limits(parameters: &Value) -> (usize, usize) {
    let acceptance = parameters["maxAcceptanceRetries"]
        .as_u64()
        .unwrap_or(2)
        .min(4) as usize;
    let repairs = parameters["maxProcedureRepairs"]
        .as_u64()
        .unwrap_or(2)
        .min(4) as usize;
    (acceptance, repairs)
}

/// Decides whether a planning policy asks for a plan. `None` means the policy
/// does not override the built-in heuristic.
pub fn outline_decision(parameters: &Value, input_chars: usize) -> Option<bool> {
    let outline = parameters["outline"].as_bool()?;
    if !outline {
        return Some(false);
    }
    match parameters["minChars"].as_u64() {
        Some(minimum) => Some(input_chars as u64 >= minimum),
        None => Some(true),
    }
}

/// Selects the best evaluated revision in a search group using the more
/// conservative of the two evaluator deltas. Ineligible or inferior revisions
/// cannot stay active.
pub fn select_best(state: &AppState, search_id: &str) -> Option<Value> {
    use crate::engine::plan_quality::MIN_SEMANTIC_DELTA;
    let policies = state
        .store
        .all("adaptation_policy")
        .into_iter()
        .filter(|policy| policy["searchId"].as_str() == Some(search_id))
        .collect::<Vec<_>>();
    if policies.is_empty() {
        return None;
    }
    let score = |policy: &Value| -> Option<f64> {
        let experiment = &policy["lastExperiment"];
        let units = experiment["independentUnits"].as_u64().unwrap_or(0);
        let verdict = experiment["verdict"].as_str().unwrap_or("");
        let main = experiment["semanticDelta"].as_f64().unwrap_or(0.0);
        let independent = experiment["independentSemanticDelta"]
            .as_f64()
            .unwrap_or(0.0);
        (units >= MIN_EXPERIMENT_UNITS
            && matches!(verdict, "improved" | "preliminary_support")
            && main >= MIN_SEMANTIC_DELTA
            && independent >= MIN_SEMANTIC_DELTA)
            .then(|| main.min(independent))
    };
    let best = policies
        .iter()
        .filter_map(|policy| {
            score(policy).map(|score| (policy["id"].as_str().unwrap_or("").to_string(), score))
        })
        .max_by(|a, b| {
            a.1.partial_cmp(&b.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| b.0.cmp(&a.0))
        });
    let policy_count = policies.len();
    let optimizer_revision = policies
        .iter()
        .find_map(|policy| policy["optimizerRevision"].as_str())
        .unwrap_or("optimizer-v1")
        .to_string();
    let selected_policy_id = best.as_ref().map(|(id, _)| id.clone());
    let mut candidates = Vec::new();
    for mut policy in policies {
        let id = policy["id"].as_str().unwrap_or("").to_string();
        let candidate_score = score(&policy);
        if best.as_ref().is_some_and(|(best_id, _)| best_id == &id) {
            policy["status"] = json!("active");
            policy["selectedBySearch"] = json!(search_id);
        } else if policy["status"].as_str() == Some("active") {
            policy["status"] = json!("superseded");
            policy["supersededBy"] = json!(best.as_ref().map(|(id, _)| id.clone()));
        }
        let status = policy["status"].clone();
        if let Some(saved) = state.store.put("adaptation_policy", &policy) {
            state.bus.record("adaptation_policy", &saved);
        }
        candidates.push(json!({
            "policyId": id,
            "score": candidate_score,
            "status": status,
        }));
    }
    let record = json!({
        "id": search_id,
        "slot": "planning",
        "selectedPolicyId": best.as_ref().map(|(id, _)| id.clone()),
        "selectedScore": best.as_ref().map(|(_, score)| score),
        "candidates": candidates,
        "selectedAt": chrono::Utc::now().to_rfc3339(),
    });
    let saved = state.store.put("policy_search", &record);
    if let Some(saved) = &saved {
        state.bus.record("policy_search", saved);
        state.store.audit(
            "policy.search_selected",
            json!({"searchId": search_id, "selected": record["selectedPolicyId"]}),
        );
    }
    state.store.put(
        "search_episode",
        &json!({
            "id": format!("search-{}", crate::engine::memory::stable_id(&[search_id, &optimizer_revision])),
            "searchId": search_id,
            "optimizerRevision": optimizer_revision,
            "generatedCandidates": policy_count,
            "selectedPolicy": selected_policy_id,
            "cost": policy_count as f64 * 0.1,
            "createdAt": chrono::Utc::now().to_rfc3339(),
        }),
    );
    saved
}

pub fn list(state: &AppState) -> Vec<Value> {
    let mut items = state.store.all("adaptation_policy");
    items.sort_by(|a, b| {
        b["createdAt"]
            .as_str()
            .unwrap_or("")
            .cmp(a["createdAt"].as_str().unwrap_or(""))
            .then_with(|| a["id"].as_str().cmp(&b["id"].as_str()))
    });
    items
}

pub fn active(state: &AppState, slot: &str) -> Option<Value> {
    state
        .store
        .all("adaptation_policy")
        .into_iter()
        .find(|policy| {
            policy["slot"].as_str() == Some(slot) && policy["status"].as_str() == Some("active")
        })
}

/// A learned guard is selected by the runtime's task contract. Model identities
/// recorded during evaluation are evidence, never live routing keys.
pub fn portable_file_guard(state: &AppState) -> Option<Value> {
    state.store.all("adaptation_policy").into_iter().find(|policy| {
        policy["slot"] == "mutation.preflight"
            && policy["status"] == "active"
            && policy["parameters"]["kernel"] == preflight::GUARD_KERNEL
            && policy["parameters"]["workload"] == preflight::GUARD_WORKLOAD
            && policy["preflightEvidence"].as_object().is_some_and(|evidence| evidence.len() >= 2)
    })
}

#[allow(dead_code)] // used by tests and by future slots that only need parameters
pub fn active_parameters(state: &AppState, slot: &str) -> Value {
    active(state, slot)
        .map(|policy| policy["parameters"].clone())
        .unwrap_or_else(|| json!({}))
}

/// Registers a candidate revision. Identical name+parameters is idempotent.
pub fn register(
    state: &AppState,
    slot: &str,
    name: &str,
    parameters: &Value,
    rationale: &str,
) -> Result<Value, String> {
    if !SLOTS.contains(&slot) {
        return Err(format!("지원하지 않는 정책 슬롯입니다: {slot}"));
    }
    if name.trim().is_empty() {
        return Err("정책 이름이 필요합니다.".into());
    }
    if !parameters.is_object() {
        return Err("정책 parameters는 객체여야 합니다.".into());
    }
    if slot == "mutation.preflight" && !is_preflight_kernel(&parameters["kernel"]) {
        return Err("지원하지 않는 변경 전 검증 커널입니다.".into());
    }
    let id = format!(
        "policy-{}",
        crate::engine::memory::stable_id(&[slot, name, &parameters.to_string()])
    );
    if let Some(existing) = state.store.get("adaptation_policy", &id) {
        return Ok(existing);
    }
    let revision = crate::engine::memory::stable_id(&[&id, &parameters.to_string()]);
    let record = json!({
        "id": id,
        "slot": slot,
        "name": name,
        "revision": revision,
        "status": "candidate",
        "evidenceGrade": "E0",
        "parameters": parameters,
        "rationale": rationale,
        "createdAt": chrono::Utc::now().to_rfc3339(),
    });
    let saved = state
        .store
        .put("adaptation_policy", &record)
        .ok_or("정책 후보를 저장할 수 없습니다.")?;
    state.bus.record("adaptation_policy", &saved);
    state.store.audit(
        "policy.candidate",
        json!({"id": saved["id"], "slot": slot, "name": name}),
    );
    Ok(saved)
}

/// Benchmark-sandbox only: force a policy active after an isolated engine is
/// disposable. Production activation still goes through the experiment gate.
pub fn force_activate(state: &AppState, policy_id: &str) -> Option<Value> {
    let mut policy = state.store.get("adaptation_policy", policy_id)?;
    let slot = policy["slot"].as_str().unwrap_or("").to_string();
    for mut other in state.store.all("adaptation_policy") {
        if other["id"].as_str() != Some(policy_id)
            && other["slot"].as_str() == Some(slot.as_str())
            && other["status"].as_str() == Some("active")
        {
            other["status"] = json!("superseded");
            other["supersededBy"] = policy["id"].clone();
            if let Some(saved) = state.store.put("adaptation_policy", &other) {
                state.bus.record("adaptation_policy", &saved);
            }
        }
    }
    policy["status"] = json!("active");
    policy["activatedAt"] = json!(chrono::Utc::now().to_rfc3339());
    let saved = state.store.put("adaptation_policy", &policy)?;
    state.bus.record("adaptation_policy", &saved);
    Some(saved)
}

pub fn deactivate_slot(state: &AppState, slot: &str) -> usize {
    let mut count = 0;
    for mut policy in state.store.all("adaptation_policy") {
        if policy["slot"].as_str() == Some(slot)
            && policy["status"].as_str() == Some("active")
        {
            policy["status"] = json!("inactive");
            if let Some(saved) = state.store.put("adaptation_policy", &policy) {
                state.bus.record("adaptation_policy", &saved);
                count += 1;
            }
        }
    }
    count
}

/// Activates or rolls back a candidate based on an isolated experiment.
pub fn experiment_result_records(
    state: &AppState,
    policy_id: &str,
    experiment: &Value,
) -> Result<Vec<(String, Value)>, String> {
    let Some(mut policy) = state.store.get("adaptation_policy", policy_id) else {
        return Err("정책 후보를 찾을 수 없습니다.".into());
    };
    let units = experiment["independentUnits"].as_u64().unwrap_or(0);
    if units == 0 {
        return Err("독립 실험 표본이 없습니다.".into());
    }
    let verdict = experiment["verdict"].as_str().unwrap_or("inconclusive");
    let delta = experiment["delta"].as_f64().unwrap_or(0.0);
    let semantic_delta = experiment["semanticDelta"].as_f64();
    let independent_semantic_delta = experiment["independentSemanticDelta"].as_f64();
    let semantic_metric = experiment["semanticMetric"]
        .as_str()
        .unwrap_or(crate::engine::plan_quality::METRIC_VERSION);
    let experiment_id = experiment["experimentId"].as_str().unwrap_or("");
    let revision = crate::engine::memory::stable_id(&[
        experiment_id,
        experiment["oracleVersion"].as_str().unwrap_or(""),
        &units.to_string(),
        &format!("{delta:.6}"),
    ]);
    policy["lastExperiment"] = json!({
        "experimentId": experiment_id,
        "verdict": verdict,
        "delta": delta,
        "independentUnits": units,
        "semanticDelta": semantic_delta,
        "independentSemanticDelta": independent_semantic_delta,
        "semanticMetric": semantic_metric,
        "oracleVersion": experiment["oracleVersion"],
        "evaluatedAt": chrono::Utc::now().to_rfc3339(),
    });

    let mut records: Vec<(String, Value)> = Vec::new();
    if policy["slot"] == "mutation.preflight" {
        return settle_file_guard(state, policy, experiment, &revision);
    }
    if units >= MIN_EXPERIMENT_UNITS && verdict == "regressed" {
        policy["status"] = json!("rolled_back");
        policy["rolledBackAt"] = json!(chrono::Utc::now().to_rfc3339());
        policy["experimentRevision"] = json!(revision);
    } else if units >= MIN_EXPERIMENT_UNITS
        && matches!(verdict, "improved" | "preliminary_support")
        && delta > 0.0
    {
        let slot = policy["slot"].as_str().unwrap_or("").to_string();
        let semantic_ok = [semantic_delta, independent_semantic_delta].into_iter().all(|delta| {
            delta.is_some_and(|value| value >= crate::engine::plan_quality::MIN_SEMANTIC_DELTA)
        });
        if !semantic_ok {
            policy["requiresExperiment"] = json!(true);
            policy["semanticVerified"] = json!(false);
            policy["reason"] = json!(format!("의미 계획 개선폭이 기준({})보다 낮아 활성화하지 않았습니다.",
                crate::engine::plan_quality::MIN_SEMANTIC_DELTA));
            return Ok(vec![("adaptation_policy".to_string(), policy)]);
        }
        policy["semanticVerified"] = json!(true);
        activate_verified(state, &mut policy, &revision, &slot, &mut records);
    }
    records.push(("adaptation_policy".to_string(), policy));
    Ok(records)
}
fn activate_verified(
    state: &AppState, policy: &mut Value, revision: &str, slot: &str,
    records: &mut Vec<(String, Value)>,
) {
    let now = chrono::Utc::now().to_rfc3339();
    for mut other in state.store.all("adaptation_policy") {
        if other["id"] != policy["id"] && other["slot"] == slot && other["status"] == "active" {
            other["status"] = json!("superseded");
            other["supersededBy"] = policy["id"].clone();
            other["supersededAt"] = json!(now);
            records.push(("adaptation_policy".into(), other));
        }
    }
    policy["status"] = json!("active");
    policy["evidenceGrade"] = json!("E2");
    policy["experimentRevision"] = json!(revision);
    policy["activatedAt"] = json!(now);
}

fn settle_file_guard(
    state: &AppState, mut policy: Value, experiment: &Value, revision: &str,
) -> Result<Vec<(String, Value)>, String> {
    let mut records = Vec::new();
    let cohort = experiment["preflightCohort"].as_str().filter(|cohort| {
        !cohort.is_empty() && policy["parameters"]["kernel"] == preflight::GUARD_KERNEL
            && policy["parameters"]["workload"] == preflight::GUARD_WORKLOAD
            && experiment["independentUnits"].as_u64().unwrap_or(0) >= 10
            && policy["status"] != "superseded"
    });
    if let Some(cohort) = cohort {
        let mut evidence = policy["preflightEvidence"].as_object().cloned().unwrap_or_default();
        let mut rejected = policy["preflightCounterEvidence"].as_object().cloned().unwrap_or_default();
        let rejection = experiment["preflightRejection"].as_str();
        match (experiment["verdict"].as_str(), rejection) {
            (Some("regressed"), _) | (_, Some("timeouts_increased" | "token_cost_exceeded")) => {
                evidence.remove(cohort);
                rejected.insert(cohort.into(), experiment["experimentId"].clone());
                if policy["status"] == "active" {
                    policy["status"] = json!("rolled_back");
                    policy["rolledBackAt"] = json!(chrono::Utc::now().to_rfc3339());
                }
            }
            (Some("preliminary_support"), None) if experiment["delta"].as_f64().unwrap_or(0.0) > 0.0 => {
                evidence.insert(cohort.into(), experiment["experimentId"].clone());
                rejected.remove(cohort);
                if evidence.len() >= 2 && rejected.is_empty() && policy["status"] != "active" {
                    activate_verified(state, &mut policy, revision, "mutation.preflight", &mut records);
                }
            }
            _ => {}
        }
        policy["lastExperiment"]["guardRejection"] = json!(rejection);
        policy["preflightEvidence"] = Value::Object(evidence);
        policy["preflightCounterEvidence"] = Value::Object(rejected);
    }
    records.push(("adaptation_policy".into(), policy));
    Ok(records)
}

pub fn apply_experiment_result(
    state: &AppState,
    policy_id: &str,
    experiment: &Value,
) -> Result<Value, String> {
    let _settlement = state.policy_settlement.lock().unwrap();
    let records = experiment_result_records(state, policy_id, experiment)?;
    let saved = state.store.transaction(&records)?;
    for record in &saved {
        state.bus.record("adaptation_policy", record);
    }
    let policy = saved
        .iter()
        .find(|record| record["id"].as_str() == Some(policy_id))
        .cloned()
        .ok_or("정책 결과를 저장할 수 없습니다.")?;
    state.store.audit(
        "policy.experiment_result",
        json!({
            "id": policy_id,
            "experimentId": experiment["experimentId"],
            "verdict": experiment["verdict"],
            "units": experiment["independentUnits"],
            "status": policy["status"],
        }),
    );
    Ok(policy)
}



#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guard_requires_independent_model_evidence_and_rolls_back_for_the_workload() {
        let (state, dir) = state();
        let candidate = register(&state, "mutation.preflight", "guard",
            &json!({"kernel":preflight::GUARD_KERNEL,"workload":preflight::GUARD_WORKLOAD}), "fixture").unwrap();
        let id = candidate["id"].as_str().unwrap();
        let settle = |cohort: Option<&str>, verdict: &str, delta: f64| apply_experiment_result(&state, id, &json!({
            "independentUnits":10,"verdict":verdict,"delta":delta,
            "preflightCohort":cohort,"experimentId":format!("{cohort:?}-{verdict}"),
        })).unwrap();
        settle(None, "preliminary_support", 1.0); // deterministic comparison
        settle(Some("model-a"), "preliminary_support", 1.0);
        settle(Some("model-a"), "preliminary_support", 1.0);
        assert!(portable_file_guard(&state).is_none());
        settle(Some("model-b"), "preliminary_support", 1.0);
        assert_eq!(portable_file_guard(&state).unwrap()["id"], id);
        settle(Some("model-c"), "regressed", -1.0);
        assert!(portable_file_guard(&state).is_none());
        settle(Some("model-b"), "preliminary_support", 1.0);
        assert!(portable_file_guard(&state).is_none());
        let _ = std::fs::remove_dir_all(dir);
    }
    #[test]
    fn costly_guard_cannot_activate_even_with_a_positive_success_verdict() {
        let (state, dir) = state();
        let candidate = register(&state, "mutation.preflight", "guard",
            &json!({"kernel":preflight::GUARD_KERNEL,"workload":preflight::GUARD_WORKLOAD}), "fixture").unwrap();
        let id = candidate["id"].as_str().unwrap();
        for (cohort, rejection) in [("model-a", None), ("model-b", Some("token_cost_exceeded"))] {
            apply_experiment_result(&state, id, &json!({
                "independentUnits":10,"verdict":"preliminary_support","delta":1.0,
                "preflightCohort":cohort,"experimentId":cohort,"preflightRejection":rejection,
            })).unwrap();
        }
        assert!(portable_file_guard(&state).is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    use crate::config::Config;
    use crate::db::Store;
    use crate::vault::Vault;

    fn state() -> (AppState, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("dominion-policy-{}", uuid::Uuid::new_v4()));
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

    #[test]
    fn proposed_candidate_stays_inactive_until_eight_units_and_rolls_back() {
        let (state, dir) = state();
        // A manual candidate stays inert until an independently verified,
        // powered experiment authorizes its activation. Removed autonomous
        // proposers cannot create or promote candidates in normal operation.
        let policy = register(
            &state, "planning", "manual-outline-off",
            &json!({"outline":false}), "benchmark-only candidate",
        ).unwrap();
        let policy_id = policy["id"].as_str().unwrap().to_string();
        assert!(active(&state, "planning").is_none());

        let small = json!({"experimentId":"small","verdict":"improved","delta":0.5,"independentUnits":4,"semanticDelta":0.25,"independentSemanticDelta":0.2,"oracleVersion":"v1"});
        let record = apply_experiment_result(&state, &policy_id, &small).unwrap();
        assert_eq!(record["status"], "candidate", "underpowered promotion");

        let powered = json!({"experimentId":"powered","verdict":"preliminary_support","delta":0.4,"independentUnits":8,"semanticDelta":0.25,"independentSemanticDelta":0.2,"oracleVersion":"v1"});
        let record = apply_experiment_result(&state, &policy_id, &powered).unwrap();
        assert_eq!(record["status"], "active");
        assert_eq!(active_parameters(&state, "planning")["outline"], false);

        let other = register(&state, "planning", "other", &json!({"outline": true}), "other").unwrap();
        let regressed = json!({"experimentId":"bad","verdict":"regressed","delta":-0.3,"independentUnits":8,"semanticDelta":0.25,"independentSemanticDelta":0.2,"oracleVersion":"v1"});
        let record = apply_experiment_result(&state, other["id"].as_str().unwrap(), &regressed).unwrap();
        assert_eq!(record["status"], "rolled_back");
        assert_eq!(active(&state, "planning").unwrap()["id"], policy_id.as_str());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn candidates_need_a_powered_experiment_before_activation() {
        let (state, dir) = state();
        let candidate = register(
            &state,
            "planning",
            "outline-off",
            &json!({"outline": false}),
            "skip planning for short tasks",
        )
        .unwrap();
        assert_eq!(candidate["status"], "candidate");
        assert!(active(&state, "planning").is_none());

        let small = json!({"experimentId":"e1","verdict":"improved","delta":0.5,"independentUnits":2,"semanticDelta":0.25,"independentSemanticDelta":0.2,"oracleVersion":"v1"});
        let record = apply_experiment_result(&state, candidate["id"].as_str().unwrap(), &small).unwrap();
        assert_eq!(record["status"], "candidate");

        let powered = json!({"experimentId":"e2","verdict":"preliminary_support","delta":0.2,"independentUnits":10,"semanticDelta":0.25,"independentSemanticDelta":0.2,"oracleVersion":"v1"});
        let record = apply_experiment_result(&state, candidate["id"].as_str().unwrap(), &powered).unwrap();
        assert_eq!(record["status"], "active");
        assert_eq!(record["evidenceGrade"], "E2");
        assert_eq!(active_parameters(&state, "planning")["outline"], false);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn activation_supersedes_the_previous_revision_and_regression_rolls_back() {
        let (state, dir) = state();
        let first = register(&state, "planning", "a", &json!({"outline": true}), "a").unwrap();
        let powered = json!({"experimentId":"e1","verdict":"improved","delta":0.3,"independentUnits":9,"semanticDelta":0.25,"independentSemanticDelta":0.2,"oracleVersion":"v1"});
        apply_experiment_result(&state, first["id"].as_str().unwrap(), &powered).unwrap();
        let second = register(&state, "planning", "b", &json!({"outline": false}), "b").unwrap();
        let powered2 = json!({"experimentId":"e2","verdict":"preliminary_support","delta":0.4,"independentUnits":9,"semanticDelta":0.25,"independentSemanticDelta":0.2,"oracleVersion":"v1"});
        apply_experiment_result(&state, second["id"].as_str().unwrap(), &powered2).unwrap();

        let first = state.store.get("adaptation_policy", first["id"].as_str().unwrap()).unwrap();
        assert_eq!(first["status"], "superseded");
        assert_eq!(active(&state, "planning").unwrap()["id"], second["id"]);
        assert_eq!(active_parameters(&state, "planning")["outline"], false);

        let regressed = json!({"experimentId":"e3","verdict":"regressed","delta":-0.3,"independentUnits":9,"semanticDelta":0.25,"independentSemanticDelta":0.2,"oracleVersion":"v1"});
        let record = apply_experiment_result(&state, second["id"].as_str().unwrap(), &regressed).unwrap();
        assert_eq!(record["status"], "rolled_back");
        assert!(active(&state, "planning").is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn planning_activation_requires_both_units_and_semantic_improvement() {
        let (state, dir) = state();
        let candidate = register(
            &state,
            "planning",
            "semantic-gate",
            &json!({"outline": true}),
            "semantic gate",
        )
        .unwrap();
        let id = candidate["id"].as_str().unwrap();

        // Enough units, file success, but no semantic plan improvement.
        let no_semantic = json!({"experimentId":"no-semantic","verdict":"preliminary_support","delta":0.5,"semanticDelta":0.0,"independentSemanticDelta":0.2,"independentUnits":8,"oracleVersion":"v1"});
        let record = apply_experiment_result(&state, id, &no_semantic).unwrap();
        assert_eq!(record["status"], "candidate", "{record}");
        assert_eq!(record["semanticVerified"], false);

        // Semantic improvement alone without enough units is not enough.
        let few_units = json!({"experimentId":"few","verdict":"preliminary_support","delta":0.5,"semanticDelta":0.4,"independentSemanticDelta":0.2,"independentUnits":4,"oracleVersion":"v1"});
        let record = apply_experiment_result(&state, id, &few_units).unwrap();
        assert_eq!(record["status"], "candidate", "{record}");

        // Both gates pass.
        let both = json!({"experimentId":"both","verdict":"preliminary_support","delta":0.5,"semanticDelta":0.2,"independentSemanticDelta":0.2,"independentUnits":8,"oracleVersion":"v1"});
        let record = apply_experiment_result(&state, id, &both).unwrap();
        assert_eq!(record["status"], "active");
        assert_eq!(record["semanticVerified"], true);
        assert_eq!(record["lastExperiment"]["semanticDelta"], 0.2);
        assert_eq!(
            record["lastExperiment"]["semanticMetric"],
            crate::engine::plan_quality::METRIC_VERSION
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn planning_activation_rejects_an_independent_evaluator_disagreement() {
        let (state, dir) = state();
        let candidate = register(
            &state,
            "planning",
            "independent-gate",
            &json!({"outline": true}),
            "independent gate",
        )
        .unwrap();
        let id = candidate["id"].as_str().unwrap();
        // Primary evaluator improves but the independent evaluator does not.
        let disagree = json!({"experimentId":"disagree","verdict":"preliminary_support","delta":0.5,"semanticDelta":0.4,"independentSemanticDelta":0.0,"independentUnits":8,"oracleVersion":"v1"});
        let record = apply_experiment_result(&state, id, &disagree).unwrap();
        assert_eq!(record["status"], "candidate", "{record}");
        assert_eq!(record["semanticVerified"], false);

        let agree = json!({"experimentId":"agree","verdict":"preliminary_support","delta":0.5,"semanticDelta":0.4,"independentSemanticDelta":0.2,"independentUnits":8,"oracleVersion":"v1"});
        let record = apply_experiment_result(&state, id, &agree).unwrap();
        assert_eq!(record["status"], "active");
        assert_eq!(record["lastExperiment"]["independentSemanticDelta"], 0.2);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn outline_thresholds_control_planning() {
        assert_eq!(
            outline_decision(&json!({"outline": true}), 10),
            Some(true)
        );
        assert_eq!(
            outline_decision(&json!({"outline":false}), 10_000),
            Some(false)
        );
        assert_eq!(
            outline_decision(&json!({"outline":true,"minChars":100}), 50),
            Some(false)
        );
        assert_eq!(
            outline_decision(&json!({"outline":true,"minChars":100}), 150),
            Some(true)
        );
        assert_eq!(outline_decision(&json!({}), 10), None);
    }

    #[test]
    fn search_selection_keeps_the_best_independent_score_only() {
        let (state, dir) = state();
        let search = "search-1";
        let make = |id: &str, main: f64, independent: f64, status: &str| {
            let mut policy = register(
                &state,
                "planning",
                id,
                &json!({"outline":true}),
                "selection test",
            )
            .unwrap();
            policy["searchId"] = json!(search);
            policy["status"] = json!(status);
            policy["lastExperiment"] = json!({
                "independentUnits":10,
                "verdict":"preliminary_support",
                "semanticDelta":main,
                "independentSemanticDelta":independent,
            });
            state.store.put("adaptation_policy", &policy);
            policy
        };
        let best = make("best", 0.4, 0.35, "active");
        let weaker = make("weaker", 0.4, 0.10, "active");
        let rejected = make("rejected", 0.4, 0.0, "candidate");
        let selected = select_best(&state, search).unwrap();
        assert_eq!(selected["selectedPolicyId"], best["id"]);
        assert_eq!(selected["selectedScore"], 0.35);
        assert_eq!(
            state.store.get("adaptation_policy", best["id"].as_str().unwrap()).unwrap()["status"],
            "active"
        );
        assert_eq!(
            state.store.get("adaptation_policy", weaker["id"].as_str().unwrap()).unwrap()["status"],
            "superseded"
        );
        assert_eq!(
            state.store.get("adaptation_policy", rejected["id"].as_str().unwrap()).unwrap()["status"],
            "candidate"
        );
        let searches = state.store.all("policy_search");
        assert_eq!(searches.len(), 1);
        assert_eq!(searches[0]["selectedPolicyId"], best["id"]);
        assert_eq!(searches[0]["candidates"].as_array().unwrap().len(), 3);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn recovery_limits_are_bounded_and_configurable() {
        assert_eq!(recovery_limits(&json!({})), (2, 2));
        assert_eq!(
            recovery_limits(&json!({"maxAcceptanceRetries":1,"maxProcedureRepairs":0})),
            (1, 0)
        );
        // A policy can tune but not remove safety bounds entirely.
        assert_eq!(
            recovery_limits(&json!({"maxAcceptanceRetries":99,"maxProcedureRepairs":99})),
            (4, 4)
        );
    }

    #[test]
    fn a_recovery_policy_uses_the_same_experiment_gate() {
        let (state, dir) = state();
        let candidate = register(
            &state,
            "recovery",
            "fewer-retries",
            &json!({"maxAcceptanceRetries":0,"maxProcedureRepairs":0}),
            "recovery gate",
        )
        .unwrap();
        let id = candidate["id"].as_str().unwrap();
        let underpowered = json!({"experimentId":"r1","verdict":"preliminary_support","delta":0.2,"semanticDelta":0.2,"independentSemanticDelta":0.2,"independentUnits":4,"oracleVersion":"v1"});
        let record = apply_experiment_result(&state, id, &underpowered).unwrap();
        assert_eq!(record["status"], "candidate");

        let staged = json!({"experimentId":"r2","verdict":"preliminary_support","delta":0.2,"semanticDelta":0.2,"independentSemanticDelta":0.2,"independentUnits":8,"oracleVersion":"v1"});
        let record = apply_experiment_result(&state, id, &staged).unwrap();
        assert_eq!(record["status"], "active");
        assert_eq!(
            recovery_limits(&record["parameters"]),
            (0, 0)
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn unknown_slots_and_non_object_parameters_are_rejected() {
        let (state, dir) = state();
        assert!(register(&state, "shell", "x", &json!({}), "").is_err());
        assert!(register(&state, "planning", "x", &json!([]), "").is_err());
        let _ = std::fs::remove_dir_all(dir);
    }
}
