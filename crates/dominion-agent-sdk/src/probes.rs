//! Task-local contrastive probes. Candidate tools are NOT an alternative
//! execution engine: the existing kernel scheduler, permissions, receipts,
//! cancellation and write-ahead journal execute every admitted probe.
//! These observations are strategy evidence, never owner acceptance checks.
use crate::{Action, Decision, Receipt, ToolSpec};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all="camelCase")]
pub struct CandidatePath {
    pub id: String,
    pub hypothesis: String,
    #[serde(default)]
    pub actions: Vec<Action>,
}

pub(crate) fn validate(paths:&[CandidatePath])->Result<(),String> {
    if paths.is_empty(){return Ok(());}
    if !(2..=3).contains(&paths.len()){
        return Err("Contrastive probe requires 2 or 3 distinct candidate paths".into());
    }
    let mut names=HashSet::new();
    let mut action_ids=HashSet::new();
    let mut hypotheses=HashSet::new();
    for path in paths {
        if path.id.trim().is_empty() || path.id.len()>48 ||
            !path.id.chars().all(|c|c.is_ascii_alphanumeric()||matches!(c,'_'|'-')){
            return Err("Candidate path ID must be 1..48 safe ASCII characters".into());
        }
        if !names.insert(path.id.clone()){
            return Err("Duplicate candidate path ID".into());
        }
        if path.hypothesis.trim().is_empty() || path.hypothesis.len()>240 ||
            !hypotheses.insert(path.hypothesis.trim().to_lowercase()){
            return Err("Candidate paths require distinct bounded hypotheses".into());
        }
        if !(1..=3).contains(&path.actions.len()){
            return Err("Each candidate path requires 1..3 read-only probes".into());
        }
        for action in &path.actions {
            if action.id.trim().is_empty() || action.id.len()>128 ||
                !action_ids.insert(action.id.clone()){
                return Err("Probe actions require distinct bounded operation IDs".into());
            }
            if !action.depends_on.is_empty(){
                return Err("Probe actions must be independent; use the normal execution plan for dependencies".into());
            }
        }
    }
    Ok(())
}

/// Validate every candidate before scheduling ANY probe. A model may not
/// label a mutating host tool as safe or turn a probe into a hidden effect.
pub(crate) fn admit(paths:&[CandidatePath],tools:&HashMap<String,ToolSpec>)
    ->Result<Vec<Action>,String> {
    validate(paths)?;
    let mut actions=Vec::new();
    for path in paths {
        for action in &path.actions {
            let spec=tools.get(&action.tool)
                .ok_or_else(||format!("Unknown or unauthorized probe tool: {}",action.tool))?;
            if !spec.read_only {
                return Err(format!("Candidate probe cannot execute effectful tool: {}",action.tool));
            }
            actions.push(action.clone());
        }
    }
    Ok(actions)
}

