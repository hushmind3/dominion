use crate::state::AppState;
use axum::{
    extract::{OriginalUri, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};

/// Record kinds that behave as plain JSON collections. The React client
/// already talks to these paths, so the Rust backend just needs to persist
/// them with the same contract.
pub const COLLECTIONS: &[&str] = &[
    "memories",
    "skills",
    "mcp",
    "knowledge",
];

/// URL segments are plural (`/api/jobs`) while the store groups records by a
/// singular kind (`job`), matching the Node data model.
pub fn store_kind(route_kind: &str) -> &str {
    match route_kind {
        "memories" => "memory",
        "skills" => "skill",
        "rooms" => "room",
        "channels" => "channel",
        "jobs" => "job",
        "executions" => "execution",
        other => other,
    }
}

fn kind_from_uri(uri: &axum::http::Uri) -> String {
    store_kind(
        uri.path()
            .trim_start_matches('/')
            .split('/')
            .nth(1)
            .unwrap_or_default(),
    )
    .to_string()
}

fn id_from_uri(uri: &axum::http::Uri) -> Option<String> {
    uri.path()
        .trim_start_matches('/')
        .split('/')
        .nth(2)
        .map(str::to_string)
        .filter(|value| !value.is_empty())
}

pub async fn list(State(state): State<AppState>, OriginalUri(uri): OriginalUri) -> Json<Value> {
    let kind = kind_from_uri(&uri);
    let mut records = state.store.all(&kind);
    if kind == "memory" {
        for record in &mut records {
            let eligible = crate::engine::memory::retrieval_eligible(record);
            if let Some(fields) = record.as_object_mut() {
                fields.insert("retrievalEligible".into(), json!(eligible));
            }
        }
    }
    Json(json!(records))
}

pub async fn save(
    State(state): State<AppState>,
    OriginalUri(uri): OriginalUri,
    Json(body): Json<Value>,
) -> Response {
    let kind = kind_from_uri(&uri);
    if !body.is_object() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "레코드 객체가 필요합니다." })),
        )
            .into_response();
    }
    // The legacy generic upsert previously allowed a caller to fabricate a
    // learned memory or overwrite any existing source/evidence by supplying
    // the record ID. Owner edits/corrections now have dedicated CAS actions.
    if kind == "memory" {
        if id_from_uri(&uri).is_some() || body["id"].as_str()
            .is_some_and(|id| !id.trim().is_empty()) {
            return (StatusCode::CONFLICT, Json(json!({
                "error":"기존 기억은 전용 수정·정정 API에서 변경하세요."
            }))).into_response();
        }
        let text = body["text"].as_str().map(str::trim).unwrap_or("");
        if text.is_empty() || text.chars().count()>16_000 {
            return (StatusCode::BAD_REQUEST, Json(json!({
                "error":"새 기억은 1~16000자로 입력하세요."
            }))).into_response();
        }
        if body["source"].as_str().is_some_and(|source|source!="user")
            || body["status"].as_str().is_some_and(|status|status!="active")
            || body["sourceMessageIds"].is_array()
            || body["evidenceQuote"].is_string()
            || body["supersedes"].is_string() {
            return (StatusCode::BAD_REQUEST, Json(json!({
                "error":"사용자가 직접 만든 기억만 새로 저장할 수 있습니다."
            }))).into_response();
        }
    }
    let mut body = body;
    if kind == "memory" {
        body["source"] = json!("user");
        body["status"] = json!("active");
        body["text"] = json!(body["text"].as_str().unwrap_or("").trim());
    }
    if kind == "mcp" {
        let id = body
            .get("id")
            .and_then(|value| value.as_str())
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        body["id"] = json!(id);
        if let Some(key) = body
            .get("apiKey")
            .and_then(|value| value.as_str())
            .filter(|value| !value.is_empty() && !value.starts_with("••"))
            .map(str::to_string)
        {
            state.save_secret(&format!("mcp:{id}"), &key);
        }
        if let Value::Object(ref mut map) = body {
            map.remove("apiKey");
        }
        crate::integrations::mcp::invalidate(&state);
    }
    if kind == "skill" {
        // The execution path ignores `draft` even when `enabled` is true.
        // Make the public toggle and the engine contract agree.
        let enabled = body["enabled"].as_bool() == Some(true);
        if enabled {
            let status = body["status"].as_str().unwrap_or("");
            if status.is_empty() || status == "draft" {
                body["status"] = json!("active");
            }
        }
    }
    match state.store.put(&kind, &body) {
        Some(record) => {
            state.bus.record(&kind, &record);
            Json(record).into_response()
        }
        None => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "레코드를 저장할 수 없습니다." })),
        )
            .into_response(),
    }
}

