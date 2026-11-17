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
use std::collections::{BTreeMap,BTreeSet,VecDeque};

const MAX_CANDIDATES:usize=8;
const MAX_ACTIONS:usize=512;

fn evidence<'a>(job:&'a Value,key:&str)->Option<&'a Value>{
 let witness=&job[key];
 (witness["complete"]==true && witness["entries"].is_object()
  &&witness["directVerdicts"].is_array()
  &&witness["criteriaDigest"].as_str().is_some_and(|digest|
    digest.len()==64 && digest.bytes().all(|byte|byte.is_ascii_hexdigit())))
  .then_some(witness)
}
fn witness_identity(a:&Value,b:&Value)->bool{
 // Workspaces may be independently cloned. Their path is not identity;
 // the captured CONTENTS on the declared acceptance surface are.
 a["entries"]==b["entries"] && a["directVerdicts"]==b["directVerdicts"]
  &&a["criteriaDigest"]==b["criteriaDigest"]
}
fn witness_changes(before:&Value,after:&Value)->Vec<String>{
 let Some(entries)=before["entries"].as_object() else{return vec![]};
 entries.iter().filter_map(|(path,value)|{
  (after["entries"].get(path)!=Some(value)).then_some(path.clone())
 }).collect()
}
fn direct_verdicts_agree(job:&Value,snapshot:&Snapshot)->bool{
 let Some(after)=evidence(job,"nativeEvidenceKernelFinal") else{return false};
 let Some(verdicts)=after["directVerdicts"].as_array() else{return false};
 if verdicts.len()!=snapshot.checks.len(){return false;}
 let expected_criteria=snapshot.checks.iter()
  .map(|check|&check.criterion).collect::<Vec<_>>();
 if after["criteriaDigest"]!=native_replay::digest(&json!(expected_criteria)){return false;}
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
 let direct=evidence(job,"nativeEvidenceKernelFinal")
  .and_then(|w|w["directVerdicts"].as_array());
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
  "directly_corroborated"
 }else{
  "partially_corroborated"
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
  "baselineArtifactDigest":before.map(|w|native_replay::digest(&w["entries"])),
  "finalArtifactDigest":after.map(|w|native_replay::digest(&w["entries"])),
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
/// Convert action IDs into topological indices, so separately executed runs
/// can share a true dependency shape despite independently generated IDs.
fn dependency_shape(actions:&[Action],index:usize)->Vec<usize>{
 let mut by_id=BTreeMap::new();
 for (ordinal,action) in actions.iter().take(index).enumerate(){
  by_id.insert(action.id.as_str(),ordinal);
 }
 let mut out=actions[index].depends_on.iter()
  .map(|id|by_id.get(id.as_str()).copied().unwrap_or(usize::MAX))
  .collect::<Vec<_>>();
 out.sort_unstable();
 out
}
fn first_difference(a:&[Action],b:&[Action])->Value{
 for (index,(left,right)) in a.iter().zip(b).enumerate(){
  if left.tool!=right.tool||left.arguments!=right.arguments
   ||dependency_shape(a,index)!=dependency_shape(b,index){
   return json!({"kind":"decision_change","ordinal":index,
    "baseline":{"id":left.id,"tool":left.tool,"argsDigest":native_replay::digest(&left.arguments)},
    "candidate":{"id":right.id,"tool":right.tool,"argsDigest":native_replay::digest(&right.arguments)}});
  }
 }
 if a.len()!=b.len(){
  return json!({"kind":"trajectory_length_change","ordinal":a.len().min(b.len()),
   "baselineLength":a.len(),"candidateLength":b.len()});
 }
 json!({"kind":"same_declared_actions","ordinal":null})
}
/// Find the earliest *pre-intervention* mismatch in externally observed
/// receipts. A shared decision prefix with different outputs is an exposed
/// confounder: later outcomes cannot be attributed to strategy divergence.
fn common_prefix_drift(
 baseline_actions:&[Action],candidate_actions:&[Action],
 baseline:&Snapshot,candidate:&Snapshot,
)->Value{
 for (ordinal,(left,right)) in baseline_actions.iter()
  .zip(candidate_actions).take(MAX_ACTIONS).enumerate()
 {
  if left.tool!=right.tool||left.arguments!=right.arguments
   ||dependency_shape(baseline_actions,ordinal)!=dependency_shape(candidate_actions,ordinal){break;}
  let l=baseline.receipts.iter().find(|receipt|receipt.id==left.id);
  let r=candidate.receipts.iter().find(|receipt|receipt.id==right.id);
  let (Some(l),Some(r))=(l,r) else{
   return json!({"kind":"unobserved_common_prefix","ordinal":ordinal,
    "prefixComparable":false});
  };
  if l.tool!=r.tool||l.arguments!=r.arguments
   ||l.success!=r.success||l.result!=r.result {
   return json!({"kind":"environment_or_tool_drift","ordinal":ordinal,
    "prefixComparable":false,"baselineResultDigest":native_replay::digest(&l.result),
    "candidateResultDigest":native_replay::digest(&r.result)});
  }
 }
 json!({"kind":"matching_observed_prefix","ordinal":null,"prefixComparable":true})
}

fn chain_from_divergence(
 baseline:&[Action],candidate:&[Action],snapshot:&Snapshot,trace:&Value,
 kernel_final:&Value,prefix:&Value
)->Value{
 let first=first_difference(baseline,candidate);
 if prefix["prefixComparable"]!=true {
  return json!({"traceDifference":first,"prefixEvidence":prefix,
   "mechanism":"pre_intervention_observation_drift",
   "actionToArtifactLinks":[],"counterfactualExecuted":false});
 }
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
  if !matches!(action.tool.as_str(),
    "workspace_write"|"workspace_replace"|"workspace_replace_range")
  {continue;}
  let Some(path)=action.arguments["path"].as_str() else{continue;};
  // A path is a task artifact only if the kernel actually checked it,
  // including checks derived from independent file-write receipts.
  let terminal=&kernel_final["entries"][path];
  if !terminal.is_object(){continue;}
  let source=trace["hostJournal"]["sourceEvidence"].as_array()
   .and_then(|items|items.iter().find(|item|
     item["operationId"]==action.id));
  let lineage=source.map(|item|&item["artifactLineage"]).unwrap_or(&Value::Null);
  let before=&lineage["before"];
  let after=&lineage["after"];
  let preimage_observed=before["complete"]==true;
  let postimage_observed=after["complete"]==true;
  let effect_changed=preimage_observed&&postimage_observed
   &&before["path"]==path&&after["path"]==path
   &&before["state"]!=after["state"];
  let postimage_matches_final=postimage_observed&&after["state"]==*terminal;
  let receipt=snapshot.receipts.iter().find(|receipt|receipt.id==action.id);
  let settled=receipt.is_some_and(|receipt|receipt.success);
  links.push(json!({"actionId":action.id,"tool":action.tool,
   "path":path,"ordinal":i,"successfulReceipt":settled,
   "observedMutation":effect_changed,
   "postimageMatchesKernelFinal":postimage_matches_final,
   "before":before["state"],"after":after["state"],
   "origin":"durable_host_operation_artifact_lineage",
   "exclusiveWriterProven":false}));
 }
 let grounded=links.iter().any(|link|
  link["successfulReceipt"]==true&&link["observedMutation"]==true
   &&link["postimageMatchesKernelFinal"]==true);
 json!({"traceDifference":first,
  "mechanism":if grounded{"host_witnessed_candidate_mutation_chain"}
    else{"unattributed_difference"},
  "actionToArtifactLinks":links,
  "prefixEvidence":prefix,
  "counterfactualExecuted":false,
  "claim":"candidate mechanism only; intervening writers and hidden-state confounders remain",
  "registeredActionCount":by_id.len()})
}
fn same_envelope(a:&Value,b:&Value)->Vec<&'static str>{
 let fields=[
  ("ownerInstructions",&a["input"]["ownerInstructions"],&b["input"]["ownerInstructions"]),
  ("strategy",&a["input"]["strategy"],&b["input"]["strategy"]),
  ("agentId",&a["input"]["agentId"],&b["input"]["agentId"]),
  ("routeBinding",&a["nativeRoute"]["binding"],&b["nativeRoute"]["binding"]),
  ("toolAuthority",&a["nativeToolAuthoritySha256"],&b["nativeToolAuthoritySha256"]),
  ("requireEvidence",&a["input"]["requireEvidence"],&b["input"]["requireEvidence"]),
 ];
 fields.iter().filter_map(|(name,x,y)|(x!=y).then_some(*name)).collect()
}

