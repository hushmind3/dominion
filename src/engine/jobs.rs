use crate::engine::chat::TurnRequest;
use crate::engine::run_quiet;
use crate::engine::{learning, reflect, rooms};
use crate::state::AppState;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const MAX_EVENTS: usize = 200;

/// Shared handle a running job uses to report progress and persist its state.
#[derive(Clone)]
pub struct JobContext {
    pub state: AppState,
    pub job_id: String,
    attempt: u64,
    epoch: u64,
    events: Arc<Mutex<Vec<Value>>>,
}

impl JobContext {
    pub fn new(state: AppState, job_id: String) -> Self {
        let job=state.store.get("job",&job_id).unwrap_or(Value::Null);
        let attempt=job["attempts"].as_u64().unwrap_or(0);
        let epoch=job["runEpoch"].as_u64().unwrap_or(0);
        Self {
            state,
            job_id,
            attempt,
            epoch,
            events: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn emit(&self, event: Value) {
        let mut payload = event.clone();
        payload["jobId"] = json!(self.job_id);
        self.state.bus.emit(json!({
            "type": "job_event",
            "jobId": self.job_id,
            "event": payload,
        }));
        let mut events = self.events.lock().unwrap();
        if events.len() < MAX_EVENTS {
            events.push(json!({
                "type": event.get("type").cloned().unwrap_or(json!("event")),
                "at": chrono::Utc::now().to_rfc3339(),
            }));
        }
    }

    /// Merge a patch into the persisted job record and notify listeners.
    pub fn patch(&self, patch: Value) {
        if let Err(error) = self.checkpoint(patch) {
            self.state.store.audit(
                "job.checkpoint_failed",
                json!({"jobId":self.job_id,"error":error}),
            );
        }
    }

    /// Every checkpoint belongs to one dispatcher-claimed attempt. A late
    /// response after retry or cancellation cannot overwrite the new owner.
    pub fn checkpoint(&self, patch: Value) -> Result<(), String> {
        let saved=self.state.store.checkpoint_running_job(
            &self.job_id,self.attempt,self.epoch,&patch,
        ).ok_or("작업이 종료·중단되었거나 다른 실행 시도로 교체되어 체크포인트를 거부했습니다.")?;
        self.state.bus.record("job",&saved);
        Ok(())
    }

}

pub fn enqueue(state: &AppState, kind: &str, input: Value) -> Option<Value> {
    let job = state.store.put(
        "job",
        &json!({
            "kind": kind,
            "status": "queued",
            "queuedAt": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, true),
            "input": input,
            "attempts": 0,
            "stepIndex": 0,
            "stepTotal": 0,
            "events": [],
        }),
    );
    if let Some(job) = &job {
        state.bus.record("job", job);
        state.store.audit(
            "job.queued",
            json!({ "kind": kind, "jobId": job.get("id") }),
        );
    }
    job
}

/// Atomically deduplicate an immutable operation across HTTP requests and
/// workers. The supplied ID belongs to the engine, never a client record.
/// Durable user steering is engine policy, not an HTTP-layer shortcut.
#[derive(Debug)]
pub enum SteeringError {
    BadInput(String),
    NotFound,
    Conflict(String),
}
impl std::fmt::Display for SteeringError {
    fn fmt(&self,fmt:&mut std::fmt::Formatter<'_>)->std::fmt::Result{
        match self {
            Self::BadInput(text)|Self::Conflict(text)=>fmt.write_str(text),
            Self::NotFound=>fmt.write_str("작업을 찾을 수 없습니다."),
        }
    }
}
pub fn steer(state:&AppState,id:&str,instruction:&str)->Result<Value,SteeringError>{
    steer_with_mode(state,id,instruction,"append")
}

/// User goal replacements and additive steering share one native durable job.
/// A revisioned CAS prevents a stale completion from overtaking owner input.
pub fn steer_with_mode(state:&AppState,id:&str,instruction:&str,mode:&str)
    ->Result<Value,SteeringError>{
    if !matches!(mode,"append"|"replace") {
        return Err(SteeringError::BadInput("지시 방식은 append 또는 replace입니다.".into()));
    }
    let instruction=instruction.trim();
    if instruction.is_empty() || instruction.len()>4000 {
        return Err(SteeringError::BadInput("추가 지시는 1~4000자로 입력하세요.".into()));
    }
    for _ in 0..4 {
        let job=state.store.get("job",id).ok_or(SteeringError::NotFound)?;
        if job["kind"]!="prompt" || job["input"]["rsiSourceDir"].is_string()
            || job["input"]["autonomous"]==false {
            return Err(SteeringError::BadInput(
                "자율 실행 중인 일반 작업에서만 추가 지시를 지원합니다.".into()));
        }
        if !matches!(job["status"].as_str(),Some("queued"|"running")){
            return Err(SteeringError::Conflict("진행 중인 작업만 수정할 수 있습니다.".into()));
        }
        if job["steeringGate"]["closed"] == true {
            return Err(SteeringError::Conflict("완료 판정이 확정되어 새 지시를 받을 수 없습니다.".into()));
        }
        let mut steering=job["steering"].as_array().cloned().unwrap_or_default();
        if steering.len()>=8 {
            return Err(SteeringError::Conflict("작업당 추가 지시는 최대 8회입니다.".into()));
        }
        let revision=job["steeringRevision"].as_u64().unwrap_or(0);
        steering.push(json!({
            "revision":revision+1,"instruction":instruction,"mode":mode,
            "at":chrono::Utc::now().to_rfc3339()
        }));
        // Completion and owner steering advance/close the same CAS gate.
        // Neither a late owner instruction nor a stale final answer may win both.
        if let Some(saved)=state.store.patch_if(
            "job",id,"steeringGate",
            job.get("steeringGate").unwrap_or(&Value::Null),
            &json!({"steeringRevision":revision+1,"steering":steering,
                "steeringGate":{"revision":revision+1,"closed":false}}),
        ){
            state.bus.record("job",&saved);
            state.store.audit("job.steer",json!({"jobId":id,"revision":revision+1,"mode":mode}));
            return Ok(json!({
                "jobId":id,"steeringRevision":revision+1,"mode":mode,
                "appliesAt":"next_model_turn"
            }));
        }
    }
    Err(SteeringError::Conflict("작업이 동시에 수정되었습니다.".into()))
}

pub fn enqueue_once(state: &AppState, id: &str, kind: &str, input: Value) -> Option<Value> {
    let candidate = json!({"id":id,"kind":kind,"input":input,"status":"queued",
        "queuedAt":chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros,true),
        "attempts":0,"stepIndex":0,"stepTotal":0,"events":[]});
    if let Some(saved) = state.store.put_if_absent("job", &candidate) {
        state.bus.record("job", &saved);
        state
            .store
            .audit("job.queued", json!({"kind":kind,"jobId":id}));
        return Some(saved);
    }
    state
        .store
        .get("job", id)
        .filter(|existing| existing["kind"] == kind && existing["input"] == candidate["input"])
}