pub async fn one(State(state): State<AppState>, OriginalUri(uri): OriginalUri) -> Response {
    let kind = kind_from_uri(&uri);
    match id_from_uri(&uri) {
        Some(id) => match state.store.get(&kind, &id) {
            Some(record) => Json(record).into_response(),
            None => (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": format!("{kind} 레코드를 찾을 수 없습니다.") })),
            )
                .into_response(),
        },
        None => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "id 가 필요합니다." })),
        )
            .into_response(),
    }
}

pub async fn remove(State(state): State<AppState>, OriginalUri(uri): OriginalUri) -> Json<Value> {
    let kind = kind_from_uri(&uri);
    if let Some(id) = id_from_uri(&uri) {
        if kind == "mcp" {
            state.store.remove_secret(&format!("mcp:{id}"));
            crate::integrations::mcp::invalidate(&state);
        }
        state.store.remove(&kind, &id);
        state
            .bus
            .emit(json!({ "type": "record", "kind": kind, "removed": id }));
    }
    Json(json!({ "ok": true }))
}

/// `POST /api/{kind}/{id}/{action}` for room execution and channel
/// diagnostics. Unknown actions return a clear JSON error instead of HTML.
pub async fn action(
    State(state): State<AppState>,
    OriginalUri(uri): OriginalUri,
    Json(body): Json<Value>,
) -> Response {
    let kind = kind_from_uri(&uri);
    let id = id_from_uri(&uri).unwrap_or_default();
    let action = uri
        .path()
        .trim_start_matches('/')
        .split('/')
        .nth(3)
        .unwrap_or_default()
        .to_string();

    match (kind.as_str(), action.as_str()) {
        ("room", "run") => enqueue_job(
            &state,
            "room",
            json!({
                "roomId": id,
                "goal": body.get("goal").or_else(|| body.get("input")).cloned().unwrap_or(json!("")),
            }),
        ),
        ("memory", "edit") => edit_owner_memory(&state, &id, &body),
        ("memory", "correct") => correct_derived_memory(&state, &id, &body),
        ("skill", "export") => match state.store.get("skill", &id) {
            Some(skill) => Json(skill).into_response(),
            None => (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "스킬을 찾을 수 없습니다." })),
            )
                .into_response(),
        },
        _ => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": format!("지원하지 않는 작업입니다: {kind}/{action}") })),
        )
            .into_response(),
    }
}

fn enqueue_job(state: &AppState, kind: &str, input: Value) -> Response {
    match crate::engine::jobs::enqueue(state, kind, input) {
        Some(job) => Json(job).into_response(),
        None => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "작업을 만들 수 없습니다." })),
        )
            .into_response(),
    }
}


/// Correct only an owner-authored active memory, preserving its scope and
/// provenance. A stale editor cannot overwrite a correction or revive an
/// archived/superseded fact: all three guards are checked in one SQLite CAS.
fn edit_owner_memory(state:&AppState,id:&str,body:&Value)->Response{
    let Some(text)=body["text"].as_str()
        .map(str::trim).filter(|value|!value.is_empty()&&value.chars().count()<=16_000)
    else{return (StatusCode::BAD_REQUEST,Json(json!({
        "error":"수정할 기억은 1~16000자여야 합니다."
    }))).into_response()};
    let Some(expected)=body["expectedText"].as_str()else{
        return (StatusCode::BAD_REQUEST,Json(json!({
            "error":"기억의 이전 원문이 필요합니다."
        }))).into_response();
    };
    let Some(current)=state.store.get("memory",id)else{
        return (StatusCode::NOT_FOUND,Json(json!({
            "error":"수정할 기억을 찾을 수 없습니다."
        }))).into_response();
    };
    if current["source"]!="user"||current["status"]!="active"{
        return (StatusCode::CONFLICT,Json(json!({
            "error":"사용자가 직접 저장한 활성 기억만 수정할 수 있습니다."
        }))).into_response();
    }
    if current["text"].as_str()!=Some(expected){
        return (StatusCode::CONFLICT,Json(json!({
            "error":"기억이 이미 변경되었습니다. 새로고침 후 다시 확인하세요."
        }))).into_response();
    }
    if text==expected{return Json(current).into_response();}
    let at=chrono::Utc::now().to_rfc3339();
    let mut history=current["editHistory"].as_array().cloned().unwrap_or_default();
    history.push(json!({"text":expected,"editedAt":at,"by":"owner"}));
    if history.len()>10{history.drain(..history.len()-10);}
    let expected_status=json!("active");
    let expected_source=json!("user");
    let edited=state.store.patch_if_all("memory",id,&[
        ("text",&current["text"]),
        ("status",&expected_status),
        ("source",&expected_source),
    ],&json!({
        "text":text,"editedAt":at,"editedBy":"owner","editHistory":history
    }));
    let Some(edited)=edited else{
        return (StatusCode::CONFLICT,Json(json!({
            "error":"기억이 이미 변경되었습니다. 새로고침 후 다시 확인하세요."
        }))).into_response();
    };
    state.store.audit("memory.owner_edit",json!({
        "memoryId":id,"at":at,"oldLength":expected.chars().count(),
        "newLength":text.chars().count()
    }));
    state.bus.record("memory",&edited);
    Json(edited).into_response()
}

