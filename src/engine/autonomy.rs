//! Engine-owned autonomous execution cycle for ordinary durable prompt jobs.
//! Unlike a chat-only loop, the cycle checks real workspace/tool evidence
//! between attempts, replans from unmet checks, and never trusts a model's
//! "done" string as proof of file edits or command execution.
use crate::engine::{
    chat::TurnRequest,
    jobs::JobContext,
    run_quiet_outcome,
    verification,
    decompose,
    TurnStats,
};
use serde_json::{json,Value};

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Strategy { Focused, Conservative }

#[derive(Clone,Copy,Debug)]
pub struct CycleConfig {
    pub attempts: usize,
    pub strategy: Strategy,
}

impl CycleConfig {
    /// User parameters are bounded by the engine; invalid values never turn
    /// into unbounded model calls.
    pub fn from_input(input:&Value)->Self {
        let attempts=input["maxAutonomousAttempts"].as_u64().unwrap_or(3).clamp(1,6) as usize;
        let strategy=if input["executionStrategy"]=="conservative" {
            Strategy::Conservative
        }else{Strategy::Focused};
        Self{attempts,strategy}
    }
}

#[derive(Debug,PartialEq,Eq)]
enum Decision { Accept, Repair, Review }

fn side_effect_unknown(evidence:&[Value])->bool {
    // Workspace edits and read-only operations are retryable after a fresh
    // verifier pass; arbitrary shell/MCP/HTTP writes are not.
    evidence.iter().any(|item|{
        let tool=item["tool"].as_str().unwrap_or("");
        !matches!(tool,
            "workspace_read"|"workspace_read_range"|"workspace_write"|
            "workspace_replace"|"workspace_replace_range"|"workspace_list"|
            "workspace_search"|"code_search"|"file_read"|"file_write"
        ) && !(matches!(tool,"shell_exec"|"terminal_exec")&&{
            let args=item["arguments"]["command"].as_str().unwrap_or("");
            matches!(args.trim_start().split_whitespace().next(),
                Some("cargo"|"npm"|"pytest"|"git"|"rg"|"grep"|"ls"|"pwd"|"cat"))
                && !args.chars().any(|ch|matches!(ch,';'|'&'|'|'|'>'|'<'|'\n'))
                && !["commit","push","add","apply","checkout","merge","rebase","clean","reset"]
                    .iter().any(|command|args.split_whitespace().nth(1)==Some(*command))
        })
    })
}
fn decide(
    strategy:Strategy,
    has_verifiable_checks:bool,
    gaps:&[String],
    error:Option<&str>,
    evidence:&[Value],
    attempt:usize,
    max_attempts:usize,
) -> Decision {
    if gaps.is_empty() && error.is_none(){return Decision::Accept;}
    // Even if a provider fails after the last successful workspace write,
    // a frozen independent verifier can demonstrate the artifact exists.
    if gaps.is_empty() && has_verifiable_checks {return Decision::Accept;}
    if side_effect_unknown(evidence) {return Decision::Review;}
    if strategy==Strategy::Conservative && !evidence.is_empty() && error.is_some(){
        return Decision::Review;
    }
    if attempt+1>=max_attempts {return Decision::Review;}
    Decision::Repair
}
fn narrow(text:&str,max:usize)->String {
    let mut limited=text.chars().take(max).collect::<String>();
    if text.chars().count()>max {limited.push('…');}
    limited
}
/// Stop paying for the same unsuccessful model turn without fresh evidence.
fn stalled(previous:&[String],current:&[String],new_evidence:usize,
    previous_error:Option<&str>,error:Option<&str>)->bool{
    !current.is_empty() && previous==current
        && new_evidence==0 && previous_error==error
}
fn next_instruction(goal:&str,gaps:&[String],error:Option<&str>,round:usize)->String {
    let evidence=gaps.iter().take(6).map(|gap|format!("- {}",narrow(gap,300)))
        .collect::<Vec<_>>().join("\n");
    let error=error.map(|text|narrow(text,400)).unwrap_or_default();
    format!(
        "원래 사용자 작업:\n{goal}\n\n\
         Rust 실행 엔진이 {round}번째 시도 이후 확인한 미완료 항목:\n{evidence}\n\
         이전 실행 오류: {error}\n\n\
         이미 수행한 변경을 되돌리거나 중복 실행하지 마세요. \
         현재 파일/테스트 상태를 필요한 범위만 확인한 뒤 실패한 항목만 수정하고 검증하세요. \
         설명만으로 완료했다고 주장하지 말고 검증 가능한 산출물을 만드세요."
    )
}

