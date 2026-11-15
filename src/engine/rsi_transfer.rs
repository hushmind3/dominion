//! Cross-domain recursive transfer must be demonstrated by *measured* A→B→A
//! descendant changes, not by a model-generated domain label or three flags.
//! No model call, build, or Git mutation occurs while replaying this audit.
use crate::{engine::{rsi, rsi_evolve}, state::AppState};
use serde_json::{json,Value};

fn sha(value:&str,length:usize)->bool{
    value.len()==length && value.bytes().all(|byte|byte.is_ascii_hexdigit())
}

#[derive(Clone)]
struct VerifiedStep {
    domain:String,
    generator:String,
    accepted:String,
    candidate:String,
    attempt:u64,
}

/// Each row is accepted, independently re-evaluated from its frozen receipt,
/// and bound to the exact tested Git tree. Optional system-evolution claims
/// additionally require the actual spawned descendant executable receipt.
fn witnessed(state:&AppState,generation:&Value)->Option<VerifiedStep>{
    let (accepted,_,holdout_domains)=rsi::generation_proof(state,generation)?;
    let generator=generation["generatorCommit"].as_str()?;
    let domain=generation["domain"].as_str()?;
    let candidate_id=generation["candidateId"].as_str()?;
    if !sha(generator,40)||!sha(&accepted,40)||generator==accepted
        || !holdout_domains.contains(domain) {return None;}
    let candidate=state.store.get("rsi_candidate",candidate_id)?;
    let tree=candidate["evaluation"]["candidateTreeSha"].as_str()?;
    let pair=state.store.get("rsi_pair",&format!("rsi-pair-{candidate_id}"))?;
    if !sha(tree,40)||pair["report"]["runner"]!="balanced-frozen-rsi-lab-v3" {
        return None;
    }
    if generation["systemEvolution"]==true {
        let job=generation["jobId"].as_str()?;
        let campaign=generation["campaignId"].as_str()?;
        let binary=generation["generatorBinarySha256"].as_str()?;
        if generation["workerProofVerified"]!=true
            || !rsi_evolve::worker_proof_matches(
                state,job,campaign,generator,binary,&generation["controllerPlan"]) {
            return None;
        }
    }
    Some(VerifiedStep{
        domain:domain.into(),generator:generator.into(),
        accepted,candidate:candidate_id.into(),
        attempt:generation["attempt"].as_u64().unwrap_or(0),
    })
}

pub(crate) fn empty()->Value{
    json!({"demonstrated":false,"verifiedGenerations":0,
        "cycles":[],"nextReturnDomain":null,
        "proofKind":"v3_frozen_holdout_replay_descendant_git_lineage"})
}

/// Fail closed if any adopted revision in a sequence lacks a valid proof.
/// Research-only or rejected attempts do not mutate the selected Git HEAD,
/// so they may occur between two accepted and independently proven changes.
pub(crate) fn audit_history(state:&AppState,history:&[Value])->Value{
    let mut chain:Vec<VerifiedStep>=Vec::new();
    let mut cycles=Vec::new();
    let mut verified_count=0usize;
    for generation in history {
        if generation["accepted"]!=true {continue;}
        let Some(step)=witnessed(state,generation) else {
            chain.clear();continue;
        };
        if chain.last().is_some_and(|prior|prior.accepted!=step.generator){
            chain.clear();
        }
        verified_count+=1;
        chain.push(step);
        if chain.len()>=3 {
            let triple=&chain[chain.len()-3..];
            if triple[0].domain==triple[2].domain
                && triple[0].domain!=triple[1].domain {
                cycles.push(json!({
                    "domains":triple.iter().map(|item|item.domain.clone()).collect::<Vec<_>>(),
                    "generatorCommits":triple.iter().map(|item|item.generator.clone()).collect::<Vec<_>>(),
                    "acceptedCommits":triple.iter().map(|item|item.accepted.clone()).collect::<Vec<_>>(),
                    "candidateIds":triple.iter().map(|item|item.candidate.clone()).collect::<Vec<_>>(),
                    "attempts":triple.iter().map(|item|item.attempt).collect::<Vec<_>>()
                }));
            }
        }
    }
    // This is only a measured planning hint. The next compiled descendant
    // remains responsible for choosing a target and producing a real patch.
    let next_return=if cycles.is_empty() && chain.len()>=2 {
        let last=&chain[chain.len()-2..];
        (last[0].domain!=last[1].domain).then(||last[0].domain.clone())
    } else {None};
    json!({
        "demonstrated":!cycles.is_empty(),
        "verifiedGenerations":verified_count,
        "cycles":cycles,
        "nextReturnDomain":next_return,
        "proofKind":"v3_frozen_holdout_replay_descendant_git_lineage"
    })
}

pub(crate) fn audit_campaign(state:&AppState,campaign:&Value)->Value{
    audit_history(state,&rsi::lineage_history(state,campaign))
}

