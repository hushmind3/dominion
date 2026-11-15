//! Learn an execution-time model-routing policy from independent RSI outcomes.
//!
//! A model is never called "better" because it authored a successful-sounding
//! proposal. Only distinct, paired source-changing candidates evaluated from
//! the SAME generator revision, SAME frozen profile and SAME capability
//! domain can compare model choices. Such evidence updates the actual request
//! router, while explicit user model selection and free/light policy remain.
//! This is learned routing of model-backed whole-agent source redesigns.
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::collections::{BTreeMap,BTreeSet};

fn sha(v:&Value)->String{
    format!("{:x}",Sha256::digest(serde_json::to_vec(v).unwrap_or_default()))
}
fn identity(g:&Value)->Option<(String,String)>{
    Some((
        g["generatorProviderId"].as_str()?.to_string(),
        g["generatorModel"].as_str()?.to_string()
    ))
}
fn eligible(g:&Value)->bool{
    g["pairedRevalidated"]==true
        && g["selfControllerVerified"]==true
        && g["candidateId"].as_str().is_some_and(|s|!s.is_empty())
        && g["candidateOrigin"]=="model_driven_detached_worktree"
        && g["changedSourcePaths"].as_array().is_some_and(|paths|
            !paths.is_empty()&&paths.iter().all(|p|
                p.as_str().is_some_and(crate::engine::rsi::safe_path)))
        && g["generatorCommit"].as_str().is_some_and(|v|
            v.len()==40 && v.bytes().all(|b|b.is_ascii_hexdigit()))
        && g["evaluation"]["source"]=="server_invoked_local_paired_git_worktrees"
        && g["evaluation"]["evidenceSha256"].as_str().is_some_and(|v|
            v.len()==64 && v.bytes().all(|b|b.is_ascii_hexdigit()))
        && g["comparisonProfileSha256"].as_str().is_some_and(|v|
            v.len()==64 && v.bytes().all(|b|b.is_ascii_hexdigit()))
        && g["domain"].as_str().is_some_and(|s|!s.is_empty())
        && identity(g).is_some()
        && g["evaluation"]["metrics"]["holdoutEvidence"].as_array()
            .is_some_and(|rows|!rows.is_empty())
}
type Cohort=(String,String,String);
fn cohort(g:&Value)->Option<Cohort>{
    if !eligible(g){return None;}
    Some((
        g["generatorCommit"].as_str()?.to_string(),
        g["comparisonProfileSha256"].as_str()?.to_string(),
        g["domain"].as_str()?.to_string()
    ))
}
#[derive(Default)]
struct Stats {
    paired_cohorts:BTreeSet<Cohort>,
    independent_jobs:BTreeSet<String>,
    verified_gains:u64,
    regressions:u64,
    adopted:u64,
    successor_trials:u64,
    successor_adoptions:u64,
    successor_cross_domain_wins:u64,
    cross_model_scaffold_transfer_trials:u64,
    successor_holdout_gains:u64,
    successor_holdout_regressions:u64,
    successor_jobs:BTreeSet<String>,
    domains:BTreeSet<String>
}
/// Train an empirical capability-selection model on *comparable* trials.
/// No comparisons between different models unless their runs share an exact
/// frozen baseline and evaluation profile. A single lucky match is untrusted.
pub(crate) fn learn(history:&[Value])->Value{
    let mut cohorts=BTreeMap::<Cohort,Vec<&Value>>::new();
    for g in history{
        if let Some(key)=cohort(g){cohorts.entry(key).or_default().push(g);}
    }
    let mut stats=BTreeMap::<(String,String),Stats>::new();
    let mut comparable=0u64;
    for (cohort,rows) in &cohorts{
        let models=rows.iter().filter_map(|g|identity(g))
            .collect::<BTreeSet<_>>();
        if models.len()<2{continue;}
        comparable+=1;
        for g in rows{
            let Some(model)=identity(g)else{continue};
            let Some(job)=g["jobId"].as_str()else{continue};
            let item=stats.entry(model).or_default();
            if !item.independent_jobs.insert(job.to_string()){continue;}
            item.paired_cohorts.insert(cohort.clone());
            item.domains.insert(cohort.2.clone());
            for case in g["evaluation"]["metrics"]["holdoutEvidence"]
                .as_array().into_iter().flatten(){
                let pass=case["baselinePass"]==false&&case["candidatePass"]==true
                    ||case["metricGains"].as_u64().unwrap_or(0)>0;
                let regress=case["baselinePass"]==true&&case["candidatePass"]==false
                    ||case["metricRegressions"].as_u64().unwrap_or(0)>0;
                if pass{item.verified_gains+=1;}
                if regress{item.regressions+=1;}
            }
            if g["accepted"]==true
                && g["evaluation"]["metrics"]["eligible"]==true
                && g["acceptedCommit"].as_str().is_some_and(|v|v.len()==40){
                item.adopted+=1;
                // Whole-agent improvement productivity is measured by the
                // RECOMPILED successor generating another independently
                // evaluated source change. An adopted parent patch alone
                // does not prove stronger future self-improvement.
                if let Some(accepted)=g["acceptedCommit"].as_str(){
                    for child in history.iter().filter(|next|
                        next["generatorCommit"]==accepted && eligible(next)
                            && next["generatorBinarySha256"].as_str()
                                .is_some_and(|v|v.len()==64
                                    && v.bytes().all(|b|b.is_ascii_hexdigit()))
                            && next["systemBlueprintVerified"]==true){
                        let Some(child_id)=child["jobId"].as_str()else{continue};
                        if !item.successor_jobs.insert(child_id.to_string()){continue;}
                        if identity(child)!=identity(g){
                            // The parent agent's rewritten scaffold may have
                            // helped a DIFFERENT model. That is cross-model
                            // software transfer, not evidence that the
                            // originating API's own model improved.
                            item.cross_model_scaffold_transfer_trials+=1;
                            continue;
                        }
                        item.successor_trials+=1;
                        if child["accepted"]==true
                            && child["evaluation"]["metrics"]["eligible"]==true{
                            item.successor_adoptions+=1;
                            if child["domain"]!=g["domain"]{
                                item.successor_cross_domain_wins+=1;
                            }
                        }
                        for row in child["evaluation"]["metrics"]["holdoutEvidence"]
                            .as_array().into_iter().flatten(){
                            if row["baselinePass"]==false&&row["candidatePass"]==true
                                || row["metricGains"].as_u64().unwrap_or(0)>0{
                                item.successor_holdout_gains+=1;
                            }
                            if row["baselinePass"]==true&&row["candidatePass"]==false
                                || row["metricRegressions"].as_u64().unwrap_or(0)>0{
                                item.successor_holdout_regressions+=1;
                            }
                        }
                    }
                }
            }
        }
    }
    let rows=stats.into_iter().map(|((provider,model),s)|{
        let independent=s.independent_jobs.len();
        let cohorts=s.paired_cohorts.len();
        let domains=s.domains.len();
        // The learned posterior needs at least two independent baselines
        // AND two domains before it can alter general-purpose routing.
        let trusted=cohorts>=2&&domains>=2&&independent>=2;
        let gains=s.verified_gains as f64;
        let losses=s.regressions as f64;
        let offspring_rate=(s.successor_adoptions as f64+1.0)
            /(s.successor_trials as f64+2.0);
        let offspring_gain=if s.successor_trials>0{
            0.25*offspring_rate
                +0.12*(s.successor_cross_domain_wins.min(4) as f64)
                -0.12*(s.successor_holdout_regressions.min(4) as f64)
        }else{0.0};
        let score=(gains+1.0)/(gains+losses+2.0)
            +0.15*(s.adopted as f64)/(independent.max(1) as f64)
            +offspring_gain;
        json!({
            "providerId":provider,"model":model,
            "comparableCohorts":cohorts,"observedDomains":domains,
            "independentTrials":independent,
            "holdoutGains":s.verified_gains,"holdoutRegressions":s.regressions,
            "independentlyAdoptedSourceCandidates":s.adopted,
            "verifiedCompiledSuccessorTrials":s.successor_trials,
            "verifiedCompiledSuccessorAdoptions":s.successor_adoptions,
            "crossCapabilitySuccessorAdoptions":s.successor_cross_domain_wins,
            "differentModelScaffoldTransferTrials":s.cross_model_scaffold_transfer_trials,
            "successorHoldoutGains":s.successor_holdout_gains,
            "successorHoldoutRegressions":s.successor_holdout_regressions,
            "posteriorSuccessorImprovement":offspring_rate,
            "posteriorMeasuredImprovement":score,
            "trustedForDeepAutoRoute":trusted,
            "attribution":"observational_model_plus_harness_not_weight_causal_proof"
        })
    }).collect::<Vec<_>>();
    let mut model=json!({
        "source":"compiled_trainable_model_routing_v1",
        "comparableBaselineProfiles":comparable,
        "models":rows,
        "modelWeightsUpdated":false,
        "learnerEditableSource":"src/engine/rsi_model_learning.rs",
        "trainingOrigin":"independent_frozen_paired_harness_trials",
        "holdoutContentsNotReusedForTraining":true,
        "qualityProvenForUnseenUserTasks":false
    });
    model["sha256"]=json!(sha(&model));model
}
/// Production routing must re-read the original signed-off artifacts, not
/// merely trust booleans stored on a generation JSON row. This is deliberately
/// stricter than the pure learner's synthetic-fixture contract.
pub(crate) fn learn_verified(state:&crate::state::AppState)->Value{
    let vetted=state.store.all("rsi_generation").into_iter().filter(|generation|{
        if !eligible(generation)
            || !crate::engine::rsi::model_route_audited(state,generation) {
            return false;
        }
        let Some(candidate_id)=generation["candidateId"].as_str()else{return false};
        let Some(candidate)=state.store.get("rsi_candidate",candidate_id)
            else{return false};
        let Some(replay)=crate::engine::rsi::replayed_pair_metrics(state,&candidate)
            else{return false};
        candidate["evaluation"]["source"]=="server_invoked_local_paired_git_worktrees"
            && candidate["evaluation"]["metrics"]==replay
            && generation["evaluation"]["metrics"]==replay
            && candidate["generatorModel"]==generation["generatorModel"]
            && candidate["baseCommit"]==generation["generatorCommit"]
    }).collect::<Vec<_>>();
    let mut result=learn(&vetted);
    result["trustedInputGenerations"]=json!(vetted.len());
    result["productionEvidence"]=json!("source_replayed_and_audited");
    result
}

