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
    // A fixed Git HEAD and model are not enough: different frozen case
    // plans or target domains cannot establish a matched controller contest.
    values.push(row["domain"].as_str().unwrap_or("unlabelled-domain"));
    values.push(row["evaluationProfileSha256"].as_str().unwrap_or("no-profile"));
    values.push(row["evaluationPlanSha256"].as_str()?);
    values.push(row["evaluationOracleVersion"].as_str()?);
    Some(json!(values).to_string())
}
fn comparable(row:&Value)->bool {
    row["generatorCommit"].as_str().is_some_and(|s| s.len()==40 && s.bytes().all(|b|b.is_ascii_hexdigit()))
        && row["jobId"].as_str().is_some_and(|s|!s.is_empty())
        && row["tokens"].as_u64().is_some_and(|v|v>0)
        && row["editPolicy"]["revision"].as_str().is_some_and(|s|!s.is_empty())
        && row["candidateId"].as_str().is_some_and(|s|!s.is_empty())
        && row["evaluationPlanSha256"].as_str().is_some_and(|s|
            s.len()==64 && s.bytes().all(|b|b.is_ascii_hexdigit()))
        && row["evaluationOracleVersion"].as_str().is_some_and(|s|!s.is_empty())
        && row["evaluation"]["source"]=="server_invoked_local_paired_git_worktrees"
        && row["evaluation"]["metrics"].is_object()
}

/// Compare policies only across independently measured generator revisions.
/// Repeated runs of one policy/source are conservatively scored at their
/// *lowest* payoff, rather than rewarding a cherry-picked lucky retry.
/// Reusing a job or candidate in different contexts invalidates both contexts;
/// changing the task label cannot manufacture a second independent trial.
/// No policy tournament can directly adopt a source patch.
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
            "generatorCommit":if commit=="sha1" {"a".repeat(40)} else if commit=="sha2" {"b".repeat(40)} else {commit.to_string()},
            "generatorModel":model,
            "generatorProviderId":"p","objectiveId":"goal",
            "knowledgePartition":"space","domain":"memory",
            "evaluationPlanSha256":"e".repeat(64),"evaluationOracleVersion":"frozen-v1",
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
    #[test]
    fn same_source_with_new_objective_is_not_a_second_independent_win(){
        let mut cases=vec![
            trial("a","base","sha1","m",false),
            trial("b","child","sha1","m",true),
            trial("c","base","sha1","m",false),
            trial("d","child","sha1","m",true),
        ];
        cases[2]["objectiveId"]=json!("different-goal");
        cases[3]["objectiveId"]=json!("different-goal");
        let refs=cases.iter().collect::<Vec<_>>();
        let outcome=compare(&refs,reward);
        assert_eq!(outcome["promotion"],false);
        let comparison=&outcome["comparisons"][0];
        assert_eq!(comparison["matchedContexts"],1);
    }

    #[test]
    fn retries_cannot_cherry_pick_success_and_order_does_not_matter(){
        let mut cases=vec![
            trial("a","base","sha1","m",false),
            trial("b","child","sha1","m",true),
            trial("b2","child","sha1","m",false),
            trial("c","base","sha2","m",false),
            trial("d","child","sha2","m",true),
        ];
        let refs=cases.iter().collect::<Vec<_>>();
        assert_eq!(compare(&refs,reward)["promotion"],false);
        cases.reverse();
        let refs=cases.iter().collect::<Vec<_>>();
        assert_eq!(compare(&refs,reward)["promotion"],false);
    }

    #[test]
    fn shared_jobs_or_candidates_cannot_create_independent_evidence(){
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
    fn non_finite_rewards_are_not_competition_evidence(){
        let cases=vec![
            trial("a","base","sha1","m",false),
            trial("b","child","sha1","m",true),
            trial("c","base","sha2","m",false),
            trial("d","child","sha2","m",true),
        ];
        let refs=cases.iter().collect::<Vec<_>>();
        assert_eq!(compare(&refs,|v| if v["accepted"]==true {f64::NAN} else {0.0})["promotion"],false);
    }

    #[test]
    fn different_frozen_oracle_or_domain_is_not_matched_evidence(){
        let mut rows=vec![
            trial("a","base","sha1","m",false),
            trial("b","child","sha1","m",true),
            trial("c","base","sha2","m",false),
            trial("d","child","sha2","m",true),
        ];
        assert_eq!(compare(&rows.iter().collect::<Vec<_>>(),reward)["promotion"],true);
        rows[3]["evaluationPlanSha256"]=json!("f".repeat(64));
        assert_eq!(compare(&rows.iter().collect::<Vec<_>>(),reward)["promotion"],false);
        rows[3]["evaluationPlanSha256"]=json!("e".repeat(64));
        rows[3]["domain"]=json!("context");
        assert_eq!(compare(&rows.iter().collect::<Vec<_>>(),reward)["promotion"],false);
        rows[3]["domain"]=json!("memory");
        rows[3]["evaluationOracleVersion"]=json!("different-oracle");
        assert_eq!(compare(&rows.iter().collect::<Vec<_>>(),reward)["promotion"],false);
    }

}
