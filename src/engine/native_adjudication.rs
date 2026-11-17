//! Evidence-lineage adjudication for matched native executions (2026).
//!
//! This is NOT a rollout optimizer and NEVER reruns models or effects. It
//! composes three independently stored observations: model decisions, host
//! operation receipts, and pre/post host artifact witnesses. An observed
//! difference is not called causal without an executed counterfactual.
//! Neither an LLM's success narrative nor a heuristic success count is a
//! substitute for a witnessed postcondition.
use crate::{engine::native_replay,state::AppState};
use dominion_sdk::harness::{Action,Snapshot};
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::collections::{BTreeMap,BTreeSet,VecDeque};

const MAX_CANDIDATES:usize=8;
const MAX_ACTIONS:usize=512;

fn digest(value:&Value)->String{
 format!("{:x}",Sha256::digest(value.to_string().as_bytes()))
}
fn evidence(job:&Value,key:&str)->Option<&Value>{
 let witness=&job[key];
 (witness["complete"]==true && witness["entries"].is_object()
  &&witness["directVerdicts"].is_array()).then_some(witness)
}
fn witness_identity(a:&Value,b:&Value)->bool{
 // Workspaces may be independently cloned. Their path is not identity;
 // the captured CONTENTS on the declared acceptance surface are.
 a["entries"]==b["entries"] && a["directVerdicts"]==b["directVerdicts"]
}
fn witness_changes(before:&Value,after:&Value)->Vec<String>{
 let Some(entries)=before["entries"].as_object() else{return vec![]};
 entries.iter().filter_map(|(path,value)|{
  (after["entries"].get(path)!=Some(value)).then_some(path.clone())
 }).collect()
}
fn direct_verdicts_agree(job:&Value,snapshot:&Snapshot)->bool{
 let Some(after)=evidence(job,"nativeEvidenceFinal") else{return false};
 let Some(verdicts)=after["directVerdicts"].as_array() else{return false};
 if verdicts.len()!=snapshot.checks.len(){return false;}
 // null denotes a criterion that this file-state witness cannot check.
 // For FileContains, the kernel's own verification is retained, but a
 // post-execution SHA alone cannot prove its string predicate.
 for (index,check) in snapshot.checks.iter().enumerate(){
  let v=&verdicts[index];
  if v.is_boolean() && v.as_bool()!=Some(check.satisfied){
   return false;
  }
 }
 true
}
fn artifact_proof(job:&Value,trace:&Value,snapshot:&Snapshot)->Value{
 let before=evidence(job,"nativeEvidenceBaseline");
 let after=evidence(job,"nativeEvidenceFinal");
 let host_ok=trace["hostJournal"]["consistent"]==true
  &&trace["traceConsistent"]==true
  &&trace["decisionTapeAvailable"]==true
  &&trace["requiresManualReconciliation"]!=true;
 let terminal=trace["reportedCompleted"]==true;
 let accepted=trace["acceptanceChecks"]["allSatisfied"]==true
  &&!snapshot.checks.is_empty();
 let agrees=direct_verdicts_agree(job,snapshot);
 let changed=before.zip(after)
  .map(|(b,a)|witness_changes(b,a)).unwrap_or_default();
 let direct=after.and_then(|w|w["directVerdicts"].as_array());
 let directly_observed=direct.map(|v|v.iter().filter(|v|v.is_boolean()).count())
  .unwrap_or(0);
 let all_direct=direct.is_some_and(|v|!v.is_empty()&&v.iter().all(Value::is_boolean));
 let grade=if !host_ok {
  "inconsistent_lineage"
 }else if before.is_none()||after.is_none()||!agrees {
  "unsealed_or_conflicting_artifacts"
 }else if !terminal||!accepted {
  "not_accepted"
 }else if all_direct {
  "directly_corrobated"
 }else{
  "partially_corrobated"
 };
 json!({
  "grade":grade,
  "hostLineageConsistent":host_ok,
  "terminalCompleted":terminal,
  "kernelAcceptancePassed":accepted,
  "criterionWitnessAgreement":agrees,
  "directWitnessCount":directly_observed,
  "totalCriteria":snapshot.checks.len(),
  "changedAcceptancePaths":changed,
  "baselineArtifactDigest":before.map(|w|digest(&w["entries"])),
  "finalArtifactDigest":after.map(|w|digest(&w["entries"])),
  "witnessScope":"declared_acceptance_paths_only",
  "worldStateFrozen":false,
 })
}
fn actions(snapshot:&Snapshot)->Result<Vec<Action>,String>{
 let list=snapshot.decision_tape.iter().flat_map(|d|d.actions.iter())
  .cloned().collect::<Vec<_>>();
 if list.len()>MAX_ACTIONS{return Err("Trajectory action cap exceeded".into())}
 Ok(list)
}
fn first_difference(a:&[Action],b:&[Action])->Value{
 for (index,(left,right)) in a.iter().zip(b).enumerate(){
  if left.tool!=right.tool||left.arguments!=right.arguments
   ||left.depends_on.len()!=right.depends_on.len(){
   return json!({"kind":"decision_change","ordinal":index,
    "baseline":{"id":left.id,"tool":left.tool,"argsDigest":digest(&left.arguments)},
    "candidate":{"id":right.id,"tool":right.tool,"argsDigest":digest(&right.arguments)}});
  }
 }
 if a.len()!=b.len(){
  return json!({"kind":"trajectory_length_change","ordinal":a.len().min(b.len()),
   "baselineLength":a.len(),"candidateLength":b.len()});
 }
 json!({"kind":"same_declared_actions","ordinal":null})
}
fn chain_from_divergence(
 baseline:&[Action],candidate:&[Action],snapshot:&Snapshot,changed:&[String]
)->Value{
 let first=first_difference(baseline,candidate);
 let Some(at)=first["ordinal"].as_u64().map(|v|v as usize) else{
  return json!({"traceDifference":first,
   "mechanism":"not_identifiable_from_unchanged_actions",
   "actionToArtifactLinks":[],"counterfactualExecuted":false});
 };
 let mut by_id=BTreeMap::<&str,usize>::new();
 for (i,action) in candidate.iter().enumerate(){by_id.insert(&action.id,i);}
 // Only explicitly recorded dependencies form lineage edges. A tool that
 // merely happens later is NOT automatically credited as a descendant.
 let mut descendants=BTreeSet::new();
 let mut queue=VecDeque::new();
 if at<candidate.len(){descendants.insert(at);queue.push_back(at);}
 while let Some(index)=queue.pop_front(){
  let id=candidate[index].id.as_str();
  for (next,action) in candidate.iter().enumerate(){
   if descendants.contains(&next){continue;}
   if action.depends_on.iter().any(|parent|parent==id) {
    descendants.insert(next);
    queue.push_back(next);
   }
  }
 }
 let mut links=Vec::new();
 for i in descendants {
  let action=&candidate[i];
  // Tool identity and arguments are immutable decisions checked by the
  // replay trace. Only path-targeted first-party tools support this link;
  // shell, browser, MCP and GitHub effects remain opaque.
  if matches!(action.tool.as_str(),
    "workspace_write"|"workspace_replace"|"workspace_replace_range"|"file_write")
  {
   if let Some(path)=action.arguments["path"].as_str(){
    if changed.iter().any(|changed|changed==path){
     let receipt=snapshot.receipts.iter().find(|r|r.id==action.id);
     links.push(json!({"actionId":action.id,"tool":action.tool,
      "path":path,"ordinal":i,"successfulReceipt":receipt.is_some_and(|r|r.success),
      "dependencyDepth":"reachable_from_divergence"}));
    }
   }
  }
 }
 let directed=links.iter().any(|link|link["successfulReceipt"]==true);
 json!({"traceDifference":first,
  "mechanism":if directed{"artifact_linked_candidate_mechanism"}
    else{"unattributed_difference"},
  "actionToArtifactLinks":links,
  "counterfactualExecuted":false,
  "claim":"mechanism candidate only; external execution and hidden-state confounders remain",
  "registeredActionCount":by_id.len()})
}
fn same_envelope(a:&Value,b:&Value)->Vec<&'static str>{
 let fields=[
  ("ownerInstructions",&a["input"]["ownerInstructions"],&b["input"]["ownerInstructions"]),
  ("strategy",&a["input"]["strategy"],&b["input"]["strategy"]),
  ("agentId",&a["input"]["agentId"],&b["input"]["agentId"]),
  ("routeBinding",&a["nativeRouteBinding"],&b["nativeRouteBinding"]),
  ("toolAuthority",&a["nativeToolAuthoritySha256"],&b["nativeToolAuthoritySha256"]),
  ("requireEvidence",&a["input"]["requireEvidence"],&b["input"]["requireEvidence"]),
 ];
 fields.iter().filter_map(|(name,x,y)|(x!=y).then_some(*name)).collect()
}

