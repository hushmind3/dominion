use super::{sessions::*,settings::*};
use axum::{extract::{Path,State},http::StatusCode,Json};
use crate::state::AppState;
use serde_json::json;
#[cfg(test)]
mod settings_tests {
    use super::*;
    fn state() -> AppState {
        let root =
            std::env::temp_dir().join(format!("dominion-core-settings-{}", uuid::Uuid::new_v4()));
        let config = crate::config::Config {
            port: 0,
            data_dir: root.clone(),
            web_dir: root.join("web"),
            workspace: root,
        };
        AppState::new(
            config.clone(),
            crate::db::Store::open(&config.db_path()).unwrap(),
            crate::vault::Vault::open(&config.key_path()),
        )
    }
    #[tokio::test]
    async fn ordinary_chat_mutations_cannot_destroy_room_or_channel_history() {
        let state = state();
        for (id, binding) in [
            ("room:active", json!({"roomId":"room-1","kind":"room"})),
            ("room:archived", json!({"roomId":"removed-room","kind":"room"})),
            ("channel:old", json!({"channelContext":{"kind":"discord","channelId":"c1"}})),
        ] {
            let mut session = binding;
            session["id"] = json!(id);
            state.store.put("session", &session).unwrap();
            state.store.write_message(&json!({
                "session_id": id, "role": "user", "content": "잃어버리면 안 되는 원문"
            })).unwrap();

            let delete = delete_session(State(state.clone()), Path(id.into())).await;
            assert_eq!(delete.status(), StatusCode::CONFLICT, "delete {id}");
            let clear = clear_session(State(state.clone()), Path(id.into())).await;
            assert_eq!(clear.status(), StatusCode::CONFLICT, "clear {id}");
            let fork = fork_session(State(state.clone()), Path(id.into())).await;
            assert_eq!(fork.status(), StatusCode::CONFLICT, "fork {id}");
            let rename = rename_session(
                State(state.clone()), Path(id.into()), Json(json!({"title":"private"}))
            ).await;
            assert_eq!(rename.status(), StatusCode::CONFLICT, "rename {id}");
            let compact = compact_session(
                State(state.clone()), Path(id.into()), Json(json!({}))
            ).await;
            assert_eq!(compact.status(), StatusCode::CONFLICT, "compact {id}");
            assert_eq!(state.store.messages(id).len(), 1, "transcript {id}");
            assert!(state.store.get("session", id).is_some(), "session {id}");
        }

        state.store.put("session", &json!({"id":"private-live"})).unwrap();
        let private = "private-live";
        state.store.write_message(&json!({
            "session_id":private,"role":"user","content":"대화 원문"
        })).unwrap();
        assert!(state.begin_session(private));
        let busy = clear_session(State(state.clone()), Path(private.into())).await;
        assert_eq!(busy.status(), StatusCode::CONFLICT);
        assert_eq!(state.store.messages(private).len(), 1);
        state.end_session(private);
        let fork = fork_session(State(state.clone()), Path(private.into())).await;
        assert_eq!(fork.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(fork.into_body(), 8192).await.unwrap();
        let copy: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let fork_id = copy["id"].as_str().unwrap();
        assert_ne!(fork_id, private);
        assert_eq!(state.store.messages(fork_id).len(), 1);
        let cleared = clear_session(State(state.clone()), Path(private.into())).await;
        assert_eq!(cleared.status(), StatusCode::OK);
        assert!(state.store.messages(private).is_empty());
        let removed = delete_session(State(state.clone()), Path(private.into())).await;
        assert_eq!(removed.status(), StatusCode::OK);
        assert!(state.store.get("session", private).is_none());
    }

    #[tokio::test]
    async fn archive_preserves_transcript_and_creation_cannot_rebind_room() {
        let state = state();
        state
            .store
            .put("session", &json!({"id":"private","title":"보관할 대화"}));
        state.store.write_message(
            &json!({"session_id":"private","role":"user","content":"지워지면 안 되는 원문"}),
        );
        let response = archive_session(
            State(state.clone()),
            Path("private".into()),
            Json(json!({"archived":true})),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            state.store.get("session", "private").unwrap()["archived"],
            true
        );
        assert_eq!(state.store.messages("private").len(), 1);
        archive_session(
            State(state.clone()),
            Path("private".into()),
            Json(json!({"archived":false})),
        )
        .await;
        assert_eq!(
            state.store.get("session", "private").unwrap()["archived"],
            false
        );
        state
            .store
            .put("session", &json!({"id":"bound","roomId":"real-room"}));
        let created = create_session(
            State(state.clone()),
            Json(json!({"id":"bound","roomId":"forged-room","title":"new chat"})),
        )
        .await;
        assert_eq!(created.status(), StatusCode::OK);
        let body = axum::body::to_bytes(created.into_body(), 8192).await.unwrap();
        let created: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_ne!(created["id"], "bound");
        assert!(created["roomId"].is_null());
        assert_eq!(
            state.store.get("session", "bound").unwrap()["roomId"],
            "real-room"
        );
        let response = save_assistant_settings(
            State(state.clone()),
            Json(json!({"instructions":"주인 공통 지침","enabled":true})),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            assistant_settings(State(state.clone())).await.0["instructions"],
            "주인 공통 지침"
        );
        let bad=save_owner_profile(
            State(state.clone()),Json(json!({"name":"주인","avatar":"javascript:alert(1)"}))
        ).await;
        assert_eq!(bad.status(),StatusCode::BAD_REQUEST);
        let empty=save_owner_profile(
            State(state.clone()),Json(json!({"name":"주인","avatar":""}))
        ).await;
        assert_eq!(empty.status(),StatusCode::OK);
        let stored=owner_profile(State(state.clone())).await.0;
        assert_eq!(stored["name"],"주인");
        assert_eq!(stored["avatar"],"");
        // A profile persists through the same Store used by other views.
        assert_eq!(state.store.get("owner_profile","global").unwrap()["name"],"주인");
        assert!(!super::super::settings::valid_profile_avatar(
            "data:image/png;base64,SGVsbG8="
        ));
        let _ = std::fs::remove_dir_all(&state.config.data_dir);
    }
}
