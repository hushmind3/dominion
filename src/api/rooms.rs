//! Owner-centered room API. Room sessions and worker checkpoints are server
//! state; clients can change the roster and policy, never rebind a transcript.
use crate::engine::{memory, rooms};
use crate::state::AppState;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};
use std::collections::HashMap;

pub(crate) type ApiResult = Result<Value, (StatusCode, String)>;
fn response(result: ApiResult) -> Response {
    match result {
        Ok(value) => Json(value).into_response(),
        Err((status, error)) => (status, Json(json!({"error":error}))).into_response(),
    }
}
fn bad(message: impl Into<String>) -> (StatusCode, String) {
    (StatusCode::BAD_REQUEST, message.into())
}
fn failed(message: impl Into<String>) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, message.into())
}
fn conflict(message: impl Into<String>) -> (StatusCode, String) {
    (StatusCode::CONFLICT, message.into())
}
fn room(state: &AppState, id: &str) -> Result<Value, (StatusCode, String)> {
    state
        .store
        .get("room", id)
        .ok_or((StatusCode::NOT_FOUND, "회의실을 찾을 수 없습니다.".into()))
}
fn lock(state: &AppState, id: &str) -> Result<crate::state::SessionGuard, (StatusCode, String)> {
    let key = format!("room-submit:{id}");
    if !state.begin_session(&key) {
        return Err(conflict("회의실 상태가 변경 중입니다. 다시 시도하세요."));
    }
    Ok(state.session_guard(&key))
}
#[cfg(test)]
fn validated_config(state:&AppState,id:&str,previous:&Value,body:&Value)->ApiResult{
    crate::features::collaboration::room_config::validated_config(state,id,previous,body).map_err(bad)
}
fn save_room(state:&AppState,id_override:Option<&str>,body:&Value)->ApiResult{
    use crate::features::collaboration::room_config::{self,ConfigError};
    room_config::save(state,id_override,body).map_err(|error|match error{
        ConfigError::Invalid(message)=>bad(message),
        ConfigError::Missing=>(StatusCode::NOT_FOUND,"그룹을 찾을 수 없습니다.".into()),
        ConfigError::Changed=>conflict("그룹 설정이 다른 곳에서 변경되었습니다. 입력한 내용은 유지하고 최신 설정을 확인하세요."),
        ConfigError::Storage(message)=>{eprintln!("Room configuration storage: {message}");failed("그룹 설정을 저장할 수 없습니다.")},
    })
}

pub async fn list(State(state): State<AppState>) -> Json<Value> {
    Json(json!(state.store.all("room")))
}
pub async fn save(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    response(save_room(&state, None, &body))
}
pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    response(save_room(&state, Some(&id), &body))
}
pub async fn one(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    response(room(&state, &id))
}

pub async fn messages(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    response((|| {
        room(&state, &id)?;
        let session = rooms::ensure_room_session(&state, &id).map_err(bad)?;
        let session_id = session["id"]
            .as_str()
            .ok_or_else(|| failed("회의실 세션 ID가 없습니다."))?;
        Ok(json!(state.store.messages(session_id)))
    })())
}

/// Committed per-Room event stream plus each agent's durable readCursor.
pub async fn events(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<HashMap<String,String>>,
)->Response{
    response((||{
        let config=room(&state,&id)?;
        let session=rooms::ensure_room_session(&state,&id).map_err(bad)?;
        let session_id=session["id"].as_str()
            .ok_or_else(||failed("Room session ID missing"))?;
        let after=query.get("after").map(|s|s.parse::<u64>())
            .transpose().map_err(|_|bad("Invalid Room event cursor"))?
            .unwrap_or(0);
        let limit=query.get("limit").map(|s|s.parse::<usize>())
            .transpose().map_err(|_|bad("Invalid Room event limit"))?
            .unwrap_or(100).clamp(1,200);
        let mut rows=state.store.room_events(session_id,after,limit+1)
            .map_err(failed)?;
        let has_more=rows.len()>limit;
        rows.truncate(limit);
        let next_cursor=rows.last().and_then(|v|v["seq"].as_u64()).unwrap_or(after);
        let bindings=config["agentIds"].as_array().into_iter().flatten()
            .filter_map(Value::as_str).map(|agent|{
                let key=crate::features::collaboration::room_cursor::binding_id(
                    session_id,agent);
                let saved=state.store.get("room_read_cursor",&key);
                json!({"roomId":id,"agentId":agent,"sessionId":session_id,
                    "readCursor":saved.as_ref()
                        .and_then(|c|c["throughSeq"].as_u64())
                        .or_else(||saved.as_ref()
                            .and_then(|c|c["throughMessageId"].as_str())
                            .and_then(|anchor|state.store.room_event_seq(session_id,anchor)))
                        .unwrap_or(0),
                    "state":"bound"})
            }).collect::<Vec<_>>();
        Ok(json!({"roomId":id,"sessionId":session_id,"events":rows,
            "nextCursor":next_cursor,"hasMore":has_more,"bindings":bindings}))
    })())
}