/// Adjudicate existing, independently executed runs without doing new work.
/// Unlike a score aggregator this emits an evidence graph and abstains on
/// uncontrolled starting states. It never designates a causal winner.
pub fn adjudicate(
 state:&AppState,baseline_id:&str,candidate_ids:&[String],
)->Result<Value,String>{
 if baseline_id.is_empty()||candidate_ids.is_empty()
  ||candidate_ids.len()>MAX_CANDIDATES {
  return Err("Adjudication requires 1..8 candidate runs".into())
 }
 let mut all=BTreeSet::new();
 for id in std::iter::once(&baseline_id.to_owned()).chain(candidate_ids.iter()){
  if id.is_empty()||id.len()>128||!all.insert(id.clone()){
   return Err("Run IDs must be distinct, nonempty and bounded".into())
  }
 }
 let base_job=state.store.get("job",baseline_id).ok_or("Baseline job not found")?;
 let base_snapshot=native_replay::snapshot(&base_job)?;
 let base_trace=native_replay::report(state,baseline_id)?;
 let base_actions=actions(&base_snapshot)?;
 let base_proof=artifact_proof(&base_job,&base_trace,&base_snapshot);
 let before=evidence(&base_job,"nativeEvidenceBaseline");
 let mut candidates=Vec::new();
 let mut comparables=0usize;
 for id in candidate_ids {
  let candidate=state.store.get("job",id)
   .ok_or_else(||format!("Candidate job not found: {id}"))?;
  native_replay::same_work(&base_job,&candidate)?;
  let snap=native_replay::snapshot(&candidate)?;
  let trace=native_replay::report(state,id)?;
  let run_actions=actions(&snap)?;
  let unmatched=same_envelope(&base_job,&candidate);
  let start=evidence(&candidate,"nativeEvidenceBaseline");
  let same_initial=before.zip(start).is_some_and(|(a,b)|witness_identity(a,b));
  let candidate_proof=artifact_proof(&candidate,&trace,&snap);
  let changed=candidate_proof["changedAcceptancePaths"].as_array()
   .cloned().unwrap_or_default().iter()
   .filter_map(|v|v.as_str().map(str::to_owned)).collect::<Vec<_>>();
  let mechanism=chain_from_divergence(&base_actions,&run_actions,&snap,&changed);
  let matched=unmatched.is_empty()&&same_initial
   &&base_proof["hostLineageConsistent"]==true
   &&candidate_proof["hostLineageConsistent"]==true
   &&base_proof["grade"]!="unsealed_or_conflicting_artifacts"
   &&candidate_proof["grade"]!="unsealed_or_conflicting_artifacts";
  if matched{comparables+=1;}
  candidates.push(json!({
   "runId":id,"matchedExperiment":matched,
   "unmatchedSettings":unmatched,"initialAcceptanceStateEqual":same_initial,
   "outcome":candidate_proof,"mechanism":mechanism,
   "comparison":if !matched{"confounded_or_unsealed"}
    else if candidate_proof["kernelAcceptancePassed"]==true
     &&base_proof["kernelAcceptancePassed"]!=true {
      "observed_task_outcome_gain"
    }else if candidate_proof["kernelAcceptancePassed"]!=true
     &&base_proof["kernelAcceptancePassed"]==true{
      "observed_task_outcome_regression"
    }else{"no_verified_outcome_difference"},
  }));
 }
 Ok(json!({
  "mode":"matched_native_rollout_evidence_adjudication_2026",
  "baseline":{"runId":baseline_id,"outcome":base_proof},
  "candidates":candidates,
  "comparableRuns":comparables,
  "totalRuns":candidate_ids.len()+1,
  "newModelCalls":0,"newToolCalls":0,"newExternalEffects":false,
  "autoPromotion":false,"causalWinner":null,
  "causalStatus":"not_identifiable_without_executed_intervention",
  "nextEvidenceRequirement":"restore and measure identical complete checkpoint states, alter one decision, execute and independently verify continuation",
  "limits":"Non-acceptance workspace state, hidden provider state and asynchronous world effects are not frozen by artifact witnesses; historical paired traces are observational, not an executed causal counterfactual.",
 }))
}

