//! Read-only native execution forensics and paired benchmark comparison.
//! This module NEVER dispatches a model/tool, updates a job or trusts a
//! model's completion claim over independent acceptance evidence.
use crate::state::AppState;
use dominion_sdk::harness::Snapshot;
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::collections::{BTreeMap,BTreeSet};

fn digest(v:&Value)->String{
 format!("{:x}",Sha256::digest(v.to_string().as_bytes()))
}
fn clip(s:&str)->String{s.chars().take(400).collect()}
fn snapshot(job:&Value)->Result<Snapshot,String>{
 if job["kind"]!="prompt"{return Err("Only native prompt jobs can be replayed".into())}
 let snap=serde_json::from_value::<Snapshot>(job["nativeState"].clone())
  .map_err(|e|format!("No durable native Rust checkpoint: {e}"))?;
 if snap.id!=job["id"]||snap.instruction!=job["input"]["prompt"]{
  return Err("Task identity mismatch between durable job and native snapshot".into())
 }
 Ok(snap)
}
fn measured_cost(job:&Value)->Option<f64>{
 let meter=&job["nativeModelUsage"];
 if meter["unpricedCalls"].as_u64().unwrap_or(0)>0
  ||meter["unreportedCalls"].as_u64().unwrap_or(0)>0
  ||meter["reportedCalls"].as_u64().unwrap_or(0)==0{
  return None
 }
 meter["configuredCostKRW"].as_f64()
  .filter(|v|v.is_finite()&&*v>=0.0)
}
fn elapsed_ms(job:&Value)->Option<i64>{
 let start=job["startedAt"].as_str()?;
 let end=job["finishedAt"].as_str()?;
 let a=chrono::DateTime::parse_from_rfc3339(start).ok()?;
 let b=chrono::DateTime::parse_from_rfc3339(end).ok()?;
 Some((b-a).num_milliseconds().max(0))
}
pub fn report(state:&AppState,id:&str)->Result<Value,String>{
 let job=state.store.get("job",id).ok_or("Native job not found")?;
 let snap=snapshot(&job)?;
 let mut tape=dominion_agent_sdk::replay::trace(&snap);
 let operations=state.store.execution_operations(id)?;
 let mut indexed=BTreeMap::new();
 let mut anomalies=Vec::new();
 let mut provenance=Vec::new();
 let mut receipts=BTreeSet::new();
 for r in &snap.receipts{receipts.insert(r.id.as_str());}
 for op in &operations{
  if let Some(opid)=op["id"].as_str(){
   indexed.insert(opid.to_owned(),op);
  }
 }
 let mut matched=0usize;
 for r in &snap.receipts{
  let id=format!("native-op-{}-{}",snap.id,r.id);
  if let Some(op)=indexed.get(&id){
   matched+=1;
   if op["runId"]!=snap.id||op["tool"]!=r.tool||op["args"]!=r.arguments{
    anomalies.push(format!("Journal action/argument mismatch at {}",r.id));
   }
   if op["outcomeUnknown"]==true{
    anomalies.push(format!("Uncertain external outcome at {}",r.id));
   }
   if op["status"]=="completed" {
    if !r.success {anomalies.push(format!("Host success disagrees with receipt {}",r.id))}
    if let Some(expected)=op["resultSha256"].as_str(){
     if expected!=digest(&r.result){
      anomalies.push(format!("Tool result digest mismatch at {}",r.id))
     }
    }else if op["result"]!=r.result{
     anomalies.push(format!("Legacy operation output differs at {}",r.id))
    }
   }else if op["status"]=="failed" {
    if r.success {anomalies.push(format!("Host failure disagrees with receipt {}",r.id))}
   }else{
    // A pending/running or unknown host operation is not settled evidence,
    // even if a stale checkpoint advertises success and outcomeUnknown=false.
    anomalies.push(format!("Operation lacks a terminal host status at {}",r.id));
   }
   let pin=op["provenance"]["contractSha256"].as_str();
   if r.tool.starts_with("mcp__"){
    if let Some(expected)=job["nativeMcpContracts"][&r.tool].as_str(){
     if pin!=Some(expected){anomalies.push(format!("MCP contract witness missing/mismatched at {}",r.id))}
    }else if pin.is_some(){
     anomalies.push(format!("MCP contract not recorded in job at {}",r.id))
    }
   }
   provenance.push(json!({"operationId":r.id,"tool":r.tool,
    "origin":op["provenance"]["origin"],"sourceTrust":op["provenance"]["trust"],
    "contractSha256":pin,"argumentsSha256":op["provenance"]["argumentsSha256"],
    "resultSha256":op["resultSha256"],"outcomeUnknown":op["outcomeUnknown"]}));
  }else if r.success && !r.tool.starts_with("dominion_"){
   anomalies.push(format!("Successful host tool has no operation journal: {}",r.id));
  }
 }
 for op in &operations{
  if let Some(key)=op["id"].as_str(){
   if let Some(act)=key.strip_prefix(&format!("native-op-{}-",snap.id)){
    if !receipts.contains(act){
     anomalies.push(format!("External effect has no matching checkpoint receipt: {act}"));
    }
   }
  }
 }
 let consistent=anomalies.is_empty()&&tape["traceConsistent"]==true;
 tape["hostJournal"] = json!({
  "operationCount":operations.len(),"matchedReceipts":matched,
  "consistent":consistent,"anomalies":anomalies,
  "sourceEvidence":provenance.into_iter().take(200).collect::<Vec<_>>()
 });
 tape["cost"] = json!({"configuredCostKRW":measured_cost(&job),
  "currency":"KRW",
  "invoiceVerified":false,
  "unknownCalls":job["nativeModelUsage"]["unpricedCalls"].as_u64().unwrap_or(0)
    + job["nativeModelUsage"]["unreportedCalls"].as_u64().unwrap_or(0),
  "basis":"owner_configured_tariff_x_provider_reported_usage"});
 tape["latencyMs"]=json!(elapsed_ms(&job));
 tape["modelIdentity"]=json!({"providerId":job["nativeRoute"]["providerId"],
  "model":job["nativeRoute"]["model"]});
 tape["offlineOnly"]=json!(true);
 Ok(tape)
}
fn same_work(a:&Value,b:&Value)->Result<(),String>{
 for (name,left,right) in [
  ("task",&a["input"]["prompt"],&b["input"]["prompt"]),
  ("checks",&a["input"]["checks"],&b["input"]["checks"]),
  ("limits",&a["input"]["limits"],&b["input"]["limits"]),
  ("permissions",&a["input"]["allowedTools"],&b["input"]["allowedTools"]),
  ("provider",&a["nativeRoute"]["providerId"],&b["nativeRoute"]["providerId"]),
  ("model",&a["nativeRoute"]["model"],&b["nativeRoute"]["model"]),
 ]{
  if left!=right{return Err(format!("Paired benchmark requires identical {name}"))}
 }
 Ok(())
}
pub fn compare(state:&AppState,baseline_id:&str,candidate_id:&str)->Result<Value,String>{
 if baseline_id==candidate_id{return Err("Compare two different run IDs".into())}
 let a=state.store.get("job",baseline_id).ok_or("Baseline job not found")?;
 let b=state.store.get("job",candidate_id).ok_or("Candidate job not found")?;
 same_work(&a,&b)?;
 let left=report(state,baseline_id)?;
 let right=report(state,candidate_id)?;
 if left["requiresManualReconciliation"]==true ||right["requiresManualReconciliation"]==true {
  return Err("Uncertain external effects cannot be used in a paired score".into())
 }
 let checks_a=&left["acceptanceChecks"];
 let checks_b=&right["acceptanceChecks"];
 let measured_quality=checks_a["total"].as_u64().unwrap_or(0)>0
  &&checks_a["total"]==checks_b["total"]
  &&left["hostJournal"]["consistent"]==true
  &&right["hostJournal"]["consistent"]==true
  &&left["decisionTapeAvailable"]==true &&right["decisionTapeAvailable"]==true;
 let cost_a=left["cost"]["configuredCostKRW"].as_f64();
 let cost_b=right["cost"]["configuredCostKRW"].as_f64();
 let delta_cost=cost_a.zip(cost_b).map(|(a,b)|b-a);
 let latency_a=left["latencyMs"].as_i64();
 let latency_b=right["latencyMs"].as_i64();
 let accepted=measured_quality &&right["reportedCompleted"]==true
  &&right["acceptanceChecks"]["allSatisfied"]==true
  &&(left["reportedCompleted"]!=true||left["acceptanceChecks"]["allSatisfied"]!=true
    ||(delta_cost.is_some_and(|v|v<=0.0)));
 Ok(json!({
  "mode":"read_only_paired_recorded_run_comparison",
  "externalCalls":0,"sameTaskModelLimitsPermissions":true,
  "baselineRunId":baseline_id,"candidateRunId":candidate_id,
  "qualityComparable":measured_quality,
  "candidateEligibleForManualReview":accepted,
  "autoPromotion":false,
  "baseline":{"status":left["status"],"completed":left["reportedCompleted"],
    "checks":checks_a,"toolCalls":left["receiptsReconstructed"],"modelCalls":left["modelCallsRecorded"],
    "chargedTokens":left["estimatedTokensRecorded"],"costKRW":cost_a,"latencyMs":latency_a,
    "hostJournalConsistent":left["hostJournal"]["consistent"]},
  "candidate":{"status":right["status"],"completed":right["reportedCompleted"],
    "checks":checks_b,"toolCalls":right["receiptsReconstructed"],"modelCalls":right["modelCallsRecorded"],
    "chargedTokens":right["estimatedTokensRecorded"],"costKRW":cost_b,"latencyMs":latency_b,
    "hostJournalConsistent":right["hostJournal"]["consistent"]},
  "differences":{
   "toolCalls":right["receiptsReconstructed"].as_i64().unwrap_or(0)
    -left["receiptsReconstructed"].as_i64().unwrap_or(0),
   "modelCalls":right["modelCallsRecorded"].as_i64().unwrap_or(0)
    -left["modelCallsRecorded"].as_i64().unwrap_or(0),
   "chargedTokens":right["estimatedTokensRecorded"].as_i64().unwrap_or(0)
    -left["estimatedTokensRecorded"].as_i64().unwrap_or(0),
   "configuredCostKRW":delta_cost,
   "latencyMs":latency_a.zip(latency_b).map(|(a,b)|b-a)
  },
  "qualification":if measured_quality {
    "Comparable historical acceptance with identical work shape; actual-world reexecution still required."
  }else{
    "Cannot rank quality: independent acceptance, immutable decision tape or consistent host journal missing."
  }
 }))
}
#[cfg(test)]
mod tests{
 use super::*;
 use crate::{config::Config,db::Store,vault::Vault};
 fn state()->AppState{
  let dir=std::env::temp_dir().join(format!("dominion-replay-{}",uuid::Uuid::new_v4()));
  let cfg=Config{port:0,data_dir:dir.clone(),web_dir:dir.join("web"),workspace:dir};
  AppState::new(cfg.clone(),Store::open(&cfg.db_path()).unwrap(),
   Vault::open(&cfg.key_path()))
 }
 #[test]
 fn replay_is_read_only_reconstructs_failure_and_rejects_unmatched_paired_runs(){
  let s=state();
  let input=json!({"prompt":"fix test","checks":[{"type":"tool_succeeded","tool":"shell_exec"}],
   "limits":{"maxSteps":4},"allowedTools":["shell_exec"]});
  let base=json!({"id":"replay-a","kind":"prompt","input":input,
   "status":"needs_review",
   "nativeRoute":{"providerId":"p","model":"m"},
   "nativeState":{"id":"replay-a","instruction":"fix test","status":"needs_review",
    "plan":[],"step":2,"modelCalls":1,"chargedTokens":100,"receipts":[
     {"id":"test1","tool":"shell_exec","arguments":{"command":"fail"},"success":false,
     "result":{"error":"exitCode 1"},"step":2}],
    "checks":[{"criterion":{"type":"tool_succeeded","tool":"shell_exec"},
     "satisfied":false,"explanation":"failed"}],
    "answer":null,"error":"verification failed","inFlight":[],"modelInFlight":false,
    "decisionTape":[{"plan":[],"actions":[{"id":"test1","tool":"shell_exec",
     "arguments":{"command":"fail"},"dependsOn":[]}],"answer":null}]}
  });
  s.store.put("job",&base).unwrap();
  s.store.put("execution_operation",&json!({"id":"native-op-replay-a-test1",
   "runId":"replay-a","tool":"shell_exec","args":{"command":"fail"},
   "status":"failed","outcomeUnknown":false})).unwrap();
  let report=report(&s,"replay-a").unwrap();
  assert_eq!(report["hostJournal"]["consistent"],true);
  assert_eq!(report["actions"][0]["status"],"observed_failure");
  assert_eq!(report["externalCalls"],Value::Null);
  assert_eq!(report["modelOrExternalToolCalls"],0);
  assert_eq!(s.store.count("execution_operation"),1);
  let mut unsettled=s.store.get("execution_operation","native-op-replay-a-test1").unwrap();
  unsettled["status"]=json!("running");
  unsettled["outcomeUnknown"]=json!(false);
  s.store.put("execution_operation",&unsettled).unwrap();
  let pending_report=super::report(&s,"replay-a").unwrap();
  assert_eq!(pending_report["hostJournal"]["consistent"],false,
   "a nonterminal operation is never accepted as settled historical evidence");
  let mut other=base.clone();other["id"]=json!("replay-b");
  other["nativeState"]["id"]=json!("replay-b");
  other["input"]["prompt"]=json!("different task");
  other["nativeState"]["instruction"]=json!("different task");
  s.store.put("job",&other).unwrap();
  assert!(compare(&s,"replay-a","replay-b").is_err());
  assert_eq!(s.store.count("job"),2);
 }
}
