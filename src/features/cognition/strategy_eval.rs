//! Evidence-based comparison of explicitly selected runtime algorithms.
//! Read-only observation; no RSI, automatic model runs or self-promotion.
use serde_json::{json,Value};
use std::collections::BTreeMap;

#[derive(Default)]
struct Score {
    runs:u64,completed:u64,verified:u64,needs_review:u64,
    tool_calls:u64,model_calls:u64,estimated_tokens:u64,
}
pub fn project(jobs:&[Value])->Value{
    let mut by_algorithm=BTreeMap::<String,Score>::new();
    for job in jobs.iter().filter(|job|job["kind"]=="prompt") {
        if !matches!(job["status"].as_str(),Some(
            "completed"|"needs_review"|"failed"|"cancelled")){continue;}
        let Some(snapshot)=job["nativeState"].as_object() else{continue;};
        if snapshot.is_empty(){continue;}
        let id=job["nativeAlgorithm"]["id"].as_str()
            .or_else(||job["input"]["algorithmId"].as_str())
            .unwrap_or("standard-v1");
        if id.len()>128||id.is_empty(){continue;}
        let score=by_algorithm.entry(id.into()).or_default();
        score.runs+=1;
        if job["status"]=="completed"&&job["result"]["status"]=="completed"{
            score.completed+=1;
            let checks=job["result"]["checks"].as_array();
            if checks.is_some_and(|v|!v.is_empty()&&v.iter().all(|c|
                c["satisfied"]==true)){
                score.verified+=1;
            }
        }
        if job["status"]=="needs_review" {score.needs_review+=1;}
        score.model_calls=score.model_calls.saturating_add(
            job["result"]["modelCalls"].as_u64().unwrap_or(0));
        score.tool_calls=score.tool_calls.saturating_add(
            job["result"]["toolCalls"].as_u64().unwrap_or(0));
        score.estimated_tokens=score.estimated_tokens.saturating_add(
            job["result"]["estimatedTokens"].as_u64().unwrap_or(0));
    }
    let rows=by_algorithm.into_iter().map(|(id,runs)|json!({
        "algorithmId":id,"runs":runs.runs,"completed":runs.completed,
        "verified":runs.verified,"needsReview":runs.needs_review,
        "completionRate":runs.completed as f64/runs.runs as f64,
        "verifiedRate":runs.verified as f64/runs.runs as f64,
        "averageModelCalls":runs.model_calls as f64/runs.runs as f64,
        "averageToolCalls":runs.tool_calls as f64/runs.runs as f64,
        "averageEstimatedTokens":runs.estimated_tokens/runs.runs,
        "sampleSufficientForComparison":runs.runs>=20,
    })).collect::<Vec<_>>();
    json!({"version":"strategy-evidence-v1","source":"durable_native_jobs",
        "unit":"per_completed_or_terminal_job","noAutomaticPromotion":true,
        "providerTokenEstimatesNotBilling":true,"strategies":rows})
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test] fn only_real_terminal_native_jobs_count_as_observations(){
        let verified=json!({"kind":"prompt","status":"completed",
            "nativeAlgorithm":{"id":"focused-v1"},"nativeState":{"id":"x"},
            "result":{"status":"completed","modelCalls":2,"toolCalls":3,
                "estimatedTokens":500,"checks":[{"satisfied":true}]}});
        let claimed=json!({"kind":"prompt","status":"completed",
            "nativeAlgorithm":{"id":"focused-v1"},"nativeState":{"id":"y"},
            "result":{"status":"completed","checks":[]}});
        let ignored=json!({"kind":"prompt","status":"queued",
            "nativeState":{"id":"z"},"result":{"status":"completed"}});
        let report=project(&[verified,claimed,ignored]);
        assert_eq!(report["strategies"][0]["runs"],2);
        assert_eq!(report["strategies"][0]["completed"],2);
        assert_eq!(report["strategies"][0]["verified"],1);
        assert_eq!(report["strategies"][0]["averageEstimatedTokens"],250);
        assert_eq!(report["noAutomaticPromotion"],true);
    }
}
