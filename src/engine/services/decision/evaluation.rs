//! Read-only evaluation derived from persisted real native execution records.
use crate::state::AppState;
use serde_json::{json,Value};
use std::collections::HashMap;
fn duration_ms(row:&Value)->Option<u64>{
    let start=chrono::DateTime::parse_from_rfc3339(row["startedAt"].as_str()?).ok()?;
    let finish=chrono::DateTime::parse_from_rfc3339(row["finishedAt"].as_str()?).ok()?;
    u64::try_from((finish-start).num_milliseconds()).ok()
}
fn root_trials(records:Vec<Value>)->Vec<Value>{
    let mut by_root=HashMap::<String,Value>::new();
    for row in records{
        let Some(id)=row["taskId"].as_str().or_else(||row["id"].as_str()) else{continue;};
        let at=row["finishedAt"].as_str().or_else(||row["startedAt"].as_str()).unwrap_or("");
        if by_root.get(id).is_none_or(|old|{
            let previous=old["finishedAt"].as_str()
                .or_else(||old["startedAt"].as_str()).unwrap_or("");
            at>previous
        }){by_root.insert(id.to_string(),row);}
    }
    let mut rows=by_root.into_values().collect::<Vec<_>>();
    rows.sort_by(|a,b|a["taskId"].as_str().cmp(&b["taskId"].as_str()));
    rows
}
pub fn summary(state:&AppState)->Value{
    let mut passed=0u64;let mut failed=0u64;let mut unverified=0u64;let mut cancelled=0u64;
    let mut model_calls=0u64;let mut tools=0u64;let mut tokens=0u64;
    let mut duration=0u64;let mut timed=0u64;
    let trials=root_trials(state.store.all("execution"));
    let tasks=trials.iter().map(|row|{
        let outcome=row["taskOutcome"].as_str().unwrap_or("unverified");
        let real_verifier=crate::engine::verification::has_result_verifier(&row["acceptanceChecks"]);
        if real_verifier&&outcome=="succeeded"{passed+=1;}
        else if real_verifier&&outcome=="failed"{failed+=1;}
        else if outcome=="cancelled"{cancelled+=1;}
        else{unverified+=1;}
        model_calls+=row["modelCalls"].as_u64().unwrap_or(0);
        tools+=row["toolCalls"].as_u64().unwrap_or(0);
        tokens+=row["tokens"].as_u64().unwrap_or(0);
        let elapsed=duration_ms(row);
        if let Some(ms)=elapsed{duration=duration.saturating_add(ms);timed+=1;}
        json!({"taskId":row["taskId"],"executionId":row["id"],
            "providerId":row["providerId"],"model":row["model"],
            "taskOutcome":outcome,"verified":real_verifier,
            "modelCalls":row["modelCalls"],"toolCalls":row["toolCalls"],
            "tokens":row["tokens"],"durationMs":elapsed,
            "costUsd":Value::Null})
    }).collect::<Vec<_>>();
    let denom=passed+failed;
    let costs=state.store.usage_summary();
    json!({"schemaVersion":1,"rootTasks":tasks.len(),
        "verified":{"passed":passed,"failed":failed,"trials":denom,
            "successRate":if denom>0{json!(passed as f64/denom as f64)}else{Value::Null}},
        "unverified":unverified,"cancelled":cancelled,
        "observedTotals":{"modelCalls":model_calls,"toolCalls":tools,"tokens":tokens,
            "meanDurationMs":if timed>0{json!(duration/timed)}else{Value::Null},
            "durationSamples":timed},
        "pricingCoverage":{"pricedCalls":costs["pricedCalls"],"unpricedCalls":costs["unpricedCalls"],
            "pricedSubtotalUsd":costs["pricedSubtotalUsd"],
            "configuredCostUsd":costs["configuredCostUsd"],
            "source":"user_configured_prices_exclude_gpu_and_provider_invoices"},
        "tasks":tasks,
        "benchmarkEvidence":"persisted_native_verifier_outcomes_no_synthetic_quality_labels"})
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn one_root_one_latest_attempt_and_real_elapsed(){
        let rows=vec![
            json!({"id":"a","taskId":"root","taskOutcome":"failed",
                "startedAt":"2026-11-16T10:00:00Z","finishedAt":"2026-11-16T10:00:10Z"}),
            json!({"id":"b","taskId":"root","taskOutcome":"succeeded",
                "startedAt":"2026-11-16T10:01:00Z","finishedAt":"2026-11-16T10:01:20Z"})
        ];
        let roots=root_trials(rows);
        assert_eq!(roots.len(),1);
        assert_eq!(roots[0]["id"],"b");
        assert_eq!(duration_ms(&roots[0]),Some(20_000));
    }
}
