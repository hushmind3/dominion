//! Deterministic, zero-provider-cost evidence replay.
//!
//! Replays durable MODEL DECISIONS and TOOL RECEIPTS as immutable observations.
//! It does not execute old tools, tests, approvals, browser actions, or model
//! generations. It is for incident reconstruction and paired comparisons,
//! never for claiming the workspace still satisfies historical checks.
use crate::{Receipt,Snapshot,Status};
use serde_json::{json,Value};
use std::collections::{BTreeMap,BTreeSet};

/// Kernel-issued acceptance checks are the only legitimate receipts without
/// model actions. The host journal independently verifies external effects.
fn kernel_verification_receipt(receipt:&Receipt)->bool{
 let Some(suffix)=receipt.id.strip_prefix(crate::acceptance::KERNEL_VERIFICATION_PREFIX)
  else{return false};
 let Some((step,index))=suffix.split_once('-') else{return false};
 receipt.tool=="shell_exec"
  &&step.parse::<usize>().ok()==Some(receipt.step)
  &&index.trim_end_matches('x').parse::<usize>().is_ok()
  &&receipt.arguments["command"].as_str()
   .is_some_and(|command|!command.trim().is_empty())
  &&receipt.result["skipped"]!=true
}

pub fn trace(snapshot:&Snapshot)->Value{
 let mut issues=Vec::<String>::new();
 let mut seen=BTreeSet::new();
 let mut by_id=BTreeMap::new();
 // Receipt order, not just final success, proves that a prerequisite
 // completed before a dependent side effect was observed.
 let mut receipt_order=BTreeMap::new();
 let mut last_step=0usize;
 for (position,receipt) in snapshot.receipts.iter().enumerate(){
  if receipt.id.is_empty()||!seen.insert(&receipt.id){
   issues.push(format!("Duplicate or empty receipt id: {}",receipt.id));
  }
  if receipt.step<last_step||receipt.step==0||receipt.step>snapshot.step {
   issues.push(format!("Invalid receipt order or step: {}",receipt.id));
  }
  last_step=receipt.step;
  by_id.insert(receipt.id.as_str(),receipt);
  receipt_order.insert(receipt.id.as_str(),position);
 }
 let mut actions=Vec::new();
 let mut planned=BTreeSet::new();
 for (turn,decision) in snapshot.decision_tape.iter().enumerate(){
  for action in &decision.actions{
   if !planned.insert(action.id.as_str()){
    issues.push(format!("Repeated planned action id: {}",action.id));
   }
   let receipt=by_id.get(action.id.as_str());
   let status=match receipt {
    Some(r) if r.success=>"observed_success",
    Some(r) if r.result["skipped"]==true=>"skipped",
    Some(_)=>"observed_failure",
    None=>"not_dispatched",
   };
   // A bridge target differs from the model's fixed tool-call name.
   if let Some(r)=receipt{
    // The discovery bridge intentionally resolves both tool name and args.
    // Ordinary actions must match their durable model decision exactly.
    if action.tool!="dominion_tool_call" {
     if r.tool!=action.tool {
      issues.push(format!("Action identity mismatch for {}",action.id));
     }
     if r.arguments!=action.arguments {
      issues.push(format!("Action arguments mismatch for {}",action.id));
     }
    }
    let current_position=receipt_order.get(action.id.as_str()).copied();
    if r.success && action.depends_on.iter().any(|dependency| {
     !by_id.get(dependency.as_str()).is_some_and(|prior|prior.success)
      || !receipt_order.get(dependency.as_str()).is_some_and(|position|
       current_position.is_some_and(|current|*position<current))
    }) {
     issues.push(format!("Succeeded action has an unsatisfied or later dependency: {}",action.id));
    }
   }
   actions.push(json!({"turn":turn+1,"id":action.id,
    "plannedTool":action.tool,"observedTool":receipt.map(|r|r.tool.as_str()),
    "step":receipt.map(|r|r.step),"status":status,
    "dependsOn":action.depends_on,
    "diagnostic":receipt.filter(|r|!r.success)
     .and_then(|r|r.result["error"].as_str()).map(|s|s.chars().take(500).collect::<String>())}));
  }
 }
 // Historical checkpoints without a decision tape still deserialize, but
 // cannot certify model-to-effect attribution.
 let legacy=snapshot.decision_tape.is_empty();
 let mut kernel_checks=Vec::new();
 for receipt in &snapshot.receipts {
  if planned.contains(receipt.id.as_str()) {
   if receipt.id.starts_with(crate::acceptance::KERNEL_VERIFICATION_PREFIX) {
    issues.push(format!("Model action reused a kernel verification id: {}",receipt.id));
   }
   continue;
  }
  if kernel_verification_receipt(receipt) {
   kernel_checks.push(json!({"id":receipt.id,"tool":receipt.tool,
    "step":receipt.step,"status":if receipt.success{"observed_success"}else{"observed_failure"}}));
  }else if !legacy {
   issues.push(format!("Receipt lacks a model decision or kernel verification origin: {}",receipt.id));
  }
 }
 let no_pending=snapshot.in_flight.is_empty()&&!snapshot.model_in_flight;
 let satisfied=snapshot.checks.iter().filter(|c|c.satisfied).count();
 let checked=snapshot.checks.len();
 let complete=matches!(snapshot.status,Status::Completed);
 let replayable=!legacy && no_pending && issues.is_empty();
 json!({
  "runId":snapshot.id,"mode":"offline_decision_and_receipt_reconstruction",
  "modelOrExternalToolCalls":0,"newExternalEffects":false,
  "decisionTapeAvailable":!legacy,"traceConsistent":issues.is_empty(),
  "safeToAutoReplayExternalEffects":false,
  "requiresManualReconciliation":!no_pending,
  "modelCallsRecorded":snapshot.model_calls,
  "plannerTurnsReconstructed":snapshot.decision_tape.len(),
  "receiptsReconstructed":snapshot.receipts.len(),
  "stepsRecorded":snapshot.step,"estimatedTokensRecorded":snapshot.charged_tokens,
  "status":snapshot.status,
  "reportedCompleted":complete,
  "acceptanceChecks":{"satisfied":satisfied,"total":checked,
    "allSatisfied":checked>0&&satisfied==checked},
  "replayableTrace":replayable,
  "issues":issues,"actions":actions,
  "kernelVerificationReceipts":kernel_checks,
  "accuracyLimit":"Recorded observations are reproducible; model behavior and physical side effects are NOT rerun. Historical acceptance is not current-world proof."
 })
}
#[cfg(test)]
mod tests{
 use super::*;
 use crate::{RunRequest,Limits,Decision,Action,Receipt,Strategy};
 #[test]
 fn preserves_failures_and_dependencies_without_executing_tools(){
  let req=RunRequest{id:"r1".into(),instruction:"test".into(),
   owner_instructions:String::new(),strategy:Strategy::Balanced,
   limits:Limits::default(),allowed_tools:vec![],checks:vec![],
   require_evidence:true};
  let mut s=Snapshot::from_request(&req);
  s.step=2;s.model_calls=2;s.status=Status::NeedsReview;
  s.decision_tape.push(Decision{plan:vec![],answer:None,
   actions:vec![Action{id:"x".into(),tool:"shell_exec".into(),
    arguments:json!({"command":"fail"}),depends_on:vec![]}]});
  s.receipts.push(Receipt{id:"x".into(),tool:"shell_exec".into(),
   arguments:json!({"command":"fail"}),success:false,
   result:json!({"error":"script failed"}),step:2});
  let report=trace(&s);
  assert_eq!(report["modelOrExternalToolCalls"],0);
  assert_eq!(report["actions"][0]["status"],"observed_failure");
  assert_eq!(report["actions"][0]["diagnostic"],"script failed");
  assert_eq!(report["traceConsistent"],true);
  let mut tampered=s.clone();tampered.receipts[0].tool="other".into();
  assert_eq!(trace(&tampered)["traceConsistent"],false);
  let mut changed_args=s.clone();
  changed_args.receipts[0].arguments=json!({"command":"different"});
  assert_eq!(trace(&changed_args)["traceConsistent"],false);
  let mut unresolved=s;unresolved.model_in_flight=true;
  assert_eq!(trace(&unresolved)["requiresManualReconciliation"],true);
 }
 #[test]
 fn successful_receipt_cannot_claim_an_unsatisfied_dependency(){
  let req=RunRequest{id:"dependency-trace".into(),instruction:"test".into(),
   owner_instructions:String::new(),strategy:Strategy::Balanced,
   limits:Limits::default(),allowed_tools:vec![],checks:vec![],
   require_evidence:true};
  let mut s=Snapshot::from_request(&req);
  s.step=2;
  s.decision_tape.push(Decision{plan:vec![],answer:None,
   actions:vec![Action{id:"inspect".into(),tool:"read".into(),
    arguments:json!({}),depends_on:vec![]},
    Action{id:"write".into(),tool:"write".into(),
    arguments:json!({}),depends_on:vec!["inspect".into()]}]});
  s.receipts.push(Receipt{id:"inspect".into(),tool:"read".into(),
   arguments:json!({}),success:false,result:json!({"error":"failed"}),step:2});
  s.receipts.push(Receipt{id:"write".into(),tool:"write".into(),
   arguments:json!({}),success:true,result:json!({}),step:2});
  assert_eq!(trace(&s)["traceConsistent"],false);
  s.receipts[1].success=false;
  s.receipts[1].result=json!({"skipped":true});
  assert_eq!(trace(&s)["traceConsistent"],true,
   "a skipped dependent action is a consistent failure record");
 }
 #[test]
 fn later_success_cannot_retroactively_authorize_earlier_side_effect(){
  let req=RunRequest{id:"ordered-dependency".into(),instruction:"inspect then edit".into(),
   owner_instructions:String::new(),strategy:Strategy::Balanced,
   limits:Limits::default(),allowed_tools:vec![],checks:vec![],
   require_evidence:true};
  let mut s=Snapshot::from_request(&req);
  s.step=2;
  s.decision_tape.push(Decision{plan:vec![],answer:None,actions:vec![
   Action{id:"inspect".into(),tool:"workspace_read".into(),
    arguments:json!({"path":"file"}),depends_on:vec![]},
   Action{id:"edit".into(),tool:"workspace_write".into(),
    arguments:json!({"path":"file"}),depends_on:vec!["inspect".into()]},
  ]});
  let inspect=Receipt{id:"inspect".into(),tool:"workspace_read".into(),
   arguments:json!({"path":"file"}),success:true,result:json!({}),step:2};
  let edit=Receipt{id:"edit".into(),tool:"workspace_write".into(),
   arguments:json!({"path":"file"}),success:true,result:json!({}),step:2};
  // Even within the same step, an earlier effect cannot depend on a
  // successful receipt which was only observed afterwards.
  s.receipts=vec![edit.clone(),inspect.clone()];
  let invalid=trace(&s);
  assert_eq!(invalid["traceConsistent"],false);
  assert!(invalid["issues"].as_array().unwrap().iter()
   .any(|issue|issue.as_str().unwrap_or("").contains("later dependency")));
  s.receipts=vec![inspect,edit];
  assert_eq!(trace(&s)["traceConsistent"],true);
 }
 #[test]
 fn only_kernel_verification_receipts_may_be_unplanned(){
  let req=RunRequest{id:"kernel-origin".into(),instruction:"test".into(),
   owner_instructions:String::new(),strategy:Strategy::Balanced,
   limits:Limits::default(),allowed_tools:vec![],checks:vec![],
   require_evidence:true};
  let mut s=Snapshot::from_request(&req);
  s.step=3;
  s.decision_tape.push(Decision{plan:vec![],answer:None,actions:vec![
   Action{id:"inspect".into(),tool:"read".into(),
    arguments:json!({"path":"a.rs"}),depends_on:vec![]}]});
  s.receipts.push(Receipt{id:"inspect".into(),tool:"read".into(),
   arguments:json!({"path":"a.rs"}),success:true,result:json!({}),step:2});
  s.receipts.push(Receipt{id:"kernel-verify-3-0".into(),tool:"shell_exec".into(),
   arguments:json!({"command":"cargo test"}),success:true,result:json!({}),step:3});
  let clean=trace(&s);
  assert_eq!(clean["traceConsistent"],true);
  assert_eq!(clean["kernelVerificationReceipts"][0]["id"],"kernel-verify-3-0");
  assert_eq!(clean["actions"].as_array().unwrap().len(),1);
  let mut orphan=s.clone();
  orphan.receipts[1].id="unplanned-tool".into();
  assert_eq!(trace(&orphan)["traceConsistent"],false);
  let mut forged=s.clone();
  forged.receipts[1].tool="workspace_write".into();
  assert_eq!(trace(&forged)["traceConsistent"],false);
  let mut mismatch=s.clone();
  mismatch.receipts[1].id="kernel-verify-2-0".into();
  assert_eq!(trace(&mismatch)["traceConsistent"],false);
  let mut skipped=s;
  skipped.receipts[1].success=false;
  skipped.receipts[1].result=json!({"skipped":true});
  assert_eq!(trace(&skipped)["traceConsistent"],false);
 }
 #[test]
 fn legacy_snapshots_deserialize_and_report_missing_decision_tape(){
  let req=RunRequest{id:"legacy".into(),instruction:"test".into(),
   owner_instructions:String::new(),strategy:Strategy::Balanced,
   limits:Limits::default(),allowed_tools:vec![],checks:vec![],
   require_evidence:false};
  let snapshot=Snapshot::from_request(&req);
  let mut encoded=serde_json::to_value(&snapshot).unwrap();
  encoded.as_object_mut().unwrap().remove("decisionTape");
  let legacy:Snapshot=serde_json::from_value(encoded).unwrap();
  assert_eq!(trace(&legacy)["replayableTrace"],false);
 }
}