#[cfg(test)]
mod memory_edit_tests{
    use super::*;
    fn state()->AppState{
        let root=std::env::temp_dir().join(format!("dominion-memory-edit-{}",uuid::Uuid::new_v4()));
        let config=crate::config::Config{
            port:0,data_dir:root.clone(),web_dir:root.join("web"),workspace:root
        };
        let store=crate::db::Store::open(&config.db_path()).unwrap();
        let vault=crate::vault::Vault::open(&config.key_path());
        AppState::new(config,store,vault)
    }

    #[test]
    fn editing_owner_memory_preserves_scope_and_history_and_rejects_stale_edits(){
        let state=state();
        let original=json!({
            "id":"manual-1","text":"선호하는 이동수단: 버스",
            "scope":"agent","agentId":"some-agent","partitionKey":"local",
            "status":"active","source":"user","importance":0.8
        });
        state.store.put("memory",&original).unwrap();
        let first=edit_owner_memory(&state,"manual-1",&json!({
            "expectedText":"선호하는 이동수단: 버스",
            "text":"선호하는 이동수단: 지하철"
        }));
        assert_eq!(first.status(),StatusCode::OK);
        let updated=state.store.get("memory","manual-1").unwrap();
        assert_eq!(updated["text"],"선호하는 이동수단: 지하철");
        assert_eq!(updated["agentId"],"some-agent");
        assert_eq!(updated["partitionKey"],"local");
        assert_eq!(updated["importance"],0.8);
        assert_eq!(updated["editHistory"][0]["text"],"선호하는 이동수단: 버스");
        let stale=edit_owner_memory(&state,"manual-1",&json!({
            "expectedText":"선호하는 이동수단: 버스","text":"오래된 수정"
        }));
        assert_eq!(stale.status(),StatusCode::CONFLICT);
        assert_eq!(state.store.get("memory","manual-1").unwrap()["text"],updated["text"]);
        state.store.patch_if("memory","manual-1","status",&json!("active"),
            &json!({"status":"archived"})).unwrap();
        let archive=edit_owner_memory(&state,"manual-1",&json!({
            "expectedText":"선호하는 이동수단: 지하철","text":"재활성화 시도"
        }));
        assert_eq!(archive.status(),StatusCode::CONFLICT);
        assert_eq!(state.store.get("memory","manual-1").unwrap()["status"],"archived");
    }