#[cfg(test)]
mod tests {
 use super::*;
 use dominion_sdk::harness::{Decision,Receipt};

 fn action(id:&str,tool:&str,path:&str,depends:&[&str])->Action{
  Action{id:id.into(),tool:tool.into(),
   arguments:json!({"path":path,"content":"x"}),
   depends_on:depends.iter().map(|s|s.to_string()).collect()}
 }
 #[test]
 fn evidence_graph_links_only_explicit_dependent_confirmed_effects() {
  let b=vec![action("a","workspace_read","file", &[])];
  let c=vec![action("a2","workspace_read","other",&[]),
   action("b","workspace_write","file",&["a2"]),
   action("c","workspace_write","file",&[])];
  let mut snap=Snapshot::from_request(&dominion_sdk::harness::RunRequest{
   id:"x".into(),instruction:"x".into(),owner_instructions:String::new(),
   strategy:Default::default(),limits:Default::default(),allowed_tools:vec![],
   checks:vec![],require_evidence:false});
  snap.receipts.push(Receipt{id:"b".into(),tool:"workspace_write".into(),
   arguments:json!({"path":"file","content":"x"}),
   success:true,result:json!({"ok":true}),step:1});
  let v=chain_from_divergence(&b,&c,&snap,&["file".into()]);
  assert_eq!(v["mechanism"],"artifact_linked_candidate_mechanism");
  assert_eq!(v["actionToArtifactLinks"].as_array().unwrap().len(),1);
  assert_eq!(v["actionToArtifactLinks"][0]["actionId"],"b");
  assert_eq!(v["counterfactualExecuted"],false);
 }
 #[test]
 fn no_action_change_never_fabricates_causal_attribution(){
  let actions=vec![action("a","workspace_write","file",&[])];
  let request=dominion_sdk::harness::RunRequest{id:"x".into(),
   instruction:"x".into(),owner_instructions:String::new(),
   strategy:Default::default(),limits:Default::default(),allowed_tools:vec![],
   checks:vec![],require_evidence:false};
  let snap=Snapshot::from_request(&request);
  let v=chain_from_divergence(&actions,&actions,&snap,&["file".into()]);
  assert_eq!(v["mechanism"],"not_identifiable_from_unchanged_actions");
 }
 #[test]
 fn identical_initial_acceptance_state_is_required() {
  let a=json!({"entries":{"x":{"state":"file","sha256":"aa"}},"directVerdicts":[false]});
  let b=json!({"entries":{"x":{"state":"file","sha256":"bb"}},"directVerdicts":[false]});
  assert!(!witness_identity(&a,&b));
  let c=json!({"entries":{"x":{"state":"file","sha256":"aa"}},"directVerdicts":[false]});
  assert!(witness_identity(&a,&c));
 }
 #[test]
 fn a_direct_witness_disagrees_with_kernel_acceptance_when_bytes_disagree() {
  let request=dominion_sdk::harness::RunRequest{id:"x".into(),
   instruction:"x".into(),owner_instructions:String::new(),
   strategy:Default::default(),limits:Default::default(),allowed_tools:vec![],
   checks:vec![],require_evidence:false};
  let mut snap=Snapshot::from_request(&request);
  snap.checks.push(dominion_sdk::harness::CheckResult{
   criterion:dominion_sdk::harness::Criterion{kind:
    dominion_sdk::harness::CriterionKind::FileExists,
    path:Some("out".into()),contains:None,tool:None},
   satisfied:true,explanation:"accepted".into()});
  let job=json!({"nativeEvidenceFinal":{"complete":true,
   "entries":{"out":{"state":"missing"}},"directVerdicts":[false]}});
  assert!(!direct_verdicts_agree(&job,&snap));
 }
}
