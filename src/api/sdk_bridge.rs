//! Compatibility facade for the main-branch NativeAgentPanel.
//!
//! There is one authoritative agent engine: the existing durable Rust "prompt"
//! worker. These endpoints expose its jobs and approvals, not a second harness.
//! The external TypeScript SDK and the native Rust SDK retain their own APIs.
use crate::{engine, state::AppState};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

use super::response::error;

const SOURCE: &str = "native_sdk_panel";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Start {
    instruction: String,
    provider_id: Option<String>,
    model: Option<String>,
    verify_command: Option<String>,
    max_rounds: Option<usize>,
    max_total_tokens: Option<u64>,
    planning_mode: Option<String>,
    mcp_enabled: Option<bool>,
    skills_enabled: Option<bool>,
    web_enabled: Option<bool>,
    subagents_enabled: Option<bool>,
}

fn panel_job(state: &AppState, id: &str) -> Option<Value> {
    state.store.get("job", id).filter(|job| {
        job["kind"] == "prompt" && job.pointer("/input/uiSource").and_then(Value::as_str) == Some(SOURCE)
    })
}

fn status_for_panel(job: &Value, pending: bool) -> &'static str {
    if pending && job["status"] == "running" {
        return "awaiting_approval";
    }
    match job["status"].as_str().unwrap_or("") {
        "queued" => "queued",
        "running" => "running",
        "completed" => "completed",
        "cancelled" => "cancelled",
        "needs_review" => "in_doubt",
        _ => "failed",
    }
}

fn public_view(state: &AppState, job: &Value) -> Value {
    let session = job["sessionId"].as_str().unwrap_or("");
    let pending = state.store.all("approval").into_iter().find(|record| {
        !session.is_empty()
            && record["sessionId"] == session
            && record["status"] == "pending"
    });
    let steps = job.pointer("/autonomousState/receipts")
        .and_then(Value::as_array).cloned().unwrap_or_default();
    let calls: usize = steps.iter().map(|step| {
        step["toolSuccess"].as_array().map_or(0, Vec::len)
            + step["toolErrors"].as_array().map_or(0, Vec::len)
    }).sum();
    let usage = if session.is_empty() { Value::Null } else {
        let stats = state.store.session_usage(session);
        if stats["calls"].as_u64().unwrap_or(0) == 0 {
            Value::Null
        } else {
            json!({
                "prompt_tokens": stats["promptTokens"].as_u64().unwrap_or(0),
                "completion_tokens": stats["completionTokens"].as_u64().unwrap_or(0),
                "cached_prompt_tokens": stats["cachedPromptTokens"].as_u64().unwrap_or(0),
            })
        }
    };
    let pending_call = pending.map(|record| json!({
        "id": record["id"],
        "name": record["tool"],
        "arguments": record["args"],
    }));
    let events: Vec<Value> = steps.iter().map(|step| json!({
        "kind": "autonomous_step", "detail": step,
    })).collect();
    let answer = if session.is_empty() { Value::Null } else {
        let message_id = job.pointer("/result/messageId").and_then(Value::as_str);
        state.store.messages(session).into_iter().rev()
            .find(|message| message["role"] == "assistant"
                && message_id.is_none_or(|id| message["id"].as_str() == Some(id)))
            .map(|message| message["content"].clone())
            .unwrap_or(Value::Null)
    };
    let status = status_for_panel(job, pending_call.is_some());
    json!({
        "id": job["id"],
        "status": status,
        "instruction": job["input"]["prompt"],
        "answer": answer,
        "error": job["error"].as_str()
            .or_else(|| job.pointer("/result/reason").and_then(Value::as_str)),
        "providerId": job["input"]["providerId"],
        "model": job["input"]["model"],
        "planningMode": job["input"]["sdkUiPolicy"]["planningMode"],
        "maxTotalTokens": job["input"]["sdkUiPolicy"]["maxTotalTokens"],
        "createdAt": job["createdAt"],
        "checkpoint": {
            "state": {"state": status},
            "rounds": steps.len(),
            "toolCalls": calls,
            "usage": usage,
            "pendingCall": pending_call,
            "events": events,
            "answer": answer,
            "verification": job.pointer("/result/verification"),
            "verified": job.pointer("/result/verified"),
            "unmetChecks": job.pointer("/result/unmetChecks"),
        },
        "reconciliation": job["reconciliation"],
    })
}

/// List only jobs created through this specific UI; do not claim that arbitrary
/// prompt jobs originated in the native-agent panel.
pub async fn list(State(state): State<AppState>) -> Json<Value> {
    let mut jobs = state.store.all("job").into_iter().filter(|job| {
        job["kind"] == "prompt" && job.pointer("/input/uiSource").and_then(Value::as_str) == Some(SOURCE)
    }).collect::<Vec<_>>();
    jobs.sort_by(|a, b| b["createdAt"].as_str().unwrap_or("")
        .cmp(a["createdAt"].as_str().unwrap_or("")));
    Json(json!(jobs.iter().take(30).map(|job| public_view(&state, job))
        .collect::<Vec<_>>()))
}

