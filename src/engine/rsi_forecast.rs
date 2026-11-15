//! Pre-modification forecasting for RSI generations.
//!
//! Inspired by "Predict Before You Modify": the coordinator records the gain it
//! expects before a candidate is built, then learns from the signed bias and
//! absolute error measured by the independent paired evaluation.

use serde_json::{json, Value};

/// Forecast training uses the evaluator's measured outcome, never a cached
/// error/bias field that can be stale after replay or rewritten by a child.
fn measured_holdout_gain(row:&Value)->Option<f64>{
    if row["evaluation"]["source"]!="server_invoked_local_paired_git_worktrees"
        || row["evaluation"]["metrics"]["eligible"]!=true {
        return None;
    }
    row["evaluation"]["metrics"]["improvedHoldouts"]
        .as_u64().map(|gain| gain as f64)
}

/// Reconstruct signed error from the forecast made before the run and the
/// frozen paired measurement. Missing, non-finite and unmeasured rows are
/// not treated as improvements or calibration evidence.
fn stats<'a>(rows: impl Iterator<Item = &'a Value>) -> Option<(f64, f64)> {
    let (mut error, mut bias, mut count) = (0.0, 0.0, 0u64);
    for row in rows {
        let (Some(predicted),Some(actual))=(
            row["forecast"]["predictedImprovedHoldouts"].as_f64(),
            measured_holdout_gain(row)
        ) else {continue};
        if !predicted.is_finite() || predicted<0.0 {continue;}
        let delta=predicted-actual;
        error+=delta.abs();
        bias+=delta;
        count+=1;
    }
    (count>0).then(||(error/count as f64,bias/count as f64))
}

/// Measured forecast error and signed bias for a domain.
///
/// Only generations that actually carried a forecast contribute. A domain with
/// no forecast history reports zeros rather than a fabricated confidence.
pub(crate) fn domain_error(history: &[Value], domain: &str) -> (f64, f64) {
    stats(history.iter().filter(|row| row["domain"] == domain))
        .unwrap_or((0.0, 0.0))
}

/// Predict the next generation's verified holdout gain from measured history.
///
/// The signed bias is subtracted, so a coordinator that repeatedly over-predicts
/// stops re-committing the same overestimate. Without measurements the forecast
/// is zero and explicitly labelled `no_measured_history`.
pub(crate) fn forecast_next(history: &[Value], domain: &str, basis: &str) -> Value {
    // Prefer observed outcomes in the chosen domain, even if that domain
    // has not yet accumulated a previous explicit forecast. Fall back to
    // other domains only if there are no measured local trials at all.
    let measured=history.iter()
        .filter(|row|measured_holdout_gain(row).is_some())
        .collect::<Vec<_>>();
    let domain_rows=measured.iter().copied()
        .filter(|row|row["domain"].as_str()==Some(domain))
        .collect::<Vec<_>>();
    let pool=if domain_rows.is_empty(){&measured}else{&domain_rows};
    let actuals=pool.iter().filter_map(|row|measured_holdout_gain(row))
        .collect::<Vec<_>>();
    if actuals.is_empty() {
        return json!({
            "domain": domain,
            "expectedImprovedHoldouts": 0.0,
            "priorError": Value::Null,
            "priorBias": Value::Null,
            "basis": "no_measured_history",
            "decisionBasis": basis,
        });
    }
    let mean = actuals.iter().sum::<f64>() / actuals.len() as f64;
    let measured_error=stats(domain_rows.iter().copied().filter(|row|
        row["forecast"]["domain"].as_str()==Some(domain)));
    let (prior_error, bias) = measured_error.unwrap_or((0.0, 0.0));
    json!({
        "domain": domain,
        "expectedImprovedHoldouts": (mean - bias).max(0.0),
        "priorError": measured_error.map(|(error, _)| error),
        "priorBias": measured_error.map(|(_, bias)| bias),
        "basis": if domain_rows.is_empty() { "measured_global_history" } else { "measured_domain_history" },
        "decisionBasis": basis,
    })
}

/// Attach the forecast that preceded this generation and the measured outcome.
///
/// Returns false when either side lacks a number, so an unmeasured or
/// unpreceded generation never earns a prediction-error record.
pub(crate) fn attach(generation: &mut Value, prior: &Value) -> bool {
    let Some(expected)=prior["expectedImprovedHoldouts"].as_f64() else {
        return false;
    };
    let Some(actual)=measured_holdout_gain(generation) else {
        return false;
    };
    if !expected.is_finite() || expected<0.0
        || prior["domain"].as_str().is_none()
        || prior["domain"]!=generation["domain"] {
        return false;
    }
    generation["forecast"] = json!({
        "domain": prior["domain"],
        "predictedImprovedHoldouts": expected,
        "actualImprovedHoldouts": actual,
        "error": (expected - actual).abs(),
        "bias": expected - actual,
        "basis": prior["basis"],
    });
    true
}

