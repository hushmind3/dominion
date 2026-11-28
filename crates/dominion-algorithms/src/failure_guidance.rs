//! Evidence-derived guidance for a *next inspection*, not a licence to retry
//! an unknown effect or to claim that an observed failure proves a cause.
//! This runs locally and never invokes a model or creates a background loop.
use dominion_harness::hosted::Receipt;
use serde_json::{json,Value};

#[derive(Debug)]
pub struct FailureGuidance {
    pub receipt_id:String,
    pub tool:String,
    pub category:&'static str,
    pub next_probe:&'static str,
    pub probe_tools:&'static [&'static str],
    pub error_excerpt:String,
    pub verification_pending_after_edit:bool,
}
impl FailureGuidance {
    /// Candidate inspections are restricted to tools the host advertised.
    pub fn for_catalog(&self,catalog:&[Value])->Value {
        let mut view=self.model_view();
        if let Some(candidates)=view["candidateInspectionTools"].as_array_mut(){
            candidates.retain(|candidate|catalog.iter().any(|tool|
                tool.pointer("/function/name").or_else(||tool.get("name"))==Some(candidate)));
        }
        view
    }
    pub fn model_view(&self)->Value {
        json!({"evidenceReceiptId":self.receipt_id,
            "failedTool":self.tool,"observedFailure":self.category,
            "errorExcerpt":self.error_excerpt,
            "suggestedNextInspection":self.next_probe,
            "candidateInspectionTools":self.probe_tools,
            "toolHintsAreNotPermission":true,
            "diagnosisStatus":if self.verification_pending_after_edit {
                "source_changed_recheck_pending"
            } else { "rule_based_observation_not_verified_cause" }})
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
pub fn latest(receipts:&[Receipt])->Option<FailureGuidance>{
    for (index,receipt) in receipts.iter().enumerate().rev(){
        if receipt.success||receipt.result["skipped"]==true{continue;}
        let later=&receipts[index+1..];
        // Only a successful repeat of this exact invocation proves that the
        // failed check has since passed. A successful write is a mutation,
        // not a verification receipt; unrelated successful checks do not
        // erase this failure either.
        if later.iter().any(|newer|newer.success
            && newer.tool==receipt.tool && newer.arguments==receipt.arguments){
            continue;
        }
        let raw=diagnostic(receipt);
        let (category,original_probe,original_tools)=classify(&receipt.tool,&raw);
        // A successful tool receipt may still represent a no-op (unchanged
        // bytes, skipped replacement). Only a host-confirmed source mutation
        // can make a previously failing check eligible for re-verification.
        let verification_pending_after_edit=matches!(category,
            "compiler_diagnostic"|"test_assertion_failure"|"missing_path")
            && later.iter().any(|newer|newer.success
                && newer.result["written"]==true
                && matches!(newer.tool.as_str(),
                    "workspace_write"|"workspace_replace"|"workspace_replace_range"
                    |"file_write"));
        let (next_probe,probe_tools)=if verification_pending_after_edit {
            ("Source changed since this failure; propose completion so the Rust kernel reruns required checks. Do not assume the edit fixed it.",
                &[][..])
        } else {(original_probe,original_tools)};
        return Some(FailureGuidance{
            receipt_id:receipt.id.clone(),tool:receipt.tool.clone(),
            category,next_probe,probe_tools,error_excerpt:tail(&raw,220),
            verification_pending_after_edit,
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
            assert_eq!(result.model_view()["toolHintsAreNotPermission"],true);
            assert_eq!(result.model_view()["candidateInspectionTools"],json!(result.probe_tools));
            assert!(result.model_view().to_string().len()<900);
        }
    }
    #[test]
    fn source_edit_is_pending_verification_not_automatically_repaired(){
        let old=failure("compiler","shell_exec","error[E0308]: mismatched types");
        let changed=Receipt{id:"edit".into(),tool:"workspace_replace_range".into(),
            arguments:json!({"path":"src/lib.rs"}),success:true,
            result:json!({"written":true}),step:2};
        let hint=latest(&[old,changed]).unwrap();
        assert_eq!(hint.category,"compiler_diagnostic");
        assert_eq!(hint.model_view()["diagnosisStatus"],"source_changed_recheck_pending");
        assert!(hint.probe_tools.is_empty());
        assert_eq!(hint.model_view()["candidateInspectionTools"],json!([]),
            "After a real edit, the planner should propose verification");
        let verified=Receipt{id:"green".into(),tool:"shell_exec".into(),
            arguments:json!({}),success:true,result:json!({"exitCode":0}),step:3};
        assert!(latest(&[
            failure("compiler","shell_exec","error[E0308]: mismatched types"),
            Receipt{id:"edit".into(),tool:"workspace_replace_range".into(),
                arguments:json!({"path":"src/lib.rs"}),success:true,
                result:json!({"written":true}),step:2},
            verified,
        ]).is_none());
    }
    #[test]
    fn no_op_source_write_does_not_hide_unrepaired_compile_failure(){
        let failure=failure("compile-1","shell_exec",
            "error[E0308]: src/lib.rs: mismatched types");
        let noop=Receipt{id:"noop".into(),tool:"workspace_replace_range".into(),
            arguments:json!({"path":"src/lib.rs"}),success:true,
            result:json!({"written":false,"unchanged":true}),step:2};
        let diagnosis=latest(&[failure,noop]).unwrap();
        assert_eq!(diagnosis.category,"compiler_diagnostic");
        assert!(!diagnosis.verification_pending_after_edit,
            "The unchanged file cannot count as a repair");
        assert!(diagnosis.model_view()["suggestedNextInspection"].as_str().unwrap()
            .contains("Read the cited source line"));
    }

    #[test]
    fn unrelated_success_never_silences_open_failure(){
        let unrelated=Receipt{id:"other".into(),tool:"shell_exec".into(),
            arguments:json!({"command":"echo ok"}),success:true,
            result:json!({"exitCode":0}),step:3};
        let hint=latest(&[
            failure("compiler","shell_exec","error[E0308]: mismatched types"),
            unrelated,
        ]).unwrap();
        assert_eq!(hint.category,"compiler_diagnostic");
        assert!(!hint.verification_pending_after_edit);
        // If the newer check passes, an older unrelated failure is still visible.
        let cleared=Receipt{id:"green".into(),tool:"shell_exec".into(),
            arguments:json!({}),success:true,result:json!({"exitCode":0}),step:3};
        let earlier=failure("missing","workspace_read","No such file or directory");
        let hint=latest(&[
            earlier,
            failure("compiler","shell_exec","error[E0308]: mismatched types"),
            cleared,
        ]).unwrap();
        assert_eq!(hint.category,"missing_path");
    }
    #[test]
    fn permission_denial_stays_closed(){
        let denied=failure("denied","mcp__private",
            &format!("{}permission denied","build log ".repeat(1000)));
        let hint=latest(&[denied]).unwrap();
        assert!(hint.probe_tools.is_empty());
        assert!(hint.error_excerpt.contains("permission denied"));
        assert!(hint.error_excerpt.chars().count()<260);
    }
}