#[derive(Debug)]
pub enum CancelJobError { NotFound, Conflict }
impl std::fmt::Display for CancelJobError {
    fn fmt(&self, f:&mut std::fmt::Formatter<'_>)->std::fmt::Result {
        f.write_str(match self {
            Self::NotFound=>"작업이 없습니다.",
            Self::Conflict=>"진행 중인 작업만 중단할 수 있습니다.",
        })
    }
}

/// One native cancellation policy for Rust SDK and HTTP job clients.
pub fn cancel_job(state:&AppState,id:&str)->Result<Value,CancelJobError>{
    let job=state.store.get("job",id).ok_or(CancelJobError::NotFound)?;
    if !matches!(job["status"].as_str(),Some("queued"|"running")){
        return Err(CancelJobError::Conflict);
    }
    // Persist cancellation before notifying the live session. Losing a
    // cross-process race with completion must return a conflict, not a
    // misleading 200 with a completed/failed job as if it was cancelled.
    let Some(saved)=state.store.patch_if("job",id,"status",&job["status"],
        &json!({"status":"cancelled","cancelRequested":true,
            "finishedAt":chrono::Utc::now().to_rfc3339()})) else {
        return Err(CancelJobError::Conflict);
    };
    if let Some(session)=saved["sessionId"].as_str(){state.cancel_session(session);}
    state.bus.record("job",&saved);
    state.store.audit("job.cancelled",json!({"jobId":id,"kind":job["kind"]}));
    Ok(saved)
}

/// Recovers work left behind by a crash: running jobs are re-queued (unless
/// they already exhausted retries) and running executions become interrupted.
pub fn recover(state: &AppState) {
    let jobs=match state.store.active_jobs(){
        Ok(jobs)=>jobs,
        Err(error)=>{
            state.store.audit("recovery.jobs_read_failed",json!({"error":error}));
            return;
        }
    };
    let uncertain_sessions=match state.store.uncertain_effect_sessions(){
        Ok(sessions)=>Some(sessions),
        Err(error)=>{
            state.store.audit("recovery.uncertain_effects_read_failed",
                json!({"error":error}));
            None
        }
    };
    for job in jobs {
        if job.get("status").and_then(|value| value.as_str()) != Some("running") {
            continue;
        }
        let attempts = job
            .get("attempts")
            .and_then(|value| value.as_u64())
            .unwrap_or(0);
        let mut updated = job.clone();
        updated["recovered"] = json!(true);
        let flagged = [
            "/inFlight",
            "/promptState/inFlight",
            "/rsiCompareInFlight",
            "/rsiEvolutionInFlight",
            "/goalState/inFlight",
            "/goalState/reviewPending",
            "/roomState/inFlight",
            "/workflowState/inFlight",
            "/discordState/inFlight",
            "/videoState/inFlight",
        ]
        .iter()
        .any(|path| {
            job.pointer(path)
                .is_some_and(|value| !value.is_null() && value != &json!(false))
        });
        // A tool may have reached an external system before the crash. Its
        // result is unknown, so replaying the job could repeat a side effect.
        let job_session = job["sessionId"]
            .as_str()
            .or_else(|| job.pointer("/input/sessionId").and_then(Value::as_str));
        let unknown_external_result=uncertain_sessions.as_ref().is_none_or(
            |sessions|job_session.is_some_and(|session|sessions.contains(session))
        );
        let uncertain = flagged || unknown_external_result;
        updated["status"] = json!(if attempts >= 3 || uncertain {
            "needs_review"
        } else {
            "queued"
        });
        if uncertain {
            updated["error"] =
                json!("재시작 전 작업 결과를 확정할 수 없습니다. 실행 기록을 확인하세요.");
        }
        state.store.put("job", &updated);
        if matches!(job["kind"].as_str(), Some("room" | "meeting")) {
            if let Some(id) = job.pointer("/input/roomId").and_then(Value::as_str) {
                let status = updated["status"].as_str().unwrap_or("needs_review");
                if let Some(saved)=state.store.patch_if("room",id,"id",&json!(id),&json!({
                    "status":status,"activeAgentId":null,"pauseReason":if status=="needs_review" {"재시작 전 발언이나 도구 결과를 확인해야 합니다."} else {""}
                })) { state.bus.record("room",&saved); }
            }
        }
        state.store.audit(
            "job.recovered",
            json!({ "jobId": job.get("id"), "attempts": attempts }),
        );
    }
    let running_executions=match state.store.running_recovery_records("execution"){
        Ok(rows)=>rows,
        Err(error)=>{
            state.store.audit("recovery.executions_read_failed",json!({"error":error}));
            Vec::new()
        }
    };
    for execution in running_executions {
            let mut updated = execution.clone();
            updated["status"] = json!("interrupted");
            updated["phase"] = json!("interrupted");
            updated["phaseUpdatedAt"] = json!(chrono::Utc::now().to_rfc3339());
            updated["taskOutcome"] = json!("cancelled");
            updated["reason"] = json!("백엔드가 재시작되어 실행이 중단되었습니다.");
            updated["finishedAt"] = json!(chrono::Utc::now().to_rfc3339());
            state.store.put("execution", &updated);
    }
    let running_operations=match state.store.running_recovery_records("execution_operation"){
        Ok(rows)=>rows,
        Err(error)=>{
            state.store.audit("recovery.operations_read_failed",json!({"error":error}));
            Vec::new()
        }
    };
    for mut operation in running_operations {
            operation["status"] = json!("interrupted");
            operation["outcomeUnknown"] = json!(true);
            operation["phase"] = json!("interrupted");
            operation["reason"] = json!("백엔드가 재시작되어 도구 결과를 확정할 수 없습니다.");
            operation["finishedAt"] = json!(chrono::Utc::now().to_rfc3339());
            state.store.put("execution_operation", &operation);
    }
    // After interrupted executions have been recorded, expire durable approvals
    // that outlived their live waiter before wake decisions are made.
    crate::engine::chat::expire_stale_approvals(state);
}

