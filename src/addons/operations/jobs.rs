use crate::engine::chat::TurnRequest;
use crate::engine::run_quiet;
use crate::engine::rooms;
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

    /// Commit terminal state only from a live job. A cancelled, already
    /// finished or missing job must never be resurrected by a late worker.
    /// The audit is emitted by the caller only after this transaction commits.
    pub fn finish_terminal(&self, patch: Value) -> Result<Value, String> {
        let fields=patch.as_object().ok_or("터미널 체크포인트가 JSON 객체가 아닙니다.")?;
        let saved=self.state.store.mutate_record(
            "job",&self.job_id,json!({"id":self.job_id}),
            |current| {
                if current["cancelRequested"]==true ||
                   !matches!(current["status"].as_str(),Some("queued"|"running")) {
                    return Err("이미 종료되거나 중단된 작업은 다시 완료할 수 없습니다.".into());
                }
                for (key,value) in fields {
                    if key!="id" { current[key]=value.clone(); }
                }
                Ok(vec![])
            },
        )?;
        self.state.bus.record("job",&saved);
        Ok(saved)
    }

    /// Atomically check liveness and persist progress. Checking only the
    /// cancelRequested flag permitted a late worker to patch a terminal job,
    /// or overwrite a cancellation that raced with the earlier read.
    pub fn checkpoint(&self, patch: Value) -> Result<(), String> {
        let fields = patch.as_object()
            .ok_or("작업 체크포인트는 JSON 객체여야 합니다.")?;
        let saved = self.state.store.mutate_record(
            "job", &self.job_id, json!({"id":self.job_id}),
            |current| {
                if current["cancelRequested"] == true
                    || !matches!(current["status"].as_str(), Some("queued" | "running")) {
                    return Err("완료·중단된 작업의 체크포인트는 변경할 수 없습니다.".into());
                }
                for (key, value) in fields {
                    if key != "id" {
                        current[key] = value.clone();
                    }
                }
                Ok(vec![])
            },
        )?;
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

/// Recovers work left behind by a crash: running jobs are re-queued (unless
/// they already exhausted retries) and running executions become interrupted.
pub fn recover(state: &AppState) {
    // CLI workers are separate processes. After a backend restart their
    // previous result is unknown; never infer completion or launch the same
    // external inspection again just because the parent job was re-queued.
    let unfinished_external=match state.store.running_recovery_records("external_agent_run"){
        Ok(records)=>records,
        Err(error)=>{
            state.store.audit("recovery.external_agents_read_failed",
                json!({"error":error}));
            Vec::new()
        }
    };
    let mut external_jobs=std::collections::HashSet::new();
    for mut external in unfinished_external {
        if let Some(job_id)=external["jobId"].as_str(){
            external_jobs.insert(job_id.to_string());
        }
        external["status"]=json!("needs_review");
        external["errorClass"]=json!("restart_unknown");
        external["reason"]=json!("외부 실행기가 재시작 전에 종료되었으며 결과를 확인하지 못했습니다.");
        external["recoveredAt"]=json!(chrono::Utc::now().to_rfc3339());
        if let Some(saved)=state.store.put("external_agent_run",&external){
            state.bus.record("external_agent_run",&saved);
        }else{
            state.store.audit("recovery.external_agent_write_failed",
                json!({"externalRunId":external["id"]}));
        }
    }
    let jobs = match state.store.active_jobs() {
        Ok(jobs) => jobs,
        Err(error) => {
            state.store.audit("recovery.jobs_read_failed", json!({"error":error}));
            Vec::new()
        }
    };
    let uncertain_sessions = match state.store.uncertain_effect_sessions() {
        Ok(sessions) => Some(sessions),
        Err(error) => {
            state.store.audit("recovery.effects_read_failed", json!({"error":error}));
            None // Fail closed: never replay an effect we could not inspect.
        }
    };
    for job in jobs {
        let external_unknown=job["id"].as_str()
            .is_some_and(|id|external_jobs.contains(id));
        if job.get("status").and_then(|value| value.as_str()) != Some("running") {
            if external_unknown && job["status"]=="queued" {
                if let Some(id)=job["id"].as_str(){
                    let saved=state.store.patch_if("job",id,"status",&json!("queued"),
                        &json!({"status":"needs_review","recovered":true,
                            "error":"외부 실행 결과가 미확정이므로 자동 재실행하지 않습니다."}));
                    if let Some(saved)=saved{state.bus.record("job",&saved);}
                }
            }
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
            "/dependencyHydrationInFlight",
            "/goalState/inFlight",
            "/goalState/reviewPending",
            "/roomState/inFlight",
            "/workflowState/inFlight",
            "/discordState/inFlight",
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
            uncertain_sessions.as_ref().is_none_or(|sessions| sessions.contains(session))
        });
        let uncertain = flagged || unknown_external_result || external_unknown;
        updated["status"] = json!(if attempts >= 3 || uncertain {
            "needs_review"
        } else {
            "queued"
        });
        if uncertain {
            updated["error"] =json!(if external_unknown{
                "외부 실행 결과가 미확정이므로 자동 재실행하지 않습니다."
            }else{
                "재시작 전 작업 결과를 확정할 수 없습니다. 실행 기록을 확인하세요."
            });
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
    // Native sub4 frames share SQLite with the product. Recovery records
    // uncertain effects; it never starts a second runtime or replays writes.
    for mut frame in state.store.all("native_frame"){
        if let Ok(mut snapshot)=serde_json::from_value::<dominion_core::hosted::Snapshot>(frame["kernelState"].clone()){
            if !snapshot.native.terminal(){
                dominion_core::journal::recover(&mut snapshot.native);
                frame["kernelState"]=serde_json::to_value(snapshot).unwrap_or(Value::Null);
                frame["recoveredAt"]=json!(chrono::Utc::now().to_rfc3339());
                state.store.put("native_frame",&frame);
            }
        }
    }
    let running_executions = match state.store.running_recovery_records("execution") {
        Ok(records) => records,
        Err(error) => {
            state.store.audit("recovery.executions_read_failed", json!({"error":error}));
            Vec::new()
        }
    };
    for execution in running_executions {
            let mut updated = execution.clone();
            if let Ok(mut snapshot)=serde_json::from_value::<dominion_core::hosted::Snapshot>(updated["kernelState"].clone()){
                dominion_core::journal::recover(&mut snapshot.native);
                updated["kernelState"]=serde_json::to_value(snapshot).unwrap_or(Value::Null);
            }
            updated["status"] = json!("interrupted");
            updated["phase"] = json!("interrupted");
            updated["phaseUpdatedAt"] = json!(chrono::Utc::now().to_rfc3339());
            updated["taskOutcome"] = json!("cancelled");
            updated["reason"] = json!("백엔드가 재시작되어 실행이 중단되었습니다.");
            updated["finishedAt"] = json!(chrono::Utc::now().to_rfc3339());
            state.store.put("execution", &updated);
    }
    let running_operations = match state.store.running_recovery_records("execution_operation") {
        Ok(records) => records,
        Err(error) => {
            state.store.audit("recovery.operations_read_failed", json!({"error":error}));
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
    crate::engine::job_control::recover_scopes(state);
}

/// Starts the durable job worker plus the automation scheduler.
/// Wake job dispatch only on queue changes or completed work. The periodic
/// fallback also catches durable jobs written without an in-process event.
fn queued_job_event(event: &Value) -> bool {
    event["type"] == "record" && event["kind"] == "job"
        && event["record"]["status"] == "queued"
}

async fn wait_dispatch_event(
    events: &mut tokio::sync::broadcast::Receiver<Value>,
    finished: &tokio::sync::Notify,
    fallback: &mut tokio::time::Interval,
) -> bool {
    loop {
        tokio::select! {
            _ = finished.notified() => return true,
            _ = fallback.tick() => return true,
            event = events.recv() => match event {
                Ok(event) if queued_job_event(&event) => return true,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => return true,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return false,
                _ => {},
            },
        }
    }
}

pub fn start(state: AppState) {
    crate::engine::extensions::channels::start_outbound(state.clone());
    let workers = std::env::var("DOMINION_JOB_WORKERS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(4)
        .clamp(1, 16);
    let capacity = Arc::new(tokio::sync::Semaphore::new(workers));
    let finished = Arc::new(tokio::sync::Notify::new());
    let mut job_events = state.bus.subscribe();
    let mut fallback = tokio::time::interval(Duration::from_secs(10));
    fallback.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let dispatch_state = state.clone();
    tokio::spawn(async move {
        loop {
            // A single dispatcher checks the durable queue once per wake,
            // not once per worker every 400 ms when the queue is empty.
            // Team coordinators must not occupy all worker slots while
            // waiting for their own child jobs. start_now CAS-claims each.
            if let Ok(active)=dispatch_state.store.active_jobs(){
                for team in active.iter().filter(|j|j["kind"]=="team"&&j["status"]=="queued"){
                    if let Some(id)=team["id"].as_str(){
                        start_now(dispatch_state.clone(),id.to_string());
                    }
                }
            }
            while let Ok(permit) = capacity.clone().try_acquire_owned() {
                let Some(job) = next_queued(&dispatch_state) else {
                    drop(permit);
                    break;
                };
                let worker_state = dispatch_state.clone();
                let wake = finished.clone();
                tokio::spawn(async move {
                    run_job(worker_state, job).await;
                    drop(permit);
                    wake.notify_one();
                });
            }
            if !wait_dispatch_event(&mut job_events, &finished, &mut fallback).await { break; }
        }
    });

    let maintenance_state = state.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(30)).await;
            // Approval expiration must run whether the UI is connected or not.
            crate::engine::chat::expire_stale_approvals(&state);
        }
    });

    tokio::spawn(async move {
        // Deterministic guidance and correction extraction are retained, but
        // idle sessions no longer trigger a full database sweep every 5 min.
        let mut changes = maintenance_state.bus.subscribe();
        let mut tick = tokio::time::interval(Duration::from_secs(300));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut dirty = true; // one startup recovery sweep for existing evidence
        loop {
            tokio::select! {
                _ = tick.tick() => {
                    if dirty && crate::engine::extensions::globally_enabled(
                        &maintenance_state,crate::engine::extensions::Extension::Memory) {
                        crate::engine::agent_growth::sweep(&maintenance_state);
                        crate::engine::distill::sweep(&maintenance_state);
                        dirty = false;
                    }
                },
                event = changes.recv() => match event {
                    Ok(event) if event["type"] == "message"
                        || (event["type"] == "record"
                            && (event["kind"] == "execution"
                                || (event["kind"] == "job" && event["record"]["status"] == "completed"))) => {
                        dirty = true;
                    },
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => dirty = true,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    _ => {},
                },
            }
        }
    });
}

fn next_queued(state: &AppState) -> Option<Value> {
    let jobs = match state.store.active_jobs() {
        Ok(jobs) => jobs,
        Err(error) => {state.store.audit("job.queue_read_failed",json!({"error":error}));return None;}
    };
    let learning_running = jobs
        .iter()
        .any(|job| job["status"] == "running" && job["kind"] == "learn");
    let mut queued = jobs
        .iter()
        .cloned()
        .filter(|job| job.get("status").and_then(|value| value.as_str()) == Some("queued"))
        .filter(|job|job["kind"]!="team")
        .filter(|job|{
            let Some(team)=job["input"]["teamJobId"].as_str() else{return true};
            let bound=job["input"]["teamMaxParallel"].as_u64().unwrap_or(3).clamp(1,4);
            jobs.iter().filter(|other|other["status"]=="running"&&
                other["input"]["teamJobId"].as_str()==Some(team)).count() < bound as usize
        })
        // Learning is background work: at most one at a time, so it can never
        // occupy every worker while a prompt/room job waits.
        .filter(|job| !(job["kind"] == "learn" && learning_running))
        .filter(|job| crate::addons::team_dag::ready_or_block(state,job))
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
        let priority = |job: &Value| u8::from(matches!(job["kind"].as_str(),Some("learn")));
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
        if !crate::addons::team_dag::ready_or_block(&state,&job){
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
        "room" | "meeting" => rooms::run(&context, &job).await,
        "prompt" => run_prompt(&context, &job).await,
        "team" => crate::addons::team_orchestrator::run(&context,&job).await,
        "resume" => run_resume(&context, &job).await,
        "learn"|"reflect"|"discord" =>
            crate::engine::extensions::jobs::run(&context,&job).await,
        other => Err(format!("지원하지 않는 작업 종류입니다: {other}")),
    };

    let patch=match result {
        Ok(result) => {
            let status=result.get("status").and_then(Value::as_str)
                .filter(|status|matches!(*status,"needs_review"|"cancelled"|"failed"))
                .unwrap_or("completed");
            json!({"status":status,"result":result,
                "finishedAt":chrono::Utc::now().to_rfc3339(),
                "events":context.events.lock().unwrap().clone()})
        }
        Err(error) if error.starts_with("deferred:") => {
            if let Err(checkpoint_error)=context.checkpoint(json!({
                "status":"queued",
                "availableAt":(chrono::Utc::now()+chrono::Duration::seconds(1)).to_rfc3339(),
                "attempts":job["attempts"].as_u64().unwrap_or(1).saturating_sub(1),
                "error":error
            })) {
                state.store.audit("job.defer_checkpoint_failed",
                    json!({"jobId":job_id,"error":checkpoint_error}));
            }
            return;
        }
        Err(error) => {
            let status=if error.starts_with("needs_review:") {
                "needs_review"
            }else if error.contains("중단")||error.contains("cancelled") {
                "cancelled"
            }else{"failed"};
            json!({"status":status,"error":error,
                "finishedAt":chrono::Utc::now().to_rfc3339(),
                "events":context.events.lock().unwrap().clone()})
        }
    };
    // No terminal audit or completion event is published if checkpoint
    // persistence fails or the owner cancelled while the model was running.
    match context.finish_terminal(patch) {
        Ok(saved) => {
            if saved["status"]=="completed" {
                if let Err(error)=crate::engine::delegation_worktree::freeze_result(&state,&job).await {
                    if let Some(review)=state.store.patch_if("job",&job_id,"status",
                        &json!("completed"),&json!({"status":"needs_review","error":error})){
                        state.bus.record("job",&review);
                    } else {
                        state.store.audit("job.freeze_failed",
                            json!({"jobId":job_id,"error":error}));
                    }
                } else if let Some(frozen)=state.store.get("job",&job_id)
                    .filter(|row|row["delegationPatchSha256"].is_string()) {
                    // Wake dependent workers only after a completed isolated
                    // writer's diff was actually frozen, not at a transient
                    // pre-freeze "completed" event.
                    state.bus.record("job",&frozen);
                }
            }
            // Final audit reflects the frozen result, not a transient completed
            // state before delegated worktree evidence has been captured.
            let final_row=state.store.get("job",&job_id).unwrap_or(saved);
            let status=final_row["status"].as_str().unwrap_or("unknown");
            state.store.audit(&format!("job.{status}"),
                json!({"jobId":job_id,"kind":kind,"error":final_row["error"]}));
        }
        Err(error)=>{
            state.store.audit("job.finalization_failed",
                json!({"jobId":job_id,"kind":kind,"error":error}));
        }
    }
    if let Some(session)=job["input"]["sessionId"].as_str(){
        crate::engine::execution_budget::close_unstarted_child(&state,session);
    }
}

pub(crate) async fn run_prompt(context: &JobContext, job: &Value) -> Result<Value, String> {
    let input = job.get("input").cloned().unwrap_or_else(|| json!({}));
    let prompt = input["prompt"].as_str().unwrap_or("").to_string();
    if prompt.trim().is_empty() {return Err("작업 입력(prompt)이 비어 있습니다.".into());}
    if let Some(result) = job.pointer("/promptState/result") {return Ok(result.clone());}
    if job.pointer("/promptState/inFlight").and_then(Value::as_bool) == Some(true) {
        return Ok(json!({"status":"needs_review","reason":"중단된 프롬프트 작업 결과를 확인해야 합니다."}));
    }
    let session_id = job.get("sessionId").or_else(|| input.get("sessionId"))
        .and_then(Value::as_str).map(str::to_string);
    let session_id = match session_id {
        Some(id) => id,
        None => context.state.store.put("session",&json!({
            "title":prompt.chars().take(40).collect::<String>(),"agentId":input.get("agentId")
        })).and_then(|session|session["id"].as_str().map(str::to_string))
            .ok_or("작업 세션을 저장할 수 없습니다.")?,
    };
    crate::engine::delegation_worktree::hydrate_team_verifier(context,job).await?;
    let (_job_scope, mut goal) = crate::engine::job_control::configure(&context.state,job,&session_id)?;
    if input["teamJobId"].is_string(){
        goal.push_str(&crate::addons::team_orchestrator::dependency_evidence(&context.state,job)?);
    }
    context.checkpoint(json!({"sessionId":session_id,"promptState":{"inFlight":true}}))?;
    let request = TurnRequest {
        session_id: Some(session_id.clone()), input: goal,
        agent_id: input["agentId"].as_str().map(str::to_string),
        provider_id: input["providerId"].as_str().map(str::to_string),
        model: input["model"].as_str().map(str::to_string),
        attachment_ids: Vec::new(),
        tools_enabled: input["toolsEnabled"].as_bool().unwrap_or(true),
        tools_execution_enabled: input["toolsExecutionEnabled"].as_bool().unwrap_or(true),
        thinking: input["thinking"].as_bool(), auto_learning: false,
        reasoning_effort: input["reasoningEffort"].as_str().map(str::to_string),
        reasoning_language: None, system_override: None,
        task_id: None, parent_task_id: None, task_revision: None, verification_owner: None,
    };
    let mut scoped = context.state.clone();
    if let Some(session)=context.state.store.get("session",&session_id) {
        if let Some(path)=crate::engine::delegation_worktree::scoped_workspace(&context.state,&session)? {
            scoped.config.workspace=path;
        } else if let Some(raw)=session["workspaceDir"].as_str() {
            scoped.config.workspace=crate::engine::workspace_scope::resolve(&context.state,raw)?;
        }
    }
    let (message,external_run)=if let Some(runtime)=input["externalRuntime"].as_str(){
        let (run_id,answer)=crate::addons::agent_bridge::inspect(
            context,&session_id,&scoped.config.workspace,&request.input,runtime
        ).await?;
        let saved=context.state.store.write_message(&json!({
            "session_id":session_id,"role":"assistant",
            "agent_id":request.agent_id,"content":answer,
            "metadata":{"authorKind":"agent","externalRuntime":runtime,
                "externalRunId":run_id,
                "nativeToolReceipts":false,"verification":"external_read_only_report"}
        })).ok_or("외부 Agent 최종 응답을 저장하지 못했습니다.")?;
        context.state.bus.message(&saved);
        (Some(saved),Some(run_id))
    }else{
        (run_quiet(&scoped,&request).await?,None)
    };
    let result=json!({
        "sessionId":message.as_ref().and_then(|item|item.get("session_id")),
        "messageId":message.as_ref().and_then(|item|item.get("id")),
        "externalRunId":external_run
    });
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
    fn restart_fences_external_inspector_and_parent_job_without_replay(){
        let state=temp_state();
        state.store.put("job",&json!({"id":"external-job","kind":"prompt",
            "status":"running","attempts":1,"input":{"sessionId":"child"}})).unwrap();
        state.store.put("external_agent_run",&json!({
            "id":"external:external-job","jobId":"external-job",
            "sessionId":"child","runtime":"qwen","status":"running",
            "nativeToolReceipts":false
        })).unwrap();
        recover(&state);
        let external=state.store.get("external_agent_run","external:external-job")
            .unwrap();
        assert_eq!(external["status"],"needs_review");
        assert_eq!(external["errorClass"],"restart_unknown");
        assert!(external["recoveredAt"].is_string());
        let job=state.store.get("job","external-job").unwrap();
        assert_eq!(job["status"],"needs_review");
        assert_eq!(job["recovered"],true);
        assert!(job["error"].as_str().unwrap().contains("자동 재실행"));
        recover(&state);
        assert_eq!(state.store.get("job","external-job").unwrap()["status"],"needs_review");
        assert_eq!(state.store.get("external_agent_run","external:external-job")
            .unwrap()["status"],"needs_review");
    }
    #[test]
    fn dependency_scheduler_does_not_start_unfinished_or_failed_predecessors(){
        let state=temp_state();
        state.store.put("session",&json!({"id":"child1"})).unwrap();
        state.store.put("job",&json!({"id":"parent","kind":"prompt",
            "status":"running","input":{"parentSessionId":"other","delegation":true}})).unwrap();
        state.store.put("job",&json!({"id":"pre","kind":"prompt","status":"running",
            "input":{"parentSessionId":"root","sessionId":"child1","delegation":true}})).unwrap();
        state.store.put("job",&json!({"id":"child","kind":"prompt","status":"queued",
            "input":{"parentSessionId":"root","delegation":true,
                "dependsOnJobIds":["pre"]}})).unwrap();
        assert!(next_queued(&state).is_none(),
            "a pending prerequisite cannot start child execution");
        state.store.patch_if("job","pre","status",&json!("running"),
            &json!({"status":"completed"})).unwrap();
        let claimed=next_queued(&state).expect("completed predecessor admits child");
        assert_eq!(claimed["id"],"child");
        state.store.patch_if("job","child","status",&json!("running"),
            &json!({"status":"queued"})).unwrap();
        state.store.patch_if("job","pre","status",&json!("completed"),
            &json!({"status":"needs_review"})).unwrap();
        assert!(next_queued(&state).is_none());
        assert_eq!(state.store.get("job","child").unwrap()["status"],"needs_review");
        assert!(state.store.get("job","child").unwrap()["error"]
            .as_str().unwrap().contains("선행 작업 실패"));
    }

    #[test]
    fn team_parallelism_never_overclaims_worker_slots(){
        let state=temp_state();
        for i in 0..3 {
            state.store.put("job",&json!({
                "id":format!("w{i}"),"kind":"prompt","status":"queued",
                "input":{"teamJobId":"team","teamMaxParallel":2,
                    "delegation":true,"parentSessionId":"owner"}
            })).unwrap();
        }
        let a=next_queued(&state).unwrap();
        let b=next_queued(&state).unwrap();
        assert_ne!(a["id"],b["id"]);
        assert!(next_queued(&state).is_none());
        let first=a["id"].as_str().unwrap();
        state.store.patch_if("job",first,"status",&json!("running"),
            &json!({"status":"completed"})).unwrap();
        let c=next_queued(&state).unwrap();
        assert!(c["id"]!=a["id"]&&c["id"]!=b["id"]);
    }

    #[test]
    fn terminal_claim_rejects_cancelled_and_already_completed_jobs(){
        let state=temp_state();
        let job=enqueue(&state,"prompt",json!({"sessionId":"s","prompt":"work"})).unwrap();
        let id=job["id"].as_str().unwrap().to_string();
        let ctx=JobContext::new(state.clone(),id.clone());
        state.cancel_session("s");
        assert!(ctx.finish_terminal(json!({"status":"completed","result":{"ok":true}})).is_err());
        let current=state.store.get("job",&id).unwrap();
        assert_eq!(current["status"],"cancelled");
        assert!(current["result"].is_null());

        let complete=enqueue(&state,"prompt",json!({"prompt":"done"})).unwrap();
        let completed_id=complete["id"].as_str().unwrap().to_string();
        let ctx=JobContext::new(state.clone(),completed_id.clone());
        assert_eq!(ctx.finish_terminal(json!({"status":"completed"})).unwrap()["status"],
            "completed");
        assert!(ctx.finish_terminal(json!({"status":"failed","error":"late"})).is_err());
        assert_eq!(state.store.get("job",&completed_id).unwrap()["status"],"completed");
        let _=std::fs::remove_dir_all(&state.config.data_dir);
    }

    #[test]
    fn checkpoint_cannot_resurrect_terminal_jobs_or_bypass_cancelled_flag() {
        let state = temp_state();
        for terminal in ["completed", "failed", "cancelled", "needs_review"] {
            let job = enqueue(&state, "prompt", json!({"prompt":"test"})).unwrap();
            let id = job["id"].as_str().unwrap().to_string();
            let ctx = JobContext::new(state.clone(), id.clone());
            ctx.checkpoint(json!({"stepIndex":1})).unwrap();
            state.store.patch_if("job",&id,"status",&json!("queued"),
                &json!({"status":terminal,"result":{"saved":true}})).unwrap();
            assert!(ctx.checkpoint(json!({"status":"running","result":{"saved":false}})).is_err());
            let current = state.store.get("job",&id).unwrap();
            assert_eq!(current["status"],terminal);
            assert_eq!(current["result"]["saved"],true);
            assert_eq!(current["stepIndex"],1);
        }
        let job = enqueue(&state, "prompt", json!({"prompt":"cancel"})).unwrap();
        let id = job["id"].as_str().unwrap().to_string();
        let ctx = JobContext::new(state.clone(), id.clone());
        state.store.patch_if("job",&id,"status",&json!("queued"),
            &json!({"status":"running","cancelRequested":true})).unwrap();
        assert!(ctx.checkpoint(json!({"stepIndex":99})).is_err());
        assert!(state.store.get("job",&id).unwrap()["stepIndex"] != 99);
        assert!(ctx.checkpoint(json!([])).is_err());
        let _ = std::fs::remove_dir_all(&state.config.data_dir);
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
    fn recovery_keeps_completed_history_and_flags_inflight_external_tools() {
        let state = temp_state();
        let job = enqueue(&state, "prompt",
            json!({"prompt":"w", "sessionId":"session-uncertain"})).unwrap();
        let mut running = job.clone();
        running["status"] = json!("running");
        state.store.put("job", &running);
        state.store.put("job", &json!({"id":"history","status":"completed"}));
        state.store.put("execution", &json!({
            "id":"run-inflight","sessionId":"session-uncertain","status":"running"
        }));
        state.store.put("execution", &json!({"id":"run-past","status":"completed"}));
        state.store.put("execution_operation", &json!({
            "id":"tool-inflight","runId":"run-inflight","status":"running","tool":"shell_exec"
        }));
        state.store.put("execution_operation", &json!({
            "id":"tool-past","runId":"run-past","status":"completed","tool":"shell_exec"
        }));
        recover(&state);
        assert_eq!(state.store.get("job",job["id"].as_str().unwrap()).unwrap()["status"],
            "needs_review", "interrupted tools may already have external effects");
        assert_eq!(state.store.get("job","history").unwrap()["status"], "completed");
        assert_eq!(state.store.get("execution","run-inflight").unwrap()["status"], "interrupted");
        assert_eq!(state.store.get("execution","run-past").unwrap()["status"], "completed");
        assert_eq!(state.store.get("execution_operation","tool-inflight").unwrap()["outcomeUnknown"], true);
        assert_eq!(state.store.get("execution_operation","tool-past").unwrap()["status"], "completed");
        let _ = std::fs::remove_dir_all(&state.config.data_dir);
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


}

#[cfg(test)]
mod dispatcher_tests {
    use super::*;
    #[tokio::test]
    async fn unrelated_events_do_not_reenter_dispatch_or_starve_fallback() {
        let (send,mut events)=tokio::sync::broadcast::channel(16);
        let finished=tokio::sync::Notify::new();
        let mut fallback=tokio::time::interval_at(tokio::time::Instant::now()+Duration::from_millis(150),Duration::from_millis(150));
        send.send(json!({"type":"message"})).unwrap();
        assert!(tokio::time::timeout(Duration::from_millis(20),wait_dispatch_event(&mut events,&finished,&mut fallback)).await.is_err());
        send.send(json!({"type":"record","kind":"job","record":{"status":"queued"}})).unwrap();
        assert!(tokio::time::timeout(Duration::from_millis(20),wait_dispatch_event(&mut events,&finished,&mut fallback)).await.unwrap());
        // A wake written without an event is still picked up by the fallback.
        assert!(tokio::time::timeout(Duration::from_secs(1),wait_dispatch_event(&mut events,&finished,&mut fallback)).await.unwrap());
    }

    #[test]
    fn only_newly_queued_job_events_wake_dispatcher() {
        assert!(queued_job_event(&json!({"type":"record","kind":"job","record":{"status":"queued"}})));
        assert!(!queued_job_event(&json!({"type":"record","kind":"job","record":{"status":"running"}})));
        assert!(!queued_job_event(&json!({"type":"record","kind":"memory","record":{"status":"queued"}})));
        assert!(!queued_job_event(&json!({"type":"message","message":{}})));
    }
}