pub async fn one(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match panel_job(&state, &id) {
        Some(job) => Json(public_view(&state, &job)).into_response(),
        None => error(StatusCode::NOT_FOUND, "해당 자동 작업을 찾을 수 없습니다."),
    }
}

/// Start the SAME durable Rust prompt worker used by the rest of Dominion.
pub async fn start(State(state): State<AppState>, Json(input): Json<Start>) -> Response {
    let instruction = input.instruction.trim();
    if instruction.is_empty() || instruction.len() > 64_000 {
        return error(StatusCode::BAD_REQUEST, "작업 지시는 1~64000자로 입력하세요.");
    }
    if input.verify_command.as_ref().is_some_and(|command| command.len() > 2048) {
        return error(StatusCode::BAD_REQUEST, "검증 명령은 최대 2048자입니다.");
    }
    let route = match engine::gateway::resolve(
        &state, input.provider_id.as_deref().unwrap_or(""), input.model.as_deref()
    ) {
        Ok(route) => route,
        Err(message) => return error(StatusCode::BAD_REQUEST, &message),
    };
    let rounds=input.max_rounds.unwrap_or(24);
    if !(1..=80).contains(&rounds) {
        return error(StatusCode::BAD_REQUEST, "최대 모델 판단 횟수는 1~80회입니다.");
    }
    let tokens=input.max_total_tokens.unwrap_or(120_000);
    if !(1_000..=2_000_000).contains(&tokens) {
        return error(StatusCode::BAD_REQUEST, "전체 토큰 한도는 1,000~2,000,000입니다.");
    }
    let planning=input.planning_mode.as_deref().unwrap_or("automatic");
    if !matches!(planning,"automatic"|"always"|"never") {
        return error(StatusCode::BAD_REQUEST, "작업 계획 모드를 확인하세요.");
    }
    // Outer repair attempts are independent of the total model-round cap.
    let attempts=rounds.clamp(1,6);
    let mut job_input = json!({
        "prompt": instruction,
        "providerId": route.id,
        "model": route.model,
        "toolsEnabled": true,
        "autonomous": true,
        "maxAutonomousAttempts": attempts,
        "executionStrategy": "focused",
        "uiSource": SOURCE,
        "sdkUiPolicy":{
            "planningMode":planning,"maxRounds":rounds,"maxTotalTokens":tokens,
            "mcpEnabled":input.mcp_enabled.unwrap_or(false),
            "skillsEnabled":input.skills_enabled.unwrap_or(true),
            "webEnabled":input.web_enabled.unwrap_or(false),
            "subagentsEnabled":input.subagents_enabled.unwrap_or(false),
        }
    });
    if let Some(command) = input.verify_command.filter(|command| !command.trim().is_empty()) {
        job_input["verifyCommand"] = json!(command.trim());
    }
    match engine::jobs::enqueue(&state, "prompt", job_input) {
        Some(job) => (StatusCode::ACCEPTED, Json(public_view(&state, &job))).into_response(),
        None => error(StatusCode::INTERNAL_SERVER_ERROR, "작업을 저장하지 못했습니다."),
    }
}

pub async fn cancel(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    if panel_job(&state, &id).is_none() {
        return error(StatusCode::NOT_FOUND, "해당 자동 작업을 찾을 수 없습니다.");
    }
    let result = super::jobs::cancel(State(state.clone()), Path(id.clone())).await;
    if !result.status().is_success() {
        return result;
    }
    one(State(state), Path(id)).await
}

/// Approvals stay bound to the existing session's durable tool request.
pub async fn approve(
    State(state): State<AppState>, Path(id): Path<String>, Json(body): Json<Value>
) -> Response {
    let Some(job) = panel_job(&state, &id) else {
        return error(StatusCode::NOT_FOUND, "해당 자동 작업을 찾을 수 없습니다.");
    };
    let Some(call_id) = body["callId"].as_str().filter(|id| !id.is_empty()) else {
        return error(StatusCode::BAD_REQUEST, "승인할 도구의 callId가 필요합니다.");
    };
    let Some(approval) = state.store.get("approval", call_id) else {
        return error(StatusCode::NOT_FOUND, "승인 요청을 찾을 수 없습니다.");
    };
    if approval["status"] != "pending"
        || job["status"] != "running"
        || job["sessionId"].as_str().is_none_or(|session| approval["sessionId"] != session)
        || !state.approvals.lock().unwrap().contains_key(call_id)
    {
        return error(StatusCode::CONFLICT, "현재 작업에서 실행 대기 중인 승인만 처리할 수 있습니다.");
    }
    super::approvals::resolve_approval(
        State(state), Json(json!({"id":call_id,"allow":true}))
    ).await
}