#[cfg(test)]
mod tests{
    use super::*;
    use crate::{config::Config,db::Store,vault::Vault};
    fn store()->(AppState,std::path::PathBuf){
        let root=std::env::temp_dir().join(format!("rsi-cycle-{}",uuid::Uuid::new_v4()));
        let cfg=Config{port:0,data_dir:root.clone(),web_dir:root.join("web"),workspace:root.clone()};
        (AppState::new(cfg.clone(),Store::open(&cfg.db_path()).unwrap(),Vault::open(&cfg.key_path())),root)
    }
    fn step(state:&AppState,n:usize,domain:&str)->Value{
        let commits=["a".repeat(40),"b".repeat(40),"c".repeat(40),"d".repeat(40)];
        let base=&commits[n];
        let adopted=&commits[n+1];
        let id=format!("cross-domain-{n}");
        let plan=(0..8).map(|i|json!({
            "id":format!("case-{i}"),"domain":if i==0{domain}else{"control"},
            "holdout":i<3,"metrics":[]
        })).collect::<Vec<_>>();
        let raw=(0..8).map(|i|json!({
            "id":format!("case-{i}"),"baselinePass":i!=0,
            "candidatePass":true,"metrics":{}
        })).collect::<Vec<_>>();
        let metrics=rsi::evaluate_cases(&plan,&raw,"harness").unwrap();
        assert_eq!(metrics["eligible"],true);
        let hash=format!("{:064x}",n+100);
        let patch_hash=format!("{:064x}",n+200);
        let baseline_tree=format!("{:040x}",n*2+1);
        let candidate_tree=format!("{:040x}",n*2+2);
        let report=json!({"runner":"balanced-frozen-rsi-lab-v3",
            "baseCommit":base,"patchSha256":patch_hash,
            "oracleVersion":"frozen-cross-domain",
            "baselineTreeSha":baseline_tree,"candidateTreeSha":candidate_tree,
            "reportSha256":hash,"baselineBuildOk":true,"candidateBuildOk":true,
            "cases":raw});
        let candidate=json!({"id":id,"status":"verified","target":"harness",
            "baseCommit":base,"patchSha256":patch_hash,
            "oracleVersion":"frozen-cross-domain","cases":plan,
            "experimentAdoption":{"commit":adopted},
            "evaluation":{"source":"server_invoked_local_paired_git_worktrees",
                "baselineTreeSha":baseline_tree,"candidateTreeSha":candidate_tree,
                "evidenceSha256":hash,"verdict":metrics["verdict"],"metrics":metrics}});
        state.store.put("rsi_candidate",&candidate).unwrap();
        state.store.put("rsi_pair",&json!({"id":format!("rsi-pair-{id}"),
            "candidateId":id,"runner":"local_gitrepo_not_os_sandbox",
            "evidenceSha256":hash,"report":report,
            "verdict":metrics["verdict"],"metrics":metrics})).unwrap();
        json!({"accepted":true,"candidateId":id,"domain":domain,"attempt":n+1,
            "generatorCommit":base,"acceptedCommit":adopted,
            "evaluation":{"evidenceSha256":hash}})
    }
    #[test]
    fn actual_replayed_holdouts_prove_a_to_b_to_a_not_three_flags(){
        let (state,root)=store();
        let a=step(&state,0,"memory");
        let b=step(&state,1,"tools");
        let c=step(&state,2,"memory");
        let history=vec![a.clone(),b.clone(),c.clone()];
        let report=audit_history(&state,&history);
        assert_eq!(report["demonstrated"],true);
        assert_eq!(report["cycles"][0]["domains"],json!(["memory","tools","memory"]));
        assert_eq!(report["verifiedGenerations"],3);
        let mut mislabeled=history.clone();
        mislabeled[2]["domain"]=json!("context");
        assert_eq!(audit_history(&state,&mislabeled)["demonstrated"],false);
        let mut disconnected=history.clone();
        disconnected[2]["generatorCommit"]=json!("f".repeat(40));
        assert_eq!(audit_history(&state,&disconnected)["demonstrated"],false);
        let mut flag_only=history.clone();
        flag_only[1]["candidateId"]=json!("fake-verified");
        assert_eq!(audit_history(&state,&flag_only)["demonstrated"],false);
        let mut with_research=vec![a,json!({"accepted":false,"domain":"context"}),b,c];
        assert_eq!(audit_history(&state,&with_research)["demonstrated"],true);
        with_research[1]=json!({"accepted":true,"domain":"context"});
        assert_eq!(audit_history(&state,&with_research)["demonstrated"],false);
        let _=std::fs::remove_dir_all(root);
    }
    #[test]
    fn heldout_or_executable_tampering_never_counts_as_transfer(){
        let (state,root)=store();
        let rows=vec![step(&state,0,"memory"),step(&state,1,"tools"),step(&state,2,"memory")];
        let hint=audit_history(&state,&rows[..2]);
        assert_eq!(hint["nextReturnDomain"],"memory");
        assert_eq!(hint["demonstrated"],false);
        let mut tampered=state.store.get("rsi_pair","rsi-pair-cross-domain-2").unwrap();
        tampered["report"]["cases"][0]["candidatePass"]=json!(false);
        state.store.put("rsi_pair",&tampered).unwrap();
        assert_eq!(audit_history(&state,&rows)["demonstrated"],false);
        tampered["report"]["cases"][0]["candidatePass"]=json!(true);
        state.store.put("rsi_pair",&tampered).unwrap();
        let mut fake_system=rows.clone();
        fake_system[2]["systemEvolution"]=json!(true);
        fake_system[2]["workerProofVerified"]=json!(true);
        assert_eq!(audit_history(&state,&fake_system)["demonstrated"],false);
        let mut claimed=state.store.get("rsi_candidate","cross-domain-1").unwrap();
        claimed["evaluation"]["candidateTreeSha"]=json!("f".repeat(40));
        state.store.put("rsi_candidate",&claimed).unwrap();
        assert_eq!(audit_history(&state,&rows)["demonstrated"],false);
        let _=std::fs::remove_dir_all(root);
    }
}
