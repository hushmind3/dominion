//! Product host persistence for external SDK applications. No code download,
//! arbitrary process launch, credentials or alternate execution engine.
use crate::state::{AppState,SessionGuard};
use axum::{extract::{Path,State},http::{HeaderMap,StatusCode},response::{IntoResponse,Response},Json};
use dominion_extensions::{Manifest,Registration,valid_id};
use serde::Deserialize;
use serde_json::{json,Value};
use super::response::error;
const KIND:&str="sdk_extension";
const GRANT_KIND:&str="sdk_extension_grant";
pub(crate) const TOKEN_HEADER:&str="x-dominion-extension-token";

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
    // Revoke credentials before removing the registration; a failed removal stays disabled.
    if state.store.get(GRANT_KIND,&id).is_some()&&!state.store.remove(GRANT_KIND,&id){return error(StatusCode::INTERNAL_SERVER_ERROR,"Extension credential revocation failed")}
    if !state.store.remove(KIND,&id){return error(StatusCode::INTERNAL_SERVER_ERROR,"Extension unregister failed")}
    state.bus.record(KIND,&json!({"id":id,"removed":true}));
    Json(json!({"id":id,"removed":true})).into_response()
}


// Scoped applications never receive an owner token. The only delegated
// capability is an isolated text turn on an owner-pinned model, with an
// explicit per-turn budget and finite submission allowance.
use sha2::{Digest,Sha256};
use rand::{rngs::OsRng,RngCore};