pub async fn system_prompt(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    response((|| {
        room(&state, &id)?;
        crate::engine::chat::system_prompt_preview(&state, &id, query.get("agentId").map(String::as_str))
            .map_err(bad)
    })())
}


/// Owner edits for the room prompt. `common` replaces the shared contract,
/// `final` replaces the whole static prompt; null clears either override.
pub async fn save_system_prompt(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    match crate::features::collaboration::prompt_settings::save_room(&state,&id,&body) {
        Ok(saved)=>Json(saved).into_response(),
        Err(value)=>super::response::prompt_error(value),
    }
}

pub async fn knowledge(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    response((|| {
        room(&state, &id)?;
        Ok(json!(crate::engine::room_knowledge::list(&state, &id)))
    })())
}

pub async fn learn(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    response((|| {
        room(&state, &id)?;
        let session = rooms::ensure_room_session(&state, &id).map_err(bad)?;
        let session_id = session["id"]
            .as_str()
            .ok_or_else(|| failed("회의실 세션 ID가 없습니다."))?;
        let through = state
            .store
            .messages(session_id)
            .last()
            .and_then(|message| message["id"].as_str().map(str::to_string))
            .ok_or_else(|| bad("학습할 대화가 없습니다."))?;
        crate::engine::room_knowledge::enqueue(&state, &id, session_id, &through)
            .ok_or_else(|| failed("방 학습 작업을 등록할 수 없습니다."))
    })())
}

fn submit(state: &AppState, id: &str, body: &Value) -> ApiResult {
    submit_internal(state, id, body, None, true)
}
pub(crate) fn submit_internal(
    state: &AppState, id: &str, body: &Value, origin_metadata: Option<Value>, tools_allowed: bool,
) -> ApiResult {
    use crate::features::collaboration::submission::{self, SubmissionError};
    submission::submit(state,id,body,origin_metadata,tools_allowed).map_err(|error| match error {
        SubmissionError::Invalid(message) => bad(message),
        SubmissionError::Missing => (StatusCode::NOT_FOUND,"그룹을 찾을 수 없습니다.".into()),
        SubmissionError::Busy(message) => conflict(message),
        SubmissionError::Storage(message) => {
            eprintln!("Room admission storage: {message}");
            failed("메시지 접수 기록을 저장하거나 확인할 수 없습니다.")
        },
    })
}
pub async fn send_message(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    response(submit(&state, &id, &body))
}
/// The historical /run endpoint uses exactly the same persisted owner flow.
pub async fn run(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    response(submit(&state, &id, &body))
}

fn stop_room(state: &AppState, id: &str) -> ApiResult {
    let _guard = lock(state, id)?;
    stop_room_locked(state, id)
}
fn stop_room_locked(state: &AppState, id: &str) -> ApiResult {
    let config = room(state, id)?;
    let session_id = config["sessionId"].as_str();
    let pending = state
        .store
        .all("job")
        .into_iter()
        .filter(|job| {
            job["kind"] == "room"
                && job["input"]["roomId"] == id
                && matches!(job["status"].as_str(), Some("queued" | "running"))
        })
        .collect::<Vec<_>>();
    if let Some(session_id) = session_id {
        state.cancel_session(session_id);
    }
    // Old imported jobs may lack a session binding until their first turn.
    for job in &pending {
        if let Some(job_id) = job["id"].as_str() {
            if let Some(saved)=state.store.patch_if("job",job_id,"cancelRequested",job.get("cancelRequested").unwrap_or(&Value::Null),
                &json!({"cancelRequested":true,"status":"cancelled","finishedAt":chrono::Utc::now().to_rfc3339()})){state.bus.record("job",&saved);}
        }
    }
    if let Some(saved) = state.store.patch_if(
        "room",
        id,
        "id",
        &json!(id),
        &json!({"stoppedAt":chrono::Utc::now().to_rfc3339()}),
    ) {
        state.bus.record("room", &saved);
    }
    Ok(
        json!({"ok":true,"roomId":id,"sessionId":session_id,"status":"stopped","cancelledJobs":pending.len()}),
    )
}
pub async fn stop(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    response(stop_room(&state, &id))
}
pub async fn remove(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    response((|| {
        let _guard = lock(&state, &id)?;
        stop_room_locked(&state, &id)?;
        if !state.store.remove("room", &id) {
            return Err(failed("회의실을 삭제할 수 없습니다."));
        }
        state
            .bus
            .emit(json!({"type":"record","kind":"room","removed":id}));
        // Keep the transcript as a durable archive; never erase owner messages.
        Ok(json!({"ok":true}))
    })())
}