/// Reuse PRISM's *multi-objective* premise without pretending a single run
/// estimates pass@k, reliability, or generalization. Harness² supplies
/// execution contrast, VeriHarness supplies evidence qualification, and
/// Dominion's SDK already supplies budget/permission enforcement.
///
/// Only independently accepted, host-consistent, same-task observations enter
/// this read-only portfolio. Missing money/time measurements cannot be treated
/// as a free or fast run. Tradeoffs remain visible instead of being compressed
/// into a fictional weighted quality score.
fn portfolio_entry(run_id:&str,matched:bool,proof:&Value,trace:&Value)->Value{
 let grade=proof["grade"].as_str().unwrap_or("");
 let proof_tier=match grade{
  "directly_corroborated"=>2,
  "partially_corroborated"=>1,
  _=>0,
 };
 let accepted=matched&&proof_tier>0
  &&proof["terminalCompleted"]==true
  &&proof["kernelAcceptancePassed"]==true
  &&proof["criterionWitnessAgreement"]==true
  &&proof["hostLineageConsistent"]==true;
 // A failure receipt followed by a real successful repair is not necessarily
 // a bad outcome, but it is *observed execution friction* for a time/cost
 // comparison. It is not a generalizable reliability estimate.
 let failed_receipts=trace["actions"].as_array().map(|actions|
  actions.iter().filter(|a|
   a["status"]=="observed_failure"||a["status"]=="skipped").count()
 ).unwrap_or(0);
 let cost=trace["cost"]["configuredCostKRW"].as_f64()
  .filter(|value|value.is_finite()&&*value>=0.0);
 let latency=trace["latencyMs"].as_i64().filter(|v|*v>=0);
 let tokens=trace["estimatedTokensRecorded"].as_u64();
 let complete_metrics=cost.is_some()&&latency.is_some()&&tokens.is_some();
 json!({
  "runId":run_id,"admissible":accepted,
  "evidenceGrade":grade,"proofTier":proof_tier,
  "observedFailedReceipts":failed_receipts,
  "costKRW":cost,"latencyMs":latency,"recordedTokens":tokens,
  "completeComparableMetrics":complete_metrics,
  "priceBasis":"owner_configured_not_invoice_verified",
  "reliabilityEstimate":null,"holdoutQuality":null,
 })
}

