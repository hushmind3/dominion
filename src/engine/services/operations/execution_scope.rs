//! A single policy owner for the Home SDK compatibility panel.
//!
//! Per-job execution settings are stored on the session for the historical
//! chat adapter, but must not silently constrain later owner chat or other
//! jobs after the original job is finished. UI presentation is not an
//! authority; only the matching running Rust job may activate this policy.
use crate::state::AppState;
use serde_json::Value;

/// Deny stale, foreign or forged session policies without mutating user data.
pub fn active_panel_policy(state: &AppState, session: &Value) -> Value {
    let Some(owner) = session["sdkUiPolicyOwnerId"].as_str() else {
        return Value::Null;
    };
    let Some(job) = state.store.get("job", owner) else {
        return Value::Null;
    };
    if job["status"] != "running"
        || job["kind"] != "prompt"
        || job["input"]["uiSource"] != "native_sdk_panel"
        || job["sessionId"] != session["id"]
        || !session["sdkUiPolicy"].is_object()
        || job["input"]["sdkUiPolicy"] != session["sdkUiPolicy"]
    {
        return Value::Null;
    }
    session["sdkUiPolicy"].clone()
}

/// Limits for a job are measured from its own admission baseline rather
/// than charging it for earlier turns in a reused conversation session.
pub fn usage_since_baseline(session: &Value, usage: &Value, key: &str) -> u64 {
    usage[key].as_u64().unwrap_or(0).saturating_sub(
        session["sdkUiPolicyBaseline"][key].as_u64().unwrap_or(0)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::Config, db::Store, vault::Vault};
    use serde_json::json;

    #[test]
    fn sdk_policy_is_only_active_for_its_live_owner_and_session() {
        let dir=std::env::temp_dir().join(format!(
            "dominion-sdk-policy-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config=Config{port:0,workspace:dir.clone(),
            web_dir:dir.join("web"),data_dir:dir.join("data")};
        let state=AppState::new(config.clone(),
            Store::open(&config.db_path()).unwrap(),
            Vault::open(&config.key_path()));
        let session=state.store.put("session",&json!({"id":"s"})).unwrap();
        let id=session["id"].as_str().unwrap();
        let policy=json!({"maxRounds":8,"maxTotalTokens":120000,
            "webEnabled":false});
        let job=state.store.put("job",&json!({
            "id":"live-panel","kind":"prompt","status":"running",
            "sessionId":id,
            "input":{"uiSource":"native_sdk_panel","sdkUiPolicy":policy}
        })).unwrap();
        let mut owned=session.clone();
        owned["sdkUiPolicyOwnerId"]=json!("live-panel");
        owned["sdkUiPolicy"]=policy.clone();
        owned["sdkUiPolicyBaseline"]=json!({"calls":10,"totalTokens":4000});
        assert_eq!(active_panel_policy(&state,&owned),policy);
        let usage=json!({"calls":12,"totalTokens":4240});
        assert_eq!(usage_since_baseline(&owned,&usage,"calls"),2);
        assert_eq!(usage_since_baseline(&owned,&usage,"totalTokens"),240);
        assert_eq!(usage_since_baseline(&owned,&json!({"calls":9}),"calls"),0);
        let mut foreign=owned.clone();
        foreign["id"]=json!("another-session");
        assert!(active_panel_policy(&state,&foreign).is_null());
        foreign=owned.clone();
        foreign["sdkUiPolicy"]["webEnabled"]=json!(true);
        assert!(active_panel_policy(&state,&foreign).is_null());
        foreign=owned.clone();
        foreign["sdkUiPolicyOwnerId"]=Value::Null;
        assert!(active_panel_policy(&state,&foreign).is_null());
        let mut done=job.clone();
        done["status"]=json!("completed");
        state.store.put("job",&done);
        assert!(active_panel_policy(&state,&owned).is_null());
        // Stale settings remain archived without becoming live authority.
        assert_eq!(owned["sdkUiPolicy"]["maxRounds"],8);
        let _=std::fs::remove_dir_all(dir);
    }
}
