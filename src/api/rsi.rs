//! Read-only compatibility for historical RSI records.
//! No generation, mutation, evaluator, auto-adoption, or native worker is reachable.
use crate::state::AppState;
use axum::{extract::{Path,State},http::StatusCode,response::{IntoResponse,Response},Json};
use serde_json::{json,Value};
fn missing() -> Response {(StatusCode::NOT_FOUND,Json(json!({"error":"Archived record not found"}))).into_response()}
fn record(state:&AppState,kind:&str,id:&str)->Response {
    state.store.get(kind,id).map(|value|Json(value).into_response()).unwrap_or_else(missing)
}
pub async fn list(State(state):State<AppState>)->Json<Value>{Json(json!(state.store.all("rsi_candidate")))}
pub async fn one(State(state):State<AppState>,Path(id):Path<String>)->Response{record(&state,"rsi_candidate",&id)}
pub async fn campaigns(State(state):State<AppState>)->Json<Value>{Json(json!(state.store.all("rsi_campaign")))}
pub async fn campaign(State(state):State<AppState>,Path(id):Path<String>)->Response{record(&state,"rsi_campaign",&id)}
pub async fn verify(State(state):State<AppState>,Path(id):Path<String>)->Response{
    // Stored evidence only: retired experiments are never re-executed by a GET.
    record(&state,"rsi_campaign",&id)
}
pub async fn issue(State(state):State<AppState>,Path(id):Path<String>)->Response{record(&state,"rsi_issue",&id)}
pub async fn evaluation_profiles(State(state):State<AppState>)->Json<Value>{
    Json(json!({"retired":true,"profiles":state.store.all("rsi_evaluation_profile")}))
}
pub async fn signals(State(state):State<AppState>)->Json<Value>{Json(json!(state.store.all("rsi_signal")))}
pub async fn index(State(state):State<AppState>)->Json<Value>{Json(json!(state.store.all("code_index")))}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn old_rsi_state_is_exposed_without_running_generations(){
        let root=std::env::temp_dir().join(format!("rsi-archive-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let config=crate::config::Config{port:0,data_dir:root.clone(),web_dir:root.join("web"),workspace:root.clone()};
        let state=AppState::new(config.clone(),crate::db::Store::open(&config.db_path()).unwrap(),
            crate::vault::Vault::open(&config.key_path()));
        state.store.put("rsi_campaign",&json!({"id":"archived","status":"completed","history":[{"accepted":true}]}));
        assert_eq!(state.store.get("rsi_campaign","archived").unwrap()["history"][0]["accepted"],true);
        drop(state);
        let _=std::fs::remove_dir_all(root);
    }
}
