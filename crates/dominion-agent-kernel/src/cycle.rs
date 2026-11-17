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
    // Bounded evidence from actual attempts; not a second execution loop.
    trials: Vec<Value>,
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
        Self{config,next:0,previous_gaps:Vec::new(),previous_error:None,trials:Vec::new()}
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
        // Evidence from a superseded goal cannot rank strategies for a new one.
        self.trials.clear();
    }

    pub fn retry_instruction(&self,goal:&str,attempt:usize)->String{
        let mut prompt=next_instruction(goal,&self.previous_gaps,
            self.previous_error.as_deref(),attempt);
        let class=crate::failure::classify(
            &self.previous_gaps,self.previous_error.as_deref());
        prompt.push_str("\n\nRust 하네스의 실패 분류별 복구 지시: ");
        prompt.push_str(class.next_step());
        let approach=self.approach_plan();
        let instruction=match approach["selected"].as_str().unwrap_or("direct"){
            "diagnose"=>"먼저 필요한 읽기·테스트 증거만 모아 원인을 구분하세요. 확인한 원인에 맞는 최소 수정만 실행하고 검증하세요.",
            "decompose"=>"실패한 요구를 독립적으로 확인 가능한 작은 단계로 분해하세요. 새 부작용 없이 한 단계씩 실행·검증하고 완료된 단계는 반복하지 마세요.",
            _=>"실패한 검증 조건만 겨냥해 직접 수정·검증하세요. 기존의 성공한 단계와 외부 효과를 중복 실행하지 마세요.",
        };
        prompt.push_str("\n\n실제 검증 결과를 비교한 이번 실행 접근: ");
        prompt.push_str(approach["selected"].as_str().unwrap_or("direct"));
        prompt.push_str(". ");
        prompt.push_str(instruction);
        prompt
    }


    /// Compare bounded alternatives using receipts from THIS owner task.
    /// Candidate ratings are deterministic heuristics, not model judgments or
    /// proof that different approaches were tried under identical conditions.
    /// These attempts use the already-authorized turn/tool budget.
    pub fn approach_plan(&self)->Value{
        use crate::failure::{classify,FailureClass};
        let failure=classify(&self.previous_gaps,self.previous_error.as_deref());
        let mut candidates=Vec::new();
        for approach in ["direct","diagnose","decompose"] {
            let mut score:i64=match (failure,approach) {
                (FailureClass::Test|FailureClass::Compile|FailureClass::MissingEvidence,"diagnose")=>6,
                (FailureClass::Permission|FailureClass::Provider|FailureClass::Other,"diagnose")=>4,
                (FailureClass::Verify,"direct")=>6,
                (_,"direct")=>2,
                _=>0,
            };
            if self.previous_gaps.len()>=3 && approach=="decompose" {score+=5;}
            let mut used=false;
            for trial in &self.trials {
                if trial["approach"].as_str()!=Some(approach){continue;}
                used=true;
                score+=trial["score"].as_i64().unwrap_or(0).clamp(-18,18)/3;
                score-=2;
            }
            if !used {score+=2;}
            if let Some(last)=self.trials.last(){
                if last["approach"].as_str()==Some(approach){
                    let closed=last["closed"].as_u64().unwrap_or(0);
                    let fresh=last["newEvidence"].as_u64().unwrap_or(0);
                    if closed>0 {score+=6;}
                    else if fresh>0 {score+=3;}
                    else {score-=8;}
                    if last["error"].as_bool()==Some(true){score-=3;}
                }
            }
            candidates.push(json!({"approach":approach,"score":score,"tried":used}));
        }
        let selected=if self.trials.is_empty() {"direct"}else{
            candidates.iter().max_by_key(|c|c["score"].as_i64().unwrap_or(i64::MIN))
                .and_then(|c|c["approach"].as_str()).unwrap_or("direct")
        };
        json!({"selected":selected,"candidates":candidates,
            "failureClass":failure.label(),"observedTrials":self.trials.len()})
    }

    /// One observed provider turn is one trial; it never schedules extra turns.
    /// The verifier's before/after gaps and genuinely novel successful tool
    /// evidence outrank model prose. Tokens are the measured delta, not an
    /// estimate. A negative result remains visible for later comparison.
    #[allow(clippy::too_many_arguments)]
    pub fn record_trial(&mut self,plan:&Value,before:&[String],after:&[String],
        new_evidence:usize,tokens:u64,error:Option<&str>,accepted:bool)->Value{
        let closed=before.iter().filter(|gap|!after.contains(gap)).count();
        let opened=after.iter().filter(|gap|!before.contains(gap)).count();
        let score=if accepted {100_i64}else{
            (closed.min(20) as i64)*16
                +(new_evidence.min(4) as i64)*4
                -(opened.min(20) as i64)*12
                -(tokens/2_000).min(12) as i64
                -if error.is_some(){8}else{0}
        };
        let trial=json!({"approach":plan["selected"],"closed":closed,
            "opened":opened,"gapsBefore":before.len(),"gapsAfter":after.len(),
            "newEvidence":new_evidence,"tokens":tokens,"error":error.is_some(),
            "accepted":accepted,"score":score,
            "candidateScores":plan["candidates"]});
        self.trials.push(trial.clone());
        if self.trials.len()>6{self.trials.remove(0);}
        trial
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
            "strategyTrials":self.trials,
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
    fn retry_prompt_targets_failed_test_instead_of_repeating_generic_actions(){
        let mut c=policy(3);
        c.next_attempt();
        c.record_retry(&["test failed: expected value".into()],None);
        let prompt=c.retry_instruction("Repair src/lib.rs",1);
        assert!(prompt.contains("실패한 테스트"));
        assert!(prompt.contains("Repair src/lib.rs"));
        assert!(!prompt.contains("컴파일 오류입니다"));
    }

    #[test]
    fn real_trial_evidence_changes_approach_without_another_execution_loop(){
        let mut c=policy(4);
        assert_eq!(c.approach_plan()["selected"],"direct");
        c.next_attempt();
        let failed=vec!["test failed: expected 4".to_string()];
        let initial=c.approach_plan();
        let first=c.record_trial(&initial,&failed,&failed,0,3000,None,false);
        assert_eq!(first["score"],-1);
        c.record_retry(&failed,None);
        let diagnostic=c.approach_plan();
        assert_eq!(diagnostic["selected"],"diagnose");
        assert_eq!(diagnostic["observedTrials"],1);
        let fixed:Vec<String>=Vec::new();
        let better=c.record_trial(&diagnostic,&failed,&fixed,2,1200,None,false);
        assert_eq!(better["closed"],1);
        assert_eq!(c.approach_plan()["selected"],"diagnose",
            "use observed progress rather than a fixed rotating strategy");
        assert_eq!(c.snapshot()["strategyTrials"].as_array().unwrap().len(),2);
        assert_eq!(c.snapshot()["remaining"],3,
            "strategy ranking cannot consume additional attempts");
    }
    #[test]
    fn superseded_goal_clears_strategy_credit_but_not_attempt_budget(){
        let mut c=policy(2);
        c.next_attempt();
        let missing=vec!["missing file".into()];
        let plan=c.approach_plan();
        c.record_trial(&plan,&missing,&missing,0,1000,Some("not found"),false);
        c.record_retry(&missing,Some("not found"));
        assert_eq!(c.approach_plan()["selected"],"diagnose");
        c.replaced_goal();
        assert_eq!(c.approach_plan()["selected"],"direct");
        assert_eq!(c.snapshot()["attempted"],1);
        assert_eq!(c.snapshot()["strategyTrials"].as_array().unwrap().len(),0);
    }
    #[test]
    fn fresh_verification_gaps_and_measured_cost_determine_trial_score(){
        let mut c=policy(2);
        let before=vec!["missing file".into(),"test failed".into()];
        let after=vec!["test failed".into(),"new regression".into()];
        let plan=c.approach_plan();
        let t=c.record_trial(&plan,&before,&after,1,4000,Some("failed"),false);
        assert_eq!(t["closed"],1);
        assert_eq!(t["opened"],1);
        assert_eq!(t["score"],-2);
        assert_eq!(t["tokens"],4000);
        assert!(t["candidateScores"].is_array());
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