/// A PRISM-style non-dominated set *after* task and provenance matching,
/// with additional local cost/latency/tool-failure constraints. This is not a
/// model choice or an optimizer; it only ranks historical observations that
/// are sufficiently comparable. Unknown metrics exclude a run from the
/// complete-cost frontier instead of counting as zero.
fn dominates(a:&Value,b:&Value)->bool{
 if a["admissible"]!=true||b["admissible"]!=true
  ||a["completeComparableMetrics"]!=true
  ||b["completeComparableMetrics"]!=true{return false;}
 let aq=a["proofTier"].as_u64().unwrap_or(0);
 let bq=b["proofTier"].as_u64().unwrap_or(0);
 let ac=a["costKRW"].as_f64().unwrap_or(f64::INFINITY);
 let bc=b["costKRW"].as_f64().unwrap_or(f64::INFINITY);
 let al=a["latencyMs"].as_i64().unwrap_or(i64::MAX);
 let bl=b["latencyMs"].as_i64().unwrap_or(i64::MAX);
 let at=a["recordedTokens"].as_u64().unwrap_or(u64::MAX);
 let bt=b["recordedTokens"].as_u64().unwrap_or(u64::MAX);
 let af=a["observedFailedReceipts"].as_u64().unwrap_or(u64::MAX);
 let bf=b["observedFailedReceipts"].as_u64().unwrap_or(u64::MAX);
 (aq>=bq&&ac<=bc&&al<=bl&&at<=bt&&af<=bf)
  &&(aq>bq||ac<bc||al<bl||at<bt||af<bf)
}

