//! External apps propose work; only the owner may admit tools or release a result.
use crate::state::AppState;
use axum::{extract::{Path,State},http::{HeaderMap,StatusCode},response::{IntoResponse,Response},Json};
use serde::Deserialize;
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use super::{extension_registry as registry,response::error,sdk};

pub(super) const KIND:&str="sdk_extension_request";
#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Submit{instruction:String,idempotency_key:String}
#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Review{approve:bool,workspace_dir:Option<String>,max_rounds:Option<usize>}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Publish{message:String}

fn receipt_id(app:&str,generation:&Value,key:&str)->String{
    format!("{:x}",Sha256::digest(json!([app,generation,key]).to_string().as_bytes()))
}
fn load(state:&AppState,app:&str,id:&str)->Result<Value,Response>{
    state.store.get(KIND,id).filter(|r|r["appId"]==app)
        .ok_or_else(||error(StatusCode::NOT_FOUND,"Task request not found"))
}
fn notify(state:&AppState,app:&str){state.bus.record("sdk_extension",&json!({"id":app,"requestChanged":true}));}
fn current_authority(row:&Value,grant:&Value)->bool{
    grant["allowTaskRequests"]==true && grant["authorizationId"].is_string()
        && row["authorizationId"]==grant["authorizationId"]
        && row["credentialGeneration"]==grant["generation"]
}
fn linked_run(state:&AppState,row:&Value)->Option<Value>{
    let run=state.store.get("sdk_run",row["runId"].as_str()?)?;
    (run["extension"]["requestId"]==row["id"]&&run["extension"]["id"]==row["appId"]
        &&run["extension"]["capability"]=="owner_approved_task").then_some(run)
}
fn external_view(state:&AppState,row:&Value)->Value{
    let run=linked_run(state,row);
    // Deliberately exclude run id, owner workspace, execution errors, tool
    // receipts and the answer. Only a manually published message leaves owner control.
    json!({"id":row["id"],"status":row["status"],"runStatus":run.as_ref().map(|r|r["status"].clone()),
        "taskOutcome":run.as_ref().map(|r|r["taskOutcome"].clone()),
        "verified":run.as_ref().map(|r|r["verified"]==true),
        "message":row["publishedMessage"]})
}
fn owner_view(state:&AppState,mut row:Value)->Value{
    let run=linked_run(state,&row);
    let grant=state.store.get(registry::GRANT_KIND,row["appId"].as_str().unwrap_or(""));
    row["canApprove"]=json!(row["status"]=="pending"&&grant.as_ref().is_some_and(|g|current_authority(&row,g))
        &&registry::load(state,row["appId"].as_str().unwrap_or("")).is_ok_and(|r|r.enabled));
    row["runStatus"]=run.as_ref().map(|r|r["status"].clone()).unwrap_or(Value::Null);
    row["canPublish"]=json!(row["publishedMessage"].is_null()&&grant.as_ref().is_some_and(|g|g["generation"]==row["credentialGeneration"])&&run.as_ref().is_some_and(|r|
        matches!(r["status"].as_str(),Some("completed"|"failed"|"cancelled"))));
    if let Some(obj)=row.as_object_mut(){obj.remove("authorizationId");obj.remove("credentialGeneration");}
    row
}
pub async fn submit(State(state):State<AppState>,Path(app):Path<String>,headers:HeaderMap,Json(input):Json<Submit>)->Response{
    let (_guard,_,mut grant)=match registry::authenticated(&state,&headers,&app){Ok(r)=>r,Err(r)=>return r};
    if input.instruction.trim().is_empty()||input.instruction.len()>16000||input.idempotency_key.is_empty()
        ||input.idempotency_key.len()>128||!input.idempotency_key.bytes().all(|b|b.is_ascii_alphanumeric()||matches!(b,b'-'|b'_'|b'.'|b':')){
        return error(StatusCode::BAD_REQUEST,"Invalid instruction or retry key")
    }
    let id=receipt_id(&app,&grant["generation"],&input.idempotency_key);
    if let Some(old)=state.store.get(KIND,&id){
        if old["appId"]!=app||old["credentialGeneration"]!=grant["generation"]||old["instruction"]!=input.instruction{
            return error(StatusCode::CONFLICT,"Retry key belongs to another request")
        }
        return Json(external_view(&state,&old)).into_response()
    }
    if grant["allowTaskRequests"]!=true||!grant["authorizationId"].is_string(){
        return error(StatusCode::FORBIDDEN,"Owner has not enabled task requests")
    }
    let remaining=grant["remainingRuns"].as_u64().unwrap_or(0);
    if remaining==0{return error(StatusCode::TOO_MANY_REQUESTS,"Extension request allowance exhausted")}
    let row=json!({"id":id,"appId":app,"credentialGeneration":grant["generation"],"authorizationId":grant["authorizationId"],
        "instruction":input.instruction,"providerId":grant["providerId"],"model":grant["model"],"tokenBudget":grant["tokenBudget"],"status":"pending"});
    grant["remainingRuns"]=json!(remaining-1);
    if state.store.transaction(&[(KIND.into(),row.clone()),(registry::GRANT_KIND.into(),grant)]).is_err(){
        return error(StatusCode::INTERNAL_SERVER_ERROR,"Cannot persist task request")
    }
    notify(&state,&app);
    (StatusCode::ACCEPTED,Json(external_view(&state,&row))).into_response()
}
pub async fn read(State(state):State<AppState>,Path((app,id)):Path<(String,String)>,headers:HeaderMap)->Response{
    let (_guard,_,grant)=match registry::authenticated(&state,&headers,&app){Ok(r)=>r,Err(r)=>return r};
    match load(&state,&app,&id){
        Ok(row) if row["credentialGeneration"]==grant["generation"]=>Json(external_view(&state,&row)).into_response(),
        _=>error(StatusCode::NOT_FOUND,"Task request not found")
    }
}
pub async fn list(State(state):State<AppState>,Path(app):Path<String>)->Response{
    if !dominion_extensions::valid_id(&app){return error(StatusCode::BAD_REQUEST,"Invalid extension ID")}
    match state.store.extension_requests(&app,100){
        Ok(rows)=>Json(rows.into_iter().map(|r|owner_view(&state,r)).collect::<Vec<_>>()).into_response(),
        Err(_)=>error(StatusCode::INTERNAL_SERVER_ERROR,"Cannot read task requests")
    }
}
pub async fn review(State(state):State<AppState>,Path((app,id)):Path<(String,String)>,Json(input):Json<Review>)->Response{
    let _guard=match registry::lock(&state,&app){Ok(g)=>g,Err(r)=>return r};
    let mut row=match load(&state,&app,&id){Ok(r)=>r,Err(r)=>return r};
    if row["status"]!="pending"{
        if (input.approve&&row["status"]=="approved")||(!input.approve&&row["status"]=="rejected"){
            return Json(owner_view(&state,row)).into_response()
        }
        return error(StatusCode::CONFLICT,"Request has already been reviewed")
    }
    if input.approve{
        let registration=match registry::load(&state,&app){Ok(r) if r.enabled=>r,_=>return error(StatusCode::CONFLICT,"App is disabled")};
        let Some(grant)=state.store.get(registry::GRANT_KIND,&app).filter(|g|current_authority(&row,g))else{
            return error(StatusCode::CONFLICT,"Task request permission was changed; submit a new request")
        };
        let Some(workspace)=input.workspace_dir.as_deref().filter(|p|std::path::Path::new(p).is_absolute())else{
            return error(StatusCode::BAD_REQUEST,"Select an absolute owner workspace")
        };
        let rounds=input.max_rounds.unwrap_or(8);
        if !(1..=24).contains(&rounds){return error(StatusCode::BAD_REQUEST,"Choose 1–24 model rounds")}
        // Grant comparison is checked again at review, not merely at submission.
        if row["providerId"]!=grant["providerId"]||row["model"]!=grant["model"]||row["tokenBudget"]!=grant["tokenBudget"]{
            return error(StatusCode::CONFLICT,"Task request model or budget changed")
        }
        row=match sdk::approve_extension_request(&state,&row,&registration,workspace,rounds){
            Ok(r)=>r,Err(e)=>return error(StatusCode::CONFLICT,&e)
        };
    }else{
        row=match state.store.patch_if(KIND,&id,"status",&json!("pending"),&json!({
            "status":"rejected","reviewedAt":chrono::Utc::now().to_rfc3339()})){
            Some(r)=>r,None=>return error(StatusCode::CONFLICT,"Request has already been reviewed")
        };
    }
    notify(&state,&app);
    Json(owner_view(&state,row)).into_response()
}
pub async fn publish(State(state):State<AppState>,Path((app,id)):Path<(String,String)>,Json(input):Json<Publish>)->Response{
    let _guard=match registry::lock(&state,&app){Ok(g)=>g,Err(r)=>return r};
    let row=match load(&state,&app,&id){Ok(r)=>r,Err(r)=>return r};
    if input.message.trim().is_empty()||input.message.len()>16000{return error(StatusCode::BAD_REQUEST,"Enter a message of at most 16000 bytes")}
    if !row["publishedMessage"].is_null(){
        if row["publishedMessage"]==input.message{return Json(owner_view(&state,row)).into_response()}
        return error(StatusCode::CONFLICT,"A message has already been released")
    }
    if !state.store.get(registry::GRANT_KIND,&app).is_some_and(|g|g["generation"]==row["credentialGeneration"]){
        return error(StatusCode::CONFLICT,"Original app credential was revoked; cannot deliver this message")
    }
    if !linked_run(&state,&row).is_some_and(|r|matches!(r["status"].as_str(),Some("completed"|"failed"|"cancelled"))){
        return error(StatusCode::CONFLICT,"Wait until the native task finishes")
    }
    let saved=match state.store.patch_if(KIND,&id,"publishedMessage",&Value::Null,&json!({
        "publishedMessage":input.message,"publishedAt":chrono::Utc::now().to_rfc3339()})){
        Some(r)=>r,None=>return error(StatusCode::CONFLICT,"Message already released")
    };
    notify(&state,&app);
    Json(owner_view(&state,saved)).into_response()
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]fn rotation_invalidates_pending_authorization(){
        let row=json!({"authorizationId":"a","credentialGeneration":"g"});
        assert!(current_authority(&row,&json!({"allowTaskRequests":true,"authorizationId":"a","generation":"g"})));
        assert!(!current_authority(&row,&json!({"allowTaskRequests":true,"authorizationId":"b","generation":"g"})));
        assert!(!current_authority(&row,&json!({"allowTaskRequests":false,"authorizationId":"a","generation":"g"})));
        assert_ne!(receipt_id("app.a",&json!("g"),"key"),receipt_id("app.b",&json!("g"),"key"));
    }
}
