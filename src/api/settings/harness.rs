//! Owner API for the actual Rust model-turn control plane.
use super::*;
use crate::engine::harness_settings::{self,HarnessSettings};

pub async fn get(State(state):State<AppState>)->Json<Value>{
    Json(json!(harness_settings::current(&state)))
}

pub async fn save(State(state):State<AppState>,Json(body):Json<Value>)->Response{
    let config=match HarnessSettings::parse_owner(&body){
        Ok(config)=>config,
        Err(message)=>return error(StatusCode::BAD_REQUEST,&message),
    };
    let record=json!(config);
    match state.store.put("harness_settings",&record){
        Some(saved)=>{
            state.store.audit("harness.owner_settings_changed",json!({
                "settings":record,"source":"owner"
            }));
            state.bus.record("harness_settings",&saved);
            Json(saved).into_response()
        }
        None=>error(StatusCode::INTERNAL_SERVER_ERROR,
            "하네스 실행 설정을 저장하지 못했습니다."),
    }
}