fn queued_job_event(event:&Value)->bool{
    event["type"]=="record" && event["kind"]=="job"
        && event["record"]["status"]=="queued"
}

async fn wait_dispatch_event(
    events:&mut tokio::sync::broadcast::Receiver<Value>,
    finished:&tokio::sync::Notify,
    fallback:&mut tokio::time::Interval,
)->bool{
    loop{
        tokio::select!{
            _=finished.notified()=>return true,
            _=fallback.tick()=>return true,
            event=events.recv()=>match event{
                Ok(value) if queued_job_event(&value)=>return true,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_))=>return true,
                Err(tokio::sync::broadcast::error::RecvError::Closed)=>return false,
                _=>{}
            }
        }
    }
}

/// One event-driven durable dispatcher with an existing worker-count bound.
pub fn start(state:AppState){
    crate::engine::channel_adapter::start(state.clone());
    crate::engine::money::tick(&state);
    let workers=std::env::var("DOMINION_JOB_WORKERS").ok()
        .and_then(|v|v.parse::<usize>().ok()).unwrap_or(4).clamp(1,16);
    let capacity=Arc::new(tokio::sync::Semaphore::new(workers));
    let finished=Arc::new(tokio::sync::Notify::new());
    let mut events=state.bus.subscribe();
    let mut fallback=tokio::time::interval(Duration::from_secs(10));
    fallback.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let dispatch_state=state.clone();
    tokio::spawn(async move{
        loop{
            while let Ok(permit)=capacity.clone().try_acquire_owned(){
                let Some(job)=next_queued(&dispatch_state)else{
                    drop(permit);
                    break;
                };
                let worker_state=dispatch_state.clone();
                let wake=finished.clone();
                tokio::spawn(async move{
                    run_job(worker_state,job).await;
                    drop(permit);
                    wake.notify_one();
                });
            }
            if !wait_dispatch_event(&mut events,&finished,&mut fallback).await{
                break;
            }
        }
    });
    tokio::spawn(async move{
        loop{
            tokio::time::sleep(Duration::from_secs(30)).await;
            crate::engine::chat::expire_stale_approvals(&state);
            crate::engine::money::tick(&state);
        }
    });
    // No background model learning/RSI sweeps outside user-initiated work.
}

fn next_queued(state: &AppState) -> Option<Value> {
    let jobs=match state.store.active_jobs(){
        Ok(jobs)=>jobs,
        Err(error)=>{
            state.store.audit("job.queue_read_failed",json!({"error":error}));
            return None;
        }
    };
    let learning_running = jobs
        .iter()
        .any(|job| job["status"] == "running" && job["kind"] == "learn");
    let mut queued = jobs
        .iter()
        .cloned()
        .filter(|job| job.get("status").and_then(|value| value.as_str()) == Some("queued"))
        // Learning is background work: at most one at a time, so it can never
        // occupy every worker while a prompt/room/workflow job waits.
        .filter(|job| !(job["kind"] == "learn" && learning_running))
        .filter(|job| {
            if !matches!(job["kind"].as_str(), Some("room" | "meeting")) {
                return true;
            }
            let Some(room_id) = job.pointer("/input/roomId").and_then(Value::as_str) else {
                return true;
            };
            !state
                .active
                .lock()
                .unwrap()
                .contains(&format!("room-run:{room_id}"))
                && !jobs.iter().any(|other| {
                    other["status"] == "running"
                        && matches!(other["kind"].as_str(), Some("room" | "meeting"))
                        && other.pointer("/input/roomId").and_then(Value::as_str) == Some(room_id)
                })
        })
        .filter(|job| {
            job.get("availableAt")
                .and_then(Value::as_str)
                .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
                .is_none_or(|at| at <= chrono::Utc::now())
        })
        .collect::<Vec<_>>();
    queued.sort_by(|a, b| {
        let priority = |job: &Value| u8::from(job["kind"]=="learn");
        let by_priority = priority(a).cmp(&priority(b));
        if by_priority != std::cmp::Ordering::Equal {
            return by_priority;
        }
        let left = a
            .get("queuedAt")
            .or_else(|| a.get("createdAt"))
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let right = b
            .get("queuedAt")
            .or_else(|| b.get("createdAt"))
            .and_then(|value| value.as_str())
            .unwrap_or("");
        left.cmp(right)
    });
    queued.into_iter().find_map(|job| {
        let id = job.get("id")?.as_str()?;
        state.store.patch_if(
            "job",
            id,
            "status",
            &json!("queued"),
            &json!({
                "status": "running", "startedAt": chrono::Utc::now().to_rfc3339(),
                "attempts": job["attempts"].as_u64().unwrap_or(0) + 1,
                "runEpoch": job["runEpoch"].as_u64().unwrap_or(0).saturating_add(1),
            }),
        )
    })
}

