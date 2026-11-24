//! Owner extension management, independent of MCP and model providers.
use crate::{extension_manager as manager,state::AppState};
use dominion_agent_sdk::plugin_protocol::Manifest;
use axum::{extract::{Path,Query,State},http::StatusCode,response::{IntoResponse,Response},Json};
use serde::Deserialize;
use serde_json::{json,Value};
use super::response::error;
#[derive(Deserialize)] pub struct Search{#[serde(default)] q:String}
#[derive(Deserialize)] pub struct Sources{urls:Vec<String>}
#[derive(Deserialize)] pub struct Install{source:String,manifest:Manifest,confirmed:bool}
#[derive(Deserialize)] pub struct Enable{enabled:bool,#[serde(default)] confirmed:bool}
fn result(value:Result<Value,String>)->Response{match value{Ok(v)=>Json(v).into_response(),Err(e)=>error(StatusCode::BAD_REQUEST,&e)}}
pub async fn sources(State(state):State<AppState>)->Json<Value>{Json(json!({"urls":manager::sources(&state)}))}
pub async fn save_sources(State(state):State<AppState>,Json(input):Json<Sources>)->Response{result(manager::save_sources(&state,input.urls).await)}
pub async fn catalog(State(state):State<AppState>,Query(input):Query<Search>)->Response{
    if input.q.chars().count()>256{return error(StatusCode::BAD_REQUEST,"search query too long");}
    Json(manager::search(&state,&input.q).await).into_response()
}
pub async fn list(State(state):State<AppState>)->Json<Value>{Json(json!(manager::list(&state)))}
pub async fn install(State(state):State<AppState>,Json(input):Json<Install>)->Response{
    if !input.confirmed{return error(StatusCode::BAD_REQUEST,"explicit owner confirmation required");}
    result(manager::install(&state,&input.source,input.manifest).await)
}
pub async fn enable(State(state):State<AppState>,Path(id):Path<String>,Json(input):Json<Enable>)->Response{
    if input.enabled&&!input.confirmed{return error(StatusCode::BAD_REQUEST,"explicit code execution consent required");}
    result(manager::enable(&state,&id,input.enabled).await)
}
pub async fn remove(State(state):State<AppState>,Path(id):Path<String>)->Response{result(manager::remove(&state,&id).await)}
