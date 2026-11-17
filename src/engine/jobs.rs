use crate::engine::chat::TurnRequest;
use crate::engine::run_quiet;
use crate::engine::{learning, reflect, rooms};
use crate::state::AppState;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const MAX_EVENTS: usize = 200;

fn retired_legacy_kind(kind: &str) -> bool {
    matches!(kind, "video" | "video_plan" | "youtube_upload"
        | "rsi_evolve" | "rsi_compare" | "rsi_generation")
}

/// Shared handle a running job uses to report progress and persist its state.
#[derive(Clone)]
pub struct JobContext {
    pub state: AppState,
    pub job_id: String,
    /// Captured when the host worker claims this job; checkpoint validity is
    /// tied to this attempt even if another worker requeues it later.
    attempt: u64,
    events: Arc<Mutex<Vec<Value>>>,
}

impl JobContext {
    pub fn new(state: AppState, job_id: String) -> Self {
        let attempt=state.store.get("job",&job_id)
            .and_then(|j|j["attempts"].as_u64()).unwrap_or(0);
        Self {
            state,
            job_id,
            attempt,
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
        // Do not perform a separate read/check/write: owner cancellation,
        // terminal completion and retry may race the in-flight model call.
        let saved=self.state.store.checkpoint_running_job(
            &self.job_id,self.attempt,&patch)
            .ok_or("작업이 종료·중단되었거나 다른 실행 시도로 교체되어 체크포인트를 거부했습니다.")?;
        self.state.bus.record("job",&saved);
        Ok(())
    }
}

pub fn enqueue(state: &AppState, kind: &str, input: Value) -> Option<Value> {
    if retired_legacy_kind(kind){return None;}

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
        state.job_wakeup.notify_one();
    }
    job
}

/// Atomically deduplicate an immutable operation across HTTP requests and
/// workers. The supplied ID belongs to the engine, never a client record.
pub fn enqueue_once(state: &AppState, id: &str, kind: &str, input: Value) -> Option<Value> {
    if retired_legacy_kind(kind){return None;}

    let candidate = json!({"id":id,"kind":kind,"input":input,"status":"queued",
        "queuedAt":chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros,true),
        "attempts":0,"stepIndex":0,"stepTotal":0,"events":[]});
    if let Some(saved) = state.store.put_if_absent("job", &candidate) {
        state.bus.record("job", &saved);
        state
            .store
            .audit("job.queued", json!({"kind":kind,"jobId":id}));
        state.job_wakeup.notify_one();
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
    for job in state.store.all("job") {
        // Removed capabilities cannot restart paid generation or experiments.
        if retired_legacy_kind(job["kind"].as_str().unwrap_or("")) {
            if matches!(job["status"].as_str(),Some("queued"|"running")) {
                if let Some(id)=job["id"].as_str() {
                    let uncertain=job["status"]=="running";
                    if let Some(saved)=state.store.patch_if("job",id,"status",&job["status"],&json!({
                        "status":if uncertain {"needs_review"} else {"cancelled"},
                        "legacyRetired":true,"cancelRequested":true,
                        "error":"Retired built-in media/RSI task: execution disabled; historical data retained",
                        "finishedAt":chrono::Utc::now().to_rfc3339()
                    })) {state.bus.record("job",&saved);}
                }
            }
            continue;
        }
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
            "/nativeInFlight",
            "/roomState/inFlight",
            "/workflowState/inFlight",
            "/discordState/inFlight",
        ]
        .iter()
        .any(|path| {
            // Stable native child IDs make new workflows replay-safe.
            if *path=="/workflowState/inFlight"
                && job.pointer(path)==Some(&json!(true))
            {
                return job.pointer("/workflowState/activeChildJobId")
                    .and_then(Value::as_str)
                    .is_none_or(|id| state.store.get("job",id).is_none());
            }
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
                operation["outcomeUnknown"] == true
                    && operation["runId"]
                        .as_str()
                        .and_then(|run_id| state.store.get("execution", run_id))
                        .is_some_and(|run| run["sessionId"].as_str() == Some(session))
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
    // Domain plug-ins are dormant unless the owner invoked or explicitly
    // enabled their background mode. The core owns only job execution.
    crate::engine::plugins::start(&state);
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
                    None => {
                        // Wake instantly when the same-process API enqueues,
                        // and run a bounded reconciliation sweep every 5s
                        // for crash recovery, deferred retries, and other
                        // writers. This avoids ~10 whole-job reads per second
                        // on a four-worker idle deployment.
                        tokio::select! {
                            _ = worker_state.job_wakeup.notified() => {},
                            _ = tokio::time::sleep(Duration::from_secs(5)) => {},
                        }
                    },
                }
            }
        });
    }

    tokio::spawn(async move {
        let mut heartbeat_ticks=0u64;
        loop {
            tokio::time::sleep(Duration::from_secs(30)).await;
            heartbeat_ticks=heartbeat_ticks.wrapping_add(1);
            // A process restart removes in-memory approval timers. The persisted
            // deadline still expires automatically during normal operation.
            crate::engine::chat::expire_stale_approvals(&state);
            // Keep host approval expiry at 30s; the optional Money plug-in
            // is checked only every 60s to halve idle SQLite lookups.
            // An explicit enable triggers the first Money job immediately.
            if heartbeat_ticks%2==0{
                crate::engine::plugins::maintenance_tick(&state);
            }
        }
    });

    // No periodic RSI/strategy growth/distillation or automatic learning.
    // Explicit user-requested edits, tests and verification still use the
    // durable native Rust job loop. Saved memory/history is untouched.
}