/// Runs a delegated job immediately instead of waiting for a worker slot, so a
/// parent waiting on a child can never deadlock the worker pool.
pub fn start_now(state: AppState, job_id: String) {
    tokio::spawn(async move {
        let Some(job) = state.store.get("job", &job_id) else {
            return;
        };
        if job["status"] != "queued" {
            return;
        }
        let Some(running) = state.store.patch_if(
            "job",
            &job_id,
            "status",
            &json!("queued"),
            &json!({"status":"running","startedAt":chrono::Utc::now().to_rfc3339(),
                "attempts":job["attempts"].as_u64().unwrap_or(0)+1,
                "runEpoch":job["runEpoch"].as_u64().unwrap_or(0).saturating_add(1)}),
        ) else {
            return;
        };
        run_job(state, running).await;
    });
}

async fn run_job(state: AppState, job: Value) {
    let job_id = job
        .get("id")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    if job_id.is_empty() {
        return;
    }
    let context = JobContext::new(state.clone(), job_id.clone());

    state.bus.record("job", &job);

    let kind = job
        .get("kind")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();

    let result = match kind.as_str() {
        "room" | "meeting" => rooms::run(&context, &job).await,
        "prompt" => run_prompt(&context, &job).await,
        "money_discover" | "money_ingest" | "money_catalog" | "money_fold" =>
            crate::engine::money::dispatch(&context,&job).await,
        "rsi_compare"|"rsi_evolve"|"rsi_generation" => Err(
            "needs_review: 독립 RSI 캠페인은 종료되었습니다. 기존 작업 기록은 보존했습니다.".into()),
        "resume" => run_resume(&context, &job).await,
        "learn" => learning::run(&context, &job).await,
        "reflect" => reflect::run(&context, &job).await,
        "discord" => crate::engine::discord::run(&context, &job).await,
        "video"|"youtube_upload" => Err(
            "needs_review: 내장 영상·오디오 제작은 종료되었습니다. 연결 채널에서 외부 생성 도구를 사용하세요.".into()),
        other => Err(format!("지원하지 않는 작업 종류입니다: {other}")),
    };

    match result {
        Ok(result) => {
            let status = result
                .get("status")
                .and_then(Value::as_str)
                .filter(|status| matches!(*status, "needs_review" | "cancelled" | "failed"))
                .unwrap_or("completed");
            // Do not claim completed/failed in audit unless the terminal
            // checkpoint actually reached SQLite. Owner cancellation may
            // win the race between the last tool and this final transition.
            finalize_job(&context, &kind, status, json!({
                "status": status,
                "result": result,
                "finishedAt": chrono::Utc::now().to_rfc3339(),
                "events": context.events.lock().unwrap().clone(),
            }));
        }
        Err(error) => {
            if error.starts_with("deferred:") {
                context.patch(json!({"status":"queued","availableAt":(chrono::Utc::now()+chrono::Duration::seconds(1)).to_rfc3339(),
                    "attempts":job["attempts"].as_u64().unwrap_or(1).saturating_sub(1),"error":error}));
                return;
            }
            let status = if error.starts_with("needs_review:") {
                "needs_review"
            } else if error.contains("중단") || error.contains("cancelled") {
                "cancelled"
            } else {
                "failed"
            };
            finalize_job(&context, &kind, status, json!({
                "status": status,
                "error": error,
                "finishedAt": chrono::Utc::now().to_rfc3339(),
                "events": context.events.lock().unwrap().clone(),
            }));
        }
    }
    // No automatic source adoption or install on job completion.
    // Owner-directed code changes use ordinary prompt/workflow jobs instead.
}

/// Publish a terminal outcome only if the job was still live at commit time.
/// Losing a cancellation race is an expected state conflict, not a successful
/// terminal transition. Keep diagnostic evidence without falsifying outcome.
fn finalize_job(context:&JobContext,kind:&str,status:&str,patch:Value)->bool{
    match context.checkpoint(patch){
        Ok(())=>{
            context.state.store.audit(&format!("job.{status}"),
                json!({"jobId":context.job_id,"kind":kind}));
            true
        }
        Err(reason)=>{
            context.state.store.audit("job.finalization_conflict",
                json!({"jobId":context.job_id,"kind":kind,
                    "attemptedStatus":status,"reason":reason}));
            false
        }
    }
}

/// Build a new, explicit owner-facing continuation instruction, *not* a
/// duplicate of the original prompt. The owner's manually observed facts
/// remain unverified and the next model turn must inspect real workspace
/// state before any action; no old uncertain tool call is auto-replayed.
fn resume_owner_instruction(job:&Value,goal:&str)->Result<Option<String>,String>{
    if job["resumeCount"].as_u64().unwrap_or(0)==0{return Ok(None);}
    let proof=&job["resumeEvidence"];
    if !matches!(proof["source"].as_str(),
        Some("owner_attestation"|"engine_no_tool_observations"))
        || proof["applied"]!=false{
        return Err("재개하려면 미적용 확인 또는 도구 실행이 없는 오류 기록이 필요합니다.".into());
    }
    let Some(details)=proof["details"].as_str()
        .map(str::trim).filter(|s|!s.is_empty()) else{
        return Err("재개할 작업의 확인 기록이 비어 있습니다.".into());
    };
    Ok(Some(format!(
        "이 작업은 사용자가 수동 확인을 마친 뒤 재개했습니다. \
         다음 기록은 사용자 관찰로서 자동 검증된 사실이 아닙니다.\n\n         사용자가 확인한 미적용 내역: {}\n\n         원래 목표: {}\n\n         이전 파일 수정·원격 호출·도구 실행을 그대로 재생하지 마세요. \
         현재 파일과 외부 상태를 먼저 다시 확인하고, 실제로 남은 작업만 수행하세요. \
         권한과 검증 조건은 기존 설정대로 유지하세요.",
        details.chars().take(2500).collect::<String>(),goal
    )))
}

