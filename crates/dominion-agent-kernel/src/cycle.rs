//! Independent, bounded lifecycle controller for real owner-directed prompt jobs.
//! Product adapters supply model turns, durable receipts and verifiers. The
//! kernel owns retry admission, evidence-stall policy, no-replay, owner-change
//! handling and termination boundaries. No providers or filesystem in here.
use crate::{decide, next_instruction, side_effect_unknown, stalled, CycleConfig, Decision};
use serde_json::{json, Value};

#[derive(Debug)]
pub struct Cycle {
    config: CycleConfig,
    next: usize,
    previous_gaps: Vec<String>,
    previous_error: Option<String>,
}

#[derive(Debug)]
pub struct Observation<'a> {
    pub has_checks: bool,
    pub gaps: &'a [String],
    pub error: Option<&'a str>,
    pub accumulated_evidence: &'a [Value],
    pub new_evidence: usize,
    pub owner_changed: bool,
    pub token_budget_exhausted: bool,
}

#[derive(Debug,PartialEq,Eq)]
pub struct Assessment {
    pub decision: Decision,
    pub stalled: bool,
    pub unknown_effect: bool,
}

impl Cycle {
    pub fn new(config:CycleConfig)->Self {
        Self{config,next:0,previous_gaps:Vec::new(),previous_error:None}
    }

    /// Only this kernel can admit another model attempt.
    pub fn next_attempt(&mut self)->Option<usize>{
        if self.next>=self.config.attempts{return None;}
        let index=self.next;
        self.next+=1;
        Some(index)
    }

    pub fn can_retry(&self)->bool {
        self.next<self.config.attempts
    }

    /// An owner's replacement resets *retry feedback*, not previous
    /// irreversible tool effects or the exhausted attempt budget.
    pub fn replaced_goal(&mut self){
        self.previous_gaps.clear();
        self.previous_error=None;
    }

    pub fn retry_instruction(&self,goal:&str,attempt:usize)->String{
        next_instruction(goal,&self.previous_gaps,self.previous_error.as_deref(),attempt)
    }

    pub fn assess(&self, observation:&Observation<'_>)->Assessment{
        let attempt=self.next.saturating_sub(1);
        let unknown_effect=side_effect_unknown(observation.accumulated_evidence);
        let stalled=self.next>1 &&
            stalled(&self.previous_gaps,observation.gaps,
                observation.new_evidence,self.previous_error.as_deref(),observation.error);
        let mut decision=decide(self.config.strategy,observation.has_checks,
            observation.gaps,observation.error,observation.accumulated_evidence,
            attempt,self.config.attempts);
        if stalled && !observation.owner_changed {
            decision=Decision::Review;
        }
        if observation.owner_changed {
            // The owner's new instructions never give permission to replay
            // a possibly non-idempotent effect.
            decision=if self.can_retry() && !unknown_effect{
                Decision::Repair
            }else{Decision::Review};
        }
        // This terminal budget gate wins even when new owner steering
        // arrives during the provider call.
        if observation.token_budget_exhausted {
            decision=Decision::Review;
        }
        Assessment{decision,stalled,unknown_effect}
    }

    /// The product adapter may downgrade a tentative terminal decision to
    /// a retry if its optimistic owner-steering CAS loses the race. Only
    /// committed repair decisions seed the next model prompt.
    pub fn record_retry(&mut self,gaps:&[String],error:Option<&str>){
        self.previous_gaps=gaps.to_vec();
        self.previous_error=error.map(str::to_owned);
    }

    /// Persisted in the host's existing autonomousState checkpoint so
    /// a crash never silently advances a cleared in-memory retry budget.
    pub fn snapshot(&self)->Value{
        json!({
            "version":1,"attempted":self.next,"maxAttempts":self.config.attempts,
            "remaining":self.config.attempts.saturating_sub(self.next),
            "pendingGapCount":self.previous_gaps.len(),
            "hasPreviousError":self.previous_error.is_some(),
        })
    }
}

#[cfg(test)]
mod tests{
    use super::*;
    use crate::Strategy;
    fn policy(attempts:usize)->Cycle {
        Cycle::new(CycleConfig{attempts,strategy:Strategy::Focused})
    }
    fn observation<'a>(gaps:&'a [String],evidence:&'a [Value])->Observation<'a>{
        Observation{has_checks:true,gaps,error:None,
            accumulated_evidence:evidence,new_evidence:0,
            owner_changed:false,token_budget_exhausted:false}
    }
    #[test]
    fn sdk_cycle_is_actual_bounded_attempt_owner(){
        let mut cycle=policy(2);
        assert_eq!(cycle.next_attempt(),Some(0));
        assert_eq!(cycle.next_attempt(),Some(1));
        assert_eq!(cycle.next_attempt(),None);
        assert!(!cycle.can_retry());
        assert_eq!(cycle.snapshot()["remaining"],0);
    }
    #[test]
    fn repeated_failure_stops_until_new_real_evidence(){
        let mut cycle=policy(4);
        assert_eq!(cycle.next_attempt(),Some(0));
        let gaps=vec!["missing file".to_string()];
        assert_eq!(cycle.assess(&observation(&gaps,&[])).decision,Decision::Repair);
        cycle.record_retry(&gaps,Some("same error"));
        assert_eq!(cycle.next_attempt(),Some(1));
        let o=Observation{error:Some("same error"),..observation(&gaps,&[])};
        let verdict=cycle.assess(&o);
        assert!(verdict.stalled);
        assert_eq!(verdict.decision,Decision::Review);
        let o=Observation{new_evidence:1,..o};
        assert_eq!(cycle.assess(&o).decision,Decision::Repair);
    }
    #[test]
    fn changed_owner_goal_never_replays_unknown_remote_effects(){
        let mut cycle=policy(3);
        assert_eq!(cycle.next_attempt(),Some(0));
        let gaps=vec!["need verification".to_string()];
        let remote=vec![json!({"tool":"github_api",
            "arguments":{"method":"POST"},"result":{"ok":true}})];
        let o=Observation{owner_changed:true,..observation(&gaps,&remote)};
        let verdict=cycle.assess(&o);
        assert!(verdict.unknown_effect);
        assert_eq!(verdict.decision,Decision::Review);
        let safe=Observation{owner_changed:true,..observation(&gaps,&[])};
        assert_eq!(cycle.assess(&safe).decision,Decision::Repair);
        cycle.record_retry(&gaps,None);
        cycle.replaced_goal();
        assert_eq!(cycle.snapshot()["pendingGapCount"],0);
        assert_eq!(cycle.snapshot()["attempted"],1);
    }
    #[test]
    fn token_cap_and_exhausted_retries_cannot_start_more_work(){
        let mut cycle=policy(1);
        cycle.next_attempt();
        let gaps=vec!["failed".to_string()];
        let cap=Observation{token_budget_exhausted:true,..observation(&gaps,&[])};
        assert_eq!(cycle.assess(&cap).decision,Decision::Review);
        let changed_owner=Observation{owner_changed:true,..cap};
        assert_eq!(cycle.assess(&changed_owner).decision,Decision::Review,
            "owner steering may not restart a fully spent token budget");
        assert!(!cycle.can_retry());
        assert_eq!(cycle.next_attempt(),None);
    }
}
