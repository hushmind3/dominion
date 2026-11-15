//! Pre-modification forecasting for RSI generations.
//!
//! Inspired by "Predict Before You Modify": the coordinator records the gain it
//! expects before a candidate is built, then learns from the signed bias and
//! absolute error measured by the independent paired evaluation.

use serde_json::{json, Value};

/// A numeric report is not automatically a measured improvement. Forecasts may
/// only learn from a candidate actually evaluated by the independent frozen
/// paired-worktree runner. Adopted descendants additionally require a
/// revalidated source receipt, so a mutable worker summary is never a reward.
fn verified_gain(row: &Value) -> Option<f64> {
    if row["evaluation"]["source"] != "server_invoked_local_paired_git_worktrees"
        || row["candidateId"].as_str().is_none_or(str::is_empty)
    {
        return None;
    }
    if row["accepted"] == true
        && (row["pairedRevalidated"] != true
            || row["evaluation"]["metrics"]["eligible"] != true)
    {
        return None;
    }
    // A rejected, independently evaluated candidate is an observed zero or
    // regression, not an unmeasured success. Its measured gains still count
    // when the evaluator provides the frozen holdout metric.
    row["evaluation"]["metrics"]["improvedHoldouts"]
        .as_u64()
        .map(|gain| gain.min(64) as f64)
}

/// Only compare a forecast with evidence about the domain it actually
/// predicted. This also excludes legacy/unverified summary-only forecasts.
fn measured_error(row: &Value, domain: &str) -> Option<(f64, f64)> {
    if row["forecast"]["domain"].as_str() != Some(domain)
        || row["domain"].as_str() != Some(domain)
    {
        return None;
    }
    let actual = verified_gain(row)?;
    let predicted = row["forecast"]["predictedImprovedHoldouts"].as_f64()?;
    if !predicted.is_finite() || predicted < 0.0 {
        return None;
    }
    Some(((predicted - actual).abs(), predicted - actual))
}

/// Mean absolute prediction error and signed bias. Never attribute an
/// unrelated domain's outcome to this one.
fn stats<'a>(rows: impl Iterator<Item = &'a Value>, domain: &str) -> Option<(f64, f64)> {
    let (mut error, mut bias, mut count) = (0.0, 0.0, 0u64);
    for row in rows {
        let Some((row_error, row_bias)) = measured_error(row, domain) else {
            continue;
        };
        error += row_error;
        bias += row_bias;
        count += 1;
    }
    (count > 0).then(|| (error / count as f64, bias / count as f64))
}

/// Measured forecast error and signed bias for a domain. Only eligible frozen
/// evaluation receipts contribute; missing data has no imaginary confidence.
pub(crate) fn domain_error(history: &[Value], domain: &str) -> (f64, f64) {
    stats(history.iter(), domain).unwrap_or((0.0, 0.0))
}

/// Predict future frozen-holdout gains from recent measurements of the SAME
/// domain. A global mean leaks unrelated domains' results into a novel target;
/// without domain evidence the only defensible prediction is zero. Recent
/// evidence is bounded so early generations cannot dominate indefinitely.
pub(crate) fn forecast_next(history: &[Value], domain: &str, basis: &str) -> Value {
    const WINDOW: usize = 8;
    let recent = history.iter().rev()
        .filter(|row| row["domain"].as_str() == Some(domain))
        .filter_map(|row| verified_gain(row).map(|gain| (row, gain)))
        .take(WINDOW)
        .collect::<Vec<_>>();
    if recent.is_empty() {
        return json!({
            "domain": domain,
            "expectedImprovedHoldouts": 0.0,
            "priorError": Value::Null,
            "priorBias": Value::Null,
            "sampleCount": 0,
            "basis": "no_measured_history",
            "decisionBasis": basis,
        });
    }
    let mean = recent.iter().map(|(_, gain)| gain).sum::<f64>() / recent.len() as f64;
    let max_observed = recent.iter().map(|(_, gain)| *gain).fold(0.0_f64, f64::max);
    let calibration = stats(recent.iter().map(|(row, _)| *row), domain);
    let bias = calibration.map(|(_, bias)| bias).unwrap_or(0.0);
    json!({
        "domain": domain,
        "expectedImprovedHoldouts": (mean - bias).clamp(0.0, max_observed),
        "priorError": calibration.map(|(error, _)| error),
        "priorBias": calibration.map(|(_, bias)| bias),
        "sampleCount": recent.len(),
        "basis": "measured_domain_history",
        "decisionBasis": basis,
    })
}

