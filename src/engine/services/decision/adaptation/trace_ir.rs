//! 2026 HarnessFix-style provenance IR for *actual* Dominion tool receipts.
//!
//! Temporal order alone does not explain a failure. Reconstruct explicit
//! version-provenance, shared-resource and program-control links from the
//! existing native gateway receipts, then provide a bounded backward slice to
//! the repair planner. No model call, tool dispatch or state mutation here.
//! Source: HarnessFix HTIR (Chen et al., 2026), adapted to Dominion's
//! execution_operation and effect-verification contracts.
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet, VecDeque};

const MAX_NODES: usize = 18;
const MAX_EDGES: usize = 36;

fn evidence_id(op: &Value) -> String {
    format!("receipt-{}", crate::engine::memory::stable_id(&[
        op["id"].as_str().unwrap_or("missing-operation"),
    ]))
}
fn digest(value: &str) -> String {
    crate::engine::memory::stable_id(&[value])
}
fn resource(op: &Value) -> Option<String> {
    op["resourceKey"].as_str().filter(|value| !value.is_empty()).map(str::to_owned)
}

/// Only machine-readable, explicit upstream receipt references qualify as
/// data provenance; substring matches to model prose are never proof.
fn references(value: &Value, out: &mut Vec<String>, depth: usize) {
    if depth > 5 || out.len() >= 12 {return;}
    match value {
        Value::Object(fields) => {
            for (key, item) in fields {
                if matches!(key.as_str(),
                    "sourceOperationId" | "fromOperationId" | "originOperationId"
                    | "sourceReceiptId")
                {
                    if let Some(id)=item.as_str().filter(|s| !s.is_empty()) {
                        out.push(id.to_string());
                    }
                } else {
                    references(item, out, depth+1);
                }
                if out.len()>=12 {break;}
            }
        }
        Value::Array(items) => {
            for item in items.iter().take(12) {
                references(item, out, depth+1);
                if out.len()>=12 {break;}
            }
        }
        _=>{}
    }
}

