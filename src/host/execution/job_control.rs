//! Owner job commands share one status/attempt fence, independent of HTTP.
use crate::state::AppState;
use serde_json::{json, Value};

pub enum ControlError {
    Invalid(String),
    Missing,
    Changed(String),
}
fn changed(message: &str) -> ControlError {
    ControlError::Changed(message.into())
}
enum SteeringMode {
    Append,
    Replace,
}
impl SteeringMode {
    fn parse(mode: &str) -> Result<Self, ControlError> {
        match mode {
            "append" => Ok(Self::Append),
            "replace" => Ok(Self::Replace),
            _ => Err(ControlError::Invalid(
                "변경 방식은 append 또는 replace여야 합니다.".into(),
            )),
        }
    }
}
fn active(job: &Value) -> bool {
    matches!(job["status"].as_str(), Some("queued" | "running")) && job["cancelRequested"] != true
}

pub fn steer(
    state: &AppState,
    id: &str,
    instruction: &str,
    mode: &str,
) -> Result<Value, ControlError> {
    let instruction = instruction.trim();
    if instruction.is_empty() || instruction.len() > 16_000 {
        return Err(ControlError::Invalid(
            "작업 변경 지시가 비어 있거나 너무 깁니다.".into(),
        ));
    }
    let selected = SteeringMode::parse(mode)?;
    let job = state.store.get("job", id).ok_or(ControlError::Missing)?;
    if job["kind"] != "prompt" || !active(&job) {
        return Err(changed(
            "진행 중인 일반 작업에만 새 지시를 전달할 수 있습니다.",
        ));
    }
    let previous = job["nativeDirective"].as_str().unwrap_or("");
    let directive = match selected {
        SteeringMode::Append if !previous.trim().is_empty() => {
            format!("{previous}\n\n{instruction}")
        }
        _ => instruction.to_string(),
    };
    if directive.len() > 32_000 {
        return Err(ControlError::Invalid(
            "누적된 작업 변경 지시가 너무 깁니다.".into(),
        ));
    }
    let revision = job["directiveRevision"]
        .as_u64()
        .unwrap_or(0)
        .checked_add(1)
        .ok_or_else(|| changed("작업 지시 버전을 갱신할 수 없습니다."))?;
    // Comparing status alone loses concurrent appends. Compare the directive,
    // cancellation flag and claimed attempt as part of the same transaction.
    let saved=state.store.patch_if_all("job",id,&[
        ("status",&job["status"]),("attempts",&job["attempts"]),("cancelRequested",&job["cancelRequested"]),
        ("nativeDirective",&job["nativeDirective"]),("directiveRevision",&job["directiveRevision"]),
    ],&json!({"nativeDirective":directive,"directiveRevision":revision,"directiveUpdatedAt":chrono::Utc::now().to_rfc3339()}))
        .ok_or_else(||changed("작업이나 지시가 다른 곳에서 변경되었습니다. 입력한 내용을 유지하고 최신 상태를 확인하세요."))?;
    state.bus.record("job", &saved);
    Ok(json!({"ok":true,"jobId":id,"directive":directive,"mode":mode,"revision":revision}))
}

pub fn cancel(state: &AppState, id: &str) -> Result<Value, ControlError> {
    let job = state.store.get("job", id).ok_or(ControlError::Missing)?;
    if job["status"] == "cancelled" {
        return Ok(job);
    }
    if !active(&job) {
        return Err(changed("진행 중인 작업만 중단할 수 있습니다."));
    }
    let saved = state
        .store
        .patch_if_all(
            "job",
            id,
            &[
                ("status", &job["status"]),
                ("attempts", &job["attempts"]),
                ("cancelRequested", &job["cancelRequested"]),
            ],
            &json!({"status":"cancelled","cancelRequested":true,
        "finishedAt":chrono::Utc::now().to_rfc3339()}),
        )
        .ok_or_else(|| {
            changed("작업 상태가 변경되어 취소하지 않았습니다. 최신 상태를 확인하세요.")
        })?;
    state.bus.record("job", &saved);
    // Cancelling a queued job must not cancel another running job that shares
    // its transcript. Running work retains the existing session cancellation.
    if job["status"] == "running" {
        if let Some(session) = saved["sessionId"].as_str() {
            state.cancel_session(session);
        }
    }
    state.cancel_wakeup.notify_waiters();
    state.job_wakeup.notify_one();
    Ok(saved)
}
