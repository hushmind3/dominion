//! Matched, independently measured policy competition for RSI.
//! A source patch only supplies a reward after the existing paired evaluator
//! validates it; competing controllers are matched on exact source and model.
use serde_json::{json,Value};
use std::collections::{BTreeMap, BTreeSet};

fn scope(row:&Value)->Option<String>{
    let mut values=Vec::new();
    for name in ["generatorCommit","generatorModel","generatorProviderId",
        "objectiveId","knowledgePartition"] {
        values.push(row.get(name)?.as_str().filter(|s|!s.is_empty())?);
    }
    Some(json!(values).to_string())
}
fn comparable(row:&Value)->bool {
    row["jobId"].as_str().is_some_and(|s|!s.is_empty())
        && row["tokens"].as_u64().is_some_and(|v|v>0)
        && row["editPolicy"]["revision"].as_str().is_some_and(|s|!s.is_empty())
        && row["candidateId"].as_str().is_some_and(|s|!s.is_empty())
        && row["evaluation"]["source"]=="server_invoked_local_paired_git_worktrees"
        && row["evaluation"]["metrics"].is_object()
}

/// Each comparison context contributes at most one evaluation per revision;
/// retries of one policy cannot be cherry-picked into superiority evidence.
/// Two independent matching contexts must favor the challenger without a
/// contrary pair. This only promotes methods: no source adoption bypass.
pub fn compare(rows:&[&Value],reward:fn(&Value)->f64)->Value {
    type Observation = (String,String,f64); // job, candidate, measured reward
    let mut cohorts=BTreeMap::<String,BTreeMap<String,Vec<Observation>>>::new();
    let mut sources=BTreeMap::<String,String>::new();
    let mut job_contexts=BTreeMap::<String,BTreeSet<String>>::new();
    let mut candidate_contexts=BTreeMap::<String,BTreeSet<String>>::new();
    for row in rows {
        if !comparable(row) {continue;}
        let Some(context)=scope(row) else {continue;};
        let measured=reward(row);
        if !measured.is_finite() {continue;}
        let revision=row["editPolicy"]["revision"].as_str().unwrap();
        let job=row["jobId"].as_str().unwrap().to_string();
        let candidate=row["candidateId"].as_str().unwrap().to_string();
        sources.insert(context.clone(),row["generatorCommit"].as_str().unwrap().to_string());
        job_contexts.entry(job.clone()).or_default().insert(context.clone());
        candidate_contexts.entry(candidate.clone()).or_default().insert(context.clone());
        cohorts.entry(context).or_default().entry(revision.to_string())
            .or_default().push((job,candidate,measured.clamp(0.0,1.0)));
    }

    // A single source commit must not produce two independent wins by changing
    // the objective/partition label. Conflicting outcomes on the same source
    // become a tie, rather than whichever row was visited first.
    let mut by_source=BTreeMap::<(String,String),BTreeMap<String,u8>>::new();
    for (context,cohort) in &cohorts {
        let reused=cohort.values().flatten().any(|(job,candidate,_)|
            job_contexts.get(job).is_some_and(|contexts|contexts.len()>1)
            || candidate_contexts.get(candidate).is_some_and(|contexts|contexts.len()>1));
        if reused {continue;}
        let source=&sources[context];
        let policies=cohort.iter().collect::<Vec<_>>();
        for (index,(left,left_rows)) in policies.iter().enumerate() {
            for (right,right_rows) in policies.iter().skip(index+1) {
                // Two revisions cannot be a matched experiment if they cite
                // the same worker job or candidate artifact.
                if left_rows.iter().any(|(job,candidate,_)|
                    right_rows.iter().any(|(other_job,other_candidate,_)|
                        job==other_job || candidate==other_candidate)) {continue;}
                let lr=left_rows.iter().map(|row|row.2).fold(f64::INFINITY,f64::min);
                let rr=right_rows.iter().map(|row|row.2).fold(f64::INFINITY,f64::min);
                let outcome=if lr-rr>0.25 {1} else if rr-lr>0.25 {2} else {0};
                let prior=by_source.entry((left.to_string(),right.to_string()))
                    .or_default().entry(source.to_string()).or_insert(outcome);
                if *prior!=outcome {*prior=0;}
            }
        }
    }

    let mut selected=None::<(String,String,u64)>;
    let mut comparisons=Vec::new();
    for ((a,b),sources) in by_source {
        let wins_a=sources.values().filter(|&&outcome|outcome==1).count() as u64;
        let wins_b=sources.values().filter(|&&outcome|outcome==2).count() as u64;
        let ties=sources.values().filter(|&&outcome|outcome==0).count() as u64;
        let champion=if wins_a>=2 && wins_b==0 {
            Some((a.clone(),b.clone(),wins_a))
        }else if wins_b>=2 && wins_a==0 {
            Some((b.clone(),a.clone(),wins_b))
        }else{None};
        comparisons.push(json!({"a":a,"b":b,
            "aWins":wins_a,"bWins":wins_b,"ties":ties,
            "matchedContexts":wins_a+wins_b+ties}));
        if let Some(next)=champion {
            if selected.as_ref().is_none_or(|old|next.2>old.2) {
                selected=Some(next);
            }
        }
    }
    let Some((winner,control,wins))=selected else {
        return json!({"status":"insufficient_matched_evidence",
            "promotion":false,"comparisons":comparisons});
    };
    json!({"status":"promote","promotion":true,
        "winnerRevision":winner,"controlRevision":control,
        "matchedWins":wins,"comparisons":comparisons})
}

