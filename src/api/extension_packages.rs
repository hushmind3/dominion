//! Owner extension management, independent of MCP and model providers.
use crate::{extension_manager as manager,state::AppState};
use dominion_agent_sdk::plugin_protocol::Manifest;
use axum::{extract::{Path,Query,State},http::StatusCode,response::{IntoResponse,Response},Json};
use serde::Deserialize;
use serde_json::{json,Value};
use base64::Engine as _;
use super::response::error;
#[derive(Deserialize)] pub struct Search{#[serde(default)] q:String}
#[derive(Deserialize)] pub struct Sources{urls:Vec<String>,#[serde(rename="githubDiscovery")] github_discovery:Option<bool>}
#[derive(Deserialize)] pub struct Install{source:String,manifest:Manifest,confirmed:bool}
#[derive(Deserialize)] #[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct LocalBundle{bundle_version:u32,manifest:Manifest,artifact_base64:String,confirmed:bool}
#[derive(Deserialize)] pub struct Enable{enabled:bool,#[serde(default)] confirmed:bool}
fn result(value:Result<Value,String>)->Response{match value{Ok(v)=>Json(v).into_response(),Err(e)=>error(StatusCode::BAD_REQUEST,&e)}}
pub async fn sources(State(state):State<AppState>)->Json<Value>{Json(manager::settings(&state))}
pub async fn save_sources(State(state):State<AppState>,Json(input):Json<Sources>)->Response{result(manager::save_sources(&state,input.urls,input.github_discovery).await)}
pub async fn catalog(State(state):State<AppState>,Query(input):Query<Search>)->Response{
    if input.q.chars().count()>256{return error(StatusCode::BAD_REQUEST,"search query too long");}
    Json(manager::search(&state,&input.q).await).into_response()
}
pub async fn list(State(state):State<AppState>)->Json<Value>{Json(json!(manager::list(&state)))}
pub async fn install(State(state):State<AppState>,Json(input):Json<Install>)->Response{
    if !input.confirmed{return error(StatusCode::BAD_REQUEST,"explicit owner confirmation required");}
    result(manager::install(&state,&input.source,input.manifest).await)
}
pub async fn import_local(State(state):State<AppState>,Json(input):Json<LocalBundle>)->Response{
    if input.bundle_version!=1||!input.confirmed{return error(StatusCode::BAD_REQUEST,"unsupported package format or missing owner confirmation");}
    const MAX:usize=8*1024*1024;
    if input.artifact_base64.len()>4*((MAX+2)/3){return error(StatusCode::PAYLOAD_TOO_LARGE,"local package exceeds 8 MiB artifact limit");}
    let bytes=match base64::engine::general_purpose::STANDARD.decode(&input.artifact_base64){
        Ok(bytes) if bytes.len()<=MAX=>bytes,
        _=>return error(StatusCode::BAD_REQUEST,"invalid local package artifact"),
    };
    result(manager::install_local(&state,input.manifest,&bytes).await)
}
pub async fn enable(State(state):State<AppState>,Path(id):Path<String>,Json(input):Json<Enable>)->Response{
    if input.enabled&&!input.confirmed{return error(StatusCode::BAD_REQUEST,"explicit code execution consent required");}
    result(manager::enable(&state,&id,input.enabled).await)
}
pub async fn remove(State(state):State<AppState>,Path(id):Path<String>)->Response{result(manager::remove(&state,&id).await)}

pub async fn diagnose(State(state):State<AppState>,Path(id):Path<String>)->Response{result(manager::diagnose(&state,&id).await)}

pub async fn versions(State(state):State<AppState>,Path(id):Path<String>)->Response{result(manager::versions(&state,&id).await)}
