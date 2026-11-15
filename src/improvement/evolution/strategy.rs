//! Weight-free meta-RSI: learn which source-edit method to try next from
//! immutable, scored attempts, without changing the owner's evaluator.
use super::CandidateRecord;
use sha2::{Digest,Sha256};

const METHODS:[(&str,&str);4]=[
    ("minimal","Preserve all interfaces; prefer a small safe algorithmic improvement."),
    ("alternative","Try a substantively different implementation with equivalent contracts."),
    ("simplify","Reduce complexity, allocations and avoidable branches without behavior loss."),
    ("algorithm","Explore an alternate algorithm while preserving tests and contracts.")
];
pub(super) fn hint(id:Option<&str>,index:usize)->&'static str{
    id.and_then(|name|METHODS.iter().find(|(key,_)|*key==name))
      .map(|(_,hint)|*hint).unwrap_or(METHODS[index%METHODS.len()].1)
}
fn digest(parts:&[&str])->String{
    let mut hasher=Sha256::new();
    for item in parts{
        hasher.update((item.len() as u64).to_be_bytes());
        hasher.update(item.as_bytes());
    }
    format!("{:x}",hasher.finalize())
}
/// Exact evaluator scope plus related-file scope. Different scoring contracts
/// cannot poison each other's learned selection policy.
pub(super) fn scopes(repo:&str,goal:&str,tests:&str,score:&str,holdout:&str,file:&str)
    ->(String,String){
    let family=digest(&[repo,goal,tests,score,holdout]);
    let specific=digest(&[&family,file]);
    (specific,family)
}
#[derive(Clone,Debug)]
pub(super) struct Choice{
    pub id:&'static str,
    pub feedback:String,
    priority:f64
}
/// Optimistic deterministic bandit: try unobserved methods first, then
/// balance fresh exploration and normalized independent score improvements.
pub(super) fn select(history:&[CandidateRecord],scope:&str,family:&str,count:usize)
    ->Vec<Choice>{
    let mut attempts=[0.0_f64;4];
    let mut wins=[0.0_f64;4];
    let mut rewards=[0.0_f64;4];
    let mut costs=[0.0_f64;4];
    for item in history {
        let Some(i)=METHODS.iter().position(|(name,_)|
            item.strategy_id.as_deref()==Some(*name))else{continue};
        let weight=if item.scope_fingerprint.as_deref()==Some(scope){1.0}
            else if item.family_fingerprint.as_deref()==Some(family){0.25}
            else{0.0};
        if weight==0.0{continue;}
        attempts[i]+=weight;
        costs[i]+=weight*item.model_cost_krw.max(0.0).min(1_000_000.0);
        if !item.eligible{continue;}
        let gain=match (item.validation_baseline_score,item.validation_score) {
            (Some(a),Some(b)) if a.is_finite()&&b.is_finite()&&b>a=>
                Some((a,b,1.0)),
            _=>match(item.training_baseline_score,item.training_score){
                (Some(a),Some(b)) if a.is_finite()&&b.is_finite()&&b>a=>
                    Some((a,b,0.55)),
                _=>None
            }
        };
        if let Some((a,b,reliability))=gain{
            wins[i]+=weight;
            rewards[i]+=weight*reliability*
                (0.65+0.35*((b-a)/(1.0+a.abs())).clamp(0.0,1.0));
        }
    }
    let total:f64=attempts.iter().sum();
    let mut choices=METHODS.iter().enumerate().map(|(i,(id,_))|{
        let n=attempts[i];
        let avg=if n>0.0{rewards[i]/n}else{0.0};
        let exploration=if n==0.0{2.0}
            else{0.6*((total+1.0).ln()/(n+1.0)).sqrt()};
        let cost_penalty=if n>0.0{0.04*((costs[i]/n)/1000.0).min(1.0)}
            else{0.0};
        Choice{id:*id,priority:avg+exploration-cost_penalty,
            feedback:format!(
                "Verified scoped history: {:.2} weighted attempts, {:.2} measured gains, bounded reward {:.3}. Keep testing alternatives when sparse.",
                n,wins[i],avg)}
    }).collect::<Vec<_>>();
    choices.sort_by(|a,b|b.priority.total_cmp(&a.priority)
       .then_with(||METHODS.iter().position(|(id,_)|*id==a.id).cmp(
           &METHODS.iter().position(|(id,_)|*id==b.id))));
    choices.truncate(count.clamp(1,4));
    choices
}
#[cfg(test)]
mod tests{
    use super::*;
    use uuid::Uuid;
    fn observed(id:&str,scope:&str,family:&str,pass:bool,delta:f64)->CandidateRecord{
        CandidateRecord{trial_id:Uuid::new_v4(),generation:0,proposal_index:0,
            strategy_id:Some(id.into()),scope_fingerprint:Some(scope.into()),
            family_fingerprint:Some(family.into()),
            training_baseline_score:Some(0.0),training_score:Some(delta),
            validation_baseline_score:Some(0.0),validation_score:Some(delta),
            model_cost_krw:2.0,eligible:pass,reason:None}
    }
    #[test]
    fn explores_methods_before_exploiting_verified_strategy(){
        let initial=select(&[],"S","F",2);
        assert_eq!(initial.iter().map(|x|x.id).collect::<Vec<_>>(),
            vec!["minimal","alternative"]);
        let mut results=vec![observed("minimal","S","F",true,0.7),
            observed("alternative","S","F",false,0.0)];
        assert_eq!(select(&results,"S","F",2).iter().map(|x|x.id).collect::<Vec<_>>(),
            vec!["simplify","algorithm"]);
        results.push(observed("simplify","S","F",false,0.0));
        results.push(observed("algorithm","S","F",false,0.0));
        assert_eq!(select(&results,"S","F",1)[0].id,"minimal");
        assert!(select(&results,"S","F",1)[0].feedback.contains("measured gains"));
    }
    #[test]
    fn isolates_incompatible_evaluators_and_allows_weak_cross_file_transfer(){
        let a=scopes("r","g","t","score1","v","a.rs");
        let b=scopes("r","g","t","score2","v","a.rs");
        let c=scopes("r","g","t","score1","v","b.rs");
        assert_ne!(a.1,b.1);assert_eq!(a.1,c.1);assert_ne!(a.0,c.0);
        let wrong=vec![observed("alternative",&b.0,&b.1,true,99.0)];
        assert_eq!(select(&wrong,&a.0,&a.1,1)[0].id,"minimal");
        assert_eq!(hint(Some("algorithm"),0),METHODS[3].1);
    }
}