#[cfg(test)]
mod tests {
    use super::{attach, domain_error, forecast_next};
    use serde_json::json;

    fn history() -> Vec<serde_json::Value> {
        vec![
            json!({"domain":"memory","forecast":{"domain":"memory","predictedImprovedHoldouts":3.0,
                "actualImprovedHoldouts":1.0,"error":2.0,"bias":2.0},
                "evaluation":{"source":"server_invoked_local_paired_git_worktrees","metrics":{"eligible":true,"improvedHoldouts":1}}}),
            json!({"domain":"memory","forecast":{"domain":"memory","predictedImprovedHoldouts":3.0,
                "actualImprovedHoldouts":2.0,"error":1.0,"bias":1.0},
                "evaluation":{"source":"server_invoked_local_paired_git_worktrees","metrics":{"eligible":true,"improvedHoldouts":2}}}),
        ]
    }

    #[test]
    fn forecast_corrects_measured_bias() {
        let forecast = forecast_next(&history(), "memory", "accepted_implementation");
        assert_eq!(forecast["basis"], "measured_domain_history");
        // Mean measured gain 1.5 minus mean bias 1.5 leaves no expected gain.
        assert_eq!(forecast["expectedImprovedHoldouts"].as_f64(), Some(0.0), "{forecast}");
        assert_eq!(forecast["priorError"].as_f64(), Some(1.5));
    }

    #[test]
    fn forecast_without_measurements_claims_nothing() {
        let forecast = forecast_next(&[], "tools", "no_progress");
        assert_eq!(forecast["basis"], "no_measured_history");
        assert_eq!(forecast["expectedImprovedHoldouts"].as_f64(), Some(0.0));
        assert!(forecast["priorError"].is_null());
    }

    #[test]
    fn domain_error_only_counts_forecast_rows() {
        let (error, bias) = domain_error(&history(), "memory");
        assert_eq!(error, 1.5);
        assert_eq!(bias, 1.5);
        assert_eq!(domain_error(&history(), "tools"), (0.0, 0.0));
    }

    #[test]
    fn attach_needs_both_a_forecast_and_a_measurement() {
        let mut generation = json!({"domain":"memory","evaluation":{"source":"server_invoked_local_paired_git_worktrees","metrics":{"eligible":true,"improvedHoldouts":2}}});
        assert!(!attach(&mut generation, &json!({})));
        assert!(generation.get("forecast").is_none());
        let prior = json!({"domain":"memory","expectedImprovedHoldouts":3.0,"basis":"measured_domain_history"});
        assert!(attach(&mut generation, &prior));
        assert_eq!(generation["forecast"]["error"].as_f64(), Some(1.0));
        assert_eq!(generation["forecast"]["bias"].as_f64(), Some(1.0));
    }
    #[test]
    fn cached_error_and_bias_cannot_poison_calibration(){
        let mut rows=history();
        rows[0]["forecast"]["error"]=json!(99999.0);
        rows[1]["forecast"]["bias"]=json!(-99999.0);
        assert_eq!(domain_error(&rows,"memory"),(1.5,1.5));
        rows[0]["evaluation"]["source"]=json!("model_claim");
        assert_eq!(domain_error(&rows,"memory"),(1.0,1.0));
    }

    #[test]
    fn measured_domain_outcomes_precede_unrelated_forecasts(){
        let mut rows=history();
        rows.push(json!({"domain":"tools",
            "evaluation":{"source":"server_invoked_local_paired_git_worktrees",
                "metrics":{"eligible":true,"improvedHoldouts":7}}}));
        let forecast=forecast_next(&rows,"tools","retry");
        assert_eq!(forecast["basis"],"measured_domain_history");
        assert_eq!(forecast["expectedImprovedHoldouts"],7.0);
        // Invalid evaluator results are not trusted as cross-domain history.
        rows[2]["evaluation"]["source"]=json!("model_claim");
        let forecast=forecast_next(&rows,"tools","retry");
        assert_eq!(forecast["basis"],"measured_global_history");
        assert_eq!(forecast["expectedImprovedHoldouts"],1.5);
    }

    #[test]
    fn off_domain_or_unverified_results_do_not_train_predictor(){
        let prior=json!({"domain":"memory",
            "expectedImprovedHoldouts":3.0,"basis":"measured_domain_history"});
        let mut generation=json!({"domain":"tools",
            "evaluation":{"source":"server_invoked_local_paired_git_worktrees",
                "metrics":{"eligible":true,"improvedHoldouts":1}}});
        assert!(!attach(&mut generation,&prior));
        generation["domain"]=json!("memory");
        generation["evaluation"]["metrics"]["eligible"]=json!(false);
        assert!(!attach(&mut generation,&prior));
        assert!(generation.get("forecast").is_none());
    }

}
