use crate::state::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};
use std::path::PathBuf;

/// The registry distinguishes DOMINION-owned executable drivers from planned connectors.
fn manifests() -> PathBuf {
    if let Some(assets) = std::env::var_os("DOMINION_ASSETS") {
        return PathBuf::from(assets).join("channels");
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/channels")
}

/// Field contract for outbound adapters. The runtime owns required destination
/// keys, and the catalog and persistence use the same list.
fn outbound_fields(kind: &str, manifest: &Value) -> Vec<Value> {
    let label = |key| match (kind, key) {
        ("zulip", "channelId") => "전송할 스트림 이름 또는 ID",
        (_, "channelId") => "전송할 채널 또는 방 ID",
        (_, "chatId") => "받는 사람 ID 또는 전화번호",
        _ => key,
    };
    let required = crate::integrations::channels::adapter::required_fields(kind);
    let mut fields = vec![json!({
        "key":"token", "type":"secret",
        "label": if kind == "zulip" { "Zulip API 키" } else if kind == "signal" { "브리지 토큰" } else { "액세스 토큰" },
        "required":kind != "signal"
    })];
    for &key in required {
        let field = manifest["fields"].as_array().and_then(|fields| {
            fields.iter().find(|field| field["key"] == key)
        });
        let mut field = field.cloned().unwrap_or_else(|| json!({"key":key,"label":label(key),"type":"text"}));
        field["required"] = json!(true);
        fields.push(field);
    }
    if kind == "zulip" {
        fields.push(json!({"key":"topic","label":"전송할 주제","type":"text","placeholder":"DOMINION"}));
    }
    fields
}

pub fn catalog() -> Vec<Value> {
    let mut entries = Vec::new();
    let Ok(dirs) = std::fs::read_dir(manifests()) else {
        return entries;
    };
    for dir in dirs.flatten() {
        let path = dir.path().join("manifest.json");
        let Ok(text) = crate::storage::bounded_file::read_utf8(&path, 256 * 1024) else {
            continue;
        };
        let Ok(mut value) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        let available = value["id"]
            .as_str()
            .is_some_and(|id| matches!(id, "discord" | "telegram")
                || crate::integrations::channels::adapter::supports(id));
        value["available"] = json!(available);
        value["status"] = json!(if available {
            "available"
        } else {
            "unavailable"
        });
        value["requiresPublicUrl"] = json!(value
            .get("mode")
            .and_then(|mode| mode.as_str())
            .map(|mode| mode == "webhook")
            .unwrap_or(false));
        let kind = value["id"].as_str().unwrap_or_default().to_owned();
        if crate::integrations::channels::adapter::supports(&kind) {
            value["fields"] = json!(outbound_fields(&kind, &value));
            value["mode"] = json!("outbound");
            value["description"] = json!("DOMINION 회의실 메시지를 지정한 대상으로 보냅니다.");
            value["features"] = json!(["메시지 보내기"]);
            value["instructions"] = json!(["전송할 대상과 DOMINION 회의실을 선택하세요."]);
            value["requiresPublicUrl"] = json!(false);
            value["requiresRoom"] = json!(true);
        }
        if kind == "telegram" {
            value["requiresRoom"] = json!(true);
            if let Some(fields) = value["fields"].as_array_mut() {
                fields.retain(|field| field["key"] != "roomId");
            }
        }
        if kind == "discord" {
            value["maxRoomParticipants"] = json!(crate::integrations::channels::discord::MAX_PARTICIPANTS);
        }
        entries.push(value);
    }
    entries.sort_by(|a, b| {
        let left = (
            a.get("category").and_then(|v| v.as_str()).unwrap_or(""),
            a.get("name").and_then(|v| v.as_str()).unwrap_or(""),
        );
        let right = (
            b.get("category").and_then(|v| v.as_str()).unwrap_or(""),
            b.get("name").and_then(|v| v.as_str()).unwrap_or(""),
        );
        left.cmp(&right)
    });
    entries
}

pub fn connected_accounts(state: &AppState) -> Vec<Value> {
    state
        .store
        .all("oauth_config")
        .into_iter()
        .filter(|record| {
            record.get("id").and_then(Value::as_str)
                .is_some_and(crate::integrations::accounts::oauth::is_supported_provider)
                && record.get("connected").and_then(Value::as_bool) == Some(true)
        })
        .map(|record| {
            let id = record
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            json!({
                "id": id,
                "provider": id,
                "displayName": record.get("account").cloned().unwrap_or(json!("연결된 계정")),
                "scopes": record.get("scopes").cloned().unwrap_or(json!("")),
                "hasToken": state.store.has_secret(&format!("oauth:{id}:token")),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_state() -> (AppState, std::path::PathBuf) {
        let dir =
            std::env::temp_dir().join(format!("dominion-channel-api-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
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

    #[test]
    fn obsolete_upload_account_never_surfaces_even_before_startup_cleanup() {
        let (state, dir) = test_state();
        for id in ["github", "google", "youtube", "removed-plugin"] {
            state.store.put("oauth_config", &json!({
                "id":id,"connected":true,"account":format!("{id}-user"),
                "scopes":"user"
            })).unwrap();
            state.save_secret(&format!("oauth:{id}:token"), "legacy-grant");
        }
        let accounts=connected_accounts(&state);
        assert_eq!(accounts.len(),2,"only currently registered providers may be listed");
        assert!(accounts.iter().any(|a| a["provider"]=="github" && a["hasToken"]==true));
        assert!(accounts.iter().any(|a| a["provider"]=="google" && a["hasToken"]==true));
        assert!(!accounts.iter().any(|a| a["provider"]=="youtube"));
        assert!(state.store.get("oauth_config","youtube").is_some(),
            "account API filtering must not modify historical records");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn native_save_encrypts_tokens_and_accepts_nine_participants() {
        let (state, dir) = test_state();
        state.store.put(
            "room",
            &json!({"id":"room9","agentIds":(0..9).map(|i|format!("a{i}")).collect::<Vec<_>>()}),
        );
        let body = json!({"id":"binding","kind":"discord","guildId":"10","channelId":"20","roomId":"room9","token":"private-token","credentials":{"token":"nested-secret"},"enabled":false});
        let response = save_config(&state, None, body);
        assert_eq!(response.status(), StatusCode::OK);
        let public = String::from_utf8(
            axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        assert!(!public.contains("private-token"));
        assert!(!public.contains("nested-secret"));
        let saved = state.store.get("channel", "binding").unwrap();
        assert!(saved.get("token").is_none());
        assert!(saved.get("credentials").is_none());
        assert_eq!(saved["driver"], "dominion-discord");
        assert_eq!(saved["replyMentionBots"], false);
        let secret = crate::integrations::channels::discord::token_name("binding");
        assert_eq!(state.read_secret(&secret).as_deref(), Some("private-token"));
        assert_ne!(
            state.store.get_secret(&secret).as_deref(),
            Some("private-token")
        );
        let response = save_config(
            &state,
            Some("binding"),
            json!({"name":"변경","token":"","replyMentionBots":true}),
        );
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(state.read_secret(&secret).as_deref(), Some("private-token"));
        drop(state);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn public_legacy_records_hide_tokens_and_require_native_activation() {
        let (state, dir) = test_state();
        let public = public_channel(
            &state,
            &json!({"id":"legacy","kind":"discord","enabled":true,"status":"connected","token":"legacy-secret","credentials":{"token":"nested-secret"}}),
        );
        assert!(!public.to_string().contains("secret"));
        assert_eq!(public["status"], "not_started");
        drop(state);
        let _ = std::fs::remove_dir_all(dir);
    }
    #[test]
    fn telegram_configuration_uses_vault_room_binding_and_explicit_shared_sync() {
        let (state, dir) = test_state();
        state.store.put(
            "room",
            &json!({"id":"room","name":"공용","agentIds":["a0"]}),
        );
        let session = crate::engine::rooms::ensure_room_session(&state, "room").unwrap();
        state.store.write_message(
            &json!({"id":"old","session_id":session["id"],"role":"user","content":"기존 기록"}),
        );
        let response = save_config(
            &state,
            None,
            json!({"id":"tg","kind":"telegram","chatId":"-100","roomId":"room","allowedUserIds":["7"],"token":"fixture-token","enabled":false,"syncRoomMessages":true}),
        );
        assert_eq!(response.status(), StatusCode::OK);
        let config = state.store.get("channel", "tg").unwrap();
        assert_eq!(config["driver"], "dominion-telegram");
        assert_eq!(config["bridgeStartMessageId"], "old");
        assert!(config.get("token").is_none());
        assert_eq!(
            state
                .read_secret(&crate::integrations::channels::discord::token_name("tg"))
                .as_deref(),
            Some("fixture-token")
        );
        let public = public_channel(&state, &config);
        assert_eq!(public["chatId"], "-100");
        assert_eq!(public["syncRoomMessages"], true);
        assert!(!public.to_string().contains("fixture-token"));
        let response = save_config(&state, Some("tg"), json!({"allowedUserIds":[7]}));
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let response = save_config(
            &state,
            Some("tg"),
            json!({"toolsEnabled":true,"allowedUserIds":[]}),
        );
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn slack_adapter_is_saved_with_its_driver_and_required_fields() {
        let (state, dir) = test_state();
        let saved = save_config(
            &state,
            Some("slack-check"),
            json!({
                "kind":"slack","name":"Slack check","channelId":"C1",
                "enabled":false,"syncRoomMessages":false,"token":"xoxb-test"
            }),
        );
        assert_eq!(saved.status(), StatusCode::OK);
        let record = state.store.get("channel", "slack-check").unwrap();
        assert_eq!(record["driver"], "dominion-slack");
        assert_eq!(record["kind"], "slack");
        assert!(record.get("token").is_none());
        let secret = crate::integrations::channels::discord::token_name("slack-check");
        assert_eq!(state.read_secret(&secret).as_deref(), Some("xoxb-test"));
        let updated = save_config(
            &state,
            Some("slack-check"),
            json!({"channelId":"C2","token":""}),
        );
        assert_eq!(updated.status(), StatusCode::OK);
        assert_eq!(state.read_secret(&secret).as_deref(), Some("xoxb-test"));
        let missing = save_config(
            &state,
            Some("slack-bad"),
            json!({"kind":"slack","name":"bad","enabled":false,"token":"xoxb-test"}),
        );
        assert_eq!(missing.status(), StatusCode::BAD_REQUEST);
        assert!(!state.store.has_secret(&crate::integrations::channels::discord::token_name("slack-bad")));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn outbound_catalog_advertises_the_runtime_destination_and_secret_contract() {
        let catalog = catalog();
        for &kind in crate::integrations::channels::adapter::SUPPORTED {
            let channel = catalog.iter().find(|entry| entry["id"] == kind).unwrap();
            assert_eq!(channel["mode"], "outbound");
            assert_eq!(channel["requiresRoom"], true);
            let fields = channel["fields"].as_array().unwrap();
            let public_required = fields.iter().filter(|field| field["type"] != "secret" && field["required"] == true)
                .map(|field| {
                    field["key"].as_str().unwrap()
                }).collect::<Vec<_>>();
            assert_eq!(public_required, crate::integrations::channels::adapter::required_fields(kind));
            let secrets = fields.iter().filter(|field| field["type"] == "secret").collect::<Vec<_>>();
            assert_eq!(secrets.len(), 1);
            assert_eq!(secrets[0]["key"], "token");
            assert_eq!(secrets[0]["required"], kind != "signal");
        }
    }

    #[tokio::test]
    async fn channel_listing_aggregates_outbox_once_and_reports_latest_failure() {
        let (state,dir)=test_state();
        for id in ["a","b"] {
            state.store.put("channel",&json!({"id":id,"kind":"slack",
                "driver":"dominion-slack","enabled":true,"roomId":"meeting",
                "channelId":"C1","syncRoomMessages":true})).unwrap();
        }
        for (id,binding,status,error,date) in [
            ("old","a","needs_review","old problem","2026-11-19T01:00:00Z"),
            ("sent","a","sent","","2026-11-20T02:00:00Z"),
            ("new","a","needs_review","latest problem","2026-11-20T04:00:00Z"),
            ("other","b","needs_review","different channel","2026-11-20T03:00:00Z"),
        ] {
            state.store.put("channel_outbox",&json!({
                "id":id,"bindingId":binding,"status":status,
                "error":error,"errorAt":date
            })).unwrap();
        }
        let values=list(State(state.clone())).await.0;
        let entries=values.as_array().unwrap();
        let a=entries.iter().find(|row|row["id"]=="a").unwrap();
        let b=entries.iter().find(|row|row["id"]=="b").unwrap();
        assert_eq!(a["delivery"]["needsReview"],2);
        assert_eq!(a["delivery"]["sent"],1);
        assert_eq!(a["delivery"]["lastError"],"latest problem");
        assert_eq!(b["delivery"]["needsReview"],1);
        assert_eq!(b["delivery"]["lastError"],"different channel");
        let direct=public_channel(&state,&state.store.get("channel","a").unwrap());
        assert_eq!(a["delivery"],direct["delivery"],
            "list and detail must use identical lifecycle accounting");
        let _=std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn review_delivery_is_explicit_scoped_and_never_resends() {
        let (state,dir)=test_state();
        for id in ["a","b"] {
            state.store.put("channel",&json!({"id":id,"kind":"slack","enabled":false})).unwrap();
        }
        for (id,binding) in [("review-a","a"),("review-b","b")] {
            state.store.put("channel_outbox",&json!({
                "id":id,"bindingId":binding,"status":"needs_review",
                "content":"Do not leak the entire original content","error":"uncertain send",
                "errorAt":"2026-11-20T08:30:00Z"
            })).unwrap();
        }
        let resp=delivery_reviews(State(state.clone()),Path("a".into())).await;
        assert_eq!(resp.status(),StatusCode::OK);
        let body=axum::body::to_bytes(resp.into_body(),usize::MAX).await.unwrap();
        let list:Value=serde_json::from_slice(&body).unwrap();
        assert_eq!(list["total"],1);
        assert_eq!(list["items"][0]["id"],"review-a");
        let foreign=reconcile_delivery(State(state.clone()),Path(("a".into(),"review-b".into())),
            Json(json!({"resolution":"confirmed_delivered"}))).await;
        assert_eq!(foreign.status(),StatusCode::CONFLICT);
        assert_eq!(state.store.get("channel_outbox","review-b").unwrap()["status"],"needs_review");
        let first=reconcile_delivery(State(state.clone()),Path(("a".into(),"review-a".into())),
            Json(json!({"resolution":"confirmed_not_delivered"}))).await;
        assert_eq!(first.status(),StatusCode::OK);
        let stored=state.store.get("channel_outbox","review-a").unwrap();
        assert_eq!(stored["status"],"cancelled");
        assert_eq!(stored["ownerResolution"],"confirmed_not_delivered");
        assert!(stored["resolvedAt"].as_str().is_some());
        assert_eq!(stored["content"],"Do not leak the entire original content");
        let repeated=reconcile_delivery(State(state.clone()),Path(("a".into(),"review-a".into())),
            Json(json!({"resolution":"confirmed_delivered"}))).await;
        assert_eq!(repeated.status(),StatusCode::CONFLICT,
            "an operator cannot accidentally double-settle an external side effect");
        assert_eq!(state.store.get("channel_outbox","review-a").unwrap()["status"],"cancelled");
        let done=delivery_reviews(State(state.clone()),Path("a".into())).await;
        let bytes=axum::body::to_bytes(done.into_body(),usize::MAX).await.unwrap();
        assert_eq!(serde_json::from_slice::<Value>(&bytes).unwrap()["total"],0);
        let _=std::fs::remove_dir_all(dir);
    }

    #[test]
    fn catalog_exposes_only_implemented_drivers() {
        let entries = catalog();
        let implemented = ["discord", "telegram", "slack", "matrix", "whatsapp",
            "line", "zulip", "mattermost", "signal"];
        assert_eq!(entries.len(), implemented.len(),
            "catalog should describe only installed, executable channel drivers");
        let discord = entries
            .iter()
            .find(|entry| entry["id"] == "discord")
            .expect("discord entry");
        assert_eq!(discord["available"], true);
        for kind in implemented {
            let entry = entries.iter().find(|e| e["id"] == kind)
                .unwrap_or_else(|| panic!("missing {kind}"));
            assert_eq!(entry["available"], true, "{entry}");
        }
        assert!(discord["fields"].as_array().is_some());
    }
}

pub async fn catalog_handler() -> Json<Value> {
    Json(serde_json::json!(catalog()))
}

pub async fn connections(State(state): State<AppState>) -> Json<Value> {
    Json(serde_json::json!(connected_accounts(&state)))
}

fn error(status: StatusCode, message: impl Into<String>) -> Response {
    (status, Json(json!({"error":message.into()}))).into_response()
}

/// Never return a submitted token, including legacy records written by the
/// generic collection API. Public configuration uses an explicit field list.
pub fn public_channel(state: &AppState, record: &Value) -> Value {
    let outbox = state.store.all("channel_outbox").into_iter()
        .filter(|item|item["bindingId"]==record["id"]).collect::<Vec<_>>();
    public_channel_with_delivery(state,record,&outbox)
}

fn public_channel_with_delivery(state:&AppState,record:&Value,outbox:&[Value])->Value{
    let mut public = json!({});
    for key in [
        "id",
        "name",
        "kind",
        "agentId",
        "providerId",
        "model",
        "enabled",
        "mentionOnly",
        "roomId",
        "roomPolicy",
        "guildId",
        "channelId",
        "chatId",
        "syncRoomMessages",
        "allowedUserIds",
        "allowedBotIds",
        "replyMentionBots",
        "toolsEnabled",
        "driver",
        "status",
        "runtimeError",
        "revision",
        "createdAt",
        "updatedAt",
    ] {
        if let Some(value) = record.get(key) {
            public[key] = value.clone();
        }
    }
    let present = state.store.has_secret(&crate::integrations::channels::discord::token_name(
        crate::integrations::channels::discord::text(record, "id"),
    )) || record
        .get("token")
        .and_then(Value::as_str)
        .map(|s| !s.is_empty())
        .unwrap_or(false);
    let kind = record["kind"].as_str().unwrap_or_default();
    let outbound = crate::integrations::channels::adapter::supports(kind);
    let required = if kind == "telegram" {
        vec!["chatId", "roomId"]
    } else if outbound {
        crate::integrations::channels::adapter::required_fields(kind).iter().copied()
            .chain(std::iter::once("roomId")).collect()
    } else {
        vec!["guildId", "channelId"]
    };
    if outbound {
        for key in crate::integrations::channels::adapter::required_fields(kind).iter().copied()
            .chain((kind == "zulip").then_some("topic"))
        {
            if let Some(value) = record.get(key) { public[key] = value.clone(); }
        }
    }
    let mut missing = required.into_iter()
        .filter(|key| crate::integrations::channels::discord::text(record, key).is_empty())
        .collect::<Vec<_>>();
    if kind != "signal" && !present { missing.push("token"); }
    public["credentialsPresent"] = json!({"token":present});
    public["configured"] = json!(missing.is_empty());
    public["missing"] = json!(missing);
    // Diagnostics should report the most recent reviewed failure, not the
    // first record returned by SQLite. The durable outbox remains untouched.
    let last_error=outbox.iter().filter(|item|item["status"]=="needs_review")
        .max_by(|a,b|{
            let when_a=a["errorAt"].as_str().or_else(||a["updatedAt"].as_str())
                .or_else(||a["createdAt"].as_str()).unwrap_or("");
            let when_b=b["errorAt"].as_str().or_else(||b["updatedAt"].as_str())
                .or_else(||b["createdAt"].as_str()).unwrap_or("");
            when_a.cmp(when_b)
        }).and_then(|item|item.get("error")).cloned();
    public["delivery"] = json!({
        "pending":outbox.iter().filter(|item|item["status"]=="pending").count(),
        "sending":outbox.iter().filter(|item|item["status"]=="sending").count(),
        "sent":outbox.iter().filter(|item|item["status"]=="sent").count(),
        "needsReview":outbox.iter().filter(|item|item["status"]=="needs_review").count(),
        "lastError":last_error,
    });
    if record.get("enabled").and_then(Value::as_bool) != Some(true) {
        public["status"] = json!("disabled");
    } else if !matches!(
        (record["kind"].as_str(), record["driver"].as_str()),
        (Some("discord"), Some("dominion-discord")) | (Some("telegram"), Some("dominion-telegram"))
    ) && !record["kind"].as_str().is_some_and(|kind| {
        crate::integrations::channels::adapter::supports(kind)
            && record["driver"].as_str() == Some(format!("dominion-{kind}").as_str())
    }) {
        public["status"] = json!("not_started");
        public["runtimeError"] = json!("Rust 연결 설정에서 저장·활성화가 필요합니다.");
    } else if outbound {
        public["status"] = json!(if public["configured"] != true { "not_started" } else if record["syncRoomMessages"] == true { "outbound_ready" } else { "sync_disabled" });
    }
    public
}

pub async fn list(State(state): State<AppState>) -> Json<Value> {
    // A busy connection list previously read the entire outbox once for
    // *each* binding. Group the durable rows in one pass for the whole page.
    let mut grouped=std::collections::HashMap::<String,Vec<Value>>::new();
    for item in state.store.all("channel_outbox") {
        if let Some(binding_id)=item["bindingId"].as_str().map(str::to_string) {
            grouped.entry(binding_id).or_default().push(item);
        }
    }
    let channels=state.store.all("channel");
    Json(json!(channels.iter().map(|record|{
        let outbox=grouped.get(record["id"].as_str().unwrap_or(""))
            .map(Vec::as_slice).unwrap_or(&[]);
        public_channel_with_delivery(&state,record,outbox)
    }).collect::<Vec<_>>()))
}

pub async fn one(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match state.store.get("channel", &id) {
        Some(c) => Json(public_channel(&state, &c)).into_response(),
        None => error(StatusCode::NOT_FOUND, "연결을 찾을 수 없습니다."),
    }
}

pub async fn save(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    save_config(&state, None, body)
}
pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    save_config(&state, Some(&id), body)
}

fn save_config(state: &AppState, path_id: Option<&str>, body: Value) -> Response {
    if !body.is_object() {
        return error(StatusCode::BAD_REQUEST, "연결 설정 객체가 필요합니다.");
    }
    let id = path_id
        .map(str::to_string)
        .or_else(|| {
            body.get("id")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        })
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    if id.len() > 128 || id.chars().any(|c| c.is_control() || c == '/' || c == ':') {
        return error(StatusCode::BAD_REQUEST, "연결 ID가 올바르지 않습니다.");
    }
    let existing = state.store.get("channel", &id);
    let mut config = json!({"id":id,"kind":"discord","agentId":"general","enabled":false,"mentionOnly":true,"replyMentionBots":false,"toolsEnabled":false,"syncRoomMessages":false,"roomPolicy":"all_or_named"});
    let kind = body["kind"].as_str().or_else(|| existing.as_ref().and_then(|record| record["kind"].as_str())).unwrap_or("discord");
    let outbound = crate::integrations::channels::adapter::supports(kind);
    if !outbound && !matches!(kind, "discord" | "telegram") {
        return error(StatusCode::BAD_REQUEST, "지원하지 않는 채널입니다.");
    }
    if existing.as_ref().is_some_and(|record| record["kind"] != kind) {
        return error(StatusCode::BAD_REQUEST, "다른 메신저는 새 연결로 등록하세요.");
    }
    for key in [
        "name",
        "kind",
        "agentId",
        "providerId",
        "model",
        "enabled",
        "mentionOnly",
        "roomId",
        "roomPolicy",
        "guildId",
        "channelId",
        "chatId",
        "syncRoomMessages",
        "allowedUserIds",
        "allowedBotIds",
        "replyMentionBots",
        "toolsEnabled",
        "topic",
        "apiBase",
        "graphBase",
    ].into_iter().chain(crate::integrations::channels::adapter::required_fields(kind).iter().copied()) {
        if let Some(value) = body
            .get(key)
            .or_else(|| existing.as_ref().and_then(|v| v.get(key)))
        {
            config[key] = value.clone();
        }
    }
    for key in [
        "enabled",
        "mentionOnly",
        "replyMentionBots",
        "toolsEnabled",
        "syncRoomMessages",
    ] {
        if !config[key].is_boolean() {
            return error(
                StatusCode::BAD_REQUEST,
                format!("{key}는 true/false여야 합니다."),
            );
        }
    }
    for key in ["allowedUserIds", "allowedBotIds"] {
        if let Some(value) = config.get(key) {
            if !value.is_string()
                && !value
                    .as_array()
                    .is_some_and(|ids| ids.iter().all(Value::is_string))
            {
                return error(
                    StatusCode::BAD_REQUEST,
                    format!("{key}에는 문자열 ID 목록이 필요합니다."),
                );
            }
        }
    }
    let kind = config["kind"].as_str().unwrap_or("");
    config["driver"] = json!(if kind == "telegram" {
        "dominion-telegram".to_string()
    } else if crate::integrations::channels::adapter::supports(kind) {
        format!("dominion-{kind}")
    } else {
        "dominion-discord".to_string()
    });
    config["revision"] = json!(uuid::Uuid::new_v4().to_string());
    config["status"] = json!(
        if config.get("enabled").and_then(Value::as_bool) == Some(true) {
            "starting"
        } else {
            "disabled"
        }
    );
    let kind = config["kind"].as_str().unwrap_or("");
    if outbound && config["enabled"] == true && crate::integrations::channels::discord::text(&config, "roomId").is_empty() {
        return error(StatusCode::BAD_REQUEST, "메시지를 보낼 DOMINION 회의실을 선택하세요.");
    }
    let validation = if kind == "telegram" {
        crate::integrations::channels::telegram::validate(state, &config)
    } else if crate::integrations::channels::adapter::supports(kind) {
        crate::integrations::channels::adapter::validate(state, &config)
    } else {
        crate::integrations::channels::discord::validate(state, &config)
    };
    if let Err(message) = validation {
        return error(StatusCode::BAD_REQUEST, message);
    }
    let token = body
        .get("token")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty() && !s.starts_with("••"))
        .or_else(|| {
            existing
                .as_ref()
                .and_then(|v| v.get("token"))
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
        });
    let secret_name = crate::integrations::channels::discord::token_name(&id);
    let has_secret = state.store.has_secret(&secret_name);
    let token = token.or_else(|| {
        if has_secret {
            return None;
        }
        existing.as_ref()?.get("token")?.as_str().filter(|value| !value.is_empty())
    });
    if kind != "signal" && token.is_none() && !has_secret {
        return error(StatusCode::BAD_REQUEST, "채널 토큰을 입력하세요.");
    }
    if let Some(token) = token {
        if token.len() > 512 || token.chars().any(char::is_whitespace) {
            return error(
                StatusCode::BAD_REQUEST,
                "Bot Token 형식이 올바르지 않습니다.",
            );
        }
    }
    if !crate::integrations::channels::discord::text(&config, "roomId").is_empty() {
        match crate::engine::rooms::ensure_room_session(
            state,
            crate::integrations::channels::discord::text(&config, "roomId"),
        ) {
            Ok(session) => {
                config["bridgeStartMessageId"] = state
                    .store
                    .messages(crate::integrations::channels::discord::text(&session, "id"))
                    .last()
                    .map(|message| message["id"].clone())
                    .unwrap_or(Value::Null)
            }
            Err(message) => return error(StatusCode::BAD_REQUEST, message),
        }
    }
    let sealed = token.map(|token| state.vault.seal(token));
    if sealed.as_ref().is_some_and(String::is_empty) {
        return error(StatusCode::INTERNAL_SERVER_ERROR, "채널 토큰을 암호화하지 못했습니다.");
    }
    match state.store.save_channel(
        &config,
        sealed.as_deref().map(|value| (secret_name.as_str(), value)),
    ) {
        Some(record) => {
            let public = public_channel(state, &record);
            state.bus.record("channel", &public);
            state.store.audit(
                "channel.saved",
                json!({"connectionId":id,"kind":config["kind"],"enabled":config.get("enabled")}),
            );
            Json(public).into_response()
        }
        None => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "연결 설정을 저장할 수 없습니다.",
        ),
    }
}

pub async fn remove(State(state): State<AppState>, Path(id): Path<String>) -> Json<Value> {
    state.store.remove("channel", &id);
    state
        .store
        .remove_secret(&crate::integrations::channels::discord::token_name(&id));
    state
        .bus
        .emit(json!({"type":"record","kind":"channel","removed":id}));
    Json(json!({"ok":true}))
}

/// Only expose owner-reviewable uncertain effects; keep the original
/// message in the durable outbox and omit unrelated channels.
pub async fn delivery_reviews(State(state):State<AppState>,Path(id):Path<String>)->Response{
    if state.store.get("channel",&id).is_none(){
        return error(StatusCode::NOT_FOUND,"채널 연결을 찾을 수 없습니다.");
    }
    let mut reviews=state.store.all("channel_outbox").into_iter()
        .filter(|row|row["bindingId"].as_str()==Some(id.as_str())&&row["status"]=="needs_review")
        .collect::<Vec<_>>();
    reviews.sort_by(|a,b|{
        let time_a=a["errorAt"].as_str().or_else(||a["updatedAt"].as_str()).unwrap_or("");
        let time_b=b["errorAt"].as_str().or_else(||b["updatedAt"].as_str()).unwrap_or("");
        time_b.cmp(time_a).then_with(||b["id"].as_str().cmp(&a["id"].as_str()))
    });
    let total=reviews.len();
    let items=reviews.into_iter().take(20).map(|row|json!({
        "id":row["id"],"messageId":row["messageId"],"partIndex":row["partIndex"],
        "error":row["error"],"errorAt":row["errorAt"],"updatedAt":row["updatedAt"],
        "preview":row["content"].as_str().unwrap_or("").chars().take(320).collect::<String>(),
    })).collect::<Vec<_>>();
    Json(json!({"total":total,"items":items})).into_response()
}

pub async fn reconcile_delivery(
    State(state):State<AppState>,
    Path((binding_id,delivery_id)):Path<(String,String)>,
    Json(body):Json<Value>,
)->Response{
    let Some(outcome)=body["resolution"].as_str()else{
        return error(StatusCode::BAD_REQUEST,"전송 확인 결과를 선택하세요.");
    };
    let confirmed=match outcome{
        "confirmed_delivered"=>true,
        "confirmed_not_delivered"=>false,
        _=>return error(StatusCode::BAD_REQUEST,"지원하지 않는 전송 확인 결과입니다."),
    };
    if state.store.get("channel",&binding_id).is_none(){
        return error(StatusCode::NOT_FOUND,"연결된 채널이 존재하지 않습니다.");
    }
    match state.store.reconcile_channel_outbox(&binding_id,&delivery_id,confirmed){
        Ok(record)=>{
            state.store.audit("channel.delivery.reconciled",json!({
                "channelId":binding_id,"deliveryId":delivery_id,
                "resolution":outcome,"automaticResend":false,
            }));
            state.bus.record("channel_outbox",&record);
            Json(json!({"ok":true,"id":delivery_id,"resolution":outcome})).into_response()
        }
        Err(message)=>error(StatusCode::CONFLICT,message),
    }
}

pub async fn diagnose(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let Some(config) = state.store.get("channel", &id) else {
        return error(StatusCode::NOT_FOUND, "연결을 찾을 수 없습니다.");
    };
    let result = match config["kind"].as_str() {
        Some("telegram") => crate::integrations::channels::telegram::diagnose(&state, &config).await,
        Some("discord") => crate::integrations::channels::discord::diagnose(&state, &config).await,
        Some(kind) if crate::integrations::channels::adapter::supports(kind) => {
            let public = public_channel(&state, &config);
            return match crate::integrations::channels::adapter::diagnose(&state, &config).await {
                Ok(mut value) => {
                    value["configured"] = public["configured"].clone();
                    Json(value).into_response()
                }
                Err(message) => Json(json!({
                    "verified": false,
                    "configured": public["configured"],
                    "status": public["status"],
                    "missing": public["missing"],
                    "message": message,
                }))
                .into_response(),
            };
        }
        _ => return error(StatusCode::NOT_IMPLEMENTED, "이 채널의 진단을 지원하지 않습니다."),
    };
    match result {
        Ok(result) => Json(result).into_response(),
        Err(message) => Json(json!({"verified":false,"message":message})).into_response(),
    }
}
