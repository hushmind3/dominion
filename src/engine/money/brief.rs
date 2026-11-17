//! Compact, user-facing business lead summaries. Model text is only a
//! hypothesis until checked; this module never executes the suggested work.
use serde_json::{json,Value};

fn field(row:&Value,key:&str,limit:usize)->String{
    row[key].as_str().unwrap_or("").trim().chars().take(limit).collect()
}
pub fn summarize(candidates:&[Value],max:usize)->Vec<Value>{
    candidates.iter().take(max.min(8)).map(|item|{
        let implementation=field(item,"implementationPlan",800);
        let fallback=field(item,"firstTest",800);
        let source_matches=item["buyerEvidence"].as_array().into_iter().flatten()
            .filter(|e|crate::engine::money_buyer::buying_signal(e)).count();
        json!({"candidateId":item["id"],
            "title":field(item,"title",180),
            "discovery":field(item,"offer",450),
            "buyer":field(item,"firstBuyer",250),
            "whyMoney":field(item,"whyPay",550),
            "implementation":if implementation.is_empty(){fallback}else{implementation},
            "firstPaidStep":field(item,"firstPaymentStep",600),
            "sourceMatchedBuyingSignals":source_matches,
            "evidenceGrade":if source_matches>0{"archived_quote_match"}
                else{"hypothesis_unverified"},
            "executionPolicy":"report_only_user_approval_before_business_implementation"})
    }).collect()
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test] fn concise_leads_never_auto_authorize_business_actions(){
        let data=vec![json!({"id":"one","title":"Local document reconciliation",
            "offer":"Invoice and delivery note review service",
            "firstBuyer":"Operations manager","whyPay":"Avoids manual reconciliation",
            "implementationPlan":"Parse inputs, compare fields, export mismatch report",
            "firstPaymentStep":"Ask for a paid pilot","buyerEvidence":[]})];
        let result=summarize(&data,8);
        assert_eq!(result.len(),1);
        assert_eq!(result[0]["evidenceGrade"],"hypothesis_unverified");
        assert_eq!(result[0]["executionPolicy"],"report_only_user_approval_before_business_implementation");
        assert!(result[0]["implementation"].as_str().unwrap().contains("compare fields"));
    }
    #[test] fn source_match_does_not_change_payment_policy(){
        let data=vec![json!({"id":"two",
            "buyerEvidence":[{"type":"official_budget",
                "observationStatus":"source_excerpt_buyer_intent"}]})];
        let result=summarize(&data,2);
        assert_eq!(result[0]["sourceMatchedBuyingSignals"],1);
        assert_eq!(result[0]["evidenceGrade"],"archived_quote_match");
        assert!(result[0]["executionPolicy"].as_str().unwrap().contains("user_approval"));
    }
}
