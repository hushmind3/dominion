//! Execution policy and provenance snapshot.
use super::*;

/// Overall compliance for an attempt. A single violated standing strategy must
/// not be masked by another followed strategy in the same bundle.
/// Evaluator version is separate from the payload hash, so re-running the same
/// terminal attempt with a new evaluator is a new evaluation revision rather
/// than a conflicting payload.
/// The single terminal judgment for every turn path. `finalize_execution`
/// remains as the implementation used by existing tests.
/// Freezes the exact policy bundle, context manifest, memory watermark and
/// model settings used by this attempt. Recorded before the first model call
/// and before any harness node so the attempt cannot drift from its evidence.
#[allow(clippy::too_many_arguments)]
pub(super) fn record_selection_snapshot(
    state: &AppState,
    execution_id: &str,
    task_id: &str,
    session_id: &str,
    request: &TurnRequest,
    strategies: &[Value],
    context_items: Vec<Value>,
    memory_watermark: String,
    model: Value,
) -> Result<(), String> {
    use crate::engine::adaptation::types::{
        revision_hash, BundleArm, CandidateScore, Decision, PolicyBundle, SelectionSnapshot,
        StrategyRevision, TaskContract,
    };
    let contract = state
        .store
        .get("task_episode", task_id)
        .and_then(|episode| serde_json::from_value::<TaskContract>(episode["contract"].clone()).ok())
        .ok_or("실행에 연결된 작업 계약을 찾을 수 없습니다.")?;
    let strategy_revisions = strategies
        .iter()
        .filter_map(|strategy| {
            let revision = StrategyRevision {
                strategy_id: strategy["id"].as_str()?.to_string(),
                revision: strategy["revision"].as_str()?.to_string(),
                parent: strategy["previousRevision"].as_str().map(str::to_string),
                applicability: strategy["verification"]["appliesTo"].clone(),
                authority: strategy["authority"].as_str().unwrap_or("adaptive").to_string(),
                procedure: Some(strategy["verification"].clone()),
                advice: strategy["text"].as_str().map(str::to_string),
                checks: Vec::new(),
                provenance: Vec::new(),
                compatibility: json!({}),
                scope: json!({"sessionId": session_id}),
            };
            revision.validate().ok()?;
            Some(revision)
        })
        .collect::<Vec<_>>();
    let arms = strategies
        .iter()
        .filter_map(|strategy| {
            let kind = strategy["kind"].as_str().unwrap_or("strategy");
            let id = strategy["id"].as_str()?.to_string();
            let policy = kind == "strategy"
                && strategy["assistanceMode"].as_str() == Some("harness_assisted")
                && crate::engine::procedure::applies(strategy, &request.input, None, None);
            // Every injected arm must appear, even if a legacy record predates
            // the revision field. A missing revision is derived, not dropped.
            let revision = strategy["revision"]
                .as_str()
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| {
                    crate::engine::memory::stable_id(&[
                        &id,
                        strategy["text"].as_str().unwrap_or(""),
                    ])
                });
            Some(BundleArm {
                arm_id: id,
                kind: kind.to_string(),
                authority: strategy["authority"]
                    .as_str()
                    .unwrap_or("adaptive")
                    .to_string(),
                revision,
                role: if policy { "policy" } else { "advice" }.to_string(),
                probability: strategy["selectionProbability"].as_f64().unwrap_or(1.0),
            })
        })
        .collect::<Vec<_>>();
    let bundle_id = format!("bundle:{execution_id}");
    let bundle = PolicyBundle {
        id: bundle_id.clone(),
        task_id: task_id.to_string(),
        slot: "strategy".into(),
        policy_revision: "legacy-bundle-v1".into(),
        selection: "deterministic".into(),
        arms,
        probability: 1.0,
        rng_seed: 0,
        context: json!({"sessionId": session_id, "armCount": strategies.len()}),
    };
    // The deterministic injector always plays the whole bundle, so the played
    // arm is the bundle itself with propensity 1. Individual arms are listed
    // inside the bundle and are not misreported as sampled arms.
    let decision = Decision {
        id: execution_id.to_string(),
        task_id: task_id.to_string(),
        slot: "strategy".into(),
        context: json!({
            "sessionId": session_id,
            "bundleId": bundle_id,
            "selection": "deterministic",
            "appliedArmIds": bundle.arms.iter().map(|arm| arm.arm_id.clone()).collect::<Vec<_>>(),
        }),
        candidates: vec![CandidateScore {
            arm_id: bundle_id.clone(),
            mean: 0.5,
            uncertainty: 0.5,
            cost: 0.0,
            risk: 0.0,
            probability: 1.0,
        }],
        selected: bundle_id,
        probability: 1.0,
        policy_revision: "legacy-bundle-v1".into(),
        rng_seed: 0,
    };
    let completeness = json!({
        "context": "recorded",
        "memory": "recorded",
        "model": "recorded",
        "harnessOutput": "recorded as execution_operation evidence after this freeze",
        "providerTemplate": "unavailable: the provider may apply its own chat template outside the engine",
        "reproducible": true,
    });
    let snapshot_hash = revision_hash(&json!({
        "schemaVersion": 2,
        "contract": contract,
        "strategies": strategy_revisions,
        "decisions": [decision.clone()],
        "contextItems": context_items,
        "memoryWatermark": memory_watermark,
        "model": model,
        "completeness": completeness,
        "verificationOwner": request.verification_owner,
    }));
    let snapshot = SelectionSnapshot {
        contract: contract.clone(),
        strategies: strategy_revisions,
        decisions: vec![decision.clone()],
        context_items,
        memory_watermark,
        model,
        policy_bundle: Some(bundle),
        completeness,
        snapshot_hash,
    };
    if contract.validate().is_err() || decision.validate().is_err() || snapshot.validate().is_err() {
        return Err("실행 선택 스냅샷이 계약을 충족하지 못했습니다.".into());
    }
    if !snapshot.is_reproducible() {
        return Err("실행 선택 스냅샷의 재현 정보가 불완전합니다.".into());
    }
    if state
        .store
        .put(
            "strategy_decision",
            &serde_json::to_value(&decision).unwrap_or_else(|_| json!({})),
        )
        .is_none()
    {
        return Err("정책 결정을 저장할 수 없습니다.".into());
    }
    if state
        .store
        .put(
            "selection_snapshot",
            &serde_json::to_value(&snapshot).unwrap_or_else(|_| json!({})),
        )
        .is_none()
    {
        return Err("실행 선택 스냅샷을 저장할 수 없습니다.".into());
    }
    Ok(())
}

