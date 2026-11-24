//! Product host persistence for external SDK applications. No code download,
//! arbitrary process launch, credentials or alternate execution engine.
use crate::state::{AppState,SessionGuard};
use axum::{extract::{Path,State},http::StatusCode,response::{IntoResponse,Response},Json};
use dominion_extensions::{Manifest,Registration,valid_id};
use serde::Deserialize;
use serde_json::{json,Value};
use super::response::error;
const KIND:&str="sdk_extension";

pub(crate) fn lock(state:&AppState,id:&str)->Result<SessionGuard,Response>{
    if !valid_id(id){return Err(error(StatusCode::BAD_REQUEST,"Invalid extension ID"));}
    let key=format!("sdk-extension:{id}");
    if !state.begin_session(&key){return Err(error(StatusCode::CONFLICT,"Extension state is changing"));}
    Ok(state.session_guard(&key))
}
fn load(state:&AppState,id:&str)->Result<Registration,Response>{
    let row=state.store.get(KIND,id).ok_or_else(||error(StatusCode::NOT_FOUND,"Extension not registered"))?;
    serde_json::from_value(row).map_err(|_|error(StatusCode::INTERNAL_SERVER_ERROR,"Invalid extension registration"))
}
/// Hold the same per-extension lock through SDK run persistence. A disable
/// blocks new admissions; already committed work remains owner-recoverable.
pub(crate) fn admit(state:&AppState,id:&str)->Result<(SessionGuard,Registration),Response>{
    let guard=lock(state,id)?;
    let row=load(state,id)?;
    row.admit(id).map_err(|message|error(StatusCode::CONFLICT,&message))?;
    Ok((guard,row))
}
pub async fn list(State(state):State<AppState>)->Json<Value>{
    Json(json!(state.store.all(KIND)))
}
pub async fn one(State(state):State<AppState>,Path(id):Path<String>)->Response{
    match load(&state,&id){Ok(row)=>Json(row).into_response(),Err(response)=>response}
}
pub async fn register(State(state):State<AppState>,Json(manifest):Json<Manifest>)->Response{
    let row=match Registration::new(manifest){Ok(row)=>row,Err(message)=>return error(StatusCode::BAD_REQUEST,&message)};
    let _guard=match lock(&state,&row.id){Ok(guard)=>guard,Err(response)=>return response};
    if state.store.get(KIND,&row.id).is_some(){return error(StatusCode::CONFLICT,"Extension already registered; disable and unregister before replacing it");}
    let Some(saved)=state.store.put_if_absent(KIND,&json!(row))else{return error(StatusCode::INTERNAL_SERVER_ERROR,"Extension registration failed")};
    state.bus.record(KIND,&saved);
    (StatusCode::CREATED,Json(saved)).into_response()
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetState{enabled:bool}
pub async fn set_state(State(state):State<AppState>,Path(id):Path<String>,Json(input):Json<SetState>)->Response{
    let _guard=match lock(&state,&id){Ok(guard)=>guard,Err(response)=>return response};
    let mut row=match load(&state,&id){Ok(row)=>row,Err(response)=>return response};
    row.enabled=input.enabled;
    if row.manifest.validate().is_err()||row.id!=id||row.manifest.id!=id{return error(StatusCode::CONFLICT,"Invalid extension registration")}
    let Some(saved)=state.store.put(KIND,&json!(row))else{return error(StatusCode::INTERNAL_SERVER_ERROR,"Extension state save failed")};
    state.bus.record(KIND,&saved);
    Json(saved).into_response()
}
pub async fn remove(State(state):State<AppState>,Path(id):Path<String>)->Response{
    let _guard=match lock(&state,&id){Ok(guard)=>guard,Err(response)=>return response};
    let row=match load(&state,&id){Ok(row)=>row,Err(response)=>return response};
    if row.enabled{return error(StatusCode::CONFLICT,"Disable extension before unregistering")}
    if !state.store.remove(KIND,&id){return error(StatusCode::INTERNAL_SERVER_ERROR,"Extension unregister failed")}
    state.bus.record(KIND,&json!({"id":id,"removed":true}));
    Json(json!({"id":id,"removed":true})).into_response()
}