/// A previously interrupted turn can be retried only after the owner
/// explicitly reconciled its unknown effects as NOT APPLIED. This is a new
/// authorized attempt on the same job/session, not blind crash replay.
pub async fn resume(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let Some(job) = panel_job(&state, &id) else {
        return error(StatusCode::NOT_FOUND, "해당 자동 작업을 찾을 수 없습니다.");
    };
    if job["status"] != "failed"
        || job["reconciliation"]["source"] != "owner_attestation"
        || job["reconciliation"]["applied"] != false
        || job["cancelRequested"] == true
    {
        return error(StatusCode::CONFLICT,
            "미확정 도구 실행은 자동 재개할 수 없습니다. 먼저 실제 결과를 확인하고 미적용으로 기록하세요.");
    }
    let resumed = job["resumeCount"].as_u64().unwrap_or(0);
    if resumed >= 2 {
        return error(StatusCode::CONFLICT,
            "재개 한도를 초과했습니다. 검증 기록을 확인한 뒤 새로운 작업을 시작하세요.");
    }
    // A prior external effect was explicitly attested NOT APPLIED. Preserve
    // that statement across the CAS and worker restart: clearing the UI
    // reconciliation without passing it into the model would silently replay
    // the original owner request as if this were a new task.
    let prior=job["reconciliation"].clone();
    let mut history=job["resumeHistory"].as_array().cloned().unwrap_or_default();
    if let Some(checkpoint)=job.get("autonomousState").filter(|v|!v.is_null()) {
        history.push(json!({"resumeNumber":resumed+1,
            "previousAutonomousState":checkpoint,
            "reconciliation":prior}));
        if history.len()>2{history.drain(..history.len()-2);}
    }
    let patch = json!({
        "status": "queued",
        "error": null,
        "result": null,
        "finishedAt": null,
        "startedAt": null,
        "recovered": false,
        "resumeCount": resumed + 1,
        "resumeEvidence": prior,
        "resumeHistory": history,
        "reconciliation": null,
        "promptState": {"inFlight": false},
        "autonomousState": null,
    });
    match state.store.patch_if("job", &id, "status", &json!("failed"), &patch) {
        Some(saved) => {
            state.bus.record("job", &saved);
            state.store.audit("job.owner_authorized_resume", json!({
                "jobId":id,"resumeCount":resumed+1,
            }));
            Json(public_view(&state, &saved)).into_response()
        }
        None => error(StatusCode::CONFLICT, "다른 실행기가 작업 상태를 변경했습니다."),
    }
}

