//! Confidence-gated model selection from independent real verifier results.
//! Explicit provider choice is still owned by the caller; this only changes
//! auto-routing when every candidate has substantial comparable evidence.
use crate::engine::{router::Complexity,adaptation::types::TaskOutcome};
use crate::state::AppState;
use serde_json::{json,Value};
use std::collections::{HashMap,HashSet};

fn bucket(c:Complexity)->&'static str{
    match c{Complexity::Light=>"light",Complexity::Standard=>"standard",
        Complexity::Deep=>"deep"}
}
#[derive(Default,Clone,Copy)]
struct Cohort{attempts:usize,passes:usize}
fn lower_bound(s:Cohort)->f64{
    let n=s.attempts as f64;
    if n==0.0{return 0.0;}
    let p=s.passes as f64/n;
    let z=1.28f64;
    (p+z*z/(2.0*n)-z*(p*(1.0-p)/n+z*z/(4.0*n*n)).sqrt())
        /(1.0+z*z/n)
}
fn cohorts(rows:Vec<Value>,complexity:Complexity)->HashMap<String,f64>{
    let mut seen=HashSet::new();
    let mut observed=HashMap::<String,Cohort>::new();
    for row in rows{
        if row["bucket"].as_str()!=Some(bucket(complexity)){continue;}
        let Some(provider)=row["providerId"].as_str() else{continue;};
        let Some(model)=row["model"].as_str() else{continue;};
        let Some(task)=row["taskId"].as_str() else{continue;};
        let Some(verdict)=row["verdict"].as_str() else{continue;};
        if !matches!(verdict,"succeeded"|"failed"){continue;}
        if !seen.insert(format!("{provider}\x1f{model}\x1f{task}")){continue;}
        let entry=observed.entry(provider.to_string()).or_default();
        entry.attempts+=1;
        if verdict=="succeeded"{entry.passes+=1;}
    }
    observed.into_iter().filter(|(_,s)|s.attempts>=8)
        .map(|(id,s)|(id,lower_bound(s))).collect()
}

/// Return no new ordering if any eligible provider lacks eight comparable
/// verified tasks. No synthetic outcomes or token-length success surrogates.
pub fn rank_with_verified_evidence(
    state:&AppState,complexity:Complexity,baseline:&[Value]
)->Option<Vec<Value>>{
    if baseline.len()<2{return None;}
    let scores=cohorts(state.store.all("route_observation"),complexity);
    if !baseline.iter().all(|p|p["id"].as_str().is_some_and(|id|scores.contains_key(id))){
        return None;
    }
    let mut indexed=baseline.iter().cloned().enumerate().collect::<Vec<_>>();
    indexed.sort_by(|(a_index,a),(b_index,b)|{
        let sa=scores[a["id"].as_str().unwrap()];
        let sb=scores[b["id"].as_str().unwrap()];
        // Quantization is transitive; small differences keep stable owner
        // capabilities, credentials and original provider precedence.
        let tier=|value:f64|(value*5.0).floor() as i32;
        tier(sb).cmp(&tier(sa)).then_with(||a_index.cmp(b_index))
    });
    Some(indexed.into_iter().map(|(_,p)|p).collect())
}
/// Feed model routing only with independently verified real finalizations.
/// Fallback sessions with multiple distinct provider/model routes are excluded
/// to avoid crediting an initial failing model for a different model's result.
pub fn observe_verified(state:&AppState,execution:&Value,outcome:TaskOutcome){
    if !matches!(outcome,TaskOutcome::Succeeded|TaskOutcome::Failed){return;}
    if !crate::engine::verification::has_result_verifier(&execution["acceptanceChecks"])
        || execution["modelCalls"].as_u64().unwrap_or(0)==0{return;}
    let (Some(session),Some(provider),Some(model),Some(id),Some(task))=(
        execution["sessionId"].as_str(),execution["providerId"].as_str(),
        execution["model"].as_str(),execution["id"].as_str(),
        execution["taskId"].as_str()) else{return;};
    let Ok(routes)=state.store.session_model_routes(session) else{return;};
    if routes.len()!=1 || routes[0]["providerId"]!=provider || routes[0]["model"]!=model{return;}
    let complexity=crate::engine::router::classify(
        execution["request"].as_str().unwrap_or(""),0,1);
    let verdict=if outcome==TaskOutcome::Succeeded{"succeeded"}else{"failed"};
    let _=state.store.put_if_absent("route_observation",&json!({
        "id":format!("route:{id}"),"executionId":id,"taskId":task,
        "providerId":provider,"model":model,
        "bucket":bucket(complexity),"verdict":verdict,
        "evidence":"native_result_verifier_and_unambiguous_session_route"
    }));
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn verified_cohorts_beat_names_only_with_sufficient_independent_trials(){
        let a=json!({"id":"a"});let b=json!({"id":"b"});
        let mut rows=Vec::new();
        for i in 0..12{
            for (id,verdict) in [("a","failed"),("b","succeeded")]{
                rows.push(json!({"id":format!("{id}:{i}"),
                    "providerId":id,"model":id,"taskId":format!("root-{i}"),
                    "bucket":"deep","verdict":verdict}));
            }
        }
        let s=cohorts(rows.clone(),Complexity::Deep);
        assert!(s["b"]>s["a"]);
        assert!(cohorts(rows.clone(),Complexity::Light).is_empty());
        let mut dup=rows;
        for i in 0..25{
            dup.push(json!({"id":format!("dupe-{i}"),"providerId":"a",
                "model":"a","taskId":"root-0","bucket":"deep","verdict":"succeeded"}));
        }
        assert!((cohorts(dup,Complexity::Deep)["a"]-s["a"]).abs()<1e-10);
        assert_eq!(lower_bound(Cohort{attempts:0,passes:0}),0.0);
        assert_eq!(a["id"],"a");assert_eq!(b["id"],"b");
    }
}
