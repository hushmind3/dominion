use crate::engine::chat::TurnRequest;
use crate::engine::run_quiet;
use crate::engine::{learning, reflect, rooms, workflows};
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
    events: Arc<Mutex<Vec<Value>>>,
}

impl JobContext {
    pub fn new(state: AppState, job_id: String) -> Self {
        Self {
            state,
            job_id,
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

    pub fn checkpoint(&self, patch: Value) -> Result<(), String> {
        let job = self
            .state
            .store
            .get("job", &self.job_id)
            .ok_or("작업 체크포인트를 찾을 수 없습니다.")?;
        if job["cancelRequested"].as_bool() == Some(true) {
            return Err("사용자 요청으로 실행이 중단되었습니다.".into());
        }
        let saved = self
            .state
            .store
            .patch_if(
                "job",
                &self.job_id,
                "cancelRequested",
                job.get("cancelRequested").unwrap_or(&Value::Null),
                &patch,
            )
            .ok_or("작업 체크포인트가 변경되었거나 실행이 중단되었습니다.")?;
        self.state.bus.record("job", &saved);
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

pub fn cancel_rsi_children(state: &AppState, campaign: &str) {
    for child in state.store.all("job") {
        if child["kind"] != "rsi_generation" || child["input"]["rsiCampaignId"] != campaign
            || !matches!(child["status"].as_str(), Some("queued" | "running")) { continue; }
        if let Some(session) = child["sessionId"].as_str() { state.cancel_session(session); }
        if let Some(saved) = child["id"].as_str().and_then(|id| state.store.patch_if(
            "job", id, "status", &child["status"], &json!({"status":"cancelled",
                "cancelRequested":true,"finishedAt":chrono::Utc::now().to_rfc3339(),
                "error":"상위 자기개선 작업이 종료되어 세대 실행을 중단했습니다."}))) {
            state.bus.record("job", &saved);
        }
    }
}

/// Recovers work left behind by a crash: running jobs are re-queued (unless
/// they already exhausted retries) and running executions become interrupted.
pub fn recover(state: &AppState) {
    for job in state.store.all("job") {
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
        let unknown_external_result = job_session.is_some_and(|session| {
            state.store.all("execution_operation").iter().any(|operation| {
                // Crash recovery previously inspected outcomeUnknown BEFORE
                // marking unfinished operations unknown. This could requeue
                // a job even while its pre-crash tool was still "running".
                let unsettled=operation["outcomeUnknown"]==true
                    || operation["status"]=="running"
                    || matches!(operation["effectState"].as_str(),
                        Some("prepared"|"started"|"effect_unknown"|"needs_review"));
                unsettled && (
                    operation["sessionId"].as_str()==Some(session)
                    || operation["runId"].as_str()
                        .and_then(|run_id|state.store.get("execution",run_id))
                        .is_some_and(|run|run["sessionId"].as_str()==Some(session))
                )
            })
        });
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
    // A detached generation cannot be replayed without its live coordinator.
    for child in state.store.all("job") {
        if child["kind"] != "rsi_generation" { continue; }
        if let Some(parent) = child["input"]["rsiCampaignId"].as_str() {
            if state.store.get("job", parent).is_none_or(|job|
                !matches!(job["status"].as_str(), Some("queued" | "running"))) {
                cancel_rsi_children(state, parent);
            }
        }
    }
    for execution in state.store.all("execution") {
        if execution.get("status").and_then(|value| value.as_str()) == Some("running") {
            let mut updated = execution.clone();
            updated["status"] = json!("interrupted");
            updated["phase"] = json!("interrupted");
            updated["phaseUpdatedAt"] = json!(chrono::Utc::now().to_rfc3339());
            updated["taskOutcome"] = json!("cancelled");
            updated["reason"] = json!("백엔드가 재시작되어 실행이 중단되었습니다.");
            updated["finishedAt"] = json!(chrono::Utc::now().to_rfc3339());
            state.store.put("execution", &updated);
        }
    }
    for mut operation in state.store.all("execution_operation") {
        if operation["status"] == "running" {
            operation["status"] = json!("interrupted");
            operation["outcomeUnknown"] = json!(true);
            operation["phase"] = json!("interrupted");
            operation["reason"] = json!("백엔드가 재시작되어 도구 결과를 확정할 수 없습니다.");
            operation["finishedAt"] = json!(chrono::Utc::now().to_rfc3339());
            state.store.put("execution_operation", &operation);
        }
    }
    // After interrupted executions have been recorded, expire durable approvals
    // that outlived their live waiter before wake decisions are made.
    crate::engine::chat::expire_stale_approvals(state);
}

/// Starts the durable job worker plus the automation scheduler.
/// Requeues learning jobs that failed because the learning model was
/// unavailable. The work is kept, not lost, and a later worker picks it up.
pub fn retry_failed_learning(state: &AppState) -> usize {
    let mut retried = 0;
    for job in state.store.all("job") {
        if job["kind"].as_str() != Some("learn")
            || job["status"].as_str() != Some("failed")
        {
            continue;
        }
        let attempts = job["attempts"].as_u64().unwrap_or(0);
        if attempts >= 3 {
            continue;
        }
        let Some(id) = job["id"].as_str() else {
            continue;
        };
        let delay = 300i64.saturating_mul(attempts.max(1) as i64);
        let available = chrono::Utc::now() + chrono::Duration::seconds(delay);
        if let Some(saved) = state.store.patch_if(
            "job",
            id,
            "status",
            &json!("failed"),
            &json!({
                "status": "queued",
                "availableAt": available.to_rfc3339(),
                "retriedAt": chrono::Utc::now().to_rfc3339(),
                "lastError": job["error"],
                "error": Value::Null,
            }),
        ) {
            state.bus.record("job", &saved);
            state.store.audit("job.learning_retry", json!({"jobId": id, "attempts": attempts}));
            retried += 1;
        }
    }
    retried
}

pub fn start(state: AppState) {
    crate::engine::channel_adapter::start(state.clone());
    let workers = std::env::var("DOMINION_JOB_WORKERS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(4)
        .clamp(1, 16);
    for _ in 0..workers {
        let worker_state = state.clone();
        tokio::spawn(async move {
            loop {
                match next_queued(&worker_state) {
                    Some(job) => run_job(worker_state.clone(), job).await,
                    None => tokio::time::sleep(Duration::from_millis(400)).await,
                }
            }
        });
    }

    let maintenance_state = state.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(30)).await;
            schedule_automations(&state);
            // A process restart removes in-memory approval timers. The persisted
            // deadline still expires automatically during normal operation.
            crate::engine::chat::expire_stale_approvals(&state);
        }
    });

    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(300)).await;
            retry_failed_learning(&maintenance_state);
            crate::engine::agent_growth::sweep(&maintenance_state);
            crate::engine::distill::sweep(&maintenance_state);
            crate::engine::memory::learning_sweep(&maintenance_state);
        }
    });
}

