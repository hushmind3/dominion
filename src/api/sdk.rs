//! Native Rust SDK entrypoints: the GUI and external clients execute the
//! same Dominion-owned agent kernel. Runs are durable, inspectable and resumable.
use crate::{engine::gateway, state::AppState};
use axum::{extract::{Path, State}, http::StatusCode, response::{IntoResponse, Response}, Json};
use dominion_harness::{
    openai::OpenAiProvider,
    sdk::{AgentSdk, CommandVerifier, FileJournal, RunStatus, Session},
    tools::{ListDir, PatchFile, ReadFile, Shell, Workspace, WriteFile},
    Decision, Limits, Permission, Policy, ToolSpec,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{atomic::{AtomicBool, Ordering}, Arc, Mutex, OnceLock};

use super::response::error;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Start {
    instruction: String,
    provider_id: Option<String>,
    model: Option<String>,
    verify_command: Option<String>,
    max_rounds: Option<usize>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Approve { call_id: String }
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReconcileInput { applied: bool, details: String }

fn active_flags() -> &'static Mutex<HashMap<String, Arc<AtomicBool>>> {
    static FLAGS: OnceLock<Mutex<HashMap<String, Arc<AtomicBool>>>> = OnceLock::new();
    FLAGS.get_or_init(|| Mutex::new(HashMap::new()))
}

struct ProductPolicy;
impl Policy for ProductPolicy {
    fn decide(&self, spec: &ToolSpec) -> Decision {
        match spec.permission {
            Permission::Read => Decision::Allow,
            Permission::Write | Permission::Execute => Decision::Ask,
            Permission::Network | Permission::Dangerous => Decision::Deny,
        }
    }
}

fn journal(state: &AppState, id: &str) -> FileJournal {
    FileJournal::new(state.config.data_dir.join("sdk-runs").join(format!("{id}.json")))
}

fn public_view(state: &AppState, record: Value) -> Value {
    let id = record["id"].as_str().unwrap_or("");
    let snapshot = journal(state, id).load().ok().flatten();
    let current = snapshot.as_ref().map(|snapshot| {
        json!({
            "state":snapshot.status,
            "rounds":snapshot.rounds,
            "toolCalls":snapshot.tool_calls,
            "usage":if snapshot.usage_reports > 0 { Some(snapshot.usage) } else { None },
            "events":snapshot.events,
            "pendingCall":snapshot.pending_calls.first(),
            "answer":snapshot.final_text(),
        })
    }).unwrap_or(Value::Null);
    let mut result = record;
    result["checkpoint"] = current;
    let id = result["id"].as_str().unwrap_or("");
    let live = state.active.lock().unwrap().contains(&format!("sdk:{id}"));
    if result["status"] == "running" && !live {
        result["status"] = json!("interrupted");
    }
    result
}

pub async fn list(State(state): State<AppState>) -> Json<Value> {
    let runs = state.store.all("sdk_run").into_iter().take(30)
        .map(|record| public_view(&state, record)).collect::<Vec<_>>();
    Json(json!(runs))
}
pub async fn one(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match state.store.get("sdk_run", &id) {
        Some(record) => Json(public_view(&state, record)).into_response(),
        None => error(StatusCode::NOT_FOUND, "작업을 찾을 수 없습니다."),
    }
}

pub async fn start(State(state): State<AppState>, Json(input): Json<Start>) -> Response {
    if input.instruction.trim().is_empty() || input.instruction.len() > 64_000 {
        return error(StatusCode::BAD_REQUEST, "작업 지시는 1~64000자로 입력하세요.");
    }
    if input.verify_command.as_ref().is_some_and(|command| command.len() > 2048) {
        return error(StatusCode::BAD_REQUEST, "검증 명령이 너무 깁니다.");
    }
    let route = match gateway::resolve(
        &state, input.provider_id.as_deref().unwrap_or(""), input.model.as_deref(),
    ) {
        Ok(route) => route,
        Err(message) => return error(StatusCode::BAD_REQUEST, &message),
    };
    if matches!(route.kind.as_str(), "cli" | "anthropic") {
        return error(StatusCode::BAD_REQUEST,
            "현재 Rust SDK 실행기는 OpenAI 호환 호출을 지원합니다. 호환 모델을 선택하세요.");
    }
    let id = uuid::Uuid::new_v4().to_string();
    let record = json!({
        "id":id,"status":"queued","instruction":input.instruction,
        "providerId":route.id,"model":route.model,
        "verifyCommand":input.verify_command,
        "maxRounds":input.max_rounds.unwrap_or(24).clamp(1,80),
    });
    let Some(saved) = state.store.put("sdk_run", &record) else {
        return error(StatusCode::INTERNAL_SERVER_ERROR, "작업을 저장하지 못했습니다.");
    };
    match launch(state.clone(), id, None) {
        Ok(()) => (StatusCode::ACCEPTED, Json(public_view(&state, saved))).into_response(),
        Err(message) => error(StatusCode::CONFLICT, &message),
    }
}

fn launch(state: AppState, id: String, approved: Option<String>) -> Result<(), String> {
    let lock_id = format!("sdk:{id}");
    if !state.begin_session(&lock_id) { return Err("이미 실행 중인 작업입니다.".into()); }
    tokio::spawn(async move {
        let _guard = state.session_guard(&lock_id);
        if let Some(record) = state.store.get("sdk_run", &id) {
            if !matches!(record["status"].as_str(), Some("completed" | "cancelled")) {
                if let Some(saved) = state.store.patch_if(
                    "sdk_run", &id, "status", &record["status"], &json!({"status":"running"})
                ) { state.bus.record("sdk_run", &saved); }
            }
        }
        let result = execute(state.clone(), id.clone(), approved).await;
        let patch = match result {
            Ok(session) => {
                let status = match &session.status {
                    RunStatus::Completed => "completed",
                    RunStatus::AwaitingApproval { .. } => "awaiting_approval",
                    RunStatus::InDoubt { .. } => "in_doubt",
                    RunStatus::Cancelled => "cancelled",
                    RunStatus::Ready | RunStatus::Running => "interrupted",
                    RunStatus::Denied { .. } => "denied",
                    RunStatus::Exhausted { .. } => "exhausted",
                    RunStatus::Failed { .. } => "failed",
                };
                json!({"status":status,"rounds":session.rounds,
                    "toolCalls":session.tool_calls,
                    "usage":if session.usage_reports > 0 { Some(session.usage) } else { None },
                    "answer":session.final_text(),
                    "runState":session.status})
            }
            Err(message) => json!({"status":"failed","error":message}),
        };
        if !state.is_cancelled(&lock_id) {
            if let Some(mut record) = state.store.get("sdk_run", &id) {
                if let Some(patch) = patch.as_object() {
                    for (key, value) in patch { record[key] = value.clone(); }
                }
                if let Some(saved) = state.store.put("sdk_run", &record) {
                    state.bus.record("sdk_run", &saved);
                }
            }
        } else if let Some(saved) = state.store.get("sdk_run", &id) {
            state.bus.record("sdk_run", &saved);
        }
    });
    Ok(())
}

async fn execute(state: AppState, id: String, approved: Option<String>) -> Result<Session, String> {
    let record = state.store.get("sdk_run", &id).ok_or("run record missing")?;
    let route = gateway::resolve(
        &state, record["providerId"].as_str().unwrap_or(""),
        record["model"].as_str(),
    )?;
    if matches!(route.kind.as_str(), "cli" | "anthropic") {
        return Err("provider no longer supports this SDK protocol".into());
    }
    let workdir = state.config.workspace.clone();
    let verify = record["verifyCommand"].as_str().map(str::to_owned);
    let instruction = record["instruction"].as_str().ok_or("instruction missing")?.to_owned();
    let rounds = record["maxRounds"].as_u64().unwrap_or(24).clamp(1,80) as usize;
    let checkpoint = journal(&state, &id);
    let flag = Arc::new(AtomicBool::new(state.is_cancelled(&format!("sdk:{id}"))));
    active_flags().lock().unwrap().insert(id.clone(), Arc::clone(&flag));
    let worker_id = id.clone();
    let result = tokio::task::spawn_blocking(move || {
        let mut tools: Vec<Box<dyn dominion_harness::Tool + Send>> = vec![
            Box::new(ReadFile(Workspace::new(&workdir)?)),
            Box::new(ListDir(Workspace::new(&workdir)?)),
            Box::new(WriteFile(Workspace::new(&workdir)?)),
            Box::new(PatchFile(Workspace::new(&workdir)?)),
            Box::new(Shell { workspace: Workspace::new(&workdir)?, timeout_secs: 300 }),
        ];
        let specs = tools.iter().map(|tool| tool.spec()).collect();
        let mut engine = AgentSdk::new(
            Box::new(OpenAiProvider::new(
                if route.kind == "ollama" && !route.base_url.ends_with("/v1") {
                    format!("{}/v1", route.base_url.trim_end_matches('/'))
                } else { route.base_url },
                route.api_key, route.model,
            ).with_tools(specs)),
            Box::new(ProductPolicy),
            Limits { max_rounds: rounds, max_tool_calls: 160, ..Limits::default() },
        );
        for tool in tools.drain(..) { engine.register(tool); }
        engine.set_journal(checkpoint.clone());
        engine.set_cancel_handle(flag);
        if let Some(command) = verify {
            engine.set_verifier(Box::new(CommandVerifier {
                command, shell: Shell { workspace: Workspace::new(&workdir)?, timeout_secs: 300 }
            }));
        }
        if let Some(call_id) = approved { engine.approve_once(call_id); }
        let mut session = checkpoint.load()?.unwrap_or_else(|| Session::new(
            worker_id,
            "You are DOMINION's native Rust agent. Follow the exact user objective. Inspect before editing. Never pretend a tool succeeded. Verify work and repair failures.",
            instruction,
        ));
        engine.run(&mut session)?;
        Ok::<Session, String>(session)
    }).await.map_err(|error| format!("run worker failed: {error}"))?;
    active_flags().lock().unwrap().remove(&id);
    result
}

pub async fn resume(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let Some(record) = state.store.get("sdk_run", &id) else {
        return error(StatusCode::NOT_FOUND, "작업이 없습니다.");
    };
    if matches!(record["status"].as_str(), Some("completed" | "cancelled")) {
        return error(StatusCode::CONFLICT, "종료된 작업은 재개하지 않습니다.");
    }
    if state.is_cancelled(&format!("sdk:{id}")) {
        return error(StatusCode::CONFLICT, "취소된 실행은 재개하지 않습니다.");
    }
    match launch(state.clone(), id, None) {
        Ok(()) => (StatusCode::ACCEPTED, Json(record)).into_response(),
        Err(message) => error(StatusCode::CONFLICT, &message),
    }
}

pub async fn approve(State(state): State<AppState>, Path(id): Path<String>, Json(input): Json<Approve>) -> Response {
    let Some(record) = state.store.get("sdk_run", &id) else {
        return error(StatusCode::NOT_FOUND, "작업이 없습니다.");
    };
    let Some(snapshot) = journal(&state, &id).load().ok().flatten() else {
        return error(StatusCode::CONFLICT, "승인 대기 상태가 아닙니다.");
    };
    if !matches!(snapshot.status, RunStatus::AwaitingApproval { ref call_id, .. } if call_id == &input.call_id) {
        return error(StatusCode::CONFLICT, "승인 대상 도구 호출 ID가 일치하지 않습니다.");
    }
    if state.is_cancelled(&format!("sdk:{id}")) {
        return error(StatusCode::CONFLICT, "취소된 작업입니다.");
    }
    match launch(state.clone(), id, Some(input.call_id)) {
        Ok(()) => (StatusCode::ACCEPTED, Json(record)).into_response(),
        Err(message) => error(StatusCode::CONFLICT, &message),
    }
}

/// An interrupted effect must be inspected outside the agent before resume.
/// Reconciliation records the operator's observation, never replays the tool.
pub async fn reconcile(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<ReconcileInput>,
) -> Response {
    let details = input.details.trim();
    if details.is_empty() || details.len() > 8192 {
        return error(StatusCode::BAD_REQUEST, "실제 확인한 결과를 1~8192자로 입력하세요.");
    }
    let Some(mut record) = state.store.get("sdk_run", &id) else {
        return error(StatusCode::NOT_FOUND, "작업이 없습니다.");
    };
    if record["status"] == "cancelled" || record["status"] == "completed" {
        return error(StatusCode::CONFLICT, "종료된 작업은 복구할 수 없습니다.");
    }
    let lock_id = format!("sdk:{id}");
    if !state.begin_session(&lock_id) {
        return error(StatusCode::CONFLICT, "작업 실행 중에는 복구 결과를 변경할 수 없습니다.");
    }
    let _guard = state.session_guard(&lock_id);
    let checkpoint = journal(&state, &id);
    let Some(mut session) = checkpoint.load().ok().flatten() else {
        return error(StatusCode::CONFLICT, "복구할 실행 기록이 없습니다.");
    };
    let observed = json!({
        "applied":input.applied,
        "details":details,
        "operatorConfirmed":true,
    });
    if let Err(message) = session.reconcile_observation(observed) {
        return error(StatusCode::CONFLICT, &message);
    }
    if let Err(message) = checkpoint.save(&session) {
        return error(StatusCode::INTERNAL_SERVER_ERROR, &message);
    }
    record["status"] = json!("interrupted");
    record["runState"] = json!(session.status);
    let Some(saved) = state.store.put("sdk_run", &record) else {
        return error(StatusCode::INTERNAL_SERVER_ERROR, "복구 상태를 저장할 수 없습니다.");
    };
    state.bus.record("sdk_run", &saved);
    Json(public_view(&state, saved)).into_response()
}

pub async fn cancel(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let Some(mut record) = state.store.get("sdk_run", &id) else {
        return error(StatusCode::NOT_FOUND, "작업이 없습니다.");
    };
    if matches!(record["status"].as_str(), Some("completed" | "cancelled")) {
        return error(StatusCode::CONFLICT, "이미 종료된 작업입니다.");
    }
    state.cancel_session(&format!("sdk:{id}"));
    if let Some(flag) = active_flags().lock().unwrap().get(&id) {
        flag.store(true, Ordering::SeqCst);
    }
    record["status"] = json!("cancelled");
    match state.store.put("sdk_run", &record) {
        Some(saved) => Json(saved).into_response(),
        None => error(StatusCode::INTERNAL_SERVER_ERROR, "취소 상태 저장에 실패했습니다."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mutating_tools_require_an_explicit_single_call_approval() {
        let spec = |permission| ToolSpec {
            name:"tool".into(), description:"tool".into(), parameters:json!({}), permission
        };
        assert_eq!(ProductPolicy.decide(&spec(Permission::Read)), Decision::Allow);
        assert_eq!(ProductPolicy.decide(&spec(Permission::Write)), Decision::Ask);
        assert_eq!(ProductPolicy.decide(&spec(Permission::Execute)), Decision::Ask);
        assert_eq!(ProductPolicy.decide(&spec(Permission::Dangerous)), Decision::Deny);
    }
}
