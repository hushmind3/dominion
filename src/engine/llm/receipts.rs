//! Execution-scoped receipts for auxiliary model calls. These are measured
//! separately from streamed primary completions, without changing routing.
use crate::{engine::gateway::Usage, state::AppState};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};

/// Persist the intent *before* contacting a paid provider. Each HTTP retry
/// gets a receipt but shares one logical invocation ID.
pub(super) fn begin(
    state: &AppState,
    context: Option<(&str, &str)>,
    invocation: &str,
    attempt: usize,
    provider_id: &str,
    model: &str,
) -> Result<Option<String>, String> {
    let Some((execution_id, phase)) = context else { return Ok(None); };
    let id = format!("aux-model-{invocation}-{attempt}");
    state.store.put("aux_model_call", &json!({
        "id": id, "executionId": execution_id, "logicalId": invocation,
        "attempt": attempt, "phase": phase, "providerId": provider_id,
        "model": model, "status": "in_flight",
        "startedAt": chrono::Utc::now().to_rfc3339()
    })).ok_or("보조 모델 호출 영수증을 저장하지 못했습니다.")?;
    Ok(Some(id))
}

/// An incomplete receipt is deliberately retained if SQLite fails at finish.
/// Unknown remote consumption is never converted into a false zero-cost call.
pub(super) fn finish(
    state: &AppState,
    receipt: Option<&str>,
    status: &str,
    usage: Usage,
    estimated: bool,
    reason: Option<&str>,
) {
    let Some(receipt) = receipt else { return; };
    let Some(mut row) = state.store.get("aux_model_call", receipt) else {
        state.store.audit("model.receipt_missing", json!({"id":receipt}));
        return;
    };
    row["status"] = json!(status);
    row["promptTokens"] = json!(usage.prompt_tokens);
    row["completionTokens"] = json!(usage.completion_tokens);
    row["totalTokens"] = json!(if usage.total_tokens > 0 {
        usage.total_tokens
    } else {
        usage.prompt_tokens.saturating_add(usage.completion_tokens)
    });
    row["estimated"] = json!(estimated);
    row["finishedAt"] = json!(chrono::Utc::now().to_rfc3339());
    if let Some(reason) = reason {
        row["error"] = json!(reason.chars().take(300).collect::<String>());
    }
    if state.store.put("aux_model_call", &row).is_none() {
        state.store.audit("model.receipt_finalize_failed", json!({"id":receipt}));
    }
}

/// Audit summary: a logical helper invocation can retry upstream, but every
/// attempt that returned usage must be charged in the observed token total.
pub(crate) fn summarize(
    rows: &[Value],
    expected_invocations: u32,
    primary_calls: u32,
    primary_tokens: u64,
    primary_estimated: bool,
) -> Value {
    let mut ids = HashSet::new();
    let mut phase_counts = BTreeMap::<String, u32>::new();
    let mut tokens = 0u64;
    let mut prompt = 0u64;
    let mut completion = 0u64;
    let mut estimated = primary_estimated;
    let mut incomplete = false;
    for row in rows {
        if let Some(id) = row["logicalId"].as_str() {
            if ids.insert(id.to_string()) {
                let phase = row["phase"].as_str().unwrap_or("unknown").to_owned();
                *phase_counts.entry(phase).or_default() += 1;
            }
        }
        let status = row["status"].as_str().unwrap_or("in_flight");
        if status != "completed" { incomplete = true; }
        if row["estimated"] == true { estimated = true; }
        prompt = prompt.saturating_add(row["promptTokens"].as_u64().unwrap_or(0));
        completion = completion.saturating_add(row["completionTokens"].as_u64().unwrap_or(0));
        tokens = tokens.saturating_add(row["totalTokens"].as_u64().unwrap_or(0));
    }
    let complete = ids.len() == expected_invocations as usize && !incomplete;
    json!({
        "primaryCalls":primary_calls,
        "auxiliaryLogicalCalls":expected_invocations,
        "auxiliaryObservedCalls":ids.len(),
        "auxiliaryUpstreamAttempts":rows.len(),
        "auxiliaryPhases":phase_counts,
        "primaryTokens":primary_tokens,
        "auxiliaryPromptTokens":prompt,
        "auxiliaryCompletionTokens":completion,
        "auxiliaryObservedTokens":tokens,
        "observedTotalTokens":primary_tokens.saturating_add(tokens),
        "usageComplete":complete,
        "usageEstimated":estimated,
        "tokenScope":"primary_plus_observed_auxiliary",
        "note":"Failures or interrupted requests may incur unreported remote tokens; observed totals are not a guaranteed bill."
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repeated_helper_attempts_count_one_logical_call_but_both_token_spends() {
        let rows = vec![
            json!({"logicalId":"a","phase":"planning","status":"completed","estimated":false,
                "promptTokens":100,"completionTokens":30,"totalTokens":130}),
            json!({"logicalId":"a","phase":"planning","status":"completed","estimated":false,
                "promptTokens":120,"completionTokens":50,"totalTokens":170}),
            json!({"logicalId":"b","phase":"acceptance_review","status":"completed","estimated":true,
                "promptTokens":70,"completionTokens":20,"totalTokens":90}),
        ];
        let result=summarize(&rows,2,4,800,false);
        assert_eq!(result["auxiliaryLogicalCalls"],2);
        assert_eq!(result["auxiliaryUpstreamAttempts"],3);
        assert_eq!(result["auxiliaryObservedTokens"],390);
        assert_eq!(result["observedTotalTokens"],1190);
        assert_eq!(result["auxiliaryPhases"]["planning"],1);
        assert_eq!(result["auxiliaryPhases"]["acceptance_review"],1);
        assert_eq!(result["usageComplete"],true);
        assert_eq!(result["usageEstimated"],true);
    }
    #[test]
    fn incomplete_or_unobserved_helper_attempts_never_claim_full_coverage() {
        let empty=summarize(&[],1,1,99,false);
        assert_eq!(empty["usageComplete"],false);
        let running=summarize(&[json!({"logicalId":"x","phase":"program_repair",
            "status":"in_flight"})],1,1,99,false);
        assert_eq!(running["usageComplete"],false);
        assert_eq!(running["observedTotalTokens"],99);
    }
}
