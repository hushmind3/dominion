//! Owner-facing algorithm management bridge. The executable kernels and
//! per-kind activation are owned by engine::algorithms, not a second runner.
use crate::{state::AppState,engine};
use axum::{extract::{Path,State},http::StatusCode,response::{IntoResponse,Response},Json};
use serde_json::{json,Value};

fn invalid(message:String)->Response{
    (StatusCode::BAD_REQUEST,Json(json!({"error":message}))).into_response()
}

pub async fn list(State(state):State<AppState>)->Json<Value>{
    Json(engine::algorithms::list(&state))
}

pub async fn register(State(state):State<AppState>,Json(body):Json<Value>)->Response{
    if body["name"].as_str().is_some_and(|s|s.len()>128)
        || body["description"].as_str().is_some_and(|s|s.len()>4096)
        || body["parameters"].to_string().len()>16_384{
        return invalid("algorithm metadata exceeds limits".into());
    }
    // register rejects unknown kinds and implementations instead of accepting
    // a model-invented algorithm that has no executable Rust kernel.
    match engine::algorithms::register(&state,&body){
        Ok(saved)=>Json(saved).into_response(),
        Err(error)=>invalid(error)
    }
}

pub async fn activate(State(state):State<AppState>,Path(id):Path<String>,
    Json(body):Json<Value>)->Response{
    if state.store.get("algorithm",&id).is_none(){
        return (StatusCode::NOT_FOUND,
            Json(json!({"error":"algorithm not found"}))).into_response();
    }
    // Explicit force is an owner action. Without it a fresh candidate needs
    // evaluation evidence. Registry activation is transactional per kind.
    match engine::algorithms::activate_checked(
        &state,&id,body["force"].as_bool()==Some(true)
    ){
        Ok(saved)=>Json(saved).into_response(),
        Err(error)=>(StatusCode::CONFLICT,Json(json!({"error":error}))).into_response()
    }
}

#[cfg(test)]
mod tests{
    use super::*;
    use crate::{config::Config,db::Store,vault::Vault};
    fn fixture()->AppState{
        let data=std::env::temp_dir().join(format!(
            "dominion-algorithm-api-{}",uuid::Uuid::new_v4()));
        let config=Config{port:0,data_dir:data.clone(),web_dir:data.join("web"),
            workspace:data.clone()};
        AppState::new(config.clone(),Store::open(&config.db_path()).unwrap(),
            Vault::open(&config.key_path()))
    }
    #[tokio::test]
    async fn selected_algorithm_reuses_existing_kernel_and_persists_global_stats(){
        let state=fixture();
        let requested=json!({"kind":"tool.combination","name":"parallel-reads",
            "implementation":"parallel-reads","parameters":{}});
        let response=register(State(state.clone()),Json(requested)).await;
        assert_eq!(response.status(),StatusCode::OK);
        let raw=axum::body::to_bytes(response.into_body(),8192).await.unwrap();
        let created:Value=serde_json::from_slice(&raw).unwrap();
        let id=created["id"].as_str().unwrap().to_string();
        let without_force=activate(State(state.clone()),Path(id.clone()),
            Json(json!({}))).await;
        assert_eq!(without_force.status(),StatusCode::CONFLICT);
        let activated=activate(State(state.clone()),Path(id.clone()),
            Json(json!({"force":true}))).await;
        assert_eq!(activated.status(),StatusCode::OK);
        assert_eq!(engine::algorithms::variant(&state,"tool.combination"),"parallel-reads");
        engine::algorithms::record_domain_outcome(&state,None,
            "tool.combination","parallel-reads",true);
        let stats=crate::api::agents::synergy(State(state.clone())).await.0;
        assert!(stats["algorithmStats"].as_array().unwrap().iter().any(|s|
            s["kind"]=="tool.combination"&&s["uses"]==1&&s["successes"]==1));
        let invalid=register(State(state),Json(json!({
            "kind":"tool.combination","name":"arbitrary-exec",
            "implementation":"arbitrary-exec"}))).await;
        assert_eq!(invalid.status(),StatusCode::BAD_REQUEST);
    }
}