/// Called by the existing durable prompt worker. The job stays one operation;
/// all intermediate rounds share its session, cancellation and audit records.
pub async fn execute(
    ctx:&JobContext,
    request:&TurnRequest,
    goal:&str,
    input:&Value,
)->Result<Value,String> {
    let config=CycleConfig::from_input(input);
    let checks=verification::prepare_acceptance_checks(&ctx.state,goal);
    let has_checks=checks.as_array().is_some_and(|v|!v.is_empty());
    let plan=decompose::decompose(goal,&json!({"implementation":"numbered-first"}));
    let session_id=request.session_id.as_deref().unwrap_or("");
    let mut all_evidence=Vec::<Value>::new();
    let mut receipts=Vec::<Value>::new();
    let mut last_gaps=Vec::<String>::new();
    let mut last_error:Option<String>=None;
    let mut last_message=Value::Null;
    ctx.checkpoint(json!({"autonomousState":{
        "status":"running","phase":"planning",
        "plan":plan,"verification":checks,"attempt":0,
    }}))?;
    for attempt in 0..config.attempts {
        // A cancelled or stale job loses authority before the next model call.
        ctx.checkpoint(json!({"autonomousState":{
            "status":"running","phase":"executing","attempt":attempt+1,
            "plan":plan,"verification":checks,
            "receipts":receipts,
        }}))?;
        let mut turn=request.clone();
        if attempt>0 {
            turn.input=next_instruction(goal,&last_gaps,last_error.as_deref(),attempt);
        }
        // Owner steering wins over the prior plan and remains durable in the
        // same job record. Only the next model turn reads a new revision.
        let owner_job=ctx.state.store.get("job",&ctx.job_id)
            .ok_or("autonomous job no longer exists")?;
        let steering_before=owner_job["steeringRevision"].as_u64().unwrap_or(0);
        if let Some(messages)=owner_job["steering"].as_array(){
            let instructions=messages.iter().filter_map(|m|m["instruction"].as_str())
                .map(|s|format!("- {}",narrow(s,1200))).collect::<Vec<_>>();
            if !instructions.is_empty(){
                turn.input.push_str("\n\n사용자가 작업 도중 추가한 최신 지시(우선 반영):\n");
                turn.input.push_str(&instructions.join("\n"));
            }
        }
        let (outcome, stats):(Result<Option<Value>,String>,TurnStats)=
            run_quiet_outcome(&ctx.state,&turn).await;
        let latest_error=outcome.as_ref().err().cloned();
        last_message=outcome.as_ref().ok().and_then(|msg|msg.as_ref())
            .map(|msg|msg["id"].clone()).unwrap_or(Value::Null);
        all_evidence.extend(stats.evidence.iter().cloned());
        let mut gaps=verification::check_acceptance(&ctx.state,goal,&checks,&all_evidence);
        // If the owner steered while a long model turn was executing, never
        // complete based on the previous instruction snapshot.
        let steering_after=ctx.state.store.get("job",&ctx.job_id)
            .and_then(|j|j["steeringRevision"].as_u64()).unwrap_or(0);
        let owner_changed=steering_after>steering_before;
        if owner_changed {
            gaps.push("실행 중 새로운 사용자 지시를 받았습니다. 다음 턴에서 반영합니다.".into());
        }
        let mut decision=decide(config.strategy,has_checks,&gaps,
            latest_error.as_deref(),&all_evidence,attempt,config.attempts);
        let repeated_without_progress=attempt>0
            && stalled(&last_gaps,&gaps,stats.evidence.len(),
                last_error.as_deref(),latest_error.as_deref());
        if repeated_without_progress && !owner_changed {
            decision=Decision::Review;
        }
        if owner_changed {
            // A new owner instruction never overrides the no-replay boundary
            // for a possibly non-idempotent external action.
            decision=if attempt+1<config.attempts
                && !side_effect_unknown(&all_evidence) {
                Decision::Repair
            }else{Decision::Review};
        }
        let receipt=json!({
            "attempt":attempt+1,
            "steeringRevision":steering_before,
            "outcome":match decision {
                Decision::Accept=>"accepted",Decision::Repair=>"repair",Decision::Review=>"needs_review"
            },
            "messageId":last_message,
            "toolSuccess":stats.tool_success,
            "toolErrors":stats.tool_errors,
            "evidenceCount":stats.evidence.len(),
            "gaps":gaps.iter().take(8).map(|g|narrow(g,400)).collect::<Vec<_>>(),
            "error":latest_error.as_deref().map(|e|narrow(e,500)),
        });
        ctx.emit(json!({"type":"autonomous_step","step":receipt}));
        receipts.push(receipt);
        // Persist outcomes before an additional model call; a crash after a
        // file write must not automatically replay that write.
        ctx.checkpoint(json!({"autonomousState":{
            "status":"running","phase":if decision==Decision::Repair{"replanning"}else{"verifying"},
            "attempt":attempt+1,"plan":plan,"verification":checks,
            "receipts":receipts,
        }}))?;
        match decision {
            Decision::Accept => {
                return Ok(json!({
                    "sessionId":session_id,"messageId":last_message,
                    "status":"completed","attempts":attempt+1,
                    "steeringRevision":steering_before,
                    "verification":if has_checks{"predicate"}else if all_evidence.is_empty(){"answer_only"}else{"tool_evidence"},
                    "verified":has_checks && gaps.is_empty(),
                    "receipts":receipts,
                }));
            }
            Decision::Review => {
                return Ok(json!({
                    "status":"needs_review","sessionId":session_id,"messageId":last_message,
                    "reason":if side_effect_unknown(&all_evidence) {
                        "외부 부작용이 발생했을 수 있어 자동 재실행하지 않습니다."
                    }else if repeated_without_progress {
                        "새로운 실행 증거 없이 동일한 검증 실패가 반복되어 재시도를 중단했습니다."
                    }else{"검증되지 않은 항목이 남았습니다. 작업 기록을 확인하세요."},
                    "unmetChecks":gaps,"lastError":latest_error,
                    "attempts":attempt+1,"receipts":receipts,
                }));
            }
            Decision::Repair => {
                last_gaps=gaps;
                last_error=latest_error;
            }
        }
    }
    Err("autonomous execution ended without a terminal decision".into())
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn verifier_beats_model_claims_and_bounds_retries(){
        let gaps=vec!["file missing".to_string()];
        assert_eq!(decide(Strategy::Focused,true,&gaps,None,&[],0,3),Decision::Repair);
        assert_eq!(decide(Strategy::Focused,true,&gaps,None,&[],2,3),Decision::Review);
        assert_eq!(decide(Strategy::Focused,true,&[],Some("provider failed"),&[],0,3),Decision::Accept);
        assert_eq!(decide(Strategy::Focused,false,&[],Some("provider failed"),&[],0,3),Decision::Repair);
    }
    #[test]
    fn unpredictable_remote_writes_are_not_auto_replayed(){
        let evidence=vec![json!({"tool":"github_api","arguments":{"method":"POST"},"result":{"ok":true}})];
        assert!(side_effect_unknown(&evidence));
        assert_eq!(decide(Strategy::Focused,true,&["no file".into()],None,&evidence,0,6),Decision::Review);
    }
    #[test]
    fn replan_keeps_owner_task_and_only_bounded_feedback(){
        let text=next_instruction("수정해","아".repeat(2000).as_str().split('x').map(str::to_string).collect::<Vec<_>>().as_slice(),None,1);
        assert!(text.contains("원래 사용자 작업"));
        assert!(text.contains("수정해"));
        assert!(text.len()<4000);
        assert_eq!(CycleConfig::from_input(&json!({"maxAutonomousAttempts":100})).attempts,6);
    }
    #[test]
    fn identical_failed_attempts_stop_before_wasting_more_tokens(){
        let gaps=vec!["file missing".to_string()];
        assert!(stalled(&gaps,&gaps,0,None,None));
        assert!(!stalled(&gaps,&gaps,1,None,None));
        assert!(!stalled(&gaps,&[],0,None,None));
    }
    #[test]
    fn conservative_strategy_stops_on_partial_write_plus_provider_error(){
        let evidence=vec![json!({"tool":"workspace_write","result":{"ok":true}})];
        assert_eq!(decide(Strategy::Conservative,true,&["missing".into()],Some("network"),&evidence,0,3),Decision::Review);
    }
}
