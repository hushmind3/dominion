//! Post-action review and plan selection.
use super::*;

pub(super) struct ActionReview {
    pub(super) gaps: Vec<String>,
    pub(super) next_action: Option<String>,
}

pub(super) fn simple_file_creation(
    state:&AppState,input:&str,acceptance:&Option<(String,Value)>
)->Option<std::path::PathBuf>{
    if !verification::simple_file_creation_goal(input){return None;}
    let (_,checks)=acceptance.as_ref()?;
    let checks=checks.as_array()?;
    let [check]=checks.as_slice() else{return None;};
    if check["type"]!="file_exists" || check["inferred"]!=true
        || check["creationOnly"]!=true{return None;}
    verification::resolve_path(state,check["path"].as_str()?).ok()
}

/// A local multi-file acceptance contract already gives the native turn
/// concrete targets and an independent verifier. The primary model still
/// decides edits and receives all user instructions; a second, separate
/// planning model would only paraphrase the same bounded file list.
/// Owner-requested planning and explicit planning policies remain authoritative.
#[allow(dead_code)]
pub(super) fn local_multifile_contract_needs_no_secondary_planner(
    input:&str,acceptance:&Option<(String,Value)>
)->bool{
    dominion_agent_kernel::decision_policy::local_multifile_skips_extra_planner(input,acceptance)
}

/// Deterministic single-step file work should not pay for an extra planner
/// call. Multi-step, long or unverified requests still get one.
#[allow(dead_code)]
pub(super) fn needs_plan(input:&str,acceptance:&Option<(String,Value)>)->bool{
    dominion_agent_kernel::decision_policy::needs_plan(input,acceptance)
}

pub(super) async fn action_plan(state: &AppState, request: &TurnRequest, text: &str) -> Option<String> {
    let plan = llm::complete_for_turn(
        state,
        request.provider_id.as_deref().unwrap_or(""),
        request.model.as_deref().unwrap_or(""),
        &crate::engine::chat::harness_prompts::effective(state,"decision.planner"),
        &format!("요청: {text}"),
        request.session_id.as_deref(),
    )
    .await
    .ok()?;
    let plan = plan.trim().chars().take(1200).collect::<String>();
    (!plan.is_empty()).then_some(plan)
}

/// Failure-aware repair admission in the *existing* chat kernel.
/// A second model should not review an action when no tool ever completed:
/// the deterministic verifier already knows what is missing. Nor should
/// repeating the same reads and identical failures buy another model round.
pub(super) fn review_worth_calling(evidence:&[Value],already_retried:bool)->bool{
    !already_retried && evidence.iter().any(|row|{
        row["status"]=="completed"
            && row["tool"].as_str().is_some_and(|tool|!tool.is_empty())
    })
}

/// The output is only an advisory reviewer context; full, original tool
/// receipts remain in SQLite for the deterministic acceptance verifier.
/// Never send whole page bodies, terminal logs, or unbounded tool arguments
/// into the secondary review call.
fn bounded_review_receipts(evidence:&[Value])->String{
    let rows=evidence.iter().rev().filter(|row|row["status"]=="completed")
        .take(8).map(|row|{
            let result=row["result"].to_string();
            json!({
                "tool":row["tool"],
                "resource":row["resourceKey"],
                "resultExcerpt":result.chars().take(400).collect::<String>(),
                "truncated":result.chars().count()>400,
                "receiptVerified":row["toolReceipt"]["verified"],
                "effectState":row["effectState"],
            })
        }).collect::<Vec<_>>();
    serde_json::to_string(&rows).unwrap_or_else(|_|"[]".into())
}

/// An unchanged set of missing checks and the same tool observations is a
/// stalled attempt, not a reason to burn another model call. Runtime
/// operation UUIDs and timestamps are excluded so replaying the same read
/// does not manufacture progress. Only the newest 48 operations are examined.
pub(super) fn repair_signature(gaps:&[String],operations:&[Value])->String{
    let mut normalized=gaps.iter().take(20).map(|gap|gap.trim().to_string())
        .collect::<Vec<_>>();
    normalized.sort();
    let mut observations=operations.iter().rev().take(48).map(|row|{
        json!({
            "tool":row["tool"],"arguments":row["arguments"],
            "status":row["status"],"effectState":row["effectState"],
            "result":row["result"],
        }).to_string()
    }).collect::<Vec<_>>();
    observations.sort();
    observations.dedup();
    let content=json!({"gaps":normalized,"observations":observations}).to_string();
    crate::engine::memory::stable_id(&[&content])
}

/// A crashed or indeterminate external mutation cannot be silently repaired
/// by asking the model to try another side-effectful command. A verified
/// observation may later mark it inspected, but this pending state is an
/// explicit stop at the final-answer boundary.
pub(super) fn unresolved_mutation(operations:&[Value])->bool{
    operations.iter().any(|row|{
        let name=row["tool"].as_str().unwrap_or("");
        let args=&row["arguments"];
        let effect_state=row["effectState"].as_str().unwrap_or("");
        // mark_inspected deliberately keeps the historical outcomeUnknown
        // flag for audit. It does clear the blind-retry barrier after a
        // verified read; that inspected state must win over the old flag.
        crate::engine::effect::is_effectful(name,args)
            && effect_state!="inspected"
            && (row["outcomeUnknown"]==true
                ||row["result"]["outcomeUnknown"]==true
                ||matches!(effect_state,
                    "effect_unknown"|"needs_review"|"started"|"prepared"))
    })
}

