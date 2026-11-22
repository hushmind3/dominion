//! One owner-message command shared by HTTP and external channel adapters.
//! Admission, retry identity and the job snapshot are independent of HTTP.
use super::rooms;
use crate::{
    db::{RoomAdmissionError, RoomReceipt},
    engine::{chat::TurnRequest, memory},
    state::AppState,
};
use serde_json::{json, Value};

pub enum SubmissionError {
    Invalid(String),
    Missing,
    Busy(String),
    Storage(String),
}
fn bad(message: impl Into<String>) -> SubmissionError {
    SubmissionError::Invalid(message.into())
}
fn busy(message: impl Into<String>) -> SubmissionError {
    SubmissionError::Busy(message.into())
}
fn failed(message: impl Into<String>) -> SubmissionError {
    SubmissionError::Storage(message.into())
}
impl std::fmt::Display for SubmissionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Invalid(message) | Self::Busy(message) | Self::Storage(message) => message,
            Self::Missing => "그룹을 찾을 수 없습니다.",
        })
    }
}

struct SubmissionInput {
    text: String,
    attachments: Vec<String>,
    recipients: Option<Value>,
    settings: TurnRequest,
    nonce: String,
}
impl SubmissionInput {
    fn parse(body: &Value, room_id: &str) -> Result<Self, SubmissionError> {
        if !body.is_object() {
            return Err(bad("메시지 객체가 필요합니다."));
        }
        for key in ["sessionId", "metadata", "role", "agentId"] {
            if body.get(key).is_some() {
                return Err(bad("화자와 세션 정보는 서버가 정합니다."));
            }
        }
        let text = body.get("input").or_else(|| body.get("goal"));
        if text.is_some_and(|value| !value.is_string()) {
            return Err(bad("메시지 내용은 문자열이어야 합니다."));
        }
        let text = text
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        let attachments = crate::engine::attachments::ids(&body["attachmentIds"]).map_err(bad)?;
        if (text.is_empty() && attachments.is_empty()) || text.chars().count() > 32000 {
            return Err(bad("메시지는 1~32000자여야 합니다."));
        }
        let recipients = body
            .get("agentIds")
            .filter(|value| !value.is_null())
            .cloned();
        if recipients.as_ref().is_some_and(|value| {
            value.as_array().is_none_or(|ids| {
                ids.len() > 32
                    || ids.iter().any(|id| {
                        id.as_str()
                            .is_none_or(|id| id.trim().is_empty() || id.len() > 256)
                    })
            })
        }) {
            return Err(bad("답변할 AI는 최대 32명의 ID 목록으로 선택하세요."));
        }
        let settings = TurnRequest::from_value(&json!({"input":if text.is_empty(){"첨부 파일"}else{&text},
            "thinking":body["thinking"],"reasoningEffort":body["reasoningEffort"],"reasoningLanguage":body["reasoningLanguage"],
            "providerId":body["providerId"],"model":body["model"]})).map_err(bad)?;
        let nonce = match body.get("clientMessageId") {
            Some(value) => {
                let client_id = value
                    .as_str()
                    .filter(|value| !value.is_empty() && value.len() <= 160)
                    .ok_or_else(|| bad("clientMessageId는 1~160자의 문자열이어야 합니다."))?;
                format!("room-message-{}", memory::stable_id(&[room_id, client_id]))
            }
            None => uuid::Uuid::new_v4().to_string(),
        };
        Ok(Self {
            text,
            attachments,
            recipients,
            settings,
            nonce,
        })
    }
    fn intent(&self, origin: &Value) -> Value {
        json!({"input":self.text,"attachmentIds":self.attachments,"agentIds":self.recipients,
            "thinking":self.settings.thinking,"reasoningEffort":self.settings.reasoning_effort,
            "reasoningLanguage":self.settings.reasoning_language,"providerId":self.settings.provider_id,"model":self.settings.model,
            "origin":{"kind":origin["kind"],"binding":origin["channelBindingId"],"message":origin["messageId"],"author":origin["authorId"]}})
    }
}

fn verify_receipt(
    command: &SubmissionInput,
    origin: &Value,
    room_id: &str,
    message: &Value,
    job: &Value,
) -> Result<(), SubmissionError> {
    if message["id"] != command.nonce
        || job["kind"] != "room"
        || job["input"]["roomId"] != room_id
        || message["role"] != "user"
        || message["session_id"] != job["input"]["sessionId"]
    {
        return Err(failed("저장된 메시지 접수 기록이 일치하지 않습니다."));
    }
    let expected = command.intent(origin);
    if let Some(saved) = job["input"].get("submissionIntent") {
        if saved == &expected {
            return Ok(());
        }
    } else {
        // Existing jobs retain their old IDs and remain replayable after upgrade.
        let old = &job["input"];
        let legacy = SubmissionInput::parse(
            &json!({"input":old["input"],"attachmentIds":old["attachmentIds"],
            "agentIds":old["requestedAgentIds"].as_array(),"thinking":old["thinking"],"reasoningEffort":old["reasoningEffort"],
            "reasoningLanguage":old["reasoningLanguage"],"providerId":old["providerId"],"model":old["model"]}),
            room_id,
        );
        if let Ok(legacy) = legacy {
            if legacy.intent(&message["metadata"]["origin"]) == expected {
                return Ok(());
            }
        }
    }
    Err(busy(
        "같은 clientMessageId로 다른 메시지를 보낼 수 없습니다.",
    ))
}
fn result(room: Value, message: Value, job: Value) -> Value {
    json!({"id":job["id"],"jobId":job["id"],"status":job["status"],"sessionId":message["session_id"],"room":room,"message":message,"job":job})
}