fn next_queued(state: &AppState) -> Option<Value> {
    let jobs = state.store.all("job");
    let learning_running = jobs
        .iter()
        .any(|job| job["status"] == "running" && job["kind"] == "learn");
    let comparison_running = jobs.iter().any(|job|
        job["status"]=="running"&&job["kind"]=="rsi_compare");
    let mut queued = jobs
        .iter()
        .cloned()
        .filter(|job| job.get("status").and_then(|value| value.as_str()) == Some("queued"))
        // Learning is background work: at most one at a time, so it can never
        // occupy every worker while a prompt/room/workflow job waits.
        .filter(|job| !(job["kind"] == "learn" && learning_running))
        .filter(|job| !(job["kind"] == "rsi_compare" && comparison_running))
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
        let priority = |job: &Value| u8::from(matches!(job["kind"].as_str(),Some("learn"|"rsi_compare")));
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
            &json!({"status":"running","startedAt":chrono::Utc::now().to_rfc3339(),"attempts":job["attempts"].as_u64().unwrap_or(0)+1}),
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
        "workflow" => workflows::run(&context, &job).await,
        "room" | "meeting" => rooms::run(&context, &job).await,
        "prompt" => run_prompt(&context, &job).await,
        "rsi_compare" => crate::engine::rsi_worktree::compare(&context,&job).await,
        "rsi_evolve" => crate::engine::rsi_evolve::run(&context,&job).await,
        "resume" => run_resume(&context, &job).await,
        "learn" => learning::run(&context, &job).await,
        "reflect" => reflect::run(&context, &job).await,
        "discord" => crate::engine::discord::run(&context, &job).await,
        "video" => crate::engine::video::run(&context, &job).await,
        "youtube_upload" => crate::engine::video::upload(&context, &job).await,
        other => Err(format!("지원하지 않는 작업 종류입니다: {other}")),
    };

    match result {
        Ok(result) => {
            let status = result
                .get("status")
                .and_then(Value::as_str)
                .filter(|status| matches!(*status, "needs_review" | "cancelled" | "failed"))
                .unwrap_or("completed");
            context.patch(json!({
                "status": status,
                "result": result,
                "finishedAt": chrono::Utc::now().to_rfc3339(),
                "events": context.events.lock().unwrap().clone(),
            }));
            state.store.audit(
                &format!("job.{status}"),
                json!({ "jobId": job_id, "kind": kind }),
            );
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
            context.patch(json!({
                "status": status,
                "error": error,
                "finishedAt": chrono::Utc::now().to_rfc3339(),
                "events": context.events.lock().unwrap().clone(),
            }));
            state.store.audit(
                "job.failed",
                json!({ "jobId": job_id, "kind": kind, "error": error }),
            );
        }
    }
    if kind == "rsi_evolve" {
        if state.store.get("job", &job_id).is_some_and(|j|
            !matches!(j["status"].as_str(), Some("queued" | "running"))) {
            cancel_rsi_children(&state, &job_id);
        }
        if job["input"]["rsiApplyWhenFinished"] == true
            && state.store.get("job", &job_id).is_some_and(|j| j["status"] == "completed") {
            if let Some(campaign) = state.store.get("rsi_campaign", &job_id).filter(|c|
                c["status"] == "completed" && c["headCommit"].is_string()
                    && c["headCommit"] != c["baseCommit"]) {
                match crate::engine::dev::start_rsi_install(state.clone(), &job_id,
                    campaign["headCommit"].as_str().unwrap_or("")) {
                    Ok(install) => { state.store.patch_if("rsi_campaign", &job_id,
                        "headCommit", &campaign["headCommit"], &json!({"installJobId":install["id"]})); }
                    Err(error) => { state.store.patch_if("rsi_campaign", &job_id,
                        "headCommit", &campaign["headCommit"], &json!({"applicationError":error})); }
                }
            }
        }
    }
}

