//! Reviewable Money research library: human decisions are distinct from model ranks.
use crate::state::AppState;
use axum::{extract::{Path,Query,State},http::StatusCode,response::{IntoResponse,Response},Json};
use serde_json::{json,Value};
use std::collections::HashMap;

fn fail(status:StatusCode,reason:&str)->Response{
    (status,Json(json!({"error":reason}))).into_response()
}
pub async fn archive(
    State(state):State<AppState>,Query(params):Query<HashMap<String,String>>
)->Response{
    let status=params.get("status").map(String::as_str).unwrap_or("");
    if !matches!(status,""|"hypothesis"|"deferred"|"rejected"|"researching"|
        "testing"|"validated"|"launched"|"implementation_requested"){
        return fail(StatusCode::BAD_REQUEST,"알 수 없는 후보 상태입니다.");
    }
    let q=params.get("q").map(|s|s.trim()).unwrap_or("");
    if q.chars().count()>120{return fail(StatusCode::BAD_REQUEST,"검색어는 최대 120자입니다.");}
    let limit=params.get("limit").and_then(|v|v.parse::<usize>().ok()).unwrap_or(24).clamp(1,50);
    let offset=params.get("offset").and_then(|v|v.parse::<usize>().ok()).unwrap_or(0).min(100_000);
    match state.store.money_candidate_archive(status,q,limit,offset){
        Ok(candidates)=>Json(json!({"candidates":candidates,"status":status,
            "query":q,"limit":limit,"offset":offset,"modelCalls":0,
            "basis":"archived_candidate_record_not_verified_sales"})).into_response(),
        Err(e)=>fail(StatusCode::INTERNAL_SERVER_ERROR,&e)
    }
}
pub async fn review(
    State(state):State<AppState>,Path(id):Path<String>,Json(body):Json<Value>
)->Response{
    let status=body["status"].as_str().unwrap_or("");
    let reason=body["reason"].as_str().unwrap_or("").trim();
    match state.store.money_review_candidate(&id,status,reason){
        Ok(saved)=>{
            state.bus.record("money_candidate",&saved);
            Json(saved).into_response()
        },
        Err(e) if e=="candidate_not_found"=>fail(StatusCode::NOT_FOUND,"후보를 찾을 수 없습니다."),
        Err(e) if e=="implementation_already_requested"=>
            fail(StatusCode::CONFLICT,"구현 승인이 진행된 후보는 보류·제외할 수 없습니다."),
        Err(e) if e=="unsupported_review_status"||e=="review_reason_too_long"=>
            fail(StatusCode::BAD_REQUEST,&e),
        Err(e)=>fail(StatusCode::INTERNAL_SERVER_ERROR,&e)
    }
}
pub async fn history(
    State(state):State<AppState>,Path(id):Path<String>
)->Response{
    let Some(candidate)=state.store.get("money_candidate",&id)else{
        return fail(StatusCode::NOT_FOUND,"후보가 없습니다.");
    };
    match state.store.money_review_history(&id,80){
        Ok(events)=>Json(json!({"candidate":candidate,"reviews":events,"modelCalls":0})).into_response(),
        Err(e)=>fail(StatusCode::INTERNAL_SERVER_ERROR,&e)
    }
}
pub async fn source(
    State(state):State<AppState>,Query(params):Query<HashMap<String,String>>
)->Response{
    let url=params.get("url").map(String::as_str).unwrap_or("");
    if url.len()>2000||!url.starts_with("https://"){
        return fail(StatusCode::BAD_REQUEST,"저장된 HTTPS 원문 URL을 지정하세요.");
    }
    let stored=match state.store.money_signal_by_url(url){
        Ok(Some(v))=>v,
        Ok(None)=>{
            let alias=match state.store.money_source_alias_by_url(url){
                Ok(Some(v))=>v,
                Ok(None)=>return fail(StatusCode::NOT_FOUND,"해당 URL의 원문은 아직 저장되지 않았습니다."),
                Err(e)=>return fail(StatusCode::INTERNAL_SERVER_ERROR,&e)
            };
            return Json(json!({"source":alias,"revisions":[],
                "sourceStatus":"similar_headline_archived_metadata_only",
                "modelCalls":0})).into_response()
        },
        Err(e)=>return fail(StatusCode::INTERNAL_SERVER_ERROR,&e)
    };
    let revisions=match state.store.money_source_revisions(
        stored["id"].as_str().unwrap_or(""),50){
        Ok(v)=>v,Err(e)=>return fail(StatusCode::INTERNAL_SERVER_ERROR,&e)
    };
    Json(json!({"source":stored,"revisions":revisions,
        "sourceStatus":"archived_observation_not_purchase_confirmation",
        "modelCalls":0})).into_response()
}