/// Observed failure surface, NOT a claim of root cause. Only statuses/flags
/// from the native tool/effect gateway are allowed to define this label.
fn surface(op: &Value) -> &'static str {
    let result=&op["result"];
    let effect=op["effectState"].as_str().unwrap_or("");
    if matches!(effect,"denied"|"needs_review")
        || result["preflightBlocked"]==true {
        "governance"
    } else if result["effectUnknown"]==true || result["outcomeUnknown"]==true
        || matches!(effect,"unknown"|"unresolved") {
        "effect_reconciliation"
    } else if result["invalidWorkspacePath"]==true {
        "tool_interface"
    } else if result["exitCode"].as_i64().is_some_and(|code|code!=0)
        || result["status"].as_u64().is_some_and(|code|code>=400)
        || result["error"].is_string() || op["status"]=="failed" {
        "execution_or_environment"
    } else {
        "not_failed"
    }
}
fn summarized(op: &Value)->Value {
    json!({
        "id": evidence_id(op),
        "sequence":op["opSeq"],
        "tool":op["tool"],
        "origin":op["origin"],
        "status":op["status"],
        "failureSurface":surface(op),
        "effectState":op["effectState"],
        "receiptVerified":op["toolReceipt"]["verified"]==true,
        "resourceFingerprint":resource(op).map(|s|digest(&s)),
        "programNode":op["procedureNodeId"],
        "hasObservedVersion":op["observedVersion"].is_string(),
        "resultHasError":op["result"]["error"].is_string(),
    })
}
fn add_edge(
    edges:&mut Vec<(usize,usize,&'static str)>,
    dedupe:&mut HashSet<(usize,usize,&'static str)>,
    from:usize,to:usize,kind:&'static str
) {
    if from!=to && from<to && dedupe.insert((from,to,kind)){
        edges.push((from,to,kind));
    }
}

/// Turn a bounded chronological sequence of receipts from *one execution*
/// into a compact evidence graph focused on the exact failed call ID. Even
/// matching tool names can be different failures: caller ID wins over guesses.
pub fn compile(
    operations:&[Value],
    failed_call_id:&str,
    failed_tool:&str,
    failed_arguments:&Value,
) -> Value {
    let mut operations=operations.iter().filter(|row|row["id"].is_string())
        .collect::<Vec<_>>();
    operations.sort_by(|a,b|
        a["opSeq"].as_u64().unwrap_or(u64::MAX)
            .cmp(&b["opSeq"].as_u64().unwrap_or(u64::MAX))
            .then_with(||a["id"].as_str().cmp(&b["id"].as_str()))
    );
    let by_id=operations.iter().enumerate()
        .filter_map(|(i,row)|row["id"].as_str().map(|id|(id.to_string(),i)))
        .collect::<HashMap<_,_>>();
    let mut edges=Vec::<(usize,usize,&'static str)>::new();
    let mut dedupe=HashSet::new();
    let mut last_resource=HashMap::<String,usize>::new();
    let mut last_program_node=HashMap::<(String,String),usize>::new();
    for (i,row) in operations.iter().enumerate() {
        if i>0 {add_edge(&mut edges,&mut dedupe,i-1,i,"temporal");}
        if let Some(source)=row["beforeVersion"]["sourceOperationId"].as_str()
            .and_then(|id|by_id.get(id)).copied() {
            add_edge(&mut edges,&mut dedupe,source,i,"observed_version");
        }
        if let Some(source)=row["fileObservation"]["sourceOperationId"].as_str()
            .and_then(|id|by_id.get(id)).copied() {
            add_edge(&mut edges,&mut dedupe,source,i,"observed_version");
        }
        let mut inputs=Vec::new();
        references(&row["arguments"],&mut inputs,0);
        for source in inputs {
            if let Some(prev)=by_id.get(&source) {
                add_edge(&mut edges,&mut dedupe,*prev,i,"input_provenance");
            }
        }
        if let Some(key)=resource(row) {
            if let Some(prev)=last_resource.insert(key,i) {
                add_edge(&mut edges,&mut dedupe,prev,i,"resource_dependency");
            }
        }
        if let (Some(strategy),Some(node))=(
            row["procedureStrategyId"].as_str(),row["procedureNodeId"].as_str()
        ){
            let key=(strategy.to_string(),node.to_string());
            if let Some(prev)=last_program_node.insert(key,i) {
                add_edge(&mut edges,&mut dedupe,prev,i,"procedure_control");
            }
        }
    }
    let focus=operations.iter().position(|op|
        op["callId"].as_str()==Some(failed_call_id) && !failed_call_id.is_empty()
    ).or_else(||operations.iter().rposition(|op|
        op["tool"].as_str()==Some(failed_tool)
        && &op["arguments"]==failed_arguments && op["status"]=="failed"
    ));
    let Some(focus)=focus else {
        return json!({
            "schema":"dominion.htir.2026.v1","focus":Value::Null,
            "nodes":[],"edges":[],"causalClaims":"none",
            "note":"No matching committed tool receipt; do not infer provenance from prose."
        });
    };
    // The diagnostic graph is a *backward provenance slice*, not a timeline
    // dump. Follow version/input/resource/program dependencies transitively.
    let mut included=HashSet::<usize>::new();
    let mut queue=VecDeque::<(usize,usize)>::new();
    queue.push_back((focus,0));
    while let Some((at,depth))=queue.pop_front() {
        if !included.insert(at) {continue;}
        if included.len()>=MAX_NODES || depth>=5 {continue;}
        for &(from,to,kind) in &edges {
            if to==at && kind!="temporal" && !included.contains(&from) {
                queue.push_back((from,depth+1));
            }
        }
    }
    // Show a small local timeline as a control-flow contrast while keeping
    // unrelated session activity out of the model context.
    for index in (focus.saturating_sub(3)..focus).rev() {
        if included.len()>=MAX_NODES {break;}
        included.insert(index);
    }
    let mut indices=included.into_iter().collect::<Vec<_>>();
    indices.sort_unstable();
    let focus_id=evidence_id(operations[focus]);
    let kept=indices.iter().copied().collect::<HashSet<_>>();
    let nodes=indices.iter().map(|&i|summarized(operations[i]))
        .collect::<Vec<_>>();
    let relational=edges.iter().filter(|(from,to,_)|
        kept.contains(from) && kept.contains(to)
    ).take(MAX_EDGES).map(|&(from,to,relation)|json!({
        "from":evidence_id(operations[from]),
        "to":evidence_id(operations[to]),
        "relation":relation,
    })).collect::<Vec<_>>();
    json!({
        "schema":"dominion.htir.2026.v1",
        "focus":{"evidenceId":focus_id,"failureSurface":surface(operations[focus])},
        "nodes":nodes,"edges":relational,
        "causalClaims":"candidate dependencies only; tool observations are facts, attribution is not",
        "totalOperationsObserved":operations.len(),
    })
}

/// A model may cite the special direct failure observation, or a real receipt
/// in the presented HTIR. Reject fabricated receipt references rather than
/// allowing an invented cause to justify a write intervention.
pub fn validate_evidence_ids(adaptation:&Value, trace:&Value)->Result<(),String>{
    let allowed=trace["nodes"].as_array().into_iter().flatten()
        .filter_map(|node|node["id"].as_str())
        .collect::<HashSet<_>>();
    for group in ["hypotheses","interventions"] {
        for item in adaptation[group].as_array().into_iter().flatten() {
            if group=="interventions"
                && crate::engine::effect::is_effectful(
                    item["tool"].as_str().unwrap_or(""),&item["inputs"])
                && item["evidenceIds"].as_array().is_none_or(Vec::is_empty) {
                return Err("state-changing repair candidate must cite observed evidenceIds".into());
            }
            for id in item["evidenceIds"].as_array().into_iter().flatten() {
                let id=id.as_str().ok_or("evidenceIds must contain strings")?;
                if id!="before" && id!="current-failure" && !allowed.contains(id) {
                    return Err(format!("unknown trace evidence id {id}; cite an observed receipt or before"));
                }
            }
        }
    }
    Ok(())
}

/// A receipt merely preceding a failure is not causal evidence.
/// Follow only explicit input/version/resource/program relations backwards
/// from the failed operation. Temporal neighbors remain usable as context
/// but cannot authorize a model's causal explanation for a mutation.
pub fn provenance_ancestors(trace:&Value)->HashSet<String>{
    let mut ancestors=HashSet::new();
    let Some(focus)=trace["focus"]["evidenceId"].as_str() else{
        return ancestors;
    };
    let mut queue=VecDeque::from([focus.to_string()]);
    while let Some(target)=queue.pop_front(){
        if !ancestors.insert(target.clone()){continue;}
        if ancestors.len()>=MAX_NODES{break;}
        for relation in trace["edges"].as_array().into_iter().flatten(){
            if relation["to"].as_str()!=Some(target.as_str())
                || relation["relation"]=="temporal" {
                continue;
            }
            if let Some(source)=relation["from"].as_str(){
                if !ancestors.contains(source){
                    queue.push_back(source.to_string());
                }
            }
        }
    }
    ancestors
}

/// A machine-verifiable, bounded repair admission certificate. This is a
/// Dominion-specific action boundary: trajectory evidence constrains which
/// of the model's interventions may run, but never grants tool permission.
/// The existing dispatcher still owns approvals and effect reconciliation.
pub fn action_certificate(
    trace:&Value,
    intervention:&Value,
    hypotheses:&[Value],
    refuted:&[String],
    is_effectful:bool,
)->Result<Value,String>{
    let ids=intervention["hypothesisIds"].as_array()
        .filter(|items|!items.is_empty())
        .ok_or("repair intervention must identify non-empty causal hypotheses")?;
    let mut live=Vec::new();
    for id in ids {
        let id=id.as_str().ok_or("repair hypothesis ID is not a string")?;
        if !hypotheses.iter().any(|h|h["id"].as_str()==Some(id)) {
            return Err(format!("intervention uses an unknown causal hypothesis {id}"));
        }
        if !refuted.iter().any(|r|r.as_str()==id) && !live.contains(&id) {
            live.push(id);
        }
    }
    if live.is_empty() {
        return Err("all hypotheses supporting the selected intervention were refuted".into());
    }
    let failure=trace["focus"]["failureSurface"].as_str().unwrap_or("unobserved");
    if is_effectful && trace["focus"].is_null() {
        return Err("no committed failure receipt: state-changing repair lacks observable provenance".into());
    }
    if is_effectful && matches!(failure,"effect_reconciliation"|"governance"){
        return Err(format!(
            "an unresolved/denied native effect ({failure}) cannot authorize a speculative write; inspect or request owner review"
        ));
    }
    let available=trace["nodes"].as_array().into_iter().flatten()
        .filter_map(|row|row["id"].as_str()).collect::<HashSet<_>>();
    let ancestors=provenance_ancestors(trace);
    let mut cited=Vec::new();
    for id in intervention["evidenceIds"].as_array().into_iter().flatten() {
        let id=id.as_str().ok_or("non-string intervention evidence ID")?;
        if id=="before" || id=="current-failure" {
            cited.push(id);
        } else if !available.contains(id) {
            return Err(format!("candidate intervention references nonexistent receipt {id}"));
        } else if !ancestors.contains(id) {
            return Err(format!("receipt {id} is a temporal neighbor, not an observed dependency of the failure"));
        } else {
            cited.push(id);
        }
    }
    if is_effectful && cited.is_empty() {
        return Err("state-changing intervention must cite at least one actual failure/provenance observation".into());
    }
    Ok(json!({
        "schema":"dominion.action-certificate.2026.v1",
        "interventionId":intervention["id"],
        "tool":intervention["tool"],
        "liveHypothesisIds":live,
        "observedFailureSurface":failure,
        "evidenceIds":cited,
        "causalAttribution":"candidate-only",
        "effectful":is_effectful,
        "gatewayRequired":true,
        "status":"admitted_by_causal_constraints",
    }))
}

#[cfg(test)]
mod tests{
    use super::*;

    fn op(id:&str,call:&str,seq:u64,status:&str,resource:&str)->Value{
        json!({"id":id,"callId":call,"opSeq":seq,"tool":"workspace_read",
          "origin":"model","status":status,"resourceKey":resource,
          "effectState":if status=="failed"{"unresolved"}else{"completed"},
          "toolReceipt":{"verified":status=="completed"},
          "arguments":{"path":"x.rs"},"result":if status=="failed"{json!({"error":"failed"})}else{json!({})}
        })
    }
    #[test]
    fn exact_call_receipt_wins_over_identical_failed_tools(){
        let mut a=op("r1","first",1,"failed","r");
        a["tool"]=json!("workspace_read");
        let b=op("r2","second",2,"failed","r");
        let trace=compile(&[a,b],"first","workspace_read",&json!({"path":"x.rs"}));
        assert_eq!(trace["focus"]["evidenceId"],evidence_id(&op("r1","first",1,"failed","r")));
    }
    #[test]
    fn backward_slice_contains_real_version_and_resource_links(){
        let original=op("read-1","a",1,"completed","r");
        let mut write=op("write-2","b",2,"completed","r");
        write["beforeVersion"]=json!({"sourceOperationId":"read-1"});
        let failure=op("failed-3","c",3,"failed","r");
        let trace=compile(&[original,write,failure],"c","workspace_read",&json!({"path":"x.rs"}));
        let edges=trace["edges"].as_array().unwrap();
        assert!(edges.iter().any(|e|e["relation"]=="observed_version"));
        assert!(edges.iter().any(|e|e["relation"]=="resource_dependency"));
        assert_eq!(trace["nodes"].as_array().unwrap().len(),3);
    }
    #[test]
    fn fabricated_causal_evidence_is_rejected(){
        let a=op("r1","x",1,"failed","r");
        let trace=compile(&[a],"x","workspace_read",&json!({"path":"x.rs"}));
        let good=json!({"hypotheses":[{"evidenceIds":[trace["focus"]["evidenceId"]]}],
            "interventions":[{"evidenceIds":["before"]}]});
        validate_evidence_ids(&good,&trace).unwrap();
        let bad=json!({"hypotheses":[{"evidenceIds":["receipt-invented"]}]});
        assert!(validate_evidence_ids(&bad,&trace).is_err());
    }
    #[test]
    fn causal_certificate_blocks_refuted_mutation_and_unknown_effect(){
        let trace=compile(&[op("r1","x",1,"failed","r")],
            "x","workspace_read",&json!({"path":"x.rs"}));
        let hypotheses=vec![json!({"id":"h1"}),json!({"id":"h2"})];
        let intervention=json!({
            "id":"fix","tool":"workspace_write","hypothesisIds":["h1"],
            "evidenceIds":[trace["focus"]["evidenceId"]]
        });
        assert!(action_certificate(&trace,&intervention,&hypotheses,&[],true).is_err());
        let read=json!({
            "id":"inspect","tool":"workspace_read","hypothesisIds":["h1","h2"],
            "evidenceIds":[trace["focus"]["evidenceId"]]
        });
        assert!(action_certificate(&trace,&read,&hypotheses,&[],false).is_ok());
        assert!(action_certificate(&trace,&read,&hypotheses,&[
            "h1".to_string(),"h2".to_string()
        ],false).is_err());
    }
    #[test]
    fn missing_native_receipt_never_promotes_write_action(){
        let trace=json!({"focus":null,"nodes":[]});
        let mutation=json!({"id":"fix","tool":"workspace_write",
            "hypothesisIds":["h1"]});
        let hypotheses=vec![json!({"id":"h1"})];
        assert!(action_certificate(&trace,&mutation,&hypotheses,&[],true).is_err());
        assert!(action_certificate(&trace,&mutation,&hypotheses,&[],false).is_ok());
    }
    #[test]
    fn evidence_bound_certificate_rejects_phantom_source(){
        let mut row=op("r1","x",1,"failed","r");
        row["effectState"]=json!("committed");
        let trace=compile(&[row],"x","workspace_read",&json!({"path":"x.rs"}));
        let hypotheses=vec![json!({"id":"h1"})];
        let valid=json!({
            "id":"fix","tool":"workspace_write","hypothesisIds":["h1"],
            "evidenceIds":[trace["focus"]["evidenceId"]]
        });
        let cert=action_certificate(&trace,&valid,&hypotheses,&[],true).unwrap();
        assert_eq!(cert["status"],"admitted_by_causal_constraints");
        let invalid=json!({
            "id":"hallucinated","tool":"workspace_write",
            "hypothesisIds":["h1"],"evidenceIds":["receipt-imaginary"]
        });
        assert!(action_certificate(&trace,&invalid,&hypotheses,&[],true).is_err());
    }
    #[test]
    fn mutation_evidence_requirement_is_checked_before_probe_cost(){
        let r=op("r1","x",1,"failed","resource");
        let trace=compile(&[r],"x","workspace_read",&json!({"path":"x.rs"}));
        let candidate=json!({"hypotheses":[{"id":"h1"}],
            "interventions":[{"id":"fix","tool":"workspace_write",
                "inputs":{"path":"x.rs","content":"new"},
                "hypothesisIds":["h1"]}]});
        assert!(validate_evidence_ids(&candidate,&trace).is_err());
        let mut cited=candidate;
        cited["interventions"][0]["evidenceIds"]=json!(["current-failure"]);
        assert!(validate_evidence_ids(&cited,&trace).is_ok());
    }
    #[test]
    fn no_matched_receipt_produces_no_fabricated_lineage(){
        let a=op("r1","x",1,"failed","r");
        let trace=compile(&[a],"not-found","shell_exec",&json!({"command":"n/a"}));
        assert!(trace["nodes"].as_array().unwrap().is_empty());
        assert_eq!(trace["focus"],Value::Null);
    }
    #[test]
    fn stored_raw_tool_content_is_not_replayed_into_model_trace(){
        let mut row=op("r1","x",1,"failed","private");
        row["result"]["error"]=json!("SECRET_TOKEN_ABC");
        row["arguments"]["accessToken"]=json!("SECRET_TOKEN_ABC");
        let text=compile(&[row],"x","workspace_read",&json!({"path":"x.rs"})).to_string();
        assert!(!text.contains("SECRET_TOKEN_ABC"));
        assert!(!text.contains("private"));
    }
    #[test]
    fn chronological_neighbor_cannot_masquerade_as_dependency(){
        let unrelated=op("unrelated","u",1,"completed","different-resource");
        let related=op("upstream","a",2,"completed","same-resource");
        let failed=op("failure","f",3,"failed","same-resource");
        let trace=compile(&[unrelated,related,failed],"f","workspace_read",&json!({"path":"x.rs"}));
        let unrelated_id=evidence_id(&op("unrelated","u",1,"completed","different-resource"));
        let related_id=evidence_id(&op("upstream","a",2,"completed","same-resource"));
        let ancestors=provenance_ancestors(&trace);
        assert!(!ancestors.contains(&unrelated_id));
        assert!(ancestors.contains(&related_id));
        let hypotheses=vec![json!({"id":"h1"})];
        let false_cause=json!({"id":"fix","tool":"workspace_read",
            "hypothesisIds":["h1"],"evidenceIds":[unrelated_id]});
        assert!(action_certificate(&trace,&false_cause,&hypotheses,&[],false).is_err());
        let related_cause=json!({"id":"fix","tool":"workspace_read",
            "hypothesisIds":["h1"],"evidenceIds":[related_id]});
        assert!(action_certificate(&trace,&related_cause,&hypotheses,&[],false).is_ok());
    }

    #[test]
    fn previous_provenance_uses_explicit_reference_not_name_similarity(){
        let a=op("source-1","a",1,"completed","one");
        let mut b=op("consumer-2","b",2,"failed","other");
        b["arguments"]["resultBinding"]=json!({"sourceOperationId":"source-1"});
        let trace=compile(&[a,b],"b","workspace_read",&json!({"path":"x.rs"}));
        assert!(trace["edges"].as_array().unwrap().iter().any(|e|
            e["relation"]=="input_provenance"));
    }
}