/// Reconstruct the most recent task-local comparison from the decision tape
/// and durable receipts. No database/snapshot extension, extra model call or
/// background optimizer is required. A successful tool call is NOT proof
/// that a strategy solves the user's task.
pub(crate) fn comparison(decisions:&[Decision],receipts:&[Receipt])->Value {
    let Some(decision)=decisions.iter().rev()
        .find(|decision|!decision.candidate_paths.is_empty()) else {
        return Value::Null;
    };
    let mut evidence=Vec::new();
    let mut all_settled=true;
    let mut best:Option<(String,usize,usize,usize)>=None;
    for path in &decision.candidate_paths {
        let observed=path.actions.iter().filter_map(|action|receipts.iter()
            .rev().find(|receipt|receipt.id==action.id)).collect::<Vec<_>>();
        let successful=observed.iter().filter(|r|r.success).count();
        let failed=observed.len()-successful;
        let pending=path.actions.len()-observed.len();
        if pending>0{all_settled=false;}
        evidence.push(json!({
            "pathId":path.id,"hypothesis":path.hypothesis,
            "successfulProbes":successful,"failedProbes":failed,
            "pendingProbes":pending,"totalProbes":path.actions.len(),
            "receiptIds":observed.iter().map(|r|r.id.as_str()).collect::<Vec<_>>()
        }));
        if pending==0 && successful>0 {
            // Compare observed fractions first, then require fewer operations
            // on ties. This is a bounded *probe preference*, not a success-rate
            // estimate or a replacement for owner acceptance checks.
            let better=best.as_ref().is_none_or(|(_,bs,bn,_)|
                successful * *bn > *bs * path.actions.len()
                || (successful * *bn == *bs * path.actions.len()
                    && path.actions.len() < *bn));
            if better {
                best=Some((path.id.clone(),successful,path.actions.len(),failed));
            }
        }
    }
    let recommended=if all_settled {
        best.map(|(id,_,_,_)|id)
    } else {None};
    json!({
        "status":if all_settled{"observed"}else{"partial"},
        "candidatePaths":evidence,
        "tentativePreferredPath":recommended,
        "interpretation":"read_only_tool_observations_not_task_success",
        "instruction":"Use observed receipts and owner acceptance requirements to choose or revise the next action. A successful inspection is not proof of task completion."
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn path(id:&str,hypothesis:&str,tool:&str)->CandidatePath{
        CandidatePath{id:id.into(),hypothesis:hypothesis.into(),
            actions:vec![Action{id:format!("{id}-read"),tool:tool.into(),
                arguments:json!({"path":"src/main.rs"}),depends_on:vec![]}]}
    }
    fn tools()->HashMap<String,ToolSpec>{
        [("workspace_read",true),("workspace_write",false)].into_iter()
            .map(|(name,read_only)|(name.into(),ToolSpec{
                name:name.into(),description:String::new(),
                schema:json!({"type":"object"}),read_only
            })).collect()
    }
    #[test]
    fn effects_and_unknown_tools_fail_before_any_probe_is_scheduled(){
        let good=path("diagnose","inspect compiler diagnostic","workspace_read");
        let write=path("change","write code early","workspace_write");
        assert!(admit(&[good.clone(),write],&tools()).is_err());
        let unknown=path("unknown","guess an unregistered tool","root_shell");
        assert!(admit(&[good,unknown],&tools()).is_err());
    }
    #[test]
    fn rejects_noncontrastive_and_duplicated_batches(){
        assert!(validate(&[path("a","inspect","workspace_read")]).is_err());
        assert!(validate(&[path("a","inspect","workspace_read"),
            path("a","inspect another thing","workspace_read")]).is_err());
        let mut b=path("b","read another file","workspace_read");
        b.actions[0].id="a-read".into();
        assert!(validate(&[path("a","inspect","workspace_read"),b]).is_err());
    }
    #[test]
    fn comparison_uses_actual_receipts_and_never_claims_completion(){
        let paths=vec![path("direct","quick scan","workspace_read"),
            path("diagnose","inspect different evidence","workspace_read")];
        let decision=Decision{plan:vec![],actions:vec![],answer:None,
            candidate_paths:paths};
        let receipt=Receipt{id:"direct-read".into(),tool:"workspace_read".into(),
            arguments:json!({}),success:true,result:json!({"content":"observed"}),
            step:1};
        let partial=comparison(&[decision.clone()],&[receipt.clone()]);
        assert_eq!(partial["status"],"partial");
        assert!(partial["tentativePreferredPath"].is_null());
        let failure=Receipt{id:"diagnose-read".into(),tool:"workspace_read".into(),
            arguments:json!({}),success:false,result:json!({"error":"not found"}),
            step:1};
        let full=comparison(&[decision],&[receipt,failure]);
        assert_eq!(full["status"],"observed");
        assert_eq!(full["tentativePreferredPath"],"direct");
        assert_eq!(full["interpretation"],"read_only_tool_observations_not_task_success");
    }
}