/// Attach only a causally matched forecast and real paired measurement.
/// Failed, missing or unverified evaluations do not calibrate the improver.
pub(crate) fn attach(generation: &mut Value, prior: &Value) -> bool {
    let Some(domain) = prior["domain"].as_str() else {
        return false;
    };
    if generation["domain"].as_str() != Some(domain) {
        return false;
    }
    let Some(expected) = prior["expectedImprovedHoldouts"].as_f64() else {
        return false;
    };
    if !expected.is_finite() || expected < 0.0 {
        return false;
    }
    let Some(actual) = verified_gain(generation) else {
        return false;
    };
    generation["forecast"] = json!({
        "domain": domain,
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
    use serde_json::{json, Value};

    fn measured(domain: &str, gain: u64, predicted: f64) -> Value {
        json!({
            "domain": domain,
            "accepted": true,
            "pairedRevalidated": true,
            "candidateId": "candidate-1",
            "evaluation": {
                "source": "server_invoked_local_paired_git_worktrees",
                "metrics": {"eligible": true, "improvedHoldouts": gain}
            },
            "forecast": {
                "domain": domain, "predictedImprovedHoldouts": predicted,
                "actualImprovedHoldouts": gain, "error": (predicted - gain as f64).abs(),
                "bias": predicted - gain as f64
            }
        })
    }

    fn history() -> Vec<Value> {
        vec![measured("memory", 1, 3.0), measured("memory", 2, 3.0)]
    }

    #[test]
    fn forecast_corrects_verified_domain_bias() {
        let forecast = forecast_next(&history(), "memory", "accepted_implementation");
        assert_eq!(forecast["basis"], "measured_domain_history");
        assert_eq!(forecast["sampleCount"], 2);
        assert_eq!(forecast["expectedImprovedHoldouts"].as_f64(), Some(0.0));
        assert_eq!(forecast["priorError"].as_f64(), Some(1.5));
    }

    #[test]
    fn unrelated_domains_never_bootstrap_expected_gain() {
        let forecast = forecast_next(&history(), "tools", "no_progress");
        assert_eq!(forecast["basis"], "no_measured_history");
        assert_eq!(forecast["expectedImprovedHoldouts"].as_f64(), Some(0.0));
        assert!(forecast["priorError"].is_null());
        assert_eq!(forecast["sampleCount"], 0);
    }

    #[test]
    fn forged_unpaired_and_summary_only_reports_cannot_train_forecasts() {
        let forged = json!({
            "domain": "memory", "accepted": true,
            "candidateId": "candidate-2",
            "evaluation": {
                "source": "server_invoked_local_paired_git_worktrees",
                "metrics": {"eligible": true, "improvedHoldouts": 64}
            },
            "forecast": {"domain": "memory", "predictedImprovedHoldouts": 64.0}
        });
        let mut summary_only = measured("memory", 64, 64.0);
        summary_only["evaluation"]["source"] = json!("worker_summary");
        let history = vec![forged, summary_only];
        assert_eq!(forecast_next(&history, "memory", "none")["sampleCount"], 0);
        assert_eq!(domain_error(&history, "memory"), (0.0, 0.0));
    }

    #[test]
    fn independent_rejections_count_as_zero_when_actually_measured() {
        let rejected = json!({
            "domain": "tools", "accepted": false, "candidateId": "candidate-r",
            "evaluation": {
                "source": "server_invoked_local_paired_git_worktrees",
                "metrics": {"eligible": false, "improvedHoldouts": 0}
            }
        });
        let forecast = forecast_next(&[rejected], "tools", "rejected");
        assert_eq!(forecast["sampleCount"], 1);
        assert_eq!(forecast["expectedImprovedHoldouts"], 0.0);
    }

    #[test]
    fn forecast_uses_bounded_recent_verified_history() {
        let mut history = vec![measured("tools", 64, 64.0); 3];
        history.extend((0..8).map(|_| measured("tools", 2, 2.0)));
        let forecast = forecast_next(&history, "tools", "bounded");
        assert_eq!(forecast["sampleCount"], 8);
        assert_eq!(forecast["expectedImprovedHoldouts"], 2.0);
    }

    #[test]
    fn domain_error_only_counts_matching_verified_forecasts() {
        let mut history = history();
        let mut wrong = measured("tools", 4, 6.0);
        wrong["forecast"]["domain"] = json!("memory");
        history.push(wrong);
        assert_eq!(domain_error(&history, "memory"), (1.5, 1.5));
        assert_eq!(domain_error(&history, "tools"), (0.0, 0.0));
    }

    #[test]
    fn attach_requires_the_predicted_domain_and_independent_receipt() {
        let mut generation = measured("memory", 2, 3.0);
        generation.as_object_mut().unwrap().remove("forecast");
        assert!(!attach(&mut generation, &json!({})));
        assert!(generation.get("forecast").is_none());
        let mismatched = json!({
            "domain": "tools", "expectedImprovedHoldouts": 3.0,
            "basis": "measured_domain_history"
        });
        assert!(!attach(&mut generation, &mismatched));
        let prior = json!({
            "domain": "memory", "expectedImprovedHoldouts": 3.0,
            "basis": "measured_domain_history"
        });
        assert!(attach(&mut generation, &prior));
        assert_eq!(generation["forecast"]["error"].as_f64(), Some(1.0));
        assert_eq!(generation["forecast"]["bias"].as_f64(), Some(1.0));
        generation["pairedRevalidated"] = json!(false);
        assert!(!attach(&mut generation, &prior));
    }
}
