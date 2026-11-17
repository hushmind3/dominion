//! Contrastive, evidence-gated ProgramIR repair for the *existing* Rust runner.
//! Multiple model proposals share ONE model call; each candidate is validated
//! by program::apply_revision_proposal, read-only preflights use the normal tool
//! gateway, and only the evidence-surviving Pareto frontier can be committed.
//! Completed external effects stay owned by the current program checkpoint.
use serde_json::{json, Value};
use std::collections::{BTreeSet, HashSet};

const MAX_PROPOSALS: usize = 3;
const MAX_VARIANTS: usize = 6;
const MAX_PREFLIGHTS: usize = 3;

fn ids(proposal:&Value)->BTreeSet<String>{
    proposal["replacePendingNodes"].as_array().into_iter().flatten()
        .chain(proposal["addNodes"].as_array().into_iter().flatten())
        .filter_map(|n|n["id"].as_str().map(str::to_owned)).collect()
}
fn mutations(proposal:&Value)->usize{
    proposal["replacePendingNodes"].as_array().into_iter().flatten()
        .chain(proposal["addNodes"].as_array().into_iter().flatten())
        .filter(|n|{
            n["type"]=="delegate"
                || n["type"]=="call_tool" && crate::engine::effect::is_effectful(
                    n["tool"].as_str().unwrap_or(""),&n["inputs"])
        }).count()
}
fn evidence_ids(proposal:&Value,checkpoint:&Value,failure:&Value)->Result<(),String>{
    let ids=proposal["evidenceIds"].as_array()
        .filter(|v|!v.is_empty())
        .ok_or("candidate repair needs non-empty evidenceIds")?;
    let node=failure["nodeId"].as_str().unwrap_or("");
    for id in ids {
        let id=id.as_str().ok_or("candidate evidence ID must be a string")?;
        if id!="program-failure" && id!=node && checkpoint["observationRefs"].get(id).is_none(){
            return Err(format!("candidate cites unknown checkpoint evidence {id}"));
        }
    }
    Ok(())
}
/// A preflight is a specific observation required to distinguish candidate
/// repairs, not a model-reported expected success or a synthetic rollout.
fn checked_probe(candidate_id:&str,probe:&Value)->Result<Value,String>{
    let tool=probe["tool"].as_str().ok_or("preflight.tool required")?;
    if !crate::engine::adaptation::probes::read_only_tool(tool) {
        return Err("preflight must be a native observational tool".into());
    }
    if !probe["inputs"].is_object() || probe["inputs"].to_string().len()>8192{
        return Err("preflight inputs must be a bounded object".into());
    }
    let predicate=&probe["predicate"];
    if !predicate.is_object()
        || predicate["from"]!="result"
        || !matches!(predicate["type"].as_str(),Some("equals"|"compare"|"exists"))
        || predicate["path"].as_str().is_none_or(|p|!p.starts_with('/')){
        return Err("preflight must verify an observable result JSON pointer".into());
    }
    Ok(json!({"id":candidate_id,"tool":tool,"inputs":probe["inputs"],
        "predicate":predicate}))
}
fn compose(a:&Value,b:&Value)->Option<Value>{
    let a_ids=ids(a);let b_ids=ids(b);
    if !a_ids.is_disjoint(&b_ids) || a["baseRevision"]!=b["baseRevision"]{return None;}
    let mut merged=json!({
        "baseRevision":a["baseRevision"],
        "reason":format!("compose {} + {}",a["reason"].as_str()?,b["reason"].as_str()?),
        "evidenceIds":[],
        "replacePendingNodes":[],
        "addNodes":[],
        "expectedObservations":[],
    });
    for key in ["evidenceIds","replacePendingNodes","addNodes","expectedObservations"]{
        let mut both=a[key].as_array().cloned().unwrap_or_default();
        both.extend(b[key].as_array().cloned().unwrap_or_default());
        if key=="evidenceIds" {
            let mut unique=HashSet::new();
            both.retain(|v|unique.insert(v.to_string()));
        }
        merged[key]=json!(both);
    }
    Some(merged)
}
/// Validated proposals plus compatible pairwise compositions. A multi-proposal
/// model response MUST provide a real read-only preflight for each candidate.
/// Legacy single revision retains the old path when no candidateRevisions exist.
pub fn prepare(
    model:&Value,program:&Value,checkpoint:&Value,failure:&Value,
    tools:&[String],repair_mode:&str,
)->Result<Vec<Value>,String>{
    let Some(raw)=model["candidateRevisions"].as_array() else {
        let revision=crate::engine::program::apply_revision_proposal(
            program,checkpoint,model,tools,64,repair_mode)?;
        return Ok(vec![json!({"id":"legacy","revision":revision,"probes":[],
            "sourceCandidates":["legacy"],"legacy":true})]);
    };
    if raw.is_empty() || raw.len()>MAX_PROPOSALS {
        return Err("candidateRevisions requires 1..3 independent proposals".into());
    }
    let mut variants=Vec::new();let mut accepted=Vec::new();
    for (index, proposal) in raw.iter().enumerate() {
        if evidence_ids(proposal,checkpoint,failure).is_err(){continue;}
        let Ok(revision)=crate::engine::program::apply_revision_proposal(
            program,checkpoint,proposal,tools,64,repair_mode
        ) else {continue;};
        let Some(probe)=proposal["preflight"].as_object() else {continue;};
        let id=format!("p{index}");
        let Ok(probe)=checked_probe(&id,&json!(probe)) else{continue;};
        let costs=json!({
            "changedNodes": ids(proposal).len(),
            "effectfulNodes":mutations(proposal),
        });
        variants.push(json!({
            "id":id,"revision":revision,"probes":[probe],
            "costs":costs,"sourceCandidates":[id],
        }));
        accepted.push((id,proposal.clone()));
    }
    if accepted.is_empty(){
        return Err("no valid evidence-grounded candidate revision with an observational preflight".into());
    }
    for a in 0..accepted.len() {
        for b in (a+1)..accepted.len() {
            if variants.len()>=MAX_VARIANTS{break;}
            let Some(merged)=compose(&accepted[a].1,&accepted[b].1) else{continue;};
            let Ok(revision)=crate::engine::program::apply_revision_proposal(
                program,checkpoint,&merged,tools,64,repair_mode) else{continue;};
            let parts=[accepted[a].0.clone(),accepted[b].0.clone()];
            let mut probes=Vec::new();
            for p in &parts {
                let Some(original)=variants.iter().find(|v|v["id"].as_str()==Some(p.as_str())) else{continue;};
                probes.extend(original["probes"].as_array().into_iter().flatten().cloned());
            }
            variants.push(json!({"id":format!("compose-{a}-{b}"),
                "revision":revision,"probes":probes,"sourceCandidates":parts,
                "costs":{"changedNodes":ids(&merged).len(),"effectfulNodes":mutations(&merged)}}));
        }
    }
    Ok(variants)
}
/// Preflight calls deduplicated by concrete tool+arguments. The gateway owns
/// the operations; this function only produces an action request shortlist.
pub fn probes(variants:&[Value])->Vec<Value>{
    let mut seen=HashSet::new();let mut out=Vec::new();
    for variant in variants{
        for probe in variant["probes"].as_array().into_iter().flatten(){
            let key=format!("{}:{}",probe["tool"],probe["inputs"]);
            if seen.insert(key){out.push(probe.clone());}
            if out.len()>=MAX_PREFLIGHTS{return out;}
        }
    }
    out
}
/// Check *actual* results. A missing, denied or unexecuted probe is Unknown
/// and cannot be credited to a candidate. No weighted model opinion enters
/// the decision.
pub fn select(variants:&[Value], observations:&Value)->Result<(Value,Value),String>{
    use crate::engine::program::PredicateVerdict;
    let mut evaluated=Vec::new();
    for variant in variants{
        let probes=variant["probes"].as_array().cloned().unwrap_or_default();
        let mut pass=0usize;let mut fail=0usize;let mut unknown=0usize;
        for probe in probes{
            let Some(id)=probe["id"].as_str() else{continue};
            let Some(observed)=observations.get(id) else{unknown+=1;continue};
            if observed["status"]!="completed" {unknown+=1;continue;}
            let scope=json!({"result":observed["result"]});
            match crate::engine::program::predicate_verdict(&probe["predicate"],&scope){
                Ok(PredicateVerdict::True)=>pass+=1,
                Ok(PredicateVerdict::False)=>fail+=1,
                _=>unknown+=1,
            }
        }
        let qualified=variant["legacy"]==true || (pass>0 && fail==0 && unknown==0);
        evaluated.push(json!({
            "id":variant["id"],"passed":pass,"failed":fail,"unknown":unknown,
            "qualified":qualified,"changedNodes":variant["costs"]["changedNodes"],
            "effectfulNodes":variant["costs"]["effectfulNodes"],
        }));
    }
    let mut frontier=Vec::new();
    for (i, candidate) in evaluated.iter().enumerate(){
        if candidate["qualified"]!=true {continue;}
        let dominates=|a:&Value,b:&Value|{
            let a_pass=a["passed"].as_u64().unwrap_or(0);
            let b_pass=b["passed"].as_u64().unwrap_or(0);
            let a_mut=a["effectfulNodes"].as_u64().unwrap_or(u64::MAX);
            let b_mut=b["effectfulNodes"].as_u64().unwrap_or(u64::MAX);
            let a_edit=a["changedNodes"].as_u64().unwrap_or(u64::MAX);
            let b_edit=b["changedNodes"].as_u64().unwrap_or(u64::MAX);
            a_pass>=b_pass && a_mut<=b_mut && a_edit<=b_edit
                && (a_pass>b_pass || a_mut<b_mut || a_edit<b_edit)
        };
        if evaluated.iter().enumerate().any(|(j,other)|
            i!=j && other["qualified"]==true && dominates(other,candidate)){continue;}
        frontier.push(i);
    }
    if frontier.is_empty(){
        return Err("no candidate revision has a successful, determinate native preflight".into());
    }
    frontier.sort_by(|&a,&b|{
        let left=&evaluated[a];let right=&evaluated[b];
        right["passed"].as_u64().unwrap_or(0).cmp(&left["passed"].as_u64().unwrap_or(0))
            .then_with(||left["effectfulNodes"].as_u64().cmp(&right["effectfulNodes"].as_u64()))
            .then_with(||left["changedNodes"].as_u64().cmp(&right["changedNodes"].as_u64()))
            .then_with(||left["id"].as_str().cmp(&right["id"].as_str()))
    });
    let selected=frontier[0];
    let audit=json!({
        "schema":"dominion.program-contrast.2026.v1",
        "evaluated":evaluated,
        "frontier":frontier.iter().map(|&i|evaluated[i]["id"].clone()).collect::<Vec<_>>(),
        "chosen":variants[selected]["id"],"observedProbeCount":observations.as_object().map(|o|o.len()).unwrap_or(0),
    });
    Ok((variants[selected]["revision"].clone(),audit))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture()->(Value,Value,Value,Vec<String>){
        let program=json!({"revision":"base","nodes":[
            {"id":"done","type":"call_tool","tool":"workspace_read",
                "inputs":{"path":"a"},"saveAs":"read"},
            {"id":"fix-a","type":"call_tool","tool":"workspace_write",
                "inputs":{"path":"a","content":"bad"}},
            {"id":"fix-b","type":"call_tool","tool":"workspace_write",
                "inputs":{"path":"b","content":"bad"}}]});
        let checkpoint=json!({"completedOperationIds":["done"],
            "observationRefs":{"done":"real-hash"},"resolvedInputs":{"read":{"content":"old"}}});
        let failure=json!({"nodeId":"fix-a","result":{"error":"failed"}});
        let tools=vec!["workspace_read".into(),"workspace_write".into()];
        (program,checkpoint,failure,tools)
    }
    fn proposal(id:&str,path:&str,expect:&str)->Value{
        json!({"baseRevision":"base","reason":format!("repair {id}"),
            "evidenceIds":["fix-a"],"replacePendingNodes":[{"id":id,"type":"call_tool",
                "tool":"workspace_write","inputs":{"path":path,"content":"new"}}],
            "preflight":{"tool":"workspace_read","inputs":{"path":path},
                "predicate":{"type":"equals","from":"result","path":"/content","value":expect}}})
    }
    #[test]
    fn validates_independent_candidates_and_composes_disjoint_pending_edits(){
        let (program,checkpoint,failure,tools)=fixture();
        let input=json!({"candidateRevisions":[
            proposal("fix-a","a","old"), proposal("fix-b","b","present")]});
        let variants=prepare(&input,&program,&checkpoint,&failure,&tools,"replace").unwrap();
        assert_eq!(variants.len(),3);
        assert_eq!(variants[2]["sourceCandidates"],json!(["p0","p1"]));
        assert_eq!(variants[2]["revision"]["nodes"].as_array().unwrap().len(),3);
        assert_eq!(probes(&variants).len(),2);
    }
    #[test]
    fn observed_contrast_eliminates_wrong_candidate_and_does_not_apply_unknown(){
        let (program,checkpoint,failure,tools)=fixture();
        let variants=prepare(&json!({"candidateRevisions":[
            proposal("fix-a","a","old"),proposal("fix-b","b","present")]}),
            &program,&checkpoint,&failure,&tools,"replace").unwrap();
        let observations=json!({"p0":{"status":"completed","result":{"content":"old"}},
            "p1":{"status":"completed","result":{"content":"missing"}}});
        let (_selected, audit)=select(&variants,&observations).unwrap();
        assert_eq!(audit["chosen"],"p0");
        assert_eq!(audit["evaluated"][1]["qualified"],false);
        assert_eq!(audit["evaluated"][2]["qualified"],false);
        assert!(select(&variants,&json!({})).is_err());
    }
    #[test]
    fn multi_candidate_with_unsafe_probe_or_fake_evidence_cannot_win(){
        let (program,checkpoint,failure,tools)=fixture();
        let mut wrong=proposal("fix-a","a","old");
        wrong["evidenceIds"]=json!(["fictional-proof"]);
        let mut unsafe_probe=proposal("fix-b","b","old");
        unsafe_probe["preflight"]["tool"]=json!("shell_exec");
        let input=json!({"candidateRevisions":[wrong,unsafe_probe]});
        assert!(prepare(&input,&program,&checkpoint,&failure,&tools,"replace").is_err());
    }
    #[test]
    fn completed_node_mutation_is_invalid_and_legacy_contract_survives(){
        let (program,checkpoint,failure,tools)=fixture();
        let mut edit=proposal("done","a","old");
        edit.as_object_mut().unwrap().remove("preflight");
        assert!(prepare(&edit,&program,&checkpoint,&failure,&tools,"replace").is_err());
        let legacy=proposal("fix-a","a","old");
        let variants=prepare(&legacy,&program,&checkpoint,&failure,&tools,"replace").unwrap();
        assert_eq!(variants.len(),1);
        let (_chosen,audit)=select(&variants,&json!({})).unwrap();
        assert_eq!(audit["chosen"],"legacy");
    }
}
