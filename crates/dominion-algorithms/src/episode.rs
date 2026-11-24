//! Aggregate the host's recorded attempt statuses without inventing success.
use serde_json::Value;
pub fn fold_root_outcome(attempts: &[Value]) -> &'static str {
    let outcome = |name: &str| {
        attempts
            .iter()
            .any(|attempt| attempt["status"].as_str() == Some(name))
    };
    if outcome("succeeded") {
        return "succeeded";
    }
    if outcome("unknown") {
        return "unknown";
    }
    if outcome("running") {
        return "running";
    }
    if outcome("failed") {
        return "failed";
    }
    if attempts.is_empty() {
        return "running";
    }
    if attempts
        .iter()
        .all(|attempt| attempt["status"].as_str() == Some("cancelled"))
    {
        return "cancelled";
    }
    "answered_unverified"
}


#[cfg(test)]
mod tests{
 use super::*;
 use serde_json::json;
 #[test]
 fn multiple_unverified_reports_do_not_become_verified_success(){
  assert_eq!(fold_root_outcome(&[json!({"status":"answered_unverified"}),json!({"status":"answered_unverified"})]),"answered_unverified");
  assert_eq!(fold_root_outcome(&[json!({"status":"failed"}),json!({"status":"unknown"})]),"unknown");
 }
}