/// Owner-launched Team task inside a group room. The engine turns the goal
/// into one worker task per eligible room agent and runs them through the
/// existing native team job journal.
pub async fn team_launch(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    response((|| {
        let goal = body["goal"]
            .as_str()
            .map(str::trim)
            .filter(|text| !text.is_empty() && text.len() <= 4000)
            .ok_or_else(|| bad("맡길 작업은 1~4000자여야 합니다."))?;
        let nonce = body["clientRequestId"]
            .as_str()
            .filter(|text| uuid::Uuid::parse_str(text).is_ok())
            .ok_or_else(|| bad("요청 ID가 올바르지 않습니다."))?;
        let parallel = body["maxParallel"].as_u64().unwrap_or(3);
        if !(1..=4).contains(&parallel) {
            return Err(bad("동시 작업은 1~4개까지 가능합니다."));
        }
        let _guard = lock(&state, &id)?;
        let launch_id = format!("room-team-launch:{}", memory::stable_id(&[&id, nonce]));
        let effective_parallel=parallel.min(3);
        if let Some(previous) = state.store.get("room_team_launch", &launch_id) {
            if previous["roomId"] != id || previous["goal"] != goal
                || previous["maxParallel"].as_u64().map(|v|v.min(3))!=Some(effective_parallel) {
                return Err(conflict("같은 요청 ID로 다른 작업을 시작할 수 없습니다."));
            }
            return Ok(project_team_launch(&state,previous));
        }
        let job_id=format!("room-team:{}",memory::stable_id(&[&id,nonce]));
        // If the engine persisted the job but the response/launch record was
        // lost, recover that job before considering a new dispatch.
        if let Some(existing)=state.store.get("job",&job_id){
            if existing["kind"]!="team"||existing["input"]["goal"]!=goal
                || existing["input"]["maxParallel"]!=json!(effective_parallel){
                return Err(conflict("기존 협업 작업의 요청 내용이 일치하지 않습니다."));
            }
            let session_id=existing["input"]["sessionId"].as_str()
                .ok_or_else(||conflict("기존 협업 작업의 대화 연결을 확인하세요."))?;
            let record=json!({"id":launch_id,"roomId":id,"sessionId":session_id,
                "status":"queued","goal":goal,"maxParallel":effective_parallel,
                "clientRequestId":nonce,"jobId":job_id,"createdAt":existing["queuedAt"]});
            let saved=state.store.put_if_absent("room_team_launch",&record)
                .or_else(||state.store.get("room_team_launch",&launch_id))
                .ok_or_else(||failed("협업 요청 기록을 복구하지 못했습니다."))?;
            return Ok(project_team_launch(&state,saved));
        }
        let config = room(&state, &id)?;
        if config["toolsEnabled"] != true {
            return Err((StatusCode::FORBIDDEN, "그룹의 도구 사용을 먼저 켜세요.".into()));
        }
        if config["status"].as_str() == Some("running") {
            return Err(conflict("그룹 대화가 진행 중입니다. 끝난 뒤 다시 시도하세요."));
        }
        let session = rooms::ensure_room_session(&state, &id).map_err(bad)?;
        let session_id = session["id"].as_str().ok_or_else(|| failed("그룹 세션이 없습니다."))?;
        let agents = config["agentIds"]
            .as_array()
            .map(|ids| {
                ids.iter()
                    .filter_map(|value| value.as_str())
                    .filter_map(|agent_id| state.store.get("agent", agent_id))
                    .filter(|agent| agent["enabled"] != false)
                    .filter(|agent| agent["providerId"].as_str().is_some_and(|p| !p.is_empty()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if agents.is_empty() {
            return Err(bad("이 그룹에는 작업을 맡길 수 있는 AI가 없습니다. AI 연결을 확인하세요."));
        }
        let mut tasks = Vec::new();
        let mut plan_steps = Vec::new();
        for (index, agent) in agents.iter().take(4).enumerate() {
            let task_id = format!("team-{}", index + 1);
            plan_steps.push((task_id.clone(), agent["id"].clone()));
            tasks.push(json!({
                "id": task_id,
                "prompt": format!("{goal}\n\n맡은 역할에 따라 조사하고, 실제로 확인한 내용만 한국어로 간결하게 보고하세요."),
                "mode": "read",
                "runtime": "native",
                "agentId": agent["id"],
            }));
        }
        let plan: Vec<Value> = plan_steps.iter().map(|(task_id,agent_id)|json!({
            "id":task_id,"agentId":agent_id,
            "jobId":crate::features::collaboration::task_team::child_id(&job_id,task_id)
        })).collect();
        // Persist complete immutable input before waking the scheduler.
        let team = json!({"goal":goal,"tasks":tasks,"maxParallel":effective_parallel,
            "plan":plan,"sessionId":session_id});
        let Some(job)=crate::engine::jobs::enqueue_once(&state,&job_id,"team",team) else{
            return Err(failed("Team 작업을 시작하지 못했습니다."));
        };
        // Patch only metadata. Never put a stale queued job over running or
        // completed work. The immutable input already carries sessionId.
        if let Some(saved)=state.store.patch_if("job",&job_id,"id",&json!(job_id),
            &json!({"sessionId":session_id,"roomId":id,"teamLaunchId":launch_id})){
            state.bus.record("job",&saved);
        }
        let launch=json!({"id":launch_id,"roomId":id,"sessionId":session_id,
            "status":"queued","goal":goal,"maxParallel":effective_parallel,
            "createdAt":job["queuedAt"],"clientRequestId":nonce,"jobId":job_id,"error":Value::Null});
        let saved=state.store.put_if_absent("room_team_launch",&launch)
            .or_else(||state.store.get("room_team_launch",&launch_id))
            .ok_or_else(||failed("협업 작업은 접수되었으나 요청 기록 저장에 실패했습니다. 같은 요청으로 다시 확인하세요."))?;
        state.bus.record("room_team_launch", &saved);
        Ok(project_team_launch(&state,saved))
    })())
}

/// Actual job state is separate from the legacy launch metadata. No model
/// planning occurs here; the room submits bounded read-only native tasks.
fn project_team_launch(state:&AppState,mut launch:Value)->Value{
    if let Some(job_id)=launch["jobId"].as_str(){
        if let Some(job)=state.store.get("job",job_id){
            launch["status"]=job["status"].clone();
            launch["error"]=job["error"].clone();
            launch["maxParallel"]=job["input"]["maxParallel"].clone();
            launch["taskCount"]=json!(job["input"]["tasks"].as_array().map_or(0,Vec::len));
            return launch;
        }
    }
    launch["status"]=json!("uncertain");
    launch["error"]=json!("협업 작업 기록을 확인할 수 없습니다.");
    launch
}

pub async fn team_launch_status(
    State(state): State<AppState>,
    Path((id, launch_id)): Path<(String, String)>,
) -> Response {
    response((|| {
        let Some(launch)=state.store.get("room_team_launch",&launch_id) else{
            return Err((StatusCode::NOT_FOUND,"Team 요청을 찾을 수 없습니다.".into()));
        };
        if launch["roomId"].as_str()!=Some(id.as_str()){
            return Err((StatusCode::NOT_FOUND,"Team 요청을 찾을 수 없습니다.".into()));
        }
        Ok(project_team_launch(&state,launch))
    })())
}

/// Archives the current room transcript and binds the room to a fresh empty
/// session. The room, members, settings and knowledge stay intact; the old
/// session and all of its messages remain available as a durable archive.
pub async fn reset(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    response((|| {
        let _guard = lock(&state, &id)?;
        stop_room_locked(&state, &id)?;
        let current = rooms::ensure_room_session(&state, &id).map_err(bad)?;
        let previous_session_id = current["id"]
            .as_str()
            .ok_or_else(|| failed("회의실 세션 ID가 없습니다."))?
            .to_string();
        crate::engine::chat::close_session_approvals(&state, &previous_session_id);

        let current_room = room(&state, &id)?;
        let title = current_room
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("대화방");
        let session_id = format!("room:{id}:{}", uuid::Uuid::new_v4().simple());
        let record = json!({
            "id": session_id,
            "title": title,
            "roomId": id,
            "kind": "room",
            "createdAt": chrono::Utc::now().to_rfc3339(),
        });
        if state.store.put_if_absent("session", &record).is_none() {
            return Err(failed("새 대화방 세션을 만들 수 없습니다."));
        }
        let binding = current_room.get("sessionId").unwrap_or(&Value::Null);
        let Some(saved) = state.store.patch_if(
            "room",
            &id,
            "sessionId",
            binding,
            &json!({
                "sessionId": session_id,
                "status": "idle",
                "pauseReason": Value::Null,
                "activeAgentId": Value::Null,
                "resetAt": chrono::Utc::now().to_rfc3339(),
            }),
        ) else {
            return Err(conflict("대화방 세션이 동시에 변경되었습니다. 다시 시도하세요."));
        };
        state.bus.record("room", &saved);
        crate::engine::room_state::clear(&state, &id);
        Ok(json!({
            "ok": true,
            "roomId": id,
            "previousSessionId": previous_session_id,
            "sessionId": session_id,
        }))
    })())
}

pub async fn state(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    response((|| {
        let config = room(&state, &id)?;
        let session = rooms::ensure_room_session(&state, &id).map_err(bad)?;
        let session_id = session["id"]
            .as_str()
            .ok_or_else(|| failed("회의실 세션 ID가 없습니다."))?;
        let snapshot = state
            .store
            .room_status_records(&id, session_id)
            .map_err(failed)?;
        let jobs = snapshot.room_jobs;
        let queue_depth = snapshot.room_queue_depth;
        let running = snapshot.room_running_job.as_ref();
        let active_agent = running
            .and_then(|job| job["roomState"]["activeAgentId"].as_str())
            .or_else(|| {
                if running.is_some() {
                    config["activeAgentId"].as_str()
                } else {
                    None
                }
            });
        let status = if running.is_some() {
            "running"
        } else if queue_depth > 0 {
            "queued"
        } else if !config["stoppedAt"].is_null() {
            "stopped"
        } else if matches!(config["status"].as_str(), Some("paused" | "needs_review")) {
            config["status"].as_str().unwrap_or("idle")
        } else {
            jobs.first()
                .and_then(|job| job["status"].as_str())
                .filter(|status| matches!(*status, "failed" | "needs_review"))
                .unwrap_or("idle")
        };
        let executions = snapshot.executions;
        let learning = snapshot.learning_jobs;
        let operations = snapshot.operations.into_iter()
            .map(|operation|json!({"id":operation["id"],"runId":operation["runId"],"tool":operation["tool"],"status":operation["status"],
                "startedAt":operation["startedAt"],"finishedAt":operation["finishedAt"],
                "arguments":bounded_json(&operation["arguments"],2000),"result":bounded_json(&operation["result"],6000)})).collect::<Vec<_>>();
        let approvals = snapshot.approvals;
        Ok(
            json!({"room":room(&state,&id)?,"sessionId":session_id,"status":status,"activeAgentId":active_agent,"queueDepth":queue_depth,
            "jobs":jobs,"executions":executions,"learningJobs":learning,"operations":operations,"approvals":approvals,"usage":state.store.session_usage(session_id)}),
        )
    })())
}

fn bounded_json(value: &Value, limit: usize) -> Value {
    let text = value.to_string();
    if text.chars().count() <= limit {
        value.clone()
    } else {
        json!({"preview":text.chars().take(limit).collect::<String>(),"truncated":true})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_room_can_be_created_without_picking_members() {
        let (state, dir) = test_state();
        let body = json!({"name":"빠른 방"});
        let saved = validated_config(&state, "room-quick", &json!({}), &body).unwrap();
        let members = saved["agentIds"].as_array().unwrap();
        assert!(!members.is_empty() && members.len() <= 3, "defaults must stay small");
        drop(state);
        let _ = std::fs::remove_dir_all(dir);
    }

    fn test_state() -> (AppState, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("dominion-room-api-{}", uuid::Uuid::new_v4()));
        let config = crate::config::Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let state = AppState::new(
            config.clone(),
            crate::db::Store::open(&config.db_path()).unwrap(),
            crate::vault::Vault::open(&config.key_path()),
        );
        for index in 0..9 {
            state.store.put(
                "agent",
                &json!({"id":format!("a{index}"),"name":format!("참가자{index}")}),
            );
        }
        (state, dir)
    }
    fn config() -> Value {
        json!({"id":"room-one","name":"우리 방","agentIds":(0..9).map(|i|format!("a{i}")).collect::<Vec<_>>(),"routingMode":"discussion"})
    }
    #[tokio::test]
    async fn room_team_launch_reuses_request_and_preserves_live_job_state(){
        let (app,dir)=test_state();
        let mut room_config=config();
        room_config["toolsEnabled"]=json!(true);
        save_room(&app,None,&room_config).unwrap();
        for index in 0..9 {
            let id=format!("a{index}");
            let agent=app.store.get("agent",&id).unwrap();
            app.store.patch_if("agent",&id,"id",&agent["id"],&json!({"providerId":"test-provider"})).unwrap();
        }
        let nonce=uuid::Uuid::new_v4().to_string();
        let body=json!({"goal":"inspect","maxParallel":4,"clientRequestId":nonce});
        let first=team_launch(State(app.clone()),Path("room-one".into()),Json(body.clone())).await;
        assert_eq!(first.status(),StatusCode::OK);
        let data:Value=serde_json::from_slice(&axum::body::to_bytes(first.into_body(),usize::MAX).await.unwrap()).unwrap();
        assert_eq!(data["status"],"queued");assert_eq!(data["maxParallel"],3);
        assert_eq!(data["taskCount"],4);
        let id=data["jobId"].as_str().unwrap();
        app.store.patch_if("job",id,"status",&json!("queued"),&json!({"status":"running","attempts":1})).unwrap();
        let second=team_launch(State(app.clone()),Path("room-one".into()),Json(body.clone())).await;
        let replay:Value=serde_json::from_slice(&axum::body::to_bytes(second.into_body(),usize::MAX).await.unwrap()).unwrap();
        assert_eq!(replay["jobId"],data["jobId"]);assert_eq!(replay["status"],"running");
        assert_eq!(app.store.all("job").iter().filter(|j|j["kind"]=="team").count(),1);
        let mut changed=body;changed["maxParallel"]=json!(1);
        assert_eq!(team_launch(State(app.clone()),Path("room-one".into()),Json(changed)).await.status(),StatusCode::CONFLICT);
        drop(app);let _=std::fs::remove_dir_all(dir);
    }
    #[tokio::test]
    async fn room_status_is_scoped_to_current_room_and_preserves_legacy_operation_receipts() {
        let (app, dir) = test_state();
        save_room(&app, None, &config()).unwrap();
        let room_session = rooms::ensure_room_session(&app, "room-one").unwrap();
        let sid = room_session["id"].as_str().unwrap().to_string();
        for index in 0..80 {
            app.store.put("job", &json!({
                "id":format!("other-job-{index}"),"kind":"room","status":"queued",
                "input":{"roomId":"unrelated"},"queuedAt":"2026-11-21T10:00:00Z"
            })).unwrap();
            app.store.put("execution", &json!({
                "id":format!("other-exec-{index}"),"sessionId":"unrelated",
                "status":"completed"
            })).unwrap();
            app.store.put("execution_operation", &json!({
                "id":format!("other-op-{index}"),"sessionId":"unrelated",
                "runId":format!("other-exec-{index}"),"status":"succeeded"
            })).unwrap();
            app.store.put("approval", &json!({
                "id":format!("other-approval-{index}"),"sessionId":"unrelated",
                "status":"approved"
            })).unwrap();
        }
        app.store.put("job", &json!({
            "id":"room-running","kind":"room","status":"running",
            "queuedAt":"2026-11-21T11:00:00Z",
            "input":{"roomId":"room-one"},"roomState":{"activeAgentId":"a0"}
        })).unwrap();
        app.store.put("job", &json!({
            "id":"room-learning","kind":"learn","status":"completed",
            "input":{"sessionId":sid}
        })).unwrap();
        app.store.put("execution", &json!({
            "id":"room-exec","sessionId":sid,"status":"completed"
        })).unwrap();
        app.store.put("execution_operation", &json!({
            "id":"legacy-room-op","runId":"room-exec","status":"succeeded",
            "tool":"workspace_read","arguments":{"path":"note.txt"}
        })).unwrap();
        app.store.put("approval", &json!({
            "id":"room-approval","sessionId":sid,"status":"approved"
        })).unwrap();
        let response = state(State(app.clone()), Path("room-one".into())).await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 64*1024).await.unwrap();
        let output: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(output["status"], "running");
        assert_eq!(output["queueDepth"], 0);
        assert_eq!(output["activeAgentId"], "a0");
        assert_eq!(output["jobs"].as_array().unwrap().len(), 1);
        assert_eq!(output["jobs"][0]["id"], "room-running");
        assert_eq!(output["learningJobs"][0]["id"], "room-learning");
        assert_eq!(output["executions"][0]["id"], "room-exec");
        assert_eq!(output["operations"][0]["id"], "legacy-room-op");
        assert_eq!(output["approvals"][0]["id"], "room-approval");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn durable_room_events_and_agent_cursor_survive_an_api_page(){
        let (state,dir)=test_state();
        let _saved=save_room(&state,None,&config()).unwrap();
        let session=rooms::ensure_room_session(&state,"room-one").unwrap();
        let session_id=session["id"].as_str().unwrap().to_string();
        let owner=state.store.write_message(&json!({
            "id":"owner-room-event","session_id":session_id,
            "role":"user","content":"Test", "metadata":{"authorKind":"owner"}
        })).unwrap();
        let source_id=owner["id"].as_str().unwrap();
        let agent=state.store.write_room_answer_with_cursor(
            &json!({"id":"agent-room-event","session_id":session_id,
                "role":"assistant","agent_id":"a0","content":"Observed",
                "metadata":{"authorKind":"agent","replyToMessageId":source_id}}),
            &crate::features::collaboration::room_cursor::checkpoint(
                &session_id,"a0",source_id,source_id)
        ).unwrap();
        assert_eq!(agent["id"],"agent-room-event");
        let response=events(State(state.clone()),Path("room-one".into()),
            Query(HashMap::from([("after".to_string(),"0".to_string()),
                ("limit".to_string(),"1".to_string())]))).await;
        assert_eq!(response.status(),StatusCode::OK);
        let bytes=axum::body::to_bytes(response.into_body(),64*1024).await.unwrap();
        let page:Value=serde_json::from_slice(&bytes).unwrap();
        assert_eq!(page["events"][0]["eventId"],"owner-room-event");
        assert_eq!(page["events"][0]["seq"],1);
        assert_eq!(page["hasMore"],true);
        assert_eq!(page["nextCursor"],1);
        let cursor=page["bindings"].as_array().unwrap().iter()
            .find(|v|v["agentId"]=="a0").unwrap();
        assert_eq!(cursor["readCursor"],1);
        let page_two=events(State(state.clone()),Path("room-one".into()),
            Query(HashMap::from([("after".to_string(),"1".to_string())]))).await;
        let bytes=axum::body::to_bytes(page_two.into_body(),64*1024).await.unwrap();
        let page:Value=serde_json::from_slice(&bytes).unwrap();
        assert_eq!(page["events"][0]["eventId"],"agent-room-event");
        assert_eq!(page["events"][0]["seq"],2);
        crate::test_support::cleanup_dir(dir);
    }
    #[test]
    fn nine_agent_roster_has_stable_private_session_and_safe_defaults() {
        let (state, dir) = test_state();
        let saved = save_room(&state, None, &config()).unwrap();
        assert_eq!(saved["agentIds"].as_array().unwrap().len(), 9);
        assert_eq!(saved["toolsEnabled"], false);
        assert_eq!(saved["autoLearning"], false);
        let session_id = saved["sessionId"].as_str().unwrap();
        let session = state.store.get("session", session_id).unwrap();
        assert_eq!(session["roomId"], "room-one");
        let update = save_room(
            &state,
            Some("room-one"),
            &json!({"name":"오래 가는 방","rounds":2}),
        )
        .unwrap();
        assert_eq!(update["sessionId"], session_id);
        assert!(save_room(
            &state,
            None,
            &json!({"id":"rogue","name":"탈취","agentIds":["a0"],"sessionId":session_id})
        )
        .is_err());
        assert!(save_room(
            &state,
            Some("room-one"),
            &json!({"sessionId":"private-session"})
        )
        .is_err());
        assert!(save_room(&state, Some("room-one"), &json!({"agentIds":["missing"]})).is_err());
        assert!(save_room(&state, Some("room-one"), &json!({"activeAgentId":"a8"})).is_err());
        assert!(save_room(&state, Some("room-one"), &json!({"maxTurns":100001})).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }
    #[test]
    fn local_runtime_models_can_be_selected_for_a_room() {
        let (state, dir) = test_state();
        state.store.put(
            "provider",
            &json!({
                "id":"local-mlx","name":"local MLX","kind":"mlx","local":true,
                "baseUrl":"http://127.0.0.1:1234/v1","model":"mlx-7b","enabled":true
            }),
        );
        let saved = save_room(
            &state,
            None,
            &json!({
                "id":"local-room","name":"로컬 방","agentIds":["a0","a1"],
                "models":[{"providerId":"local-mlx","model":"mlx-7b"}]
            }),
        )
        .unwrap();
        assert_eq!(saved["models"][0]["providerId"], "local-mlx", "{saved}");
        assert_eq!(saved["models"][0]["model"], "mlx-7b", "{saved}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn room_attachments_and_response_overrides_are_bound_and_immutable() {
        use base64::Engine;
        let (state, dir) = test_state();
        save_room(&state, None, &config()).unwrap();
        state
            .store
            .put("provider", &json!({"id":"chosen","enabled":true}));
        let file=crate::engine::attachments::upload(&state,&json!({"roomId":"room-one","name":"notes.txt","data":base64::engine::general_purpose::STANDARD.encode("실제 사용자 원문") })).unwrap();
        let body = json!({"input":"첨부 확인","attachmentIds":[file["id"]],"thinking":false,"reasoningEffort":"minimal","providerId":"chosen","model":"chosen-model","clientMessageId":"file-turn"});
        let submitted = submit(&state, "room-one", &body).unwrap();
        assert_eq!(
            submitted["message"]["metadata"]["attachments"][0]["id"],
            file["id"]
        );
        assert_eq!(submitted["job"]["input"]["reasoningEffort"], "minimal");
        assert_eq!(submitted["job"]["input"]["providerId"], "chosen");
        let mut changed = body.clone();
        changed["thinking"] = json!(true);
        assert!(submit(&state, "room-one", &changed).is_err());
        let internal = submit_internal(
            &state,
            "room-one",
            &json!({"input":"외부 명령","clientMessageId":"external"}),
            Some(json!({"transport":"telegram","messageId":"inbound-one"})),
            false,
        )
        .unwrap();
        assert_eq!(
            internal["job"]["input"]["roomSnapshot"]["toolsEnabled"],
            false
        );
        assert_eq!(
            internal["message"]["metadata"]["origin"]["transport"],
            "telegram"
        );
        assert!(submit_internal(
            &state,
            "room-one",
            &json!({"input":"봇을 주인으로 속임"}),
            Some(json!({"authorIsBot":true})),
            false
        )
        .is_err());
        let _ = std::fs::remove_dir_all(dir);
    }
    #[test]
    fn owner_message_and_job_are_atomic_idempotent_and_stop_preserves_transcript() {
        let (state, dir) = test_state();
        save_room(&state, None, &config()).unwrap();
        let body =
            json!({"input":"아홉 명 다 같이 계획을 논의해","clientMessageId":"owner-message-one"});
        let sent = submit(&state, "room-one", &body).unwrap();
        let session_id = sent["sessionId"].as_str().unwrap();
        let messages = state.store.messages(session_id);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["role"], "user");
        assert!(messages[0]["agent_id"].is_null());
        assert_eq!(messages[0]["metadata"]["authorKind"], "owner");
        assert_eq!(
            sent["job"]["input"]["agentIds"].as_array().unwrap().len(),
            9
        );
        assert_eq!(sent["job"]["input"]["sourceMessageId"], messages[0]["id"]);
        assert_eq!(sent["job"]["input"]["roomSnapshot"]["toolsEnabled"], false);
        let again = submit(&state, "room-one", &body).unwrap();
        assert_eq!(sent["jobId"], again["jobId"]);
        assert_eq!(state.store.messages(session_id).len(), 1);
        assert_eq!(state.store.count("job"), 1);
        assert!(submit(
            &state,
            "room-one",
            &json!({"input":"다른 문장","clientMessageId":"owner-message-one"})
        )
        .is_err());
        assert!(submit(
            &state,
            "room-one",
            &json!({"input":"화자 탈취","role":"assistant"})
        )
        .is_err());
        assert!(submit(
            &state,
            "room-one",
            &json!({"input":"다른 방 참가자","agentIds":["outsider"]})
        )
        .is_err());
        let original_generation = sent["job"]["input"]["generation"].as_u64().unwrap();
        let stopped = stop_room(&state, "room-one").unwrap();
        assert_eq!(stopped["cancelledJobs"], 1);
        let cancelled = state
            .store
            .get("job", sent["jobId"].as_str().unwrap())
            .unwrap();
        assert_eq!(cancelled["status"], "cancelled");
        assert_eq!(cancelled["cancelRequested"], true);
        assert!(state.is_cancelled(session_id));
        assert_eq!(state.store.messages(session_id).len(), 1);
        let next = submit(
            &state,
            "room-one",
            &json!({"input":"이제 새 주제로 다시 시작해","clientMessageId":"second"}),
        )
        .unwrap();
        assert!(next["job"]["input"]["generation"].as_u64().unwrap() > original_generation);
        // Submitting fresh work never clears the stop of an older running turn.
        assert!(state.is_cancelled(session_id));
        assert_eq!(state.store.messages(session_id).len(), 2);
        let _ = std::fs::remove_dir_all(dir);
    }
}