pub(super) async fn review_action(
    state: &AppState,
    request: &TurnRequest,
    acceptance_text: &str,
    claim: &str,
    evidence: &[Value],
    deterministic_gaps: &[String],
) -> Option<ActionReview> {
    let prompt = format!(
        "요청: {acceptance_text}\n\n모델의 완료 주장(신뢰하지 말 것): {claim}\n\n실제 성공 도구 증거:\n{}\n\n결정적 검증 미충족:\n- {}\n\nJSON만 출력: {{\"complete\":false,\"gaps\":[\"부족한 점\"],\"nextAction\":\"다음에 실제로 실행할 구체적 도구/작업\"}}",
        bounded_review_receipts(evidence),
        deterministic_gaps.iter().take(8)
            .map(|gap|gap.chars().take(280).collect::<String>())
            .collect::<Vec<_>>().join("\n- ")
    );
    let raw = llm::complete_for_turn(
        state,
        request.provider_id.as_deref().unwrap_or(""),
        request.model.as_deref().unwrap_or(""),
        &crate::engine::chat::harness_prompts::effective(state,"decision.reviewer"),
        &prompt,
        request.session_id.as_deref(),
    )
    .await
    .ok()?;
    let start = raw.find('{')?;
    let end = raw.rfind('}')?;
    let value: Value = serde_json::from_str(&raw[start..=end]).ok()?;
    let gaps = value["gaps"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let next_action = value["nextAction"]
        .as_str()
        .or_else(|| value["next_action"].as_str())
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string);
    Some(ActionReview { gaps, next_action })
}


#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn multi_file_contract_reuses_native_plan_but_preserves_explicit_planning(){
        let multiple=Some(("make two files".to_string(),json!([
            {"type":"file_changed","path":"docs/a.txt"},
            {"type":"file_changed","path":"docs/b.txt"},
            {"type":"file_contains","path":"SUMMARY.md","contains":"docs/a.txt"}
        ])));
        assert!(local_multifile_contract_needs_no_secondary_planner(
            "docs/a.txt, docs/b.txt 그리고 SUMMARY.md 작성해",&multiple));
        assert!(!local_multifile_contract_needs_no_secondary_planner(
            "make a plan first then create docs/a.txt and docs/b.txt",&multiple));
        let single=Some(("single".to_string(),json!([
            {"type":"file_changed","path":"main.rs"}
        ])));
        assert!(!local_multifile_contract_needs_no_secondary_planner("fix main.rs",&single));
        let mixed=Some(("mixed".to_string(),json!([
            {"type":"file_changed","path":"main.rs"},
            {"type":"command_succeeded","command":"cargo test"}
        ])));
        assert!(!local_multifile_contract_needs_no_secondary_planner("fix and test",&mixed));
        assert!(!local_multifile_contract_needs_no_secondary_planner("write",&None));
    }

    #[test]
    fn reviewer_needs_real_successful_tool_evidence(){
        assert!(!review_worth_calling(&[],false));
        assert!(!review_worth_calling(&[json!({"status":"failed","tool":"shell_exec"})],false));
        assert!(review_worth_calling(&[json!({"status":"completed","tool":"file_read"})],false));
        assert!(!review_worth_calling(&[json!({"status":"completed","tool":"file_read"})],true));
    }
    #[test]
    fn repeated_identical_reads_cannot_reset_the_repair_stall_gate(){
        let x=json!({"tool":"file_read","arguments":{"path":"src/lib.rs"},
            "status":"completed","effectState":"observed",
            "result":{"content":"unchanged"},"id":"random-one","startedAt":"first"});
        let mut y=x.clone();y["id"]=json!("random-two");
        y["startedAt"]=json!("later");
        let gaps=vec!["missing output file".to_string()];
        assert_eq!(repair_signature(&gaps,&[x.clone()]),
            repair_signature(&gaps,&[x.clone(),y.clone()]));
        y["result"]["content"]=json!("new information");
        assert_ne!(repair_signature(&gaps,&[x]),repair_signature(&gaps,&[y]));
    }
    #[test]
    fn unknown_remote_effect_requires_review_not_another_blind_attempt(){
        let remote=json!({"tool":"github_api","arguments":{"method":"POST",
            "path":"/repos/acme/demo/issues"},
            "status":"failed","effectState":"effect_unknown"});
        assert!(unresolved_mutation(&[remote.clone()]));
        let mut inspected=remote;
        inspected["effectState"]=json!("inspected");
        inspected["outcomeUnknown"]=json!(true);
        inspected["result"]["outcomeUnknown"]=json!(true);
        assert!(!unresolved_mutation(&[inspected]),
            "an independently inspected effect keeps its audit uncertainty but may progress");
        let read=json!({"tool":"file_read","status":"interrupted",
            "outcomeUnknown":true,"arguments":{"path":"readme.txt"}});
        assert!(!unresolved_mutation(&[read]));
        let definite=json!({"tool":"github_file_commit","status":"completed",
            "effectState":"applied"});
        assert!(!unresolved_mutation(&[definite]));
    }
    #[test]
    fn reviewer_context_bounded_without_losing_original_receipt(){
        let receipt=json!({"tool":"file_read","status":"completed",
            "resourceKey":"file:README.md","result":{"content":"A".repeat(8000)},
            "toolReceipt":{"verified":true}});
        let projected=bounded_review_receipts(&[receipt.clone()]);
        assert!(projected.len()<1400);
        assert!(projected.contains("file_read"));
        assert!(!projected.contains(&"A".repeat(700)));
        assert_eq!(receipt["result"]["content"].as_str().unwrap().len(),8000);
    }
}
