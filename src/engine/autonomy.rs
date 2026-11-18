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
use crate::engine::agent_kernel::{
    CycleConfig, Cycle, Observation, Decision, side_effect_unknown,
    usage_delta, owner_intent, narrow,
};
#[cfg(test)]
use crate::engine::agent_kernel::{Strategy,read_only_shell_command,decide,stalled,next_instruction};
use serde_json::{json,Value};

/// The explicit UI command is a frozen acceptance criterion. Only a real
/// successful command invocation (captured by the Rust tool runtime) satisfies
/// it; prose or an unexecuted suggestion cannot make the job pass.
fn add_explicit_verifier(checks: &mut Value, command: Option<&str>) -> Result<(), String> {
    let Some(command) = command.map(str::trim).filter(|command| !command.is_empty()) else {
        return Ok(());
    };
    if command.len() > 2048 {
        return Err("검증 명령은 최대 2048자입니다.".into());
    }
    let items = checks.as_array_mut().ok_or("검증 조건이 배열이 아닙니다.")?;
    if items.len() >= 20 {
        return Err("검증 조건은 최대 20개입니다.".into());
    }
    if !items.iter().any(|check| {
        check["type"] == "command_succeeded" && check["command"] == command
    }) {
        items.push(json!({"type":"command_succeeded","command":command}));
    }
    Ok(())
}

/// A passing build/test from before the most recent workspace edit is stale.
/// The verifier must be run again on the final bytes, not on an earlier draft.
fn verification_after_last_edit(command:&str,evidence:&[Value])->bool{
    crate::engine::agent_kernel::verification::command_is_fresh(
        &verification::verification_events(evidence),command)
}

/// Count new, successful observations instead of counting tool invocations.
/// Re-reading the same file or repeating the same shell output is not progress,
/// and failed receipts are safety evidence, never a reason to buy another round.
fn evidence_signature(item:&Value)->u64{
    use std::hash::{Hash,Hasher};
    let mut result=item["result"].clone();
    if let Some(fields)=result.as_object_mut(){
        for key in ["id","callId","operationId","timestamp","at","startedAt",
            "finishedAt","elapsedMs","durationMs","latencyMs"]{
            fields.remove(key);
        }
    }
    let payload=json!({"tool":verification::tool_name(item),
        "arguments":item["arguments"],"result":result});
    let mut hasher=std::collections::hash_map::DefaultHasher::new();
    payload.to_string().hash(&mut hasher);
    hasher.finish()
}
fn novel_successful_evidence(previous:&[Value],latest:&[Value])->usize{
    let mut seen:std::collections::HashSet<_>=previous.iter()
        .filter(|item|verification::successful_evidence(item))
        .map(evidence_signature).collect();
    latest.iter().filter(|item|{
        if !verification::successful_evidence(item){return false;}
        if matches!(verification::tool_name(item),
            "workspace_write"|"workspace_replace"|"workspace_replace_range"|
            "workspace_apply_patch"|"file_write"
        ) && !verification::is_action_evidence(item){return false;}
        seen.insert(evidence_signature(item))
    }).count()
}