#[derive(Clone,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct GrantInput{
    provider_id:String,
    model:String,
    token_budget:u64,
    max_runs:u64,
}
fn public_grant(row:&Value)->Value{
    json!({"id":row["id"],"providerId":row["providerId"],"model":row["model"],
        "tokenBudget":row["tokenBudget"],"remainingRuns":row["remainingRuns"],
        "capability":"isolated_chat","generation":row["generation"]})
}
pub async fn grant_status(State(state):State<AppState>,Path(id):Path<String>)->Response{
    if let Err(response)=load(&state,&id){return response}
    Json(state.store.get(GRANT_KIND,&id).map(|r|public_grant(&r)).unwrap_or(Value::Null)).into_response()
}
/// Issue or rotate once, with explicit owner limits. Only the hash persists.
/// Rotation preserves registration identity; unregister destroys it.
pub async fn issue_grant(State(state):State<AppState>,Path(id):Path<String>,Json(input):Json<GrantInput>)->Response{
    let _guard=match lock(&state,&id){Ok(g)=>g,Err(r)=>return r};
    if let Err(r)=load(&state,&id){return r}
    if input.provider_id.is_empty()||input.model.trim().is_empty()||
        !(1024..=32768).contains(&input.token_budget)||!(1..=100).contains(&input.max_runs){
        return error(StatusCode::BAD_REQUEST,"Choose a provider/model, token budget 1024–32768 and 1–100 requests")
    }
    let route=match crate::engine::gateway::resolve(&state,&input.provider_id,Some(&input.model)){
        Ok(r)=>r,Err(e)=>return error(StatusCode::BAD_REQUEST,&e)
    };
    // CLI adapters may act outside the native tool gateway. Do not delegate them.
    if route.kind=="cli" || route.kind=="codex"{
        return error(StatusCode::BAD_REQUEST,"Scoped text apps require a native text API provider")
    }
    let mut random=[0u8;32];
    OsRng.fill_bytes(&mut random);
    let token=random.iter().map(|b|format!("{b:02x}")).collect::<String>();
    let previous=state.store.get(GRANT_KIND,&id);
    let generation=previous.as_ref().and_then(|v|v["generation"].as_str()).map(str::to_owned)
        .unwrap_or_else(||uuid::Uuid::new_v4().to_string());
    let row=json!({"id":id,"generation":generation,"providerId":route.id,"model":route.model,
        "tokenBudget":input.token_budget,"remainingRuns":input.max_runs,
        "tokenSha256":format!("{:x}",Sha256::digest(token.as_bytes()))});
    let Some(saved)=state.store.put(GRANT_KIND,&row)else{return error(StatusCode::INTERNAL_SERVER_ERROR,"Credential save failed")};
    state.bus.record(KIND,&json!({"id":id,"grantChanged":true}));
    Json(json!({"grant":public_grant(&saved),"token":token})).into_response()
}
pub async fn revoke_grant(State(state):State<AppState>,Path(id):Path<String>)->Response{
    let _guard=match lock(&state,&id){Ok(g)=>g,Err(r)=>return r};
    if let Err(r)=load(&state,&id){return r}
    if state.store.get(GRANT_KIND,&id).is_some()&&!state.store.remove(GRANT_KIND,&id){
        return error(StatusCode::INTERNAL_SERVER_ERROR,"Credential revocation failed")
    }
    state.bus.record(KIND,&json!({"id":id,"grantChanged":true}));
    Json(json!({"revoked":true})).into_response()
}
fn authenticated(state:&AppState,headers:&HeaderMap,id:&str)->Result<(SessionGuard,Registration,Value),Response>{
    let _guard=lock(state,id)?;
    let row=load(state,id).map_err(|_|error(StatusCode::UNAUTHORIZED,"Invalid extension credential"))?;
    let grant=state.store.get(GRANT_KIND,id).ok_or_else(||error(StatusCode::UNAUTHORIZED,"Invalid extension credential"))?;
    let token=headers.get(TOKEN_HEADER).and_then(|v|v.to_str().ok()).unwrap_or("");
    let actual=format!("{:x}",Sha256::digest(token.as_bytes()));
    let expected=grant["tokenSha256"].as_str().unwrap_or("");
    if token.len()!=64||expected.len()!=64||actual.bytes().zip(expected.bytes()).fold(0u8,|d,(a,b)|d|(a^b))!=0{
        return Err(error(StatusCode::UNAUTHORIZED,"Invalid extension credential"))
    }
    row.admit(id).map_err(|e|error(StatusCode::FORBIDDEN,&e))?;
    Ok((_guard,row,grant))
}
#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct ScopedRequest{instruction:String,idempotency_key:String}
pub async fn scoped_start(State(state):State<AppState>,Path(id):Path<String>,headers:HeaderMap,Json(input):Json<ScopedRequest>)->Response{
    let (_guard,row,grant)=match authenticated(&state,&headers,&id){Ok(v)=>v,Err(r)=>return r};
    if input.instruction.trim().is_empty()||input.instruction.len()>16000||
        input.idempotency_key.is_empty()||input.idempotency_key.len()>128||
        !input.idempotency_key.bytes().all(|b|b.is_ascii_alphanumeric()||matches!(b,b'-'|b'_'|b'.'|b':')){
        return error(StatusCode::BAD_REQUEST,"Invalid instruction or retry key")
    }
    super::sdk::start_scoped(state,row,grant,input.instruction,input.idempotency_key)
}
/// Owner-only history, separate from the delegated credential ingress.
pub async fn history(State(state):State<AppState>,Path(id):Path<String>)->Response{
    if !valid_id(&id){return error(StatusCode::BAD_REQUEST,"Invalid extension ID")}
    match state.store.sdk_runs_for_source_id(dominion_extensions::SDK_SOURCE_KIND,&id,40){
        Ok(rows)=>Json(json!(rows.into_iter().map(|r|json!({
            "id":r["id"],"status":r["status"],"model":r["model"],"createdAt":r["createdAt"],
            "instruction":r["instruction"],"taskOutcome":r["taskOutcome"],
            "verified":r["verified"],"scoped":r["extension"]["capability"]=="isolated_chat"
        })).collect::<Vec<_>>())).into_response(),
        Err(e)=>{state.store.audit("sdk.extension_history_failed",json!({"error":e}));
            error(StatusCode::INTERNAL_SERVER_ERROR,"Extension history unavailable")}
    }
}
pub async fn scoped_one(State(state):State<AppState>,Path((id,run)):Path<(String,String)>,headers:HeaderMap)->Response{
    let (_guard,_,grant)=match authenticated(&state,&headers,&id){Ok(v)=>v,Err(r)=>return r};
    let Some(record)=state.store.get("sdk_run",&run).filter(|r|
        r["extension"]["id"]==id && r["extension"]["credentialGeneration"]==grant["generation"] &&
        r["extension"]["capability"]=="isolated_chat")else{
        return error(StatusCode::NOT_FOUND,"Run not found")
    };
    Json(super::sdk::scoped_view(&state,record)).into_response()
}