fn next_queued(state: &AppState) -> Option<Value> {
    let jobs = match state.store.active_jobs() {
        Ok(jobs)=>jobs,
        Err(error)=>{
            state.store.audit("job.scheduler_query_failed",json!({"error":error}));
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
        .filter(|job| !retired_legacy_kind(job["kind"].as_str().unwrap_or("")))
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

/// Never report a completed/failed result unless its terminal checkpoint
/// actually reached durable storage. Cancellation or concurrent replacement
/// can make a post-execution checkpoint invalid; the existing job record
/// remains authoritative and is left untouched in that case.
fn commit_terminal_job(
    context:&JobContext, kind:&str, status:&str, patch:Value, audit_detail:Value,
) -> bool {
    match context.checkpoint(patch) {
        Ok(()) => {
            context.state.store.audit(&format!("job.{status}"),audit_detail);
            true
        }
        Err(error) => {
            context.state.store.audit("job.final_checkpoint_refused",
                json!({"jobId":context.job_id,"kind":kind,
                    "attemptedStatus":status,"error":error,
                    "storedStatus":context.state.store.get("job",&context.job_id)
                        .map(|job|job["status"].clone())}));
            false
        }
    }
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

    if retired_legacy_kind(&kind) {
        context.patch(json!({"status":"needs_review","legacyRetired":true,
            "cancelRequested":true,"error":"Retired feature cannot execute",
            "finishedAt":chrono::Utc::now().to_rfc3339()}));
        return;
    }
    let result = match kind.as_str() {
        "room" | "meeting" => rooms::run(&context, &job).await,
        "prompt" => run_prompt(&context, &job).await,
        kind if crate::engine::plugins::supports(kind) =>
            crate::engine::plugins::run(&context,&job).await,
        "resume" => run_resume(&context, &job).await,
        "learn" => learning::run(&context, &job).await,
        "reflect" => reflect::run(&context, &job).await,
        "discord" => crate::engine::discord::run(&context, &job).await,
        other => Err(format!("지원하지 않는 작업 종류입니다: {other}")),
    };

    match result {
        Ok(result) => {
            let status = result
                .get("status")
                .and_then(Value::as_str)
                .filter(|status| matches!(*status, "needs_review" | "cancelled" | "failed"))
                .unwrap_or("completed");
            commit_terminal_job(&context,&kind,status,json!({
                "status": status,
                "result": result,
                "finishedAt": chrono::Utc::now().to_rfc3339(),
                "events": context.events.lock().unwrap().clone(),
            }),json!({ "jobId": job_id, "kind": kind }));
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
            commit_terminal_job(&context,&kind,status,json!({
                "status": status,
                "error": error,
                "finishedAt": chrono::Utc::now().to_rfc3339(),
                "events": context.events.lock().unwrap().clone(),
            }),json!({ "jobId": job_id, "kind": kind, "error": error }));
        }
    }
}

/// Every user-requested prompt runs on the same durable Rust SDK kernel.
pub(crate) async fn run_prompt(context:&JobContext,job:&Value)->Result<Value,String>{
    crate::engine::native_agent::run(context,job).await
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
    fn retired_media_and_rsi_work_is_never_requeued_and_history_survives(){
        let state=temp_state();
        for (kind,id,status) in [
            ("rsi_evolve","old-campaign","queued"),
            ("rsi_generation","old-child","running"),
            ("video","old-media","queued"),
            ("youtube_upload","old-upload","running")] {
            state.store.put("job",&json!({"id":id,"kind":kind,"status":status,
                "attempts":1,"input":{"brief":"kept"}})).unwrap();
        }
        state.store.put("job",&json!({"id":"historical","kind":"video",
            "status":"completed","result":{"output":"preserved"}})).unwrap();
        recover(&state);
        assert!(next_queued(&state).is_none());
        assert_eq!(state.store.get("job","old-campaign").unwrap()["status"],"cancelled");
        assert_eq!(state.store.get("job","old-child").unwrap()["status"],"needs_review");
        assert_eq!(state.store.get("job","old-media").unwrap()["legacyRetired"],true);
        assert_eq!(state.store.get("job","old-upload").unwrap()["cancelRequested"],true);
        assert_eq!(state.store.get("job","historical").unwrap()["result"]["output"],"preserved");
        assert!(enqueue(&state,"rsi_evolve",json!({})).is_none());
        assert!(enqueue_once(&state,"new-retired","video",json!({})).is_none());
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
    fn recover_new_workflow_reuses_durable_rust_sdk_child() {
        let state=temp_state();
        state.store.put("job",&json!({
            "id":"native-child","kind":"prompt","status":"completed",
            "input":{"prompt":"Analyze source"},"result":{"answer":"Done"}
        }));
        state.store.put("job",&json!({
            "id":"workflow-parent","kind":"workflow","status":"running",
            "attempts":1,"workflowState":{"inFlight":true,
                "activeChildJobId":"native-child","activeStepIndex":0}
        }));
        recover(&state);
        assert_eq!(state.store.get("job","workflow-parent").unwrap()["status"],"queued");
        assert_eq!(state.store.get("job","native-child").unwrap()["status"],"completed");
    }
    #[test]
    fn recover_legacy_workflow_still_requires_review() {
        let state=temp_state();
        state.store.put("job",&json!({
            "id":"legacy-workflow","kind":"workflow","status":"running",
            "attempts":1,"workflowState":{"inFlight":true,"activeStepIndex":0}
        }));
        recover(&state);
        assert_eq!(state.store.get("job","legacy-workflow").unwrap()["status"],"needs_review");
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
    fn checkpoint_rejects_late_stale_attempt_after_requeue() {
        let state=temp_state();
        state.store.put("job",&json!({"id":"attempt-fence",
            "kind":"prompt","status":"running","attempts":1})).unwrap();
        let old=JobContext::new(state.clone(),"attempt-fence".into());
        old.checkpoint(json!({"stepIndex":1})).unwrap();
        let _=state.store.patch_if("job","attempt-fence","status",&json!("running"),
            &json!({"status":"queued"})).unwrap();
        let _=state.store.patch_if("job","attempt-fence","status",&json!("queued"),
            &json!({"status":"running","attempts":2})).unwrap();
        assert!(old.checkpoint(json!({"stepIndex":999,"status":"completed"})).is_err());
        let saved=state.store.get("job","attempt-fence").unwrap();
        assert_eq!(saved["status"],"running");
        assert_eq!(saved["attempts"],2);
        assert_eq!(saved["stepIndex"],1);
        let fresh=JobContext::new(state.clone(),"attempt-fence".into());
        fresh.checkpoint(json!({"stepIndex":2})).unwrap();
        assert_eq!(state.store.get("job","attempt-fence").unwrap()["stepIndex"],2);
    }
    #[test]
    fn checkpoint_refuses_late_write_after_owner_cancel_or_terminal_state() {
        let state=temp_state();
        state.store.put("job",&json!({"id":"cancel-fence",
            "kind":"prompt","status":"running","attempts":1})).unwrap();
        let owner=JobContext::new(state.clone(),"cancel-fence".into());
        state.store.patch_if("job","cancel-fence","status",&json!("running"),
            &json!({"status":"cancelled","cancelRequested":true})).unwrap();
        assert!(owner.checkpoint(json!({"status":"completed","result":"late"})).is_err());
        let cancelled=state.store.get("job","cancel-fence").unwrap();
        assert_eq!(cancelled["status"],"cancelled");
        assert!(cancelled["result"].is_null());
        state.store.put("job",&json!({"id":"terminal-fence",
            "kind":"prompt","status":"running","attempts":1})).unwrap();
        let previous=JobContext::new(state.clone(),"terminal-fence".into());
        previous.checkpoint(json!({"status":"completed","result":"finished"})).unwrap();
        assert!(previous.checkpoint(json!({"result":"overwritten"})).is_err());
        assert_eq!(state.store.get("job","terminal-fence").unwrap()["result"],"finished");
    }
    #[test]
    fn checkpoint_cannot_forge_cancellation_or_job_identity() {
        let state=temp_state();
        state.store.put("job",&json!({"id":"job-identity","status":"running",
            "kind":"prompt","attempts":1})).unwrap();
        let context=JobContext::new(state.clone(),"job-identity".into());
        assert!(context.checkpoint(json!({"cancelRequested":false})).is_err());
        assert!(context.checkpoint(json!({"id":"other"})).is_err());
        let stored=state.store.get("job","job-identity").unwrap();
        assert_eq!(stored["id"],"job-identity");
        assert!(stored["cancelRequested"].is_null());
    }

    #[test]
    fn terminal_checkpoint_cannot_turn_cancelled_work_into_a_completed_job() {
        let state=temp_state();
        state.store.put("job",&json!({"id":"terminal-race",
            "kind":"prompt","status":"running","cancelRequested":false}));
        let context=JobContext::new(state.clone(),"terminal-race".into());
        assert!(commit_terminal_job(&context,"prompt","completed",
            json!({"status":"completed","result":{"answer":"done"}}),
            json!({"jobId":"terminal-race"})));
        assert_eq!(state.store.get("job","terminal-race").unwrap()["status"],"completed");
        state.store.put("job",&json!({"id":"terminal-race",
            "kind":"prompt","status":"cancelled","cancelRequested":true}));
        assert!(!commit_terminal_job(&context,"prompt","completed",
            json!({"status":"completed","result":{"answer":"stale"}}),
            json!({"jobId":"terminal-race"})));
        let persisted=state.store.get("job","terminal-race").unwrap();
        assert_eq!(persisted["status"],"cancelled");
        assert!(persisted["result"].is_null(),
            "a stale worker cannot publish a success after cancellation");
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
