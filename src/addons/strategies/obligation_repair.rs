//! Host adapter: native effect classification and resource identity stay authoritative.
use serde_json::Value;
pub fn reconstruct(checks:&Value,gaps:&[String],operations:&[Value])->Value{
    dominion_algorithms::obligation_repair::reconstruct(checks,gaps,operations,
        crate::engine::effect::resource_key,crate::engine::effect::is_effectful)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn gap(check:&Value)->String {
        format!("검증 조건 미충족: {}",check)
    }

    #[test]
    fn a_successful_write_is_not_success_if_the_original_verifier_still_fails() {
        let check=json!({"type":"file_contains","path":"report.md","contains":"ready"});
        let ops=vec![
            json!({"id":"read","tool":"workspace_read",
                "arguments":{"path":"report.md"},"opSeq":1,"status":"completed"}),
            json!({"id":"write","tool":"workspace_write",
                "arguments":{"path":"report.md","content":"wrong"},"opSeq":2,"status":"completed"}),
            json!({"id":"reread","tool":"workspace_read",
                "arguments":{"path":"report.md"},"opSeq":3,"status":"completed"}),
            json!({"id":"unrelated","tool":"workspace_write",
                "arguments":{"path":"elsewhere.md"},"opSeq":4,"status":"completed"}),
        ];
        let g=reconstruct(&json!([check.clone()]),&[gap(&check)],&ops);
        let frontier=&g["frontiers"][0];
        assert_eq!(frontier["kind"],"postcondition_contradicts_mutation");
        assert_eq!(frontier["receiptIds"],json!(["read","write","reread"]));
        assert_eq!(g["graph"]["edges"].as_array().unwrap().len(),3);
        assert!(g["directive"].as_str().unwrap().contains("frozen verifier remains false"));
        assert_eq!(g["verified"],false);
    }

    #[test]
    fn unresolved_prior_effect_prevents_direct_retry_guidance() {
        let check=json!({"type":"file_exists","path":"output.json"});
        let ops=vec![json!({
            "id":"unsafe","tool":"workspace_write",
            "arguments":{"path":"output.json","content":"{}"},
            "status":"interrupted","outcomeUnknown":true,"opSeq":1
        })];
        let result=reconstruct(&json!([check.clone()]),&[gap(&check)],&ops);
        assert_eq!(result["frontiers"][0]["kind"],"unresolved_external_effect");
        assert!(result["directive"].as_str().unwrap().contains("Inspect/reconcile"));
    }

    #[test]
    fn independent_targets_cannot_supply_a_fake_proof() {
        let check=json!({"type":"file_equals","path":"expected.rs","content":"ok"});
        let ops=vec![json!({"id":"foreign","tool":"workspace_write",
            "arguments":{"path":"different.rs","content":"ok"},"opSeq":1,"status":"completed"})];
        let result=reconstruct(&json!([check.clone()]),&[gap(&check)],&ops);
        assert_eq!(result["frontiers"][0]["kind"],"no_attributable_receipt");
        assert!(result["frontiers"][0]["receiptIds"].as_array().unwrap().is_empty());
        assert_eq!(result["graph"]["edges"],json!([]));
    }

    #[test]
    fn satisfied_checks_do_not_appear_as_failed_obligations() {
        let a=json!({"type":"file_exists","path":"a.rs"});
        let b=json!({"type":"file_exists","path":"b.rs"});
        let result=reconstruct(&json!([a.clone(),b.clone()]),&[gap(&b)],&[]);
        assert_eq!(result["frontiers"].as_array().unwrap().len(),1);
        assert_eq!(result["frontiers"][0]["resource"],"workspace:b.rs");
    }

    #[test]
    fn two_failed_criteria_for_one_resource_share_the_same_receipt_lineage() {
        let a=json!({"type":"file_exists","path":"x.rs"});
        let b=json!({"type":"file_contains","path":"x.rs","contains":"ready"});
        let result=reconstruct(&json!([a.clone(),b.clone()]),&[gap(&a),gap(&b)],&[]);
        assert_eq!(result["frontiers"].as_array().unwrap().len(),1);
        assert_eq!(result["frontiers"][0]["unmetObligations"].as_array().unwrap().len(),2);
    }

    #[test]
    fn only_verifier_failures_can_open_a_repair_frontier() {
        let check=json!({"type":"file_exists","path":"x.rs"});
        let result=reconstruct(&json!([check]),&["free-form model said failure".to_string()],&[]);
        assert!(result["frontiers"].as_array().unwrap().is_empty());
    }
}