    #[test]
    fn learned_evidence_cannot_be_rewritten_using_owner_edit(){
        let state=state();
        state.store.put("memory",&json!({
            "id":"learned","text":"출처 근거","source":"learn","status":"active",
            "sourceMessageIds":["source-1"],"scope":"session"
        })).unwrap();
        let denied=edit_owner_memory(&state,"learned",&json!({
            "expectedText":"출처 근거","text":"위조된 근거"
        }));
        assert_eq!(denied.status(),StatusCode::CONFLICT);
        assert_eq!(state.store.get("memory","learned").unwrap()["text"],"출처 근거");
        let absent=edit_owner_memory(&state,"missing",&json!({
            "expectedText":"a","text":"b"
        }));
        assert_eq!(absent.status(),StatusCode::NOT_FOUND);
        let empty=edit_owner_memory(&state,"learned",&json!({
            "expectedText":"출처 근거","text":" "
        }));
        assert_eq!(empty.status(),StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn generic_memory_upsert_cannot_overwrite_or_forge_provenance(){
        let state=state();
        state.store.put("memory",&json!({
            "id":"original","text":"원본 내용","source":"learn","status":"active",
            "sourceMessageIds":["immutable-evidence"]
        })).unwrap();
        let uri=||OriginalUri("/api/memories".parse().unwrap());
        let overwritten=save(State(state.clone()),uri(),Json(json!({
            "id":"original","text":"위조 내용","source":"user","status":"active"
        }))).await;
        assert_eq!(overwritten.status(),StatusCode::CONFLICT);
        let forged=save(State(state.clone()),uri(),Json(json!({
            "text":"가짜 학습 기록","source":"learn",
            "sourceMessageIds":["forged"]
        }))).await;
        assert_eq!(forged.status(),StatusCode::BAD_REQUEST);
        let valid=save(State(state.clone()),uri(),Json(json!({
            "text":"사용자가 직접 작성한 기억","scope":"shared"
        }))).await;
        assert_eq!(valid.status(),StatusCode::OK);
        let list=state.store.all("memory");
        assert_eq!(list.len(),2);
        assert_eq!(state.store.get("memory","original").unwrap()["sourceMessageIds"][0],
            "immutable-evidence");
        assert!(list.iter().any(|record|record["source"]=="user"
            && record["text"]=="사용자가 직접 작성한 기억"));
    }

    #[test]
    fn owner_correction_atomically_supersedes_learned_evidence_in_original_scope(){
        let state=state();
        state.store.put("memory",&json!({
            "id":"learned","text":"자동 추출한 낡은 사실","status":"active",
            "scope":"session","sessionId":"session-A","agentId":"agent-A",
            "partitionKey":"local","source":"learn","sourceMessageIds":["owner-1"],
            "evidenceQuote":"원본 근거는 그대로"
        })).unwrap();
        let result=correct_derived_memory(&state,"learned",&json!({
            "expectedText":"자동 추출한 낡은 사실",
            "text":"주인이 정정한 최신 사실"
        }));
        assert_eq!(result.status(),StatusCode::OK);
        let old=state.store.get("memory","learned").unwrap();
        assert_eq!(old["status"],"superseded");
        assert_eq!(old["sourceMessageIds"][0],"owner-1");
        assert_eq!(old["evidenceQuote"],"원본 근거는 그대로");
        let new_id=old["supersededBy"].as_str().unwrap();
        let replacement=state.store.get("memory",new_id).unwrap();
        assert_ne!(replacement["id"],old["id"]);
        assert_eq!(replacement["text"],"주인이 정정한 최신 사실");
        assert_eq!(replacement["source"],"user");
        assert_eq!(replacement["scope"],"session");
        assert_eq!(replacement["sessionId"],"session-A");
        assert_eq!(replacement["agentId"],"agent-A");
        assert_eq!(replacement["partitionKey"],"local");
        assert_eq!(replacement["supersedes"],"learned");
        assert!(replacement["sourceMessageIds"].is_null(),
            "A new owner correction may not borrow old provenance");
        let stale=correct_derived_memory(&state,"learned",&json!({
            "expectedText":"자동 추출한 낡은 사실","text":"또 다른 수정"
        }));
        assert_eq!(stale.status(),StatusCode::CONFLICT);
        assert_eq!(state.store.all("memory").len(),2,
            "Two stale corrections must not publish competing active facts");
        assert!(crate::engine::memory::retrieval_eligible(&replacement));
        assert!(!crate::engine::memory::retrieval_eligible(&old));
    }
}


/// Owner corrections supersede a learned claim without erasing immutable
/// source message references. The record pair is committed atomically.
fn correct_derived_memory(state:&AppState,id:&str,body:&Value)->Response{
    let Some(text)=body["text"].as_str()
        .map(str::trim).filter(|text|!text.is_empty()&&text.chars().count()<=16_000)
    else{return (StatusCode::BAD_REQUEST,Json(json!({
        "error":"정정 내용은 1~16000자여야 합니다."
    }))).into_response()};
    let Some(expected)=body["expectedText"].as_str()else{
        return (StatusCode::BAD_REQUEST,Json(json!({
            "error":"정정할 기억의 이전 원문이 필요합니다."
        }))).into_response();
    };
    let (original,replacement)=match state.store.correct_memory_with_owner(
        id,expected,text
    ){
        Ok(result)=>result,
        Err(message)=>{
            let status=if message=="수정할 기억을 찾을 수 없습니다." {
                StatusCode::NOT_FOUND
            }else{StatusCode::CONFLICT};
            return (status,Json(json!({"error":message}))).into_response();
        }
    };
    state.store.audit("memory.owner_correction",json!({
        "oldMemoryId":id,"newMemoryId":replacement["id"],
        "oldSource":original["source"],
        "at":replacement["correctedAt"]
    }));
    state.bus.record("memory",&original);
    state.bus.record("memory",&replacement);
    Json(json!({"original":original,"memory":replacement})).into_response()
}