/// Owner reconciliation is explicitly owner-attested, NEVER engine-verified.
pub async fn reconcile(
    State(state): State<AppState>, Path(id): Path<String>, Json(body): Json<Value>
) -> Response {
    let Some(job) = panel_job(&state, &id) else {
        return error(StatusCode::NOT_FOUND, "해당 자동 작업을 찾을 수 없습니다.");
    };
    let Some(applied) = body["applied"].as_bool() else {
        return error(StatusCode::BAD_REQUEST, "변경 적용 여부를 지정하세요.");
    };
    let Some(details) = body["details"].as_str().map(str::trim).filter(|s| !s.is_empty() && s.len() <= 8192) else {
        return error(StatusCode::BAD_REQUEST, "실제 확인한 결과를 1~8192자로 입력하세요.");
    };
    if job["status"] != "needs_review" {
        return error(StatusCode::CONFLICT, "수동 확인이 필요한 작업만 조정할 수 있습니다.");
    }
    let patch = json!({
        "status": if applied { "completed" } else { "failed" },
        "reconciliation": {
            "source": "owner_attestation",
            "applied": applied,
            "details": details,
            "verifiedByEngine": false,
            "recordedAt": chrono::Utc::now().to_rfc3339(),
        },
    });
    match state.store.patch_if("job", &id, "status", &json!("needs_review"), &patch) {
        Some(saved) => {
            state.bus.record("job", &saved);
            state.store.audit("job.owner_reconciled", json!({"jobId":id,"applied":applied}));
            Json(public_view(&state, &saved)).into_response()
        }
        None => error(StatusCode::CONFLICT, "작업 상태가 동시에 변경되었습니다."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> AppState {
        let root = std::env::temp_dir().join(format!("dominion-panel-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let config = crate::config::Config {
            port: 0,
            data_dir: root.clone(),
            web_dir: root.join("web"),
            workspace: root.clone(),
        };
        AppState::new(config.clone(),
            crate::db::Store::open(&config.db_path()).unwrap(),
            crate::vault::Vault::open(&config.key_path()))
    }
    #[test]
    fn main_ui_reads_real_prompt_job_and_requires_genuine_approval() {
        let state = state();
        let job = engine::jobs::enqueue(&state, "prompt", json!({
            "prompt":"inspect", "uiSource": SOURCE, "autonomous": true
        })).unwrap();
        let id = job["id"].as_str().unwrap();
        let view = public_view(&state, &job);
        assert_eq!(view["status"], "queued");
        assert_eq!(view["instruction"], "inspect");
        assert_eq!(view["checkpoint"]["rounds"], 0);
        assert_eq!(view["checkpoint"]["pendingCall"], Value::Null);
        assert!(panel_job(&state, id).is_some());
        let other = engine::jobs::enqueue(&state, "prompt", json!({"prompt":"other"})).unwrap();
        assert!(panel_job(&state, other["id"].as_str().unwrap()).is_none());
    }
    #[tokio::test]
    async fn main_ui_creation_and_owner_reconciliation_have_one_rust_source_of_truth() {
        let state = state();
        state.store.put("provider", &json!({
            "id":"test-provider","name":"test","enabled":true,"baseUrl":"http://127.0.0.1:9/v1",
            "kind":"openai","model":"test-model"
        }));
        let created = start(State(state.clone()), Json(Start {
            instruction:"Inspect then fix a file".into(),
            provider_id: Some("test-provider".into()),
            model: None,
            verify_command: Some("cargo test".into()),
            max_rounds: Some(2),
            max_total_tokens: Some(50_000),
            planning_mode: Some("always".into()),
            mcp_enabled: Some(false),skills_enabled: Some(true),
            web_enabled: Some(false),subagents_enabled: Some(false),
        })).await;
        assert_eq!(created.status(), StatusCode::ACCEPTED);
        let jobs = state.store.all("job");
        assert_eq!(jobs.len(), 1);
        let job = &jobs[0];
        let id = job["id"].as_str().unwrap();
        assert_eq!(job["input"]["providerId"], "test-provider");
        assert_eq!(job["input"]["verifyCommand"], "cargo test");
        assert_eq!(job["input"]["maxAutonomousAttempts"], 2);
        assert_eq!(job["input"]["sdkUiPolicy"]["maxTotalTokens"],50_000);
        assert_eq!(job["input"]["sdkUiPolicy"]["planningMode"],"always");
        assert_eq!(one(State(state.clone()), Path(id.to_string())).await.status(), StatusCode::OK);
        state.store.patch_if("job", id, "status", &json!("queued"), &json!({
            "status":"needs_review", "result":{"verified":false}
        }));
        let result = reconcile(State(state.clone()), Path(id.to_string()), Json(json!({
            "applied":true,"details":"Inspected the file manually"
        }))).await;
        assert_eq!(result.status(), StatusCode::OK);
        let stored = state.store.get("job", id).unwrap();
        assert_eq!(stored["status"], "completed");
        assert_eq!(stored["reconciliation"]["verifiedByEngine"], false);
        assert_eq!(stored["result"]["verified"], false);
    }
    #[tokio::test]
    async fn interrupted_effect_requires_owner_attestation_before_resume() {
        let state=state();
        let job=engine::jobs::enqueue(&state,"prompt",json!({
            "prompt":"Fix the source", "uiSource":SOURCE, "autonomous":true
        })).unwrap();
        let id=job["id"].as_str().unwrap().to_string();
        state.store.patch_if("job",&id,"status",&json!("queued"),&json!({
            "status":"needs_review","promptState":{"inFlight":true},
            "autonomousState":{"receipts":[{"outcome":"needs_review","toolSuccess":["shell_exec"],"toolErrors":[]}]}
        }));
        assert_eq!(resume(State(state.clone()),Path(id.clone())).await.status(),StatusCode::CONFLICT);
        assert_eq!(reconcile(State(state.clone()),Path(id.clone()),Json(json!({
            "applied":false,"details":"The remote operation did not happen"
        }))).await.status(),StatusCode::OK);
        let resumed=resume(State(state.clone()),Path(id.clone())).await;
        assert_eq!(resumed.status(),StatusCode::OK);
        let record=state.store.get("job",&id).unwrap();
        assert_eq!(record["status"],"queued");
        assert_eq!(record["promptState"]["inFlight"],false);
        assert_eq!(record["resumeCount"],1);
        assert_eq!(record["resumeEvidence"]["applied"],false);
        assert_eq!(record["resumeEvidence"]["source"],"owner_attestation");
        assert_eq!(record["resumeHistory"][0]["previousAutonomousState"]["receipts"][0]["outcome"],
            "needs_review");
        assert_eq!(record["autonomousState"],Value::Null);
        assert_eq!(resume(State(state.clone()),Path(id.clone())).await.status(),StatusCode::CONFLICT);
    }
}