/// Only route matches with sufficient, shared independent evidence may
/// overrule static capability heuristics. User-selected model never calls
/// this function.
pub(crate) fn compared_preference(model:&Value,a:&Value,b:&Value)->Option<std::cmp::Ordering>{
    let find=|row:&Value|model["models"].as_array().and_then(|models|
        models.iter().find(|candidate|
            candidate["providerId"]==row["id"]
                && candidate["model"]==row["model"]
                && candidate["trustedForDeepAutoRoute"]==true));
    let a=find(a)?;let b=find(b)?;
    let pa=a["posteriorMeasuredImprovement"].as_f64()?;
    let pb=b["posteriorMeasuredImprovement"].as_f64()?;
    if (pa-pb).abs()<0.12{return None;}
    Some(pb.total_cmp(&pa))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample(job:&str,base:&str,domain:&str,provider:&str,good:bool)->Value{
        json!({"jobId":job,"generatorCommit":base,
            "generatorProviderId":provider,"generatorModel":"model",
            "domain":domain,"comparisonProfileSha256":"f".repeat(64),
            "changedSourcePaths":["src/engine/router.rs"],
            "pairedRevalidated":true,"selfControllerVerified":true,
            "candidateOrigin":"model_driven_detached_worktree",
            "candidateId":job,"accepted":good,
            "acceptedCommit":if good{json!("c".repeat(40))}else{Value::Null},
            "evaluation":{"source":"server_invoked_local_paired_git_worktrees",
                "evidenceSha256":"e".repeat(64),
                "metrics":{"eligible":good,"holdoutEvidence":[{
                "id":"frozen","baselinePass":!good,"candidatePass":good,
                "metricGains":0,"metricRegressions":0
            }]}}})
    }
    #[test]
    fn comparable_cross_domain_model_learning_changes_route_preference(){
        let a="a".repeat(40);let b="b".repeat(40);
        let evidence=vec![
            sample("a1",&a,"memory","strong",true),
            sample("b1",&a,"memory","weak",false),
            sample("a2",&b,"tools","strong",true),
            sample("b2",&b,"tools","weak",false)
        ];
        let model=learn(&evidence);
        let strong=json!({"id":"strong","model":"model"});
        let weak=json!({"id":"weak","model":"model"});
        assert_eq!(compared_preference(&model,&strong,&weak),
            Some(std::cmp::Ordering::Less));
        let mut not_measured=evidence;
        not_measured[0]["pairedRevalidated"]=json!(false);
        let untrusted=learn(&not_measured);
        assert!(compared_preference(&untrusted,&strong,&weak).is_none());
    }
    #[test]
    fn fabricated_no_edit_candidate_does_not_train_model_routing(){
        let a="a".repeat(40);let b="b".repeat(40);
        let mut fabricated=sample("a1",&a,"memory","strong",true);
        fabricated["changedSourcePaths"]=json!([]);
        let model=learn(&[
            fabricated,sample("b1",&a,"memory","weak",false),
            sample("a2",&b,"tools","strong",true),
            sample("b2",&b,"tools","weak",false)
        ]);
        let strong=json!({"id":"strong","model":"model"});
        let weak=json!({"id":"weak","model":"model"});
        assert!(compared_preference(&model,&strong,&weak).is_none());
    }
    #[test]
    fn api_model_is_credited_only_for_measured_compiled_successor_capability(){
        let a="a".repeat(40);let other="b".repeat(40);
        let parent=sample("parent",&a,"memory","api",true);
        let competitor=sample("competitor",&a,"memory","local",false);
        let unrelated_parent=sample("unrelated",&other,"tools","local",false);
        let unrelated_cmp=sample("second",&other,"tools","api",true);
        let mut offspring=sample("offspring",&"c".repeat(40),
            "tools","api",true);
        offspring["generatorBinarySha256"]=json!("a".repeat(64));
        offspring["systemBlueprintVerified"]=json!(true);
        let history=vec![parent.clone(),competitor.clone(),
            unrelated_parent.clone(),unrelated_cmp.clone(),
            offspring.clone()];
        let model=learn(&history);
        let api=model["models"].as_array().unwrap().iter()
            .find(|row|row["providerId"]=="api").unwrap();
        assert_eq!(api["verifiedCompiledSuccessorTrials"],1);
        assert_eq!(api["crossCapabilitySuccessorAdoptions"],1);
        offspring["generatorBinarySha256"]=Value::Null;
        let changed=learn(&[parent,competitor,unrelated_parent,
            unrelated_cmp,offspring]);
        let api=changed["models"].as_array().unwrap().iter()
            .find(|row|row["providerId"]=="api").unwrap();
        assert_eq!(api["verifiedCompiledSuccessorTrials"],0);
    }
    #[test]
    fn code_transfer_across_models_does_not_fake_improvement_of_api_weights(){
        let base="a".repeat(40);
        let parent=sample("parent",&base,"memory","api",true);
        let competitor=sample("competitor",&base,"memory","local",false);
        let mut offspring=sample("offline-child",&"c".repeat(40),
            "tools","local",true);
        offspring["generatorBinarySha256"]=json!("1".repeat(64));
        offspring["systemBlueprintVerified"]=json!(true);
        let model=learn(&[parent,competitor,offspring]);
        let api=model["models"].as_array().unwrap().iter()
            .find(|row|row["providerId"]=="api").unwrap();
        assert_eq!(api["differentModelScaffoldTransferTrials"],1);
        assert_eq!(api["verifiedCompiledSuccessorTrials"],0);
    }
    #[test]
    fn no_model_comparison_without_same_frozen_baseline(){
        let a="a".repeat(40);let b="b".repeat(40);
        let model=learn(&[
            sample("solo",&a,"memory","model-a",true),
            sample("other",&b,"memory","model-b",false)
        ]);
        assert_eq!(model["comparableBaselineProfiles"],0);
    }
}