fn compose_portfolio(entries:&[Value])->Value{
 let mut frontier=Vec::<String>::new();
 let mut excluded=Vec::<Value>::new();
 for candidate in entries{
  let id=candidate["runId"].as_str().unwrap_or("");
  if candidate["admissible"]!=true{
   excluded.push(json!({"runId":id,"reason":"unmatched_or_unverified_execution"}));
   continue
  }
  if candidate["completeComparableMetrics"]!=true{
   excluded.push(json!({"runId":id,"reason":"unknown_price_or_latency_or_tokens"}));
   continue
  }
  if entries.iter().any(|other|other["runId"]!=candidate["runId"]
    &&dominates(other,candidate)){
   excluded.push(json!({"runId":id,"reason":"evidence_cost_latency_frontier_dominated"}));
   continue
  }
  frontier.push(id.to_string());
 }
 let suggestion=if frontier.len()==1{Some(frontier[0].clone())}else{None};
 json!({
  "status":if frontier.is_empty(){"insufficient_comparable_evidence"}
    else{"observational_pareto_frontier"},
  "nondominatedRunIds":frontier,
  "excluded":excluded,
  "reviewSuggestion":suggestion,
  "recommendationType":"manual_review_only",
  "promotionAuthorized":false,
  "holdoutValidated":false,
  "repeatedTrialReliabilityMeasured":false,
  "bounds":"No causal ranking or cross-task reliability inference from isolated historical rollouts",
  "dimensions":["kernel_evidence_grade","configured_cost_KRW",
    "wall_latency_ms","recorded_tokens","observed_failure_receipts"],
 })
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
 if !all.insert(baseline_id.to_string()){return Err("Duplicate baseline".into())}
 for id in candidate_ids {
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
 let scoped_initial=before.and_then(|w|w["entries"].as_object())
  .is_some_and(|entries|!entries.is_empty());
 let mut portfolio=vec![portfolio_entry(
  baseline_id,scoped_initial,&base_proof,&base_trace,
 )];
 let mut candidates=Vec::new();
 let mut comparables=0usize;
 for id in candidate_ids {
  let candidate=state.store.get("job",id)
   .ok_or_else(||format!("Candidate job not found: {id}"))?;
  let shape_error=native_replay::same_work(&base_job,&candidate).err();
  let snap=native_replay::snapshot(&candidate)?;
  let trace=native_replay::report(state,id)?;
  let run_actions=actions(&snap)?;
  let unmatched=same_envelope(&base_job,&candidate);
  let start=evidence(&candidate,"nativeEvidenceBaseline");
  let same_initial=before.zip(start).is_some_and(|(a,b)|witness_identity(a,b));
  let candidate_proof=artifact_proof(&candidate,&trace,&snap);
  let prefix=common_prefix_drift(
   &base_actions,&run_actions,&base_snapshot,&snap,
  );
  let mechanism=chain_from_divergence(
   &base_actions,&run_actions,&snap,&trace,
   &candidate["nativeEvidenceKernelFinal"],&prefix,
  );
  let matched=shape_error.is_none()&&unmatched.is_empty()
   &&scoped_initial&&same_initial
   &&prefix["prefixComparable"]==true
   &&base_proof["hostLineageConsistent"]==true
   &&candidate_proof["hostLineageConsistent"]==true
   &&base_proof["grade"]!="unsealed_or_conflicting_artifacts"
   &&candidate_proof["grade"]!="unsealed_or_conflicting_artifacts";
  if matched{comparables+=1;}
  portfolio.push(portfolio_entry(id,matched,&candidate_proof,&trace));
  candidates.push(json!({
   "runId":id,"matchedExperiment":matched,
   "unmatchedSettings":unmatched,"taskShapeMismatch":shape_error,
   "initialAcceptanceStateEqual":same_initial,
   "initialArtifactScopeNonempty":scoped_initial,
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
  // No shared reference environment means there is no portfolio
  // experiment, even if the baseline itself was a valid historical task.
  "evidenceQualifiedPortfolio":if comparables>0 {
    compose_portfolio(&portfolio)
   }else{
    compose_portfolio(&[])
   },
  "portfolioMetrics":portfolio,
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
 use dominion_sdk::harness::Receipt;

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
  let trace=json!({"hostJournal":{"sourceEvidence":[{
   "operationId":"b","artifactLineage":{
    "before":{"path":"file","complete":true,"state":{"state":"missing"}},
    "after":{"path":"file","complete":true,
     "state":{"state":"file","bytes":1,"sha256":"abc"}}
   }}]}});
  let finish=json!({"entries":{"file":{"state":"file","bytes":1,"sha256":"abc"}}});
  let v=chain_from_divergence(&b,&c,&snap,&trace,&finish,
   &json!({"prefixComparable":true}));
  assert_eq!(v["mechanism"],"host_witnessed_candidate_mutation_chain");
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
  let v=chain_from_divergence(&actions,&actions,&snap,&json!({}),&json!({}),
   &json!({"prefixComparable":true}));
  assert_eq!(v["mechanism"],"not_identifiable_from_unchanged_actions");
 }
 #[test]
 fn identical_initial_acceptance_state_is_required() {
  let hash="a".repeat(64);
  let a=json!({"entries":{"x":{"state":"file","sha256":"aa"}},"directVerdicts":[false],"criteriaDigest":hash});
  let b=json!({"entries":{"x":{"state":"file","sha256":"bb"}},"directVerdicts":[false],"criteriaDigest":hash});
  assert!(!witness_identity(&a,&b));
  let c=json!({"entries":{"x":{"state":"file","sha256":"aa"}},"directVerdicts":[false],"criteriaDigest":hash});
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
  let expected=vec![&snap.checks[0].criterion];
  let job=json!({"nativeEvidenceKernelFinal":{"complete":true,
   "entries":{"out":{"state":"missing"}},"directVerdicts":[false],
   "criteriaDigest":native_replay::digest(&json!(expected))}});
  assert!(!direct_verdicts_agree(&job,&snap));
 }
 #[test]
 fn identical_model_actions_with_divergent_host_results_are_exposed_confounders(){
  let actions_a=vec![action("first","workspace_read","a",&[]),
   action("last","workspace_write","out",&["first"])];
  let actions_b=vec![action("other","workspace_read","a",&[]),
   action("finish","workspace_write","new",&["other"])];
  let new_snap=|id:&str,result:Value|{
   let request=dominion_sdk::harness::RunRequest{id:id.to_string(),
    instruction:"task".into(),owner_instructions:String::new(),
    strategy:Default::default(),limits:Default::default(),allowed_tools:vec![],
    checks:vec![],require_evidence:false};
   let mut snap=Snapshot::from_request(&request);
   snap.receipts.push(Receipt{id:if id=="a"{"first"}else{"other"}.into(),
    tool:"workspace_read".into(),arguments:json!({"path":"a","content":"x"}),
    success:true,result,step:1});
   snap
  };
  let a=new_snap("a",json!({"file":"before"}));
  let b=new_snap("b",json!({"file":"after"}));
  let drift=common_prefix_drift(&actions_a,&actions_b,&a,&b);
  assert_eq!(drift["kind"],"environment_or_tool_drift");
  let graph=chain_from_divergence(&actions_a,&actions_b,&b,&json!({}),&json!({}),&drift);
  assert_eq!(graph["mechanism"],"pre_intervention_observation_drift");
  assert_eq!(graph["counterfactualExecuted"],false);
 }

 #[test]
 fn different_dependency_topology_is_not_mistaken_for_identical_strategy() {
  let base=vec![action("a","workspace_read","first",&[]),
   action("b","workspace_read","second",&[]),
   action("c","workspace_write","out",&["a"])];
  let candidate=vec![action("x","workspace_read","first",&[]),
   action("y","workspace_read","second",&[]),
   action("z","workspace_write","out",&["y"])];
  let change=first_difference(&base,&candidate);
  assert_eq!(change["kind"],"decision_change");
  assert_eq!(change["ordinal"],2);
  let renumbered=vec![action("x","workspace_read","first",&[]),
   action("y","workspace_read","second",&[]),
   action("z","workspace_write","out",&["x"])];
  assert_eq!(first_difference(&base,&renumbered)["kind"],"same_declared_actions");
 }

 #[test]
 fn matched_runs_attribute_real_candidate_path_but_abstain_from_causality(){
  use crate::{config::Config,db::Store,vault::Vault};
  use dominion_sdk::harness::{
   CheckResult,Criterion,CriterionKind,Decision,RunRequest,Status,
  };
  let root=std::env::temp_dir().join(format!("adjudication-{}",uuid::Uuid::new_v4()));
  std::fs::create_dir_all(&root).unwrap();
  let cfg=Config{port:0,data_dir:root.clone(),web_dir:root.join("web"),
   workspace:root.clone()};
  let state=AppState::new(cfg.clone(),
   Store::open(&cfg.db_path()).unwrap(),Vault::open(&cfg.key_path()));
  let c=Criterion{kind:CriterionKind::FileExists,path:Some("out.txt".into()),
   contains:None,tool:None};
  let criteria=vec![c.clone()];
  let criteria_hash=native_replay::digest(&json!(criteria));
  let absent=json!({"complete":true,"entries":{"out.txt":{"state":"missing"}},
   "directVerdicts":[false],"criteriaDigest":criteria_hash});
  let present=json!({"complete":true,"entries":{"out.txt":{
   "state":"file","bytes":2,"sha256":"f00d"}},
   "directVerdicts":[true],"criteriaDigest":criteria_hash});
  for (id,tool,accepted) in [
   ("baseline","workspace_read",false),("candidate","workspace_write",true)
  ] {
   let action=Action{id:"a".into(),tool:tool.to_owned(),
    arguments:json!({"path":"out.txt","content":"ok"}),depends_on:vec![]};
   let request=RunRequest{id:id.into(),instruction:"create out.txt".into(),
    owner_instructions:String::new(),strategy:Default::default(),
    limits:Default::default(),allowed_tools:vec![tool.into()],
    checks:vec![c.clone()],require_evidence:true};
   let mut snap=Snapshot::from_request(&request);
   snap.status=if accepted{Status::Completed}else{Status::NeedsReview};
   snap.step=1;
   snap.decision_tape=vec![Decision{plan:vec![],actions:vec![action.clone()],answer:None}];
   let result=if accepted{json!({"written":true})}else{json!({"error":"missing"})};
   snap.receipts.push(Receipt{id:"a".into(),tool:tool.into(),
    arguments:action.arguments.clone(),success:accepted,
    result:result.clone(),step:1});
   snap.checks.push(CheckResult{criterion:c.clone(),
    satisfied:accepted,explanation:"observed".into()});
   let input=json!({"prompt":"create out.txt","checks":[{"type":"file_exists",
    "path":"out.txt"}],"limits":{"maxSteps":1},
    "allowedTools":["workspace_read","workspace_write"]});
   let final_evidence=if accepted{present.clone()}else{absent.clone()};
   state.store.put("job",&json!({"id":id,"kind":"prompt",
    "status":if accepted{"completed"}else{"needs_review"},
    "startedAt":"2026-11-18T00:00:00Z",
    "finishedAt":"2026-11-18T00:00:02Z",
    "nativeModelUsage":{"reportedCalls":1,"unreportedCalls":0,
      "unpricedCalls":0,"configuredCostKRW":if accepted{25.0}else{20.0}},
    "input":input,"nativeState":snap,
    "nativeRoute":{"providerId":"p","model":"m"},
    "nativeToolAuthoritySha256":"frozen",
    "nativeEvidenceBaseline":absent,"nativeEvidenceFinal":final_evidence,
    "nativeEvidenceKernelFinal":final_evidence})).unwrap();
   let mutation=if accepted {
    json!({"before":{"path":"out.txt","complete":true,"state":{"state":"missing"}},
     "after":{"path":"out.txt","complete":true,
      "state":{"state":"file","bytes":2,"sha256":"f00d"}}})
   }else{Value::Null};
   state.store.put("execution_operation",&json!({
    "id":format!("native-op-{id}-a"),"runId":id,"tool":tool,
    "args":action.arguments,"status":if accepted{"completed"}else{"failed"},
    "outcomeUnknown":false,"result":result,
    "artifactLineage":mutation})).unwrap();
  }
  let result=adjudicate(&state,"baseline",&["candidate".to_string()]).unwrap();
  assert_eq!(result["comparableRuns"],1);
  assert_eq!(result["evidenceQualifiedPortfolio"]["nondominatedRunIds"],
    json!(["candidate"]));
  assert_eq!(result["evidenceQualifiedPortfolio"]["reviewSuggestion"],"candidate");
  assert_eq!(result["candidates"][0]["comparison"],"observed_task_outcome_gain");
  assert_eq!(result["candidates"][0]["mechanism"]["mechanism"],
   "host_witnessed_candidate_mutation_chain");
  assert!(result["causalWinner"].is_null());
  assert_eq!(result["newExternalEffects"],false);
  let after_jobs=state.store.count("job");
  assert_eq!(after_jobs,2,"forensic adjudication must not create work");
  drop(state);
  let _=std::fs::remove_dir_all(root);
 }

 #[test]
 fn complementary_quality_cost_recovery_dimensions_preserve_tradeoffs() {
  let good=|id:&str,tier:u64,cost:Value,latency:Value,tokens:Value,failures:u64|{
   json!({"runId":id,"admissible":true,"proofTier":tier,
    "costKRW":cost,"latencyMs":latency,"recordedTokens":tokens,
    "observedFailedReceipts":failures,
    "completeComparableMetrics":!cost.is_null()&&!latency.is_null()&&!tokens.is_null()})
  };
  let a=good("fast",2,json!(120.0),json!(1000),json!(300),0);
  let b=good("slow",2,json!(150.0),json!(1200),json!(500),1);
  let c=good("cheap",2,json!(60.0),json!(1400),json!(200),0);
  let d=good("missing",2,Value::Null,json!(900),json!(100),0);
  let runs=vec![a.clone(),b,c.clone(),d];
  assert!(dominates(&a,&runs[1]));
  assert!(!dominates(&a,&c),"cost and speed tradeoffs are incomparable");
  assert!(!dominates(&d,&a),"unpriced calls cannot win on a fake zero");
  let p=compose_portfolio(&runs);
  assert_eq!(p["nondominatedRunIds"],json!(["fast","cheap"]));
  assert_eq!(p["reviewSuggestion"],Value::Null);
  assert_eq!(p["promotionAuthorized"],false);
  assert_eq!(p["repeatedTrialReliabilityMeasured"],false);
 }

 #[test]
 fn unverified_candidate_never_enters_frontier_even_when_cheap() {
  let invalid=json!({"runId":"phantom","admissible":false,"proofTier":2,
   "costKRW":0.0,"latencyMs":1,"recordedTokens":1,
   "observedFailedReceipts":0,"completeComparableMetrics":true});
  let p=compose_portfolio(&[invalid]);
  assert_eq!(p["status"],"insufficient_comparable_evidence");
  assert!(p["nondominatedRunIds"].as_array().unwrap().is_empty());
 }

 #[test]
 fn portfolio_admission_rejects_success_text_without_trusted_trace(){
  let proof=json!({"grade":"directly_corroborated",
   "terminalCompleted":true,"kernelAcceptancePassed":true,
   "criterionWitnessAgreement":true,"hostLineageConsistent":false});
  let trace=json!({"cost":{"configuredCostKRW":150.0},
   "latencyMs":1200,"estimatedTokensRecorded":140,"actions":[]});
  let row=portfolio_entry("unproven",true,&proof,&trace);
  assert_eq!(row["admissible"],false);
 }

 #[test]
 fn zero_matched_experiments_cannot_elect_an_unrelated_baseline(){
  let baseline=json!({"runId":"only_base","admissible":true,
   "proofTier":2,"costKRW":15.0,"latencyMs":50,"recordedTokens":20,
   "observedFailedReceipts":0,"completeComparableMetrics":true});
  assert_eq!(compose_portfolio(&[])["status"],"insufficient_comparable_evidence");
  assert_eq!(compose_portfolio(&[])["reviewSuggestion"],Value::Null);
  assert_eq!(compose_portfolio(&[baseline])["reviewSuggestion"],"only_base");
 }

}