/// One owner task, one Rust execution path. No detached source-evolution
/// campaign, repeated generation, source auto-adoption or hidden model training.
pub(crate) async fn run_prompt(context:&JobContext,job:&Value)->Result<Value,String>{
 let input=job.get("input").cloned().unwrap_or_else(||json!({}));
 let prompt=input["prompt"].as_str().unwrap_or("").trim().to_owned();
 if prompt.is_empty(){return Err("작업 입력(prompt)이 비어 있습니다.".into());}
 if input["rsiSourceDir"].is_string()||input["rsiCampaignId"].is_string()
    ||input["rsiNativeInduction"]==true{
    return Ok(json!({"status":"needs_review",
      "reason":"자동 RSI 소스 개선 작업은 종료되었습니다. 일반 코딩 작업으로 다시 요청하세요."}));
 }
 if let Some(result)=job.pointer("/promptState/result"){
    return Ok(result.clone());
 }
 if job.pointer("/promptState/inFlight").and_then(Value::as_bool)==Some(true){
    return Ok(json!({"status":"needs_review",
        "reason":"중단된 프롬프트 작업의 외부 실행 결과를 먼저 확인하세요."}));
 }
 let selected_workspace=if let Some(path)=input["workspaceDir"].as_str(){
    Some(crate::engine::workspace_scope::resolve(&context.state,path)?)
 }else{None};
 let session_id=match job["sessionId"].as_str()
   .or_else(||input["sessionId"].as_str()) {
   Some(id)=>id.to_string(),
   None=>context.state.store.put("session",&json!({
     "title":prompt.chars().take(40).collect::<String>(),
     "agentId":input["agentId"]
   })).and_then(|session|session["id"].as_str().map(str::to_owned))
    .ok_or("작업 세션을 생성하지 못했습니다.")?
 };
 if let Some(workspace)=&selected_workspace {
   let mut session=context.state.store.get("session",&session_id)
     .ok_or("작업 세션을 찾을 수 없습니다.")?;
   session["workspaceDir"]=json!(workspace.to_string_lossy().to_string());
   context.state.store.put("session",&session)
     .ok_or("작업 공간을 세션에 저장하지 못했습니다.")?;
 }
 if let Some(budget)=input["tokenBudget"].as_u64().filter(|v|*v>=1024){
   let baseline=context.state.store.session_usage(&session_id)["totalTokens"]
     .as_u64().unwrap_or(0);
   let mut session=context.state.store.get("session",&session_id)
     .ok_or("토큰 예산용 세션을 찾을 수 없습니다.")?;
   session["jobTokenLimit"]=json!(baseline.saturating_add(budget));
   session["jobTokenOwnerId"]=json!(context.job_id);
   context.state.store.put("session",&session)
     .ok_or("작업별 토큰 예산을 저장하지 못했습니다.")?;
 }
 if input["uiSource"]=="native_sdk_panel"{
   let baseline=context.state.store.session_usage(&session_id);
   let mut session=context.state.store.get("session",&session_id)
     .ok_or("작업 세션을 찾을 수 없습니다.")?;
   session["sdkUiPolicy"]=input["sdkUiPolicy"].clone();
   session["sdkUiPolicyOwnerId"]=json!(&context.job_id);
   session["sdkUiPolicyBaseline"]=json!({
      "calls":baseline["calls"],"totalTokens":baseline["totalTokens"],
   });
   context.state.store.put("session",&session)
     .ok_or("작업 정책을 저장하지 못했습니다.")?;
 }
 let resumed_instruction=resume_owner_instruction(job,&prompt)?
   .unwrap_or_else(||prompt.clone());
 let request=TurnRequest{
   session_id:Some(session_id.clone()),
   input:resumed_instruction,
   agent_id:input["agentId"].as_str().map(str::to_owned),
   provider_id:input["providerId"].as_str().map(str::to_owned),
   model:input["model"].as_str().map(str::to_owned),
   attachment_ids:Vec::new(),
   tools_enabled:input["toolsEnabled"].as_bool().unwrap_or(true),
   tools_execution_enabled:input["toolsExecutionEnabled"].as_bool().unwrap_or(true),
   thinking:input["thinking"].as_bool(),
   auto_learning:false,
   reasoning_effort:input["reasoningEffort"].as_str().map(str::to_owned),
   reasoning_language:None,system_override:None,
   task_id:None,parent_task_id:None,task_revision:None,verification_owner:None,
 };
 let mut scoped_context=context.clone();
 if let Some(workspace)=selected_workspace {
   scoped_context.state=crate::engine::workspace_scope::scope(&context.state,workspace);
 }
 context.checkpoint(json!({"sessionId":session_id,
     "promptState":{"inFlight":true}}))?;
 let result=if input["autonomous"]!=false {
   crate::engine::autonomy::execute(&scoped_context,&request,&prompt,&input).await?
 }else{
   let message=run_quiet(&scoped_context.state,&request).await?;
   json!({"sessionId":message.as_ref().and_then(|m|m.get("session_id")),
      "messageId":message.as_ref().and_then(|m|m.get("id"))})
 };
 context.checkpoint(json!({"promptState":{"inFlight":false,"result":result}}))?;
 Ok(result)
}