/// Seal the inner autonomy state before the outer job transitions to its
/// terminal status. Without this, a task whose file exists but whose verifier
/// returns needs_review misleadingly retains autonomousState.status=running.
/// Keep the exact unresolved checks and model/tool evidence; this does not
/// promote a review to success or discard potentially uncertain effects.
fn terminal_checkpoint(
    ctx:&JobContext,
    status:&str,
    attempt:usize,
    checks:&Value,
    gaps:&[String],
    receipts:&[Value],
    last_error:Option<&str>,
    evidence:&[Value],
    verified:bool,
)->Result<(),String>{
    ctx.checkpoint(json!({"autonomousState":{
        "status":status,
        "phase":if status=="completed"{"completed"}else{"needs_review"},
        "attempt":attempt,
        "verification":checks,
        "unmetChecks":gaps,
        "lastError":last_error.map(|error|narrow(error,1000)),
        // A completed but non-idempotent external write may be unsafe to
        // replay without having an unknown outcome. Keep the two separate.
        "replayUnsafeEffects":side_effect_unknown(evidence),
        "unknownToolOutcomes":evidence.iter().any(|item|
            item["outcomeUnknown"]==true || item["result"]["outcomeUnknown"]==true
            || item["effectUnknown"]==true || item["result"]["effectUnknown"]==true),
        "verificationPassed":verified,
        "receipts":receipts
    }}))
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
    let mut checks=verification::prepare_acceptance_checks(&ctx.state,goal);
    let verify_command=input["verifyCommand"].as_str().filter(|command|!command.trim().is_empty());
    add_explicit_verifier(&mut checks, verify_command)?;
    let mut has_checks=checks.as_array().is_some_and(|v|!v.is_empty());
    let mut active_goal=goal.to_owned();
    let mut local_code_change=verification::local_edit_intent(goal);
    let mut steering_checked_revision=0u64;
    let mut latest_replacement_revision=0u64;
    let mut evidence_epoch=0usize;
    let mut plan=decompose::decompose(goal,&json!({"implementation":"numbered-first"}));
    let session_id=request.session_id.as_deref().unwrap_or("");
    let initial_tokens=ctx.state.store.session_usage(session_id)["totalTokens"]
        .as_u64().unwrap_or(0);
    let mut all_evidence=Vec::<Value>::new();
    let mut receipts=Vec::<Value>::new();
    let mut cycle=Cycle::new(config);
    let mut last_message=Value::Null;
    ctx.checkpoint(json!({"autonomousState":{
        "status":"running","phase":"planning",
        "plan":plan,"verification":checks,"attempt":0,
    }}))?;
    while let Some(attempt)=cycle.next_attempt() {
        // A cancelled or stale job loses authority before the next model call.
        ctx.checkpoint(json!({"autonomousState":{
            "status":"running","phase":"executing","attempt":attempt+1,
            "plan":plan,"verification":checks,
            "receipts":receipts,
        }}))?;
        // A running model/tool call is not interrupted mid-effect. At the
        // next turn boundary, replay the owner's revisioned intent from Rust.
        let owner_job=ctx.state.store.get("job",&ctx.job_id)
            .ok_or("autonomous job no longer exists")?;
        let steering_before=owner_job["steeringRevision"].as_u64().unwrap_or(0);
        let messages=owner_job["steering"].as_array().map(Vec::as_slice).unwrap_or(&[]);
        let intent=owner_intent(goal,messages);
        let replaced=intent.replacement_revision>latest_replacement_revision;
        if replaced{
            // The old goal cannot continue to gate success. Historical
            // receipts and side effects remain visible; old tool evidence
            // cannot accidentally satisfy the replacement's new predicates.
            checks=verification::prepare_acceptance_checks(&ctx.state,&intent.goal);
            add_explicit_verifier(&mut checks,verify_command)?;
            steering_checked_revision=intent.replacement_revision;
            latest_replacement_revision=intent.replacement_revision;
            evidence_epoch=all_evidence.len();
            cycle.replaced_goal();
        }
        if steering_before>steering_checked_revision{
            for message in messages.iter().filter(|m|
                m["revision"].as_u64().unwrap_or(0)>steering_checked_revision
                && m["mode"]!="replace"
            ){
                if let Some(instruction)=message["instruction"].as_str(){
                    let additional=verification::prepare_acceptance_checks(&ctx.state,instruction);
                    if let (Some(existing),Some(extra))=(checks.as_array_mut(),additional.as_array()){
                        for check in extra{
                            if existing.len()>=20 {break;}
                            if !existing.iter().any(|prior|prior==check){
                                existing.push(check.clone());
                            }
                        }
                    }
                }
            }
            steering_checked_revision=steering_before;
        }
        if steering_before>0{
            active_goal=intent.effective_goal();
            plan=decompose::decompose(&active_goal,&json!({"implementation":"numbered-first"}));
            local_code_change=verification::local_edit_intent(&active_goal);
        }
        has_checks=checks.as_array().is_some_and(|v|!v.is_empty());
        let mut turn=request.clone();
        if replaced{
            turn.input=format!(
                "사용자가 기존 작업 목표를 교체했습니다. 이전 목표의 미완료 항목을 계속 수행하지 마세요. \
                이전 외부 작업의 중복 실행은 피하고 현재 상태를 확인하세요.\n\n새 사용자 목표:\n{active_goal}"
            );
            if request.input!=goal{
                // A resumed run already carries the owner's confirmation
                // about uncertain previous effects. Superseding the goal
                // must not erase that evidence from the next model turn.
                turn.input.push_str("\n\n재개 전에 확인한 이전 작업 상태:\n");
                turn.input.push_str(&request.input);
            }
        }else if attempt>0{
            turn.input=cycle.retry_instruction(&active_goal,attempt);
        }else if steering_before>0{
            // A resumed turn has a new owner confirmation in request.input.
            // Do not erase that instruction just because previous steering
            // notes have an older revision; preserve both sources of intent.
            if request.input==goal {
                turn.input=active_goal.clone();
            }else{
                turn.input=format!("{}\n\n현재 적용 중인 사용자 목표와 추가 조건:\n{}",
                    request.input,active_goal);
            }
        }
        if let Some(command)=verify_command{
            turn.input.push_str("\n\n사용자가 지정한 필수 검증 명령을 실제 도구로 실행하고 결과를 확인하세요. ");
            turn.input.push_str("기존 코드 변경이 완료된 뒤 명령이 성공해야 작업이 통과됩니다. ");
            turn.input.push_str("실행에는 기존 도구 승인 정책이 적용됩니다. 명령: ");
            turn.input.push_str(command);
        }
        if steering_before>0{
            ctx.checkpoint(json!({"autonomousState":{
                "status":"running","phase":"executing","attempt":attempt+1,
                "activeGoal":active_goal,"steeringRevision":steering_before,
                "plan":plan,"verification":checks,"receipts":receipts,
            }}))?;
        }
        let usage_before=ctx.state.store.session_usage(session_id);
        let (outcome, stats):(Result<Option<Value>,String>,TurnStats)=
            run_quiet_outcome(&ctx.state,&turn).await;
        let usage_after=ctx.state.store.session_usage(session_id);
        let measured_usage=usage_delta(&usage_before,&usage_after);
        let consumed_tokens=usage_after["totalTokens"].as_u64().unwrap_or(0)
            .saturating_sub(initial_tokens);
        let latest_error=outcome.as_ref().err().cloned();
        last_message=outcome.as_ref().ok().and_then(|msg|msg.as_ref())
            .map(|msg|msg["id"].clone()).unwrap_or(Value::Null);
        // Tool failures are retained for no-replay decisions, but must not
        // make an unchanged failed attempt appear to have advanced the task.
        let before_evidence=all_evidence.len();
        let new_evidence=novel_successful_evidence(
            &all_evidence[evidence_epoch..before_evidence],&stats.evidence);
        all_evidence.extend(stats.evidence.iter().cloned());
        let current_evidence=&all_evidence[evidence_epoch..];
        let mut gaps=verification::check_acceptance(
            &ctx.state,&active_goal,&checks,current_evidence);
        if let Some(command)=verify_command {
            if !verification_after_last_edit(command,current_evidence) {
                gaps.push("최종 파일 변경 이후 검증 명령이 성공한 증거가 없습니다. 마지막 변경본으로 명령을 다시 실행하세요.".into());
            }
        }
        // If the owner steered while a long model turn was executing, never
        // complete based on the previous instruction snapshot.
        let steering_after=ctx.state.store.get("job",&ctx.job_id)
            .and_then(|j|j["steeringRevision"].as_u64()).unwrap_or(0);
        let owner_changed=steering_after>steering_before;
        if owner_changed {
            gaps.push("실행 중 새로운 사용자 지시를 받았습니다. 다음 턴에서 반영합니다.".into());
        }
        let local_proof=local_code_change
            && current_evidence.iter().any(|item|
                verification::proven_workspace_edit(&ctx.state,item));
        let tested=verification::test_after_last_edit(current_evidence);
        // The independent SDK kernel owns retry, stop and no-replay
        // admission. This is the real prompt worker, not a shadow evaluator.
        let token_budget_exhausted=latest_error.as_deref()
            .is_some_and(|error|error.contains("작업 토큰 예산"))
            || input["tokenBudget"].as_u64().filter(|limit|*limit>=1024)
                .is_some_and(|limit|consumed_tokens>=limit);
        let assessment=cycle.assess(&Observation{
            has_checks,gaps:&gaps,error:latest_error.as_deref(),
            accumulated_evidence:&all_evidence,new_evidence,
            owner_changed,token_budget_exhausted,
        });
        let repeated_without_progress=assessment.stalled;
        let mut decision=assessment.decision;
        if decision!=Decision::Repair {
            // Atomically close the exact version observed before this model turn.
            // A concurrent owner steer invalidates this decision even when the
            // latest in-memory observation looked unchanged.
            let expected_gate=owner_job.get("steeringGate").unwrap_or(&Value::Null);
            match ctx.state.store.patch_if("job",&ctx.job_id,"steeringGate",expected_gate,
                &json!({"steeringGate":{"revision":steering_before,"closed":true}})){
                Some(saved)=>ctx.state.bus.record("job",&saved),
                None=>{
                    gaps.push("최종 검증 중 새 사용자 지시가 들어와 완료 판정을 보류했습니다.".into());
                    decision=if cycle.can_retry() && !assessment.unknown_effect{
                        Decision::Repair
                    }else{Decision::Review};
                }
            }
        }
        let receipt=json!({
            "attempt":attempt+1,
            "steeringRevision":steering_before,"activeGoal":active_goal,
            "outcome":match decision {
                Decision::Accept=>"accepted",Decision::Repair=>"repair",Decision::Review=>"needs_review"
            },
            "messageId":last_message,
            "toolSuccess":stats.tool_success,
            "toolErrors":stats.tool_errors,
            "evidenceCount":stats.evidence.len(),
            "failureClass":crate::engine::agent_kernel::failure::classify(
                &gaps,latest_error.as_deref()).label(),
            "usage":measured_usage,
            "tokenUsage":{"used":consumed_tokens,"limit":input["tokenBudget"]},
            "gaps":gaps.iter().take(8).map(|g|narrow(g,400)).collect::<Vec<_>>(),
            "error":latest_error.as_deref().map(|e|narrow(e,500)),
        });
        ctx.emit(json!({"type":"autonomous_step","step":receipt}));
        receipts.push(receipt);
        // Commit kernel feedback before serializing its durable snapshot.
        if decision==Decision::Repair {
            cycle.record_retry(&gaps,latest_error.as_deref());
        }
        // Persist outcomes before an additional model call; a crash after a
        // file write must not automatically replay that write.
        ctx.checkpoint(json!({"autonomousState":{
            "status":"running","phase":if decision==Decision::Repair{"replanning"}else{"verifying"},
            "attempt":attempt+1,"plan":plan,"verification":checks,
            "activeGoal":active_goal,"steeringRevision":steering_before,
            "tokenUsage":{"used":consumed_tokens,"limit":input["tokenBudget"]},
            "kernelCycle":cycle.snapshot(),
            "receipts":receipts,
        }}))?;
        match decision {
            Decision::Accept => {
                let verified=gaps.is_empty() && (has_checks || (local_proof && tested));
                terminal_checkpoint(ctx,"completed",attempt+1,&checks,&gaps,
                    &receipts,latest_error.as_deref(),&all_evidence,verified)?;
                return Ok(json!({
                    "sessionId":session_id,"messageId":last_message,
                    "status":"completed","attempts":attempt+1,
                    "steeringRevision":steering_before,
                    "verification":if has_checks{"predicate"}else if local_code_change{"post_edit_test"}else if all_evidence.is_empty(){"answer_only"}else{"tool_evidence"},
                    "verified":verified,
                    "proof":{"workspaceMutation":local_proof,"postEditTest":tested},
                    "receipts":receipts,
                }));
            }
            Decision::Review => {
                terminal_checkpoint(ctx,"needs_review",attempt+1,&checks,&gaps,
                    &receipts,latest_error.as_deref(),&all_evidence,false)?;
                return Ok(json!({
                    "status":"needs_review","sessionId":session_id,"messageId":last_message,
                    "reason":if side_effect_unknown(&all_evidence) {
                        "외부 부작용이 발생했을 수 있어 자동 재실행하지 않습니다."
                    }else if token_budget_exhausted{
                        "사용자가 설정한 작업 토큰 예산에 도달해 추가 호출을 중단했습니다."
                    }else if repeated_without_progress {
                        "새로운 실행 증거 없이 동일한 검증 실패가 반복되어 재시도를 중단했습니다."
                    }else{"검증되지 않은 항목이 남았습니다. 작업 기록을 확인하세요."},
                    "unmetChecks":gaps,"lastError":latest_error,
                    "attempts":attempt+1,"receipts":receipts,
                }));
            }
            Decision::Repair => {}
        }
    }
    Err("autonomous execution ended without a terminal decision".into())
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn verifier_terminal_checkpoint_records_review_without_promoting_file_success() {
        use crate::{config::Config,db::Store,state::AppState,vault::Vault};
        let root=std::env::temp_dir().join(format!(
            "dominion-autonomy-terminal-{}",uuid::Uuid::new_v4()));
        let config=Config{port:0,data_dir:root.join("data"),
            web_dir:root.join("web"),workspace:root.clone()};
        let state=AppState::new(config.clone(),
            Store::open(&config.db_path()).unwrap(),
            Vault::open(&config.key_path()));
        let job_id="terminal-review";
        state.store.put("job",&json!({"id":job_id,"kind":"prompt",
            "status":"running","attempts":1,"runEpoch":1,
            "input":{"prompt":"change output.py"}})).unwrap();
        let context=JobContext::new(state.clone(),job_id.into());
        let criteria=json!([{"type":"file_changed","path":"output.py"}]);
        let gaps=vec!["write receipt did not match output.py".to_owned()];
        let receipts=vec![json!({"attempt":1,"outcome":"needs_review",
            "gaps":gaps,"toolSuccess":1})];
        let unknown=vec![json!({"tool":"github_api",
            "arguments":{"method":"POST"},"result":{"ok":true}})];
        terminal_checkpoint(&context,"needs_review",1,&criteria,&gaps,
            &receipts,Some("late review needed"),&unknown,false).unwrap();
        let job=state.store.get("job",job_id).unwrap();
        assert_eq!(job["status"],"running",
            "only the outer job owner may commit the terminal job status");
        assert_eq!(job["autonomousState"]["status"],"needs_review");
        assert_eq!(job["autonomousState"]["phase"],"needs_review");
        assert_eq!(job["autonomousState"]["unmetChecks"],json!(gaps));
        assert_eq!(job["autonomousState"]["receipts"],json!(receipts));
        assert_eq!(job["autonomousState"]["lastError"],"late review needed");
        assert_eq!(job["autonomousState"]["replayUnsafeEffects"],true);
        assert_eq!(job["autonomousState"]["unknownToolOutcomes"],false);
        assert_eq!(job["autonomousState"]["verificationPassed"],false);
        assert_eq!(job["input"]["prompt"],"change output.py");

        // The same helper records an independently verified terminal state
        // without synthesizing additional tool invocations.
        terminal_checkpoint(&context,"completed",2,&criteria,&[],&receipts,
            None,&[],true).unwrap();
        let saved=state.store.get("job",job_id).unwrap();
        assert_eq!(saved["autonomousState"]["status"],"completed");
        assert_eq!(saved["autonomousState"]["verificationPassed"],true);
        assert_eq!(saved["autonomousState"]["unmetChecks"],json!([]));
        state.store.patch_if("job",job_id,"status",&json!("running"),
            &json!({"status":"cancelled"})).unwrap();
        assert!(terminal_checkpoint(&context,"completed",3,&criteria,&[],
            &receipts,None,&[],true).is_err(),
            "a cancelled owner must reject a stale terminal checkpoint");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn shell_replay_barrier_handles_git_global_flags_and_package_commands() {
        for safe in ["git status","git -C project diff --stat",
                     "cargo test --release","npm run typecheck --prefix web",
                     "rg token src","pwd"] {
            assert!(read_only_shell_command(safe),"{safe}");
        }
        for command in ["git -C project push","git -c core.hooksPath=abc commit",
                        "git stash","cargo publish","npm install",
                        "npm run deploy","rg --pre sh secret",
                        "git status && git push"] {
            assert!(!read_only_shell_command(command),"{command}");
            assert!(side_effect_unknown(&[json!({"tool":"shell_exec",
                "arguments":{"command":command},"result":{"exitCode":0}})]));
        }
    }
    #[test]
    fn owner_goal_replacement_invalidates_old_additions(){
        let notes=vec![
            json!({"revision":1,"mode":"append","instruction":"Old extra file"}),
            json!({"revision":2,"mode":"replace","instruction":"Create a new report"}),
            json!({"revision":3,"mode":"append","instruction":"Include a chart"}),
        ];
        let intent=owner_intent("Delete the old report",&notes);
        assert_eq!(intent.replacement_revision,2);
        assert_eq!(intent.goal,"Create a new report");
        assert!(intent.effective_goal().contains("Include a chart"));
        assert!(!intent.effective_goal().contains("Old extra file"));
        assert!(!intent.effective_goal().contains("Delete the old report"));
        let legacy=owner_intent("Original",&notes[..1]);
        assert_eq!(legacy.replacement_revision,0);
        assert!(legacy.effective_goal().contains("Original"));
        assert!(legacy.effective_goal().contains("Old extra file"));
    }
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
    fn repeated_tool_reads_and_errors_are_not_false_progress(){
        let read=json!({"tool":"shell_exec","callId":"first",
            "arguments":{"command":"git status"},
            "result":{"exitCode":0,"stdout":"clean","elapsedMs":14}});
        let repeated=json!({"tool":"shell_exec","callId":"second",
            "arguments":{"command":"git status"},
            "result":{"exitCode":0,"stdout":"clean","elapsedMs":99}});
        let changed=json!({"tool":"shell_exec","callId":"third",
            "arguments":{"command":"git status"},
            "result":{"exitCode":0,"stdout":"modified","elapsedMs":22}});
        let failed=json!({"tool":"shell_exec","status":"failed",
            "arguments":{"command":"git status"},"result":{"error":"timeout"}});
        assert_eq!(novel_successful_evidence(&[],&[read.clone()]),1);
        assert_eq!(novel_successful_evidence(&[read.clone()],
            &[repeated,failed]),0);
        assert_eq!(novel_successful_evidence(&[read],&[changed]),1);
        let noop=json!({"tool":"workspace_write","result":{
            "written":true,"changed":false,"success":true}});
        assert_eq!(novel_successful_evidence(&[],&[noop]),0);
    }
    #[test]
    fn model_usage_is_measured_from_engine_audit_not_estimated_by_length(){
        let before=json!({"calls":2,"promptTokens":100,"completionTokens":10,"totalTokens":110});
        let after=json!({"calls":3,"promptTokens":170,"completionTokens":25,"totalTokens":195});
        let delta=usage_delta(&before,&after);
        assert_eq!(delta["calls"],1);
        assert_eq!(delta["promptTokens"],70);
        assert_eq!(delta["totalTokens"],85);
    }
    #[test]
    fn identical_failed_attempts_stop_before_wasting_more_tokens(){
        let gaps=vec!["file missing".to_string()];
        assert!(stalled(&gaps,&gaps,0,None,None));
        assert!(!stalled(&gaps,&gaps,1,None,None));
        assert!(!stalled(&gaps,&[],0,None,None));
    }
    #[test]
    fn explicit_ui_verifier_is_not_optional_and_cannot_be_faked() {
        let mut checks=json!([{"type":"file_exists","path":"src/main.rs"}]);
        add_explicit_verifier(&mut checks,Some("cargo test")).unwrap();
        assert_eq!(checks.as_array().unwrap().len(),2);
        assert_eq!(checks[1]["type"],"command_succeeded");
        assert_eq!(checks[1]["command"],"cargo test");
        add_explicit_verifier(&mut checks,Some("cargo test")).unwrap();
        assert_eq!(checks.as_array().unwrap().len(),2);
        assert!(add_explicit_verifier(&mut checks,Some(&"x".repeat(2049))).is_err());
        let mut full=json!(vec![json!({"type":"file_exists","path":"some.rs"});20]);
        assert!(add_explicit_verifier(&mut full,Some("cargo test")).is_err());
    }
    #[test]
    fn verifier_must_run_after_the_last_successful_source_mutation(){
        let test=json!({"tool":"shell_exec","arguments":{"command":"cargo test"},
            "result":{"exitCode":0}});
        let edit=json!({"tool":"workspace_replace_range",
            "result":{"success":true}});
        assert!(!verification_after_last_edit("cargo test",&[]));
        assert!(verification_after_last_edit("cargo test",&[test.clone()]));
        assert!(!verification_after_last_edit("cargo test",&[test.clone(),edit.clone()]));
        assert!(verification_after_last_edit("cargo test",&[edit.clone(),test.clone()]));
        assert!(verification_after_last_edit("cargo test",&[edit,test.clone(),test]));
    }
    #[test]
    fn partial_multi_file_patch_with_unknown_effect_requires_owner_review(){
        let uncertain=json!({"tool":"workspace_apply_patch","result":{
            "status":"partial","error":"move deletion status unclear",
            "outcomeUnknown":true,"partialApplied":true,
            "files":[{"path":"src/one.rs","written":true,"changed":true}]
        }});
        assert!(side_effect_unknown(&[uncertain.clone()]));
        assert_eq!(decide(Strategy::Focused,false,&[],None,&[uncertain],0,6),
            Decision::Review);
    }

    #[test]
    fn conservative_strategy_stops_on_partial_write_plus_provider_error(){
        let evidence=vec![json!({"tool":"workspace_write","result":{"ok":true}})];
        assert_eq!(decide(Strategy::Conservative,true,&["missing".into()],Some("network"),&evidence,0,3),Decision::Review);
    }
}