pub fn submit(
    state: &AppState,
    id: &str,
    body: &Value,
    origin: Option<Value>,
    tools_allowed: bool,
) -> Result<Value, SubmissionError> {
    let command = SubmissionInput::parse(body, id)?;
    let origin = origin.unwrap_or(Value::Null);
    if origin["authorIsBot"] == true {
        return Err(bad("봇 발언은 방 주인의 메시지로 제출할 수 없습니다."));
    }
    let key = format!("room-submit:{id}");
    if !state.begin_session(&key) {
        return Err(busy("그룹 상태가 변경 중입니다. 다시 시도하세요."));
    }
    let _guard = state.session_guard(&key);
    let original = state
        .store
        .get("room", id)
        .ok_or(SubmissionError::Missing)?;
    let job_id = format!("room-job-{}", memory::stable_id(&[id, &command.nonce]));
    // A retry returns the accepted receipt even if a connection was removed
    // or an attachment's availability changed since the original admission.
    if let Some(job) = state.store.get("job", &job_id) {
        let message = state
            .store
            .message(&command.nonce)
            .ok_or_else(|| failed("저장된 메시지를 찾을 수 없습니다."))?;
        verify_receipt(&command, &origin, id, &message, &job)?;
        return Ok(result(original, message, job));
    }
    let session = rooms::ensure_room_session(state, id).map_err(bad)?;
    let session_id = session["id"]
        .as_str()
        .ok_or_else(|| failed("그룹 세션 ID가 없습니다."))?;
    // ensure_room_session can bind a legacy room on its first submission.
    let original = state
        .store
        .get("room", id)
        .ok_or(SubmissionError::Missing)?;
    let mut config = original.clone();
    if !tools_allowed {
        config["toolsEnabled"] = json!(false);
    }
    let attachments =
        crate::engine::attachments::validate_ids(state, session_id, &command.attachments)
            .map_err(bad)?;
    if let Some(provider) = command.settings.provider_id.as_deref() {
        crate::engine::gateway::resolve(state, provider, command.settings.model.as_deref())
            .map_err(bad)?;
    }
    let speakers = rooms::plan_speakers(state, &config, &command.text, command.recipients.as_ref())
        .map_err(bad)?;
    let generation = state.cancellation_generation(session_id);
    let acceptance = crate::engine::chat::prepare_owner_action(state, &command.text)
        .map(|(request, checks)| json!({"request":request,"checks":checks}));
    let message = json!({"id":command.nonce,"session_id":session_id,"role":"user","agent_id":null,"content":command.text,
        "metadata":{"acceptance":acceptance,"authorKind":"owner","roomId":id,"ownerName":config["ownerName"],"clientMessageId":body["clientMessageId"],"attachments":attachments,"origin":origin}});
    let job = json!({"id":job_id,"kind":"room","status":"queued","attempts":0,"stepIndex":0,"stepTotal":0,"events":[],
        "queuedAt":chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros,true),"sessionId":session_id,
        "input":{"roomId":id,"sessionId":session_id,"messageId":command.nonce,"sourceMessageId":command.nonce,"input":command.text,
            "agentIds":speakers,"requestedAgentIds":command.recipients,"explicitAgentIds":command.recipients.is_some(),"routingMode":config["routingMode"],
            "generation":generation,"roomSnapshot":config,"attachmentIds":command.attachments,"thinking":command.settings.thinking,
            "reasoningEffort":command.settings.reasoning_effort,"reasoningLanguage":command.settings.reasoning_language,
            "providerId":command.settings.provider_id,"model":command.settings.model,"submissionIntent":command.intent(&origin)}});
    let RoomReceipt {
        room,
        message,
        job,
        created,
    } = state
        .store
        .admit_room_message(&original, &message, &job)
        .map_err(|error| match error {
            RoomAdmissionError::Changed => busy(
                "그룹 설정이나 중단 상태가 변경되었습니다. 최신 상태를 확인하고 다시 보내세요.",
            ),
            RoomAdmissionError::Storage(message) => failed(message),
        })?;
    if !created {
        verify_receipt(&command, &origin, id, &message, &job)?;
    } else {
        state.bus.message(&message);
        state.bus.record("job", &job);
        state.bus.record("room", &room);
        super::room_state::observe_owner_message(state, id, session_id, &message);
        state.store.audit(
            "room.owner_message",
            json!({"roomId":id,"sessionId":session_id,"messageId":message["id"],"jobId":job["id"]}),
        );
        state.job_wakeup.notify_one();
    }
    Ok(result(room, message, job))
}