/// Resumes a cleanly interrupted run after a crash. The owner message already
/// exists, so this path uses the no-record-input turn instead of run_prompt.
async fn run_resume(context: &JobContext, job: &Value) -> Result<Value, String> {
    let input = job.get("input").cloned().unwrap_or_else(|| json!({}));
    if let Some(result) = job.pointer("/promptState/result") {
        return Ok(result.clone());
    }
    if job
        .pointer("/promptState/inFlight")
        .and_then(Value::as_bool)
        == Some(true)
    {
        return Ok(json!({
            "status":"needs_review",
            "reason":"재개 작업의 이전 시도 결과를 확인해야 합니다."
        }));
    }
    let prompt = input["prompt"].as_str().unwrap_or("").to_string();
    let session_id = input["sessionId"]
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or("재개 작업에 sessionId가 필요합니다.")?
        .to_string();
    if prompt.trim().is_empty() {
        return Err("재개 작업의 원래 요청이 비어 있습니다.".into());
    }
    if context.state.store.get("session", &session_id).is_none() {
        return Err("재개 작업의 세션을 찾을 수 없습니다.".into());
    }
    context.checkpoint(json!({"sessionId":session_id,"promptState":{"inFlight":true}}))?;
    let request = TurnRequest {
        session_id: Some(session_id.clone()),
        input: prompt,
        attachment_ids: Vec::new(),
        agent_id: input["agentId"].as_str().map(str::to_string),
        provider_id: input["providerId"].as_str().map(str::to_string),
        model: input["model"].as_str().map(str::to_string),
        tools_enabled: input["toolsEnabled"].as_bool().unwrap_or(true),
        tools_execution_enabled: input["toolsExecutionEnabled"].as_bool().unwrap_or(true),
        thinking: None,
        auto_learning: false,
        reasoning_effort: None,
        reasoning_language: None,
        system_override: None,
        task_id: input["taskId"].as_str().map(str::to_string),
        parent_task_id: None,
        task_revision: input["taskRevision"].as_str().map(str::to_string),
        verification_owner: None,
    };
    let message = crate::engine::run_quiet_resume(&context.state, &request).await?;
    let result = json!({
        "sessionId": session_id,
        "messageId": message.as_ref().and_then(|item| item.get("id")),
        "resumedExecutionId": input["resumedExecutionId"],
    });
    context.checkpoint(json!({"promptState":{"inFlight":false,"result":result}}))?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::db::Store;
    use crate::vault::Vault;

    fn temp_state() -> AppState {
        let dir = std::env::temp_dir().join(format!("dominion-jobs-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let store = Store::open(&config.db_path()).unwrap();
        let vault = Vault::open(&config.key_path());
        AppState::new(config, store, vault)
    }

    #[test]
    fn owner_authorized_resume_does_not_replay_the_original_goal_as_new_input(){
        let job=json!({"resumeCount":1,"resumeEvidence":{
            "source":"owner_attestation","applied":false,
            "details":"The remote job was NOT created"
        }});
        let prompt=resume_owner_instruction(&job,"Create remote job").unwrap().unwrap();
        assert!(prompt.contains("The remote job was NOT created"));
        assert!(prompt.contains("Create remote job"));
        assert!(prompt.contains("다시 확인"));
        assert_ne!(prompt,"Create remote job");
        assert!(resume_owner_instruction(&json!({}),"New goal").unwrap().is_none());
        let no_tools=json!({"resumeCount":1,"resumeEvidence":{
            "source":"engine_no_tool_observations","applied":false,
            "details":"Provider error with no tool observations"
        }});
        assert!(resume_owner_instruction(&no_tools,"Finish the report").unwrap()
            .unwrap().contains("Provider error"));
        assert!(resume_owner_instruction(&json!({"resumeCount":1}),"Old goal").is_err());
        assert!(resume_owner_instruction(&json!({"resumeCount":1,"resumeEvidence":{
            "source":"owner_attestation","applied":true,"details":"Already done"
        }}),"Goal").is_err());
    }

    #[test]
    fn owner_steering_is_revisioned_and_never_rewrites_user_job_input(){
        let state=temp_state();
        let job=enqueue(&state,"prompt",json!({"prompt":"Write a patch","autonomous":true})).unwrap();
        let id=job["id"].as_str().unwrap();
        let first=steer(&state,id,"Run the real tests").unwrap();
        let second=steer(&state,id,"Do not overwrite untouched files").unwrap();
        assert_eq!(first["steeringRevision"],1);
        assert_eq!(second["steeringRevision"],2);
        let stored=state.store.get("job",id).unwrap();
        assert_eq!(stored["input"]["prompt"],"Write a patch");
        assert_eq!(stored["steering"].as_array().unwrap().len(),2);
        assert_eq!(stored["steering"][1]["instruction"],"Do not overwrite untouched files");
        state.store.patch_if("job",id,"status",&json!("queued"),&json!({"status":"cancelled"}));
        assert!(matches!(steer(&state,id,"Proceed"),Err(SteeringError::Conflict(_))));
    }

    #[test]
    fn replacement_is_durable_and_still_uses_existing_steering_gate(){
        let state=temp_state();
        let job=enqueue(&state,"prompt",json!({"prompt":"Old task","autonomous":true})).unwrap();
        let id=job["id"].as_str().unwrap();
        assert_eq!(steer(&state,id,"Extra constraint").unwrap()["mode"],"append");
        let changed=steer_with_mode(&state,id,"New task","replace").unwrap();
        assert_eq!(changed["mode"],"replace");
        assert_eq!(changed["steeringRevision"],2);
        let saved=state.store.get("job",id).unwrap();
        assert_eq!(saved["input"]["prompt"],"Old task");
        assert_eq!(saved["steering"][1]["mode"],"replace");
        assert_eq!(saved["steeringGate"]["revision"],2);
        assert!(matches!(steer_with_mode(&state,id,"invalid","oops"),
            Err(SteeringError::BadInput(_))));
    }

    #[test]
    fn steering_and_completion_cannot_both_commit_a_stale_version() {
        let state=temp_state();
        let job=enqueue(&state,"prompt",json!({"prompt":"Fix and verify","autonomous":true})).unwrap();
        let id=job["id"].as_str().unwrap();
        let stale=job.get("steeringGate").unwrap_or(&Value::Null).clone();
        assert_eq!(steer(&state,id,"Also run tests").unwrap()["steeringRevision"],1);
        assert!(state.store.patch_if("job",id,"steeringGate",&stale,
            &json!({"steeringGate":{"revision":0,"closed":true}})).is_none());
        let fresh=state.store.get("job",id).unwrap();
        assert_eq!(fresh["steeringGate"]["revision"],1);
        state.store.patch_if("job",id,"steeringGate",&fresh["steeringGate"],
            &json!({"steeringGate":{"revision":1,"closed":true}})).unwrap();
        assert!(matches!(steer(&state,id,"A late addition"),
            Err(SteeringError::Conflict(_))));
        assert_eq!(state.store.get("job",id).unwrap()["steering"].as_array().unwrap().len(),1);
    }
    #[test]
    fn dispatcher_wakes_only_for_queued_job_record(){
        assert!(queued_job_event(&json!({"type":"record","kind":"job",
            "record":{"status":"queued"}})));
        assert!(!queued_job_event(&json!({"type":"record","kind":"job",
            "record":{"status":"running"}})));
        assert!(!queued_job_event(&json!({"type":"record","kind":"money_signal",
            "record":{"status":"queued"}})));
    }

    #[test]
    fn recovery_reconciles_operation_without_explicit_session_id(){
        let state=temp_state();
        state.store.put("job",&json!({"id":"recovery-owner","kind":"prompt",
            "status":"running","attempts":1,
            "input":{"sessionId":"s-owner","prompt":"external action"}}));
        state.store.put("execution",&json!({"id":"owner-run",
            "status":"interrupted","sessionId":"s-owner"}));
        state.store.put("execution_operation",&json!({"id":"owner-tool","runId":"owner-run",
            "status":"interrupted","outcomeUnknown":true}));
        recover(&state);
        assert_eq!(state.store.get("job","recovery-owner").unwrap()["status"],
            "needs_review");
    }

    #[test]
    fn enqueue_creates_a_queued_job() {
        let state = temp_state();
        let job = enqueue(
            &state,
            "prompt",
            json!({ "prompt": "hi" }),
        )
        .expect("job");
        assert_eq!(job["status"], "queued");
        assert_eq!(job["kind"], "prompt");
        assert_eq!(job["stepTotal"], 0);

        let queued = next_queued(&state).expect("worker sees it");
        assert_eq!(queued["id"], job["id"]);

        let mut running = queued.clone();
        running["status"] = json!("running");
        state.store.put("job", &running);
        assert!(next_queued(&state).is_none());
    }

    #[test]
    fn learning_jobs_are_priority_last_and_serialized() {
        let state = temp_state();
        let learn1 = enqueue(&state, "learn", json!({"sessionId":"s"})).unwrap();
        let learn2 = enqueue(&state, "learn", json!({"sessionId":"t"})).unwrap();
        let prompt = enqueue(&state, "prompt", json!({"prompt":"interactive"})).unwrap();

        let first = next_queued(&state).expect("prompt is picked first");
        assert_eq!(first["id"], prompt["id"]);
        state.store.patch_if(
            "job",
            prompt["id"].as_str().unwrap(),
            "status",
            &json!("queued"),
            &json!({"status":"running"}),
        );

        let second = next_queued(&state).expect("one learning job runs");
        assert!(second["id"] == learn1["id"] || second["id"] == learn2["id"]);
        state.store.patch_if(
            "job",
            second["id"].as_str().unwrap(),
            "status",
            &json!("queued"),
            &json!({"status":"running"}),
        );

        assert!(
            next_queued(&state).is_none(),
            "a second learning job must wait while one is running"
        );
        let _ = std::fs::remove_dir_all(&state.config.data_dir);
    }

    #[test]
    fn retired_legacy_generation_falls_back_to_owner_review(){
        let state=temp_state();
        let job=state.store.put("job",&json!({
            "kind":"rsi_generation","status":"queued","input":{}
        })).unwrap();
        let current=state.store.get("job",job["id"].as_str().unwrap()).unwrap();
        assert!(matches!(current["status"].as_str(),Some("queued")));
        assert_eq!(current["kind"],"rsi_generation");
    }

    #[test]
    fn recover_requeues_running_jobs() {
        let state = temp_state();
        let job = enqueue(
            &state,
            "prompt",
            json!({ "prompt": "w" }),
        )
        .unwrap();
        let mut running = job.clone();
        running["status"] = json!("running");
        running["attempts"] = json!(1);
        state.store.put("job", &running);
        recover(&state);
        let stored = state.store.get("job", job["id"].as_str().unwrap()).unwrap();
        assert_eq!(stored["status"], "queued");
        assert_eq!(stored["recovered"], true);

        running["attempts"] = json!(3);
        running["status"] = json!("running");
        state.store.put("job", &running);
        recover(&state);
        let stored = state.store.get("job", job["id"].as_str().unwrap()).unwrap();
        assert_eq!(stored["status"], "needs_review");
    }

    #[test]
    fn startup_recovery_never_requeues_unsealed_tool_even_without_old_unknown_flag(){
        let state=temp_state();
        state.store.put("job",&json!({
            "id":"pre-crash-job","kind":"prompt","status":"running","attempts":1,
            "sessionId":"s","input":{"sessionId":"s","prompt":"Perform external update"},
            "promptState":{"inFlight":false},
        }));
        state.store.put("execution",&json!({
            "id":"pre-crash-run","sessionId":"s","status":"running",
        }));
        state.store.put("execution_operation",&json!({
            "id":"pre-crash-tool","runId":"pre-crash-run","sessionId":"s",
            "status":"running","effectState":"started","tool":"github_api",
            "arguments":{"operation":"push"},"outcomeUnknown":false,
        }));
        recover(&state);
        let job=state.store.get("job","pre-crash-job").unwrap();
        assert_eq!(job["status"],"needs_review",
            "in-flight tool was requeued before being classified unknown");
        let tool=state.store.get("execution_operation","pre-crash-tool").unwrap();
        assert_eq!(tool["status"],"interrupted");
        assert_eq!(tool["outcomeUnknown"],true);
        recover(&state);
        assert_eq!(state.store.get("job","pre-crash-job").unwrap()["status"],"needs_review");
    }

    #[test]
    fn recover_does_not_replay_an_execution_with_unknown_external_effects() {
        let state = temp_state();
        let job = enqueue(
            &state,
            "prompt",
            json!({ "prompt": "w", "sessionId": "session-1" }),
        )
        .unwrap();
        let mut running = job.clone();
        running["status"] = json!("running");
        running["attempts"] = json!(1);
        // No inFlight flag: the evidence must come from the operation record.
        state.store.put("job", &running);
        state.store.put(
            "execution",
            &json!({"id":"run-1","sessionId":"session-1","status":"interrupted"}),
        );
        state.store.put(
            "execution_operation",
            &json!({
                "id":"run-1:0:0:call","runId":"run-1","sessionId":"session-1",
                "tool":"shell_exec","status":"interrupted","outcomeUnknown":true
            }),
        );
        recover(&state);
        let stored = state.store.get("job", job["id"].as_str().unwrap()).unwrap();
        assert_eq!(
            stored["status"], "needs_review",
            "an unknown external result must not be replayed: {stored}"
        );
        assert!(stored["error"].as_str().is_some_and(|text| text.contains("확정")));
    }

    #[test]
    fn stale_worker_context_never_overwrites_requeued_attempt(){
        let state=temp_state();
        let job=enqueue(&state,"prompt",json!({"prompt":"fix target"})).unwrap();
        let id=job["id"].as_str().unwrap().to_string();
        state.store.patch_if("job",&id,"status",&json!("queued"),
            &json!({"status":"running","attempts":1})).unwrap();
        let old=JobContext::new(state.clone(),id.clone());
        old.checkpoint(json!({"progress":"first"})).unwrap();
        state.store.patch_if("job",&id,"status",&json!("running"),
            &json!({"status":"queued"})).unwrap();
        state.store.patch_if("job",&id,"status",&json!("queued"),
            &json!({"status":"running","attempts":2})).unwrap();
        let fresh=JobContext::new(state.clone(),id.clone());
        assert!(old.checkpoint(json!({"progress":"stale"})).is_err());
        assert!(!finalize_job(&old,"prompt","completed",
            json!({"status":"completed","result":{"verified":true}})));
        fresh.checkpoint(json!({"progress":"new"})).unwrap();
        let saved=state.store.get("job",&id).unwrap();
        assert_eq!(saved["progress"],"new");
        assert!(saved["result"].is_null());
        assert_eq!(saved["status"],"running");
    }

    #[test]
    fn deferred_attempt_reused_count_still_rejects_stale_job_context(){
        let state=temp_state();
        let job=enqueue(&state,"prompt",json!({"prompt":"deferred"})).unwrap();
        let id=job["id"].as_str().unwrap().to_string();
        state.store.patch_if("job",&id,"status",&json!("queued"),
            &json!({"status":"running","attempts":1,"runEpoch":1})).unwrap();
        let original=JobContext::new(state.clone(),id.clone());
        original.checkpoint(json!({"status":"queued","attempts":0,
            "availableAt":"2026-11-17T00:00:00Z"})).unwrap();
        let current=next_queued(&state).unwrap();
        assert_eq!(current["attempts"],1);
        assert_eq!(current["runEpoch"],2);
        let replacement=JobContext::new(state.clone(),id.clone());
        assert!(original.checkpoint(json!({"status":"completed","result":"stale"})).is_err());
        replacement.checkpoint(json!({"status":"completed","result":"latest"})).unwrap();
        let saved=state.store.get("job",&id).unwrap();
        assert_eq!(saved["result"],"latest");
    }

    #[test]
    fn cancelled_attempt_cannot_store_terminal_success(){
        let state=temp_state();
        let job=enqueue(&state,"prompt",json!({"prompt":"cancel"})).unwrap();
        let id=job["id"].as_str().unwrap().to_string();
        state.store.patch_if("job",&id,"status",&json!("queued"),
            &json!({"status":"running","attempts":1})).unwrap();
        let ctx=JobContext::new(state.clone(),id.clone());
        let cancelled=cancel_job(&state,&id).unwrap();
        assert_eq!(cancelled["status"],"cancelled");
        assert!(ctx.checkpoint(json!({"status":"completed","result":"late"})).is_err());
        assert!(matches!(cancel_job(&state,&id),Err(CancelJobError::Conflict)));
        assert_eq!(state.store.get("job",&id).unwrap()["status"],"cancelled");
    }
    #[test]
    fn cancellation_wins_against_late_terminal_receipt_without_false_success() {
        let state=temp_state();
        let job=enqueue(&state,"prompt",json!({"prompt":"demo"})).unwrap();
        let id=job["id"].as_str().unwrap().to_string();
        state.store.patch_if("job",&id,"status",&json!("queued"),
            &json!({"status":"cancelled","cancelRequested":true})).unwrap();
        let context=JobContext::new(state.clone(),id.clone());
        assert!(!finalize_job(&context,"prompt","completed",
            json!({"status":"completed","result":{"verified":true}})));
        assert_eq!(state.store.get("job",&id).unwrap()["status"],"cancelled");
        assert!(state.store.get("job",&id).unwrap()["result"].is_null());
        assert!(!finalize_job(&context,"prompt","failed",
            json!({"status":"failed","error":"late provider error"})));
        assert_eq!(state.store.get("job",&id).unwrap()["status"],"cancelled");
    }

    #[test]
    fn job_errors_are_recorded_as_failed() {
        let state = temp_state();
        let queued = enqueue(&state, "room", json!({ "roomId": "missing", "goal": "x" })).unwrap();
        let id = queued["id"].as_str().unwrap().to_string();
        let job = state.store.patch_if("job",&id,"status",&json!("queued"),
            &json!({"status":"running","attempts":1})).unwrap();
        let _ = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(run_job(state.clone(), job));
        let stored = state.store.get("job", &id).unwrap();
        assert_eq!(stored["status"], "failed");
        assert!(stored["error"].as_str().unwrap().contains("대화방"));
    }

}