/// The verification controller is generated by DOMINION from completed RSI
/// outcomes. A model cannot override the bounded retry count mid-execution.
fn rsi_edit_repair_limit(input:&Value)->usize {
    input["rsiEditPolicy"]["repairLimit"].as_u64().unwrap_or(2).min(3) as usize
}

/// Explicitly opted-in source search may try a native operator before a model
/// call, including its first generation. Fixed owner file edits and inherited
/// patches stay model-led. A native proposal still needs compilation and A/B.
fn rsi_native_preflight(input: &Value) -> bool {
    input["rsiNativeInduction"]==true
        && input["rsiAutoFocus"]==true
        && !input["rsiInheritedCandidateId"].is_string()
        && !input["rsiFailedPatchRecovery"]["candidateId"].is_string()
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

pub(crate) async fn run_prompt(context: &JobContext, job: &Value) -> Result<Value, String> {
    let input = job.get("input").cloned().unwrap_or_else(|| json!({}));
    let prompt = input
        .get("prompt")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    if prompt.trim().is_empty() {
        return Err("작업 입력(prompt)이 비어 있습니다.".into());
    }
    if let Some(result) = job.pointer("/promptState/result") {
        return Ok(result.clone());
    }
    if job
        .pointer("/promptState/inFlight")
        .and_then(Value::as_bool)
        == Some(true)
    {
        return Ok(
            json!({"status":"needs_review","reason":"중단된 프롬프트 작업 결과를 확인해야 합니다."}),
        );
    }
    // A selected project is validated again at claim time: it may have been
    // moved or removed since the request was queued. Each job gets a cloned
    // configuration; the global workspace and other jobs do not change.
    let selected_workspace=if let Some(path)=input["workspaceDir"].as_str(){
        if input["rsiSourceDir"].is_string(){
            return Err("일반 작업 폴더 선택과 RSI 분리 작업공간을 함께 지정할 수 없습니다.".into());
        }
        Some(crate::engine::workspace_scope::resolve(&context.state,path)?)
    }else{None};
    let rsi_workspace = if input["rsiSourceDir"].is_string() {
        Some(crate::engine::rsi_worktree::prepare(context,&input).await?)
    } else { None };
    // The dedicated source checkout is validated and detached before a model
    // call, never modified directly by the RSI execution.
    let session_id = job
        .get("sessionId")
        .or_else(|| input.get("sessionId"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let session_id = match session_id {
        Some(id) => id,
        None => context.state.store.put("session",&json!({"title":prompt.chars().take(40).collect::<String>(),"agentId":input.get("agentId")}))
            .and_then(|session|session["id"].as_str().map(str::to_string)).ok_or("작업 세션을 저장할 수 없습니다.")?,
    };
    if let Some(workspace)=&selected_workspace{
        if let Some(mut session)=context.state.store.get("session",&session_id){
            session["workspaceDir"]=json!(workspace.to_string_lossy().to_string());
            context.state.store.put("session",&session)
                .ok_or("선택한 작업 폴더를 세션에 저장할 수 없습니다.")?;
        }
    }
    // The budget is owned by this ONE job, not a global agent/session cap.
    // Prior conversation usage is excluded; other chat turns cannot inherit
    // an exhausted ceiling after this job reaches a terminal state.
    if let Some(budget)=input["tokenBudget"].as_u64().filter(|v|*v>=1024){
        let baseline=context.state.store.session_usage(&session_id)["totalTokens"]
            .as_u64().unwrap_or(0);
        let mut session=context.state.store.get("session",&session_id)
            .ok_or("토큰 예산을 적용할 세션이 없습니다.")?;
        session["jobTokenLimit"]=json!(baseline.saturating_add(budget));
        session["jobTokenOwnerId"]=json!(context.job_id);
        context.state.store.put("session",&session)
            .ok_or("작업 토큰 예산을 저장할 수 없습니다.")?;
    }
    if input["uiSource"]=="native_sdk_panel" {
        let mut session=context.state.store.get("session",&session_id)
            .ok_or("사용자 작업 세션을 찾을 수 없습니다.")?;
        session["sdkUiPolicy"]=input["sdkUiPolicy"].clone();
        context.state.store.put("session",&session)
            .ok_or("사용자 작업 정책을 저장할 수 없습니다.")?;
    }
    if rsi_workspace.is_some() {
        if let Some(mut session)=context.state.store.get("session",&session_id) {
            session["rsiRun"]=json!(true);
            session["rsiSystemEvolution"]=input["rsiSystemEvolution"].clone();
            session["rsiLocalOnly"]=input["rsiLocalOnly"].clone();
            session["rsiApprovedProviderId"]=input["providerId"].clone();
            session["rsiApprovedModel"]=input["model"].clone();
            session["rsiEpisodeTokenLimit"]=input["rsiEpisodeTokenLimit"].clone();
            session["rsiObjective"]=input["rsiObjective"].clone();
            session["rsiFocusFiles"]=input["rsiFocusFiles"].clone();
            // The improvement algorithm, not the model, selects exploration
            // budget and preferred source-edit operator for this generation.
            session["rsiEditPolicy"]=input["rsiEditPolicy"].clone();
            session["rsiAutoFocus"]=input["rsiAutoFocus"].clone();
            // The task classifier uses the owner's actual target, not the
            // generic RSI prompt or a guessed source file.
            session["rsiExplicitFocus"]=json!(input["rsiAutonomous"] != true
                && input["rsiFocusFiles"].as_array().is_some_and(|files| !files.is_empty()));
            session["rsiFocusContextReady"]=json!(
                input["rsiFocusContextChars"].as_u64().unwrap_or(0) > 0
                && input["rsiContextPlan"]["sourceExcerptDeferred"] != true);
            crate::engine::rsi_evolve::configure_method_session(&context.state, &input, &mut session);
            // A neutral candidate is restored only inside the detached worker.
            // The coordinator index still describes the pristine parent tree.
            // Re-index the ACTUAL inherited bytes before code_search, otherwise
            // symbol locations and source ownership point to the wrong revision.
            if input["rsiInheritedCandidateId"].is_string() {
                let source=rsi_workspace.as_ref().expect("RSI worktree exists");
                let indexed=crate::engine::code_index::refresh(
                    &context.state,&source.path)?;
                session["rsiCodeIndexId"]=indexed["id"].clone();
                session["rsiSourceDir"]=indexed["sourceDir"].clone();
            } else {
                session["rsiCodeIndexId"]=input["rsiIndexId"].clone();
                session["rsiSourceDir"]=input["rsiSourceDir"].clone();
            }
            if let Some(id)=input["rsiCampaignId"].as_str(){
                session["rsiCampaignId"]=json!(id);
                session["rsiGenerationJobId"]=json!(context.job_id);
            }
            context.state.store.put("session",&session);
        }
    }
    let resumed_instruction=resume_owner_instruction(job,&prompt)?;
    let reused_research=if let Some(worktree)=&rsi_workspace {
        crate::engine::rsi_evolve::reuse_research(&context.state,&input,&worktree.path,&session_id)
    }else{String::new()};
    context.checkpoint(json!({"sessionId":session_id,"promptState":{"inFlight":true}}))?;
    let request = TurnRequest {
        session_id: Some(session_id.clone()),
        input: if rsi_workspace.is_some() {
            let focused = input["rsiAutonomous"] != true
                && input["rsiFocusFiles"].as_array().is_some_and(|files| !files.is_empty());
            let completion = if focused {
                "명시적 수정 목표를 조사 보고로 대체하지 마세요. 충분한 소스 원문이 확보되면 작은 수정은 workspace_read_range 결과의 줄 번호와 sourceSha256으로 workspace_replace_range를 사용해 바뀔 코드만 제출하세요. 정확한 oldText가 있으면 workspace_replace도 사용하세요. 수정을 수행할 수 없을 때에는 막힌 정확한 이유를 설명하세요."
            } else {
                "개선 근거가 있으면 workspace_replace_range(읽은 줄과 sourceSha256) 또는 workspace_replace로 변경하고, 근거가 없다면 변경 없이 이유를 보고하세요."
            };
            format!("{prompt}{reused_research}\n\n현재 대상은 별도의 Git 작업공간입니다. 제공된 검증 원문과 인덱스는 바로 활용하고, 빠진 정보만 도구로 조회하세요. {completion} 절대경로 파일 도구, 셸, Git API, 다른 에이전트는 사용하지 마세요. 빌드·타입 검사와 패치 수집은 실행 엔진이 종료 후 별도로 처리합니다.")
        } else { resumed_instruction.unwrap_or_else(||prompt.clone()) },
        agent_id: input
            .get("agentId")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        provider_id: input
            .get("providerId")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        model: input
            .get("model")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        attachment_ids: Vec::new(),
        tools_enabled: input["toolsEnabled"].as_bool().unwrap_or(true),tools_execution_enabled: input["toolsExecutionEnabled"].as_bool().unwrap_or(true),
        thinking: input["thinking"].as_bool(),
        auto_learning: false,
        reasoning_effort: input["reasoningEffort"].as_str().map(str::to_string),
        reasoning_language: None,
        system_override: None,
        task_id: None,
        parent_task_id: None,
        task_revision: None,
        verification_owner: None,
    };

    // A detached worktree protects the running DOMINION checkout. The worker
    // captures the real diff and verifier evidence before removing that copy.
    let mut scoped_state = context.state.clone();
    if let Some(worktree)=&rsi_workspace {scoped_state.config.workspace=worktree.path.clone();}
    else if let Some(workspace)=selected_workspace {
        scoped_state=crate::engine::workspace_scope::scope(&context.state,workspace);
    }
    // An independently accepted ancestor already supplied an executable
    // improvement procedure and measured next-domain target. Run that
    // procedure FIRST, rather than letting a new model turn overwrite its
    // clean worktree before it can be evaluated. All writes are isolated;
    // compilation and paired holdouts are still required for adoption.
    let preflight_native=if rsi_native_preflight(&input) {
        if let Some(worktree)=&rsi_workspace {
            match crate::engine::rsi_mutation::induce(
                &context.state,&context.job_id,&input,
                &worktree.source,&worktree.path
            ) {
                Ok(Some(receipt))=>{
                    context.state.store.audit("rsi.native_descendant_preflight",
                        json!({"jobId":context.job_id,
                            "parentCommit":input["rsiGeneratorCommit"],
                            "sourceRevision":input["rsiBaseCommit"],
                            "operator":receipt["operator"],
                            "path":receipt["path"],
                            "qualityProven":false}));
                    Some(receipt)
                }
                Ok(None)=>None,
                Err(error)=>{
                    context.state.store.audit("rsi.native_preflight_error",
                        json!({"jobId":context.job_id,"error":error}));
                    None
                }
            }
        }else{None}
    }else{None};
    // Ordinary prompt jobs use the native verifier-driven execution cycle.
    // Detached RSI generations keep their own independent worktree verifier.
    let autonomous_result=if rsi_workspace.is_none() && input["autonomous"]!=false {
        let mut scoped_context=context.clone();
        scoped_context.state=scoped_state.clone();
        // The verifier always scores the immutable owner goal; the *model*
        // sees the continuation instruction on resume, including the owner's
        // attestation and fresh-observation requirement.
        Some(crate::engine::autonomy::execute(
            &scoped_context,&request,&prompt,&input).await?)
    }else{None};
    let mut message=if autonomous_result.is_some() || preflight_native.is_some(){
        // A native source operator is not a model invocation, and must not
        // be credited to the model in tool or token accounting.
        Ok(None)
    }else{
        run_quiet(&scoped_state, &request).await
    };
    let result = if let Some(worktree)=&rsi_workspace {
        // Native source induction is a genuine second candidate generator,
        // learned from the other RSI branch. It only runs when opted in and
        // the model left the ENTIRE detached worktree unchanged. It writes
        // actual Rust bytes, but is never misreported as a model tool write.
        let native_mutation=if preflight_native.is_some() {
            preflight_native
        }else if input["rsiNativeInduction"]==true {
            match crate::engine::rsi_mutation::induce(
                &context.state,&context.job_id,&input,
                &worktree.source,&worktree.path
            ) {
                Ok(receipt)=>receipt,
                Err(error)=>{
                    context.state.store.audit("rsi.native_induction_error",
                        json!({"jobId":context.job_id,"error":error}));
                    None
                }
            }
        } else {None};
        // The evolved verification controller sets a bounded number of
        // verifier-guided source repairs. This is an executed decision, not a
        // suggestion in the model prompt. Every attempt shares one isolated
        // worktree and its source/version receipts.
        let max_repairs=rsi_edit_repair_limit(&input);
        let mut attempts=Vec::<Value>::new();
        let mut repair_failure=Value::Null;
        let mut compiler_repairs=Vec::<Value>::new();
        let mut finished=crate::engine::rsi_worktree::finish(
            context,job,worktree,message,&session_id
        ).await;
        // Borrow the evidence-grounded repair principle from the stronger
        // branch: before spending another MODEL turn on a compiler error, ask
        // rustc for machine-applicable edits to the model's changed files.
        // Each proposed edit is rolled back unless cargo independently passes.
        for revision in 0..=max_repairs {
            let compiler_failed=finished.as_ref().ok().is_some_and(|prior|
                prior["reason"]=="build_or_typecheck_failed"
                && (prior["rsi"]["editEvidence"]["verifiedWrites"].as_u64().unwrap_or(0)>0
                    || prior["rsi"]["candidateOrigin"]=="native_inductive_source_mutation"));
            if compiler_failed {
                match crate::engine::rsi_compiler_repair::recover(worktree).await {
                    Ok(Some(receipt)) => {
                        // The entire candidate must be recaptured after rustc
                        // amended its bytes. Never evaluate the stale patch
                        // produced before a successful compiler repair.
                        match crate::engine::rsi_worktree::finish(
                            context,job,worktree,Ok(None),&session_id
                        ).await {
                            Ok(rechecked) => {
                                context.state.store.audit("rsi.compiler_repair_verified",
                                    json!({"jobId":context.job_id,"receipt":receipt,
                                        "candidateId":rechecked["rsi"]["candidateId"]}));
                                compiler_repairs.push(receipt);
                                finished=Ok(rechecked);
                            }
                            Err(error) => context.state.store.audit(
                                "rsi.compiler_repair_capture_failed",
                                json!({"jobId":context.job_id,"error":error})),
                        }
                    }
                    Ok(None) => {}
                    Err(error) => context.state.store.audit("rsi.compiler_repair_unavailable",
                        json!({"jobId":context.job_id,"error":error})),
                }
            }
            if revision==max_repairs {break;}
            let Some(feedback)=finished.as_ref().ok()
                .and_then(crate::engine::rsi_worktree::repair_feedback) else {break;};
            // Cancellation or process restart is never a reason to replay a write.
            if context.checkpoint(json!({
                "rsiPhase":"repairing","rsiRepairAttempt":revision+1
            })).is_err(){break;}
            let prior=finished.as_ref().unwrap();
            let iteration=json!({
                "id":format!("{}-{}",context.job_id,revision),
                "jobId":context.job_id,
                "attempt":revision+1,"result":prior["rsi"],
                "status":prior["status"],"reason":prior["reason"]
            });
            context.state.store.put("rsi_iteration",&iteration);
            attempts.push(iteration.clone());
            let mut retry=request.clone();
            retry.input=feedback;
            // The independent verifier, not a model summary, selects the
            // exact file whose compiler error must be repaired. The next
            // model turn is then routed through fresh-read -> edit instead
            // of repeating discovery over the entire repository.
            if let Some(mut repair_session)=context.state.store.get("session",&session_id) {
                repair_session["rsiRepairPath"]=
                    crate::engine::rsi_worktree::repair_target(prior)
                        .map(|(path,_)|json!(path)).unwrap_or(Value::Null);
                context.state.store.put("session",&repair_session);
            }
            message=run_quiet(&scoped_state,&retry).await;
            if let Err(error)=&message {
                // Do not erase the existing candidate/checks merely because
                // a later repair model call exhausted its token budget.
                repair_failure=json!(error);
                let mut attempted=iteration.clone();
                attempted["repairModelError"]=json!(error);
                context.state.store.put("rsi_iteration",&attempted);
                context.state.store.audit("rsi.repair_interrupted_preserved_candidate",
                    json!({"jobId":context.job_id,"reason":error,
                        "candidateId":prior["rsi"]["candidateId"]}));
                break;
            }
            let next=crate::engine::rsi_worktree::finish(
                context,job,worktree,message,&session_id
            ).await;
            if let Err(error)=&next {
                repair_failure=json!(error);
                context.state.store.audit("rsi.repair_capture_interrupted",
                    json!({"jobId":context.job_id,"reason":error,
                        "candidateId":prior["rsi"]["candidateId"]}));
                break;
            }
            finished=next;
        }
        let candidate_id=finished.as_ref().ok().map(|result|result["rsi"]["candidateId"].clone()).unwrap_or(Value::Null);
        let hypothesis=finished.as_ref().ok().map(|result|result["rsi"]["modelMessageId"].clone()).unwrap_or(Value::Null);
        let generation=json!({"jobId":context.job_id,"candidateId":candidate_id,"accepted":false,"domain":job["rsiDomain"],"hypothesisMessageId":hypothesis});
        crate::engine::rsi_evolve::remember_research(&context.state,&input,&generation,&session_id);
        crate::engine::rsi_evolve::collect_method_executions(&context.state,&session_id,&worktree.path);
        if let Err(error)=crate::engine::rsi_worktree::cleanup(worktree).await {
            context.state.store.audit("rsi.worktree_cleanup_failed",
                json!({"jobId":context.job_id,"error":error}));
        }
        let mut output=finished?;
        output["repairAttempts"]=json!(attempts.len());
        output["nativeMutation"]=json!(native_mutation);
        output["compilerRepairReceipts"]=json!(compiler_repairs);
        output["repairError"]=repair_failure;
        output["previousAttempts"]=json!(attempts);
        output
    } else if let Some(result)=autonomous_result {
        result
    } else {
        let message=message?;
        json!({
            "sessionId": message.as_ref().and_then(|item| item.get("session_id")),
            "messageId": message.as_ref().and_then(|item| item.get("id")),
        })
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

fn schedule_automations(state: &AppState) {
    let now = chrono::Utc::now();
    for automation in state.store.all("automation") {
        if automation.get("enabled").and_then(|value| value.as_bool()) != Some(true) {
            continue;
        }
        let Some(next_at) = automation.get("nextAt").and_then(|value| value.as_str()) else {
            continue;
        };
        let Ok(next) = chrono::DateTime::parse_from_rfc3339(next_at) else {
            continue;
        };
        if next.with_timezone(&chrono::Utc) > now {
            continue;
        }
        let kind = automation
            .get("kind")
            .and_then(|value| value.as_str())
            .unwrap_or("prompt");
        let input = automation
            .get("input")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let job_kind = if kind == "meeting" { "room" } else { kind };
        if !matches!(job_kind, "prompt" | "workflow" | "room" | "learn") {
            continue;
        }
        enqueue(state, job_kind, input);
        let mut updated = automation.clone();
        let interval = automation
            .get("intervalMinutes")
            .and_then(|value| value.as_i64())
            .unwrap_or(1440)
            .max(1);
        updated["lastAt"] = json!(now.to_rfc3339());
        updated["nextAt"] = json!((now + chrono::Duration::minutes(interval)).to_rfc3339());
        state.store.put("automation", &updated);
    }
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
    fn verified_controller_changes_the_executed_repair_budget() {
        assert_eq!(rsi_edit_repair_limit(&json!({})),2);
        assert_eq!(rsi_edit_repair_limit(&json!({"rsiEditPolicy":{"repairLimit":0}})),0);
        assert_eq!(rsi_edit_repair_limit(&json!({"rsiEditPolicy":{"repairLimit":1}})),1);
        assert_eq!(rsi_edit_repair_limit(&json!({"rsiEditPolicy":{"repairLimit":3}})),3);
        assert_eq!(rsi_edit_repair_limit(&json!({"rsiEditPolicy":{"repairLimit":999}})),3);
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
    fn enqueue_creates_a_queued_job() {
        let state = temp_state();
        let job = enqueue(
            &state,
            "workflow",
            json!({ "workflowId": "w1", "input": "hi" }),
        )
        .expect("job");
        assert_eq!(job["status"], "queued");
        assert_eq!(job["kind"], "workflow");
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
    fn rsi_stop_cancels_only_live_descendants_and_cannot_be_resurrected() {
        let state=temp_state();
        for (id,parent,status) in [("live","parent","running"),("queued","parent","queued"),
            ("done","parent","completed"),("other","sibling","running")] {
            state.store.put("job",&json!({"id":id,"kind":"rsi_generation","status":status,
                "input":{"rsiCampaignId":parent}}));
        }
        cancel_rsi_children(&state,"parent");
        cancel_rsi_children(&state,"parent");
        assert_eq!(state.store.get("job","live").unwrap()["status"],"cancelled");
        assert_eq!(state.store.get("job","queued").unwrap()["status"],"cancelled");
        assert_eq!(state.store.get("job","done").unwrap()["status"],"completed");
        assert_eq!(state.store.get("job","other").unwrap()["status"],"running");
        assert!(JobContext::new(state,"live".into()).checkpoint(json!({"status":"completed"})).is_err());
    }

    #[test]
    fn recovery_does_not_requeue_an_orphan_rsi_generation() {
        let state=temp_state();
        state.store.put("job",&json!({"id":"parent","kind":"rsi_evolve","status":"running",
            "rsiEvolutionInFlight":true,"attempts":1}));
        state.store.put("job",&json!({"id":"child","kind":"rsi_generation","status":"running",
            "input":{"rsiCampaignId":"parent"},"attempts":1}));
        recover(&state);
        assert_eq!(state.store.get("job","parent").unwrap()["status"],"needs_review");
        assert_eq!(state.store.get("job","child").unwrap()["status"],"cancelled");
    }

    #[test]
    fn recover_requeues_running_jobs() {
        let state = temp_state();
        let job = enqueue(
            &state,
            "workflow",
            json!({ "workflowId": "w", "input": "" }),
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
            "workflow",
            json!({ "workflowId": "w", "input": "", "sessionId": "session-1" }),
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
    fn job_errors_are_recorded_as_failed() {
        let state = temp_state();
        let job = enqueue(&state, "room", json!({ "roomId": "missing", "goal": "x" })).unwrap();
        let id = job["id"].as_str().unwrap().to_string();
        let _ = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(run_job(state.clone(), job));
        let stored = state.store.get("job", &id).unwrap();
        assert_eq!(stored["status"], "failed");
        assert!(stored["error"].as_str().unwrap().contains("대화방"));
    }

    #[test]
    fn failed_learning_jobs_are_requeued_with_backoff() {
        use crate::config::Config;
        use crate::db::Store;
        use crate::vault::Vault;
        let dir = std::env::temp_dir().join(format!("dominion-retry-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let store = Store::open(&config.db_path()).unwrap();
        let vault = Vault::open(&config.key_path());
        let state = crate::state::AppState::new(config, store, vault);
        state.store.put(
            "job",
            &json!({"id":"learn-failed","kind":"learn","status":"failed","attempts":1,"input":{},"error":"model unavailable"}),
        );
        assert_eq!(retry_failed_learning(&state), 1);
        let job = state.store.get("job", "learn-failed").unwrap();
        assert_eq!(job["status"], "queued");
        assert!(job["availableAt"].is_string());
        assert_eq!(job["lastError"], "model unavailable");
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[cfg(test)]
mod rsi_native_preflight_tests {
    use super::*;
    #[test]
    fn opted_in_source_search_can_start_natively_without_spending_a_model_turn() {
        let input=json!({"rsiNativeInduction":true,
            "rsiSourceScopeExpanded":true,"rsiAutonomous":true,
            "rsiAutoFocus":true,"rsiInheritedCandidateId":null});
        assert!(rsi_native_preflight(&input));
        for key in ["rsiNativeInduction","rsiAutoFocus"] {
            let mut blocked=input.clone();
            blocked[key]=json!(false);
            assert!(!rsi_native_preflight(&blocked));
        }
        let mut initial=input.clone();
        initial["rsiSourceScopeExpanded"]=json!(false);
        initial["rsiAutonomous"]=json!(false);
        assert!(rsi_native_preflight(&initial));
        initial["rsiFailedPatchRecovery"]=json!({"candidateId":"compiler-failure"});
        assert!(!rsi_native_preflight(&initial));
        let mut inherited=input;
        inherited["rsiInheritedCandidateId"]=json!("neutral-patch");
        assert!(!rsi_native_preflight(&inherited));
    }
}
