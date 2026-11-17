//! Evidence-derived guidance for a *next inspection*, not a licence to retry
//! an unknown effect or to claim that an observed failure proves a cause.
//! This runs locally and never invokes a model or creates a background loop.
use crate::Receipt;
use serde_json::{json,Value};

#[derive(Debug)]
pub(crate) struct FailureGuidance {
    pub receipt_id:String,
    pub tool:String,
    pub category:&'static str,
    pub next_probe:&'static str,
    pub probe_tools:&'static [&'static str],
    pub error_excerpt:String,
}
impl FailureGuidance {
    pub fn model_view(&self)->Value {
        json!({"evidenceReceiptId":self.receipt_id,
            "failedTool":self.tool,"observedFailure":self.category,
            "errorExcerpt":self.error_excerpt,
            "suggestedNextInspection":self.next_probe,
            "diagnosisStatus":"rule_based_observation_not_verified_cause"})
    }
}
fn diagnostic(receipt:&Receipt)->String {
    receipt.result["error"].as_str()
        .map(str::to_string).unwrap_or_else(||receipt.result.to_string())
}
fn tail(input:&str,max:usize)->String {
    let mut chars=input.chars().rev().take(max+1).collect::<Vec<_>>();
    let truncated=chars.len()>max;
    if truncated {chars.truncate(max);}
    chars.reverse();
    let short=chars.into_iter().collect::<String>();
    if truncated{format!("[earlier details omitted] {short}")}else{short}
}
fn classify(tool:&str,content:&str)
 ->(&'static str,&'static str,&'static [&'static str]){
    let t=content.to_lowercase();
    if t.contains("error[e")||t.contains("could not compile")
      ||t.contains("syntaxerror")||t.contains("syntax error"){
        return ("compiler_diagnostic",
         "Read the cited source line and dependent symbols before editing; do not rerun an unchanged failing check.",
         &["workspace_read_range","code_search"]);
    }
    if t.contains("test result: failed")||t.contains("assertion failed")
      ||t.contains("panicked at")||t.contains("tests failed"){
        return ("test_assertion_failure",
         "Inspect the failing test and implementation; repair before rerunning the same test.",
         &["workspace_read_range","code_search"]);
    }
    if t.contains("no such file")||t.contains("not found: file")
      ||t.contains("enoent")||t.contains("os error 2"){
        return ("missing_path",
         "Inspect the workspace tree and locate the actual path; do not invent replacement files.",
         &["workspace_list","code_search"]);
    }
    if t.contains("permission denied")||t.contains("not approved")
      ||t.contains("approval required")||t.contains("forbidden")
      ||t.contains("unauthorized")||t.contains("access denied"){
        return ("permission_or_approval",
         "Review the owner-granted scope or request approval; do not widen permissions.",
         &[]);
    }
    if t.contains("invalid arguments")||t.contains("schema")
      ||t.contains("missing required")||t.contains("unexpected token"){
        return ("argument_contract",
         "Inspect the authorized tool schema; change invalid arguments before dispatch.",
         &[]);
    }
    if t.contains("429")||t.contains("rate limit")
      ||t.contains("timeout")||t.contains("timed out")
      ||t.contains("http 503")||t.contains("service unavailable"){
        return ("provider_or_transport",
         "Check service availability and the retry budget; the prior external effect may be uncertain.",
         &["account_status"]);
    }
    if tool=="shell_exec"||tool=="terminal_exec"{
        return ("command_failed",
         "Inspect command diagnostics and affected files before a changed action.",
         &["workspace_read_range","code_search"]);
    }
    ("tool_failed","Inspect the failed receipt and tool contract before a changed action.",
     &["dominion_receipt_read"])
}

/// The most recent actionable failure, suppressed when a subsequent observed
/// source mutation has already repaired the diagnostic context. Every original
/// result stays in the complete journal for evidence replay.
pub(crate) fn latest(receipts:&[Receipt])->Option<FailureGuidance>{
    for (index,receipt) in receipts.iter().enumerate().rev(){
        if receipt.success||receipt.result["skipped"]==true{continue;}
        let raw=diagnostic(receipt);
        let (category,next_probe,probe_tools)=classify(&receipt.tool,&raw);
        if matches!(category,
           "compiler_diagnostic"|"test_assertion_failure"|"missing_path")
           && receipts[index+1..].iter().any(|later|later.success
             && matches!(later.tool.as_str(),
                "workspace_write"|"workspace_replace"|"workspace_replace_range"
                |"file_write"|"github_file_commit")) {
            return None;
        }
        return Some(FailureGuidance{
            receipt_id:receipt.id.clone(),tool:receipt.tool.clone(),
            category,next_probe,probe_tools,error_excerpt:tail(&raw,220),
        });
    }
    None
}

#[cfg(test)]
mod tests{
    use super::*;
    fn failure(id:&str,tool:&str,error:&str)->Receipt{
        Receipt{id:id.into(),tool:tool.into(),arguments:json!({}),
            success:false,result:json!({"error":error}),step:1}
    }
    #[test]
    fn unseen_compiler_test_missing_permission_and_provider_errors_are_distinguished(){
        for (tool,error,expected) in [
            ("shell_exec","error[E0308]: mismatched types","compiler_diagnostic"),
            ("shell_exec","test result: FAILED; 1 failed","test_assertion_failure"),
            ("workspace_read","No such file or directory","missing_path"),
            ("mcp__service","permission denied","permission_or_approval"),
            ("mcp__service","HTTP 429 rate limit exceeded","provider_or_transport"),
            ("mcp__service","invalid arguments schema mismatch","argument_contract"),
        ]{
            let result=latest(&[failure("f1",tool,error)]).unwrap();
            assert_eq!(result.category,expected);
            assert_eq!(result.model_view()["diagnosisStatus"],
                "rule_based_observation_not_verified_cause");
            assert!(result.model_view().to_string().len()<900);
        }
    }
    #[test]
    fn repaired_diagnostic_is_not_reintroduced_and_permission_stays_closed(){
        let old=failure("compiler","shell_exec","error[E0308]: mismatched types");
        let changed=Receipt{id:"edit".into(),tool:"workspace_replace_range".into(),
            arguments:json!({}),success:true,result:json!({"written":true}),step:2};
        assert!(latest(&[old,changed]).is_none());
        let denied=failure("denied","mcp__private",
            &format!("{}permission denied","build log ".repeat(1000)));
        let hint=latest(&[denied]).unwrap();
        assert!(hint.probe_tools.is_empty());
        assert!(hint.error_excerpt.contains("permission denied"));
        assert!(hint.error_excerpt.chars().count()<260);
    }
}