#[cfg(test)]
mod tests {
    use super::*;
    fn reward(v:&Value)->f64{
        if v["accepted"]==true && v["evidenceVerified"]==true {1.0}
        else if v["candidateId"].is_string() {0.1}else{0.0}
    }
    fn trial(job:&str,rev:&str,commit:&str,model:&str,accepted:bool)->Value{
        json!({"jobId":job,"tokens":2300,"candidateId":job,
            "accepted":accepted,"evidenceVerified":accepted,
            "generatorCommit":commit,"generatorModel":model,
            "generatorProviderId":"p","objectiveId":"goal",
            "knowledgePartition":"space",
            "editPolicy":{"revision":rev},
            "evaluation":{"source":"server_invoked_local_paired_git_worktrees",
                "metrics":{"eligible":accepted}}})
    }
    #[test]
    fn two_matched_independent_wins_promote_but_one_cannot(){
        let cases=[
            trial("a","base","sha1","m",false),
            trial("b","child","sha1","m",true),
            trial("c","base","sha2","m",false),
            trial("d","child","sha2","m",true),
        ];
        let refs=cases.iter().collect::<Vec<_>>();
        assert_eq!(compare(&refs[..2],reward)["promotion"],false);
        let result=compare(&refs,reward);
        assert_eq!(result["winnerRevision"],"child");
        assert_eq!(result["matchedWins"],2);
    }
    #[test]
    fn retried_policy_cannot_cherry_pick_a_win_from_row_order(){
        let mut cases=vec![
            trial("a","base","sha1","m",false),
            trial("b","child","sha1","m",true),
            trial("retry","child","sha1","m",false),
            trial("c","base","sha2","m",false),
            trial("d","child","sha2","m",true),
        ];
        let refs=cases.iter().collect::<Vec<_>>();
        let result=compare(&refs,reward);
        assert_eq!(result["promotion"],false);
        assert_eq!(result["comparisons"][0]["matchedContexts"],2);
        cases.reverse();
        let reversed=cases.iter().collect::<Vec<_>>();
        assert_eq!(compare(&reversed,reward),result);
    }

    #[test]
    fn one_commit_with_many_objective_labels_is_only_one_independent_context(){
        let mut cases=vec![
            trial("a","base","sha1","m",false),
            trial("b","child","sha1","m",true),
            trial("c","base","sha1","m",false),
            trial("d","child","sha1","m",true),
        ];
        cases[2]["objectiveId"]=json!("another-goal");
        cases[3]["objectiveId"]=json!("another-goal");
        let refs=cases.iter().collect::<Vec<_>>();
        let result=compare(&refs,reward);
        assert_eq!(result["promotion"],false);
        assert_eq!(result["comparisons"][0]["matchedContexts"],1);
    }

    #[test]
    fn reused_candidate_or_job_cannot_fake_a_second_trial(){
        let mut cases=vec![
            trial("a","base","sha1","m",false),
            trial("b","child","sha1","m",true),
            trial("c","base","sha2","m",false),
            trial("d","child","sha2","m",true),
        ];
        assert_eq!(compare(&cases.iter().collect::<Vec<_>>(),reward)["promotion"],true);
        cases[2]["jobId"]=json!("a");
        assert_eq!(compare(&cases.iter().collect::<Vec<_>>(),reward)["promotion"],false);
        cases[2]["jobId"]=json!("c");
        cases[3]["candidateId"]=json!("b");
        assert_eq!(compare(&cases.iter().collect::<Vec<_>>(),reward)["promotion"],false);
    }

    #[test]
    fn unmatched_model_and_contradictory_results_block_promotion(){
        let mut cases=[
            trial("a","base","sha1","m",false),
            trial("b","child","sha1","other",true),
            trial("c","base","sha2","m",true),
            trial("d","child","sha2","m",false),
        ];
        let refs=cases.iter().collect::<Vec<_>>();
        assert_eq!(compare(&refs,reward)["promotion"],false);
        cases[1]["generatorModel"]=json!("m");
        let refs=cases.iter().collect::<Vec<_>>();
        assert_eq!(compare(&refs,reward)["promotion"],false);
        cases[2]["evaluation"]["source"]=json!("model_story");
        let refs=cases.iter().collect::<Vec<_>>();
        assert_eq!(compare(&refs,reward)["promotion"],false);
    }
}
