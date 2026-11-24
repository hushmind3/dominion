//! One-shot unattended upgrades of an owner-pinned OCI digest, only while
//! stopped. No automatic restart or selection of mutable upstream tags.
use super::{owner,inspect,valid_id,valid_image,versions,recipe,package};
use crate::{state::AppState,api::response::error};
use axum::{extract::{Path,State},http::StatusCode,response::{IntoResponse,Response},Json};
use serde::Deserialize;
use serde_json::{json,Value};
use std::time::Duration;

const KIND:&str="plugin_scheduled_update";
#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Request{image:String,expected_image_id:String,confirmed:bool}
fn pinned(image:&str)->bool{valid_image(image)&&image.contains("@sha256:")&&!recipe::voice_tag(image)&&!package::local_tag(image)}
fn generic(row:&Value)->bool{row["image"].as_str().is_some_and(|s|!recipe::voice_tag(s)&&!package::local_tag(s))}
fn same_target(record:&Value,row:&Value)->bool{
    row["imageId"].is_string()&&row["imageId"]==record["expectedImageId"]
        &&row["containerId"]==record["expectedContainerId"]
}
fn save(state:&AppState,r:&Value)->Result<(),String>{state.store.put(KIND,r).map(|_|()).ok_or("예약 업데이트 기록 저장 실패".into())}
pub(crate) fn attach(state:&AppState,row:&mut Value){
    let Some(id)=row["id"].as_str()else{return};
    if let Some(r)=state.store.get(KIND,id){
        row["scheduledUpdate"]=json!({"status":r["status"],"image":r["image"],"message":r["message"]});
    }
}
pub async fn queue(State(state):State<AppState>,Path(id):Path<String>,Json(input):Json<Request>)->Response{
    if !input.confirmed||!valid_id(&id)||!pinned(&input.image)||!versions::immutable(&input.expected_image_id){
        return error(StatusCode::BAD_REQUEST,"자동 업데이트는 승인된 sha256 이미지 주소만 지원합니다.")
    }
    let _guard=match state.integrations.plugin_mutations.try_lock(){Ok(g)=>g,Err(_)=>return error(StatusCode::CONFLICT,"플러그인 변경 진행 중")};
    let result:Result<Value,String>=async{
        let scope=owner(&state)?;
        if versions::pending(&state,&id){return Err("진행 중인 변경을 먼저 복구하세요.".into())}
        let row=inspect(&id,&scope).await?.ok_or("설치된 플러그인이 없습니다.")?;
        if !generic(&row)||versions::image_id(&row)!=Some(input.expected_image_id.as_str()){return Err("현재 일반 Docker 이미지 버전이 변경되었습니다.".into())}
        if state.store.get(KIND,&id).is_some_and(|r|r["status"]=="applying"){return Err("예약 작업이 이미 실행 중입니다.".into())}
        let record=json!({"id":id,"status":"queued","image":input.image,"expectedImageId":row["imageId"],"expectedContainerId":row["containerId"],"message":"중지 상태에서 적용 대기"});
        save(&state,&record)?;Ok(record)
    }.await;
    match result{Ok(v)=>Json(v).into_response(),Err(e)=>error(StatusCode::CONFLICT,&e)}
}
pub async fn cancel(State(state):State<AppState>,Path(id):Path<String>)->Response{
    if !valid_id(&id){return error(StatusCode::BAD_REQUEST,"Invalid plugin identity")}
    let _guard=match state.integrations.plugin_mutations.try_lock(){Ok(g)=>g,Err(_)=>return error(StatusCode::CONFLICT,"플러그인 변경 진행 중")};
    let Some(mut r)=state.store.get(KIND,&id).filter(|r|r["status"]=="queued")else{return error(StatusCode::CONFLICT,"대기 중인 예약이 없습니다.")};
    r["status"]=json!("cancelled");r["message"]=json!("예약 취소");
    match save(&state,&r){Ok(())=>Json(r).into_response(),Err(e)=>error(StatusCode::INTERNAL_SERVER_ERROR,&e)}
}
async fn apply(state:&AppState,r:&mut Value)->Result<(),String>{
    let id=r["id"].as_str().filter(|id|valid_id(id)).ok_or("Invalid scheduled identity")?.to_owned();
    let image=r["image"].as_str().filter(|image|pinned(image)).ok_or("Invalid scheduled image")?.to_owned();
    let scope=owner(state)?;
    if versions::pending(state,&id){return Err("이전 변경 복구가 필요합니다.".into())}
    let row=inspect(&id,&scope).await?.ok_or("예약한 플러그인이 없습니다.")?;
    if !generic(&row)||!same_target(r,&row){return Err("예약 당시의 실행 이미지가 변경되었습니다.".into())}
    if row["running"]==true{return Ok(())}
    if row["running"]!=false{return Err("중지 상태를 확인하지 못했습니다.".into())}
    let next=versions::spec(&row,&image).ok_or("Invalid installed settings")?;
    // Persist before any expensive pull/copy. A restart never repeats an
    // unresolved application attempt; the owner can inspect and reschedule.
    r["status"]=json!("applying");r["message"]=json!("승인된 이미지 적용 중");
    save(state,r)?;
    let target=versions::pull(&image).await?;
    if versions::image_id(&row)!=Some(target.as_str()){
        versions::replace_stopped(state,&scope,row,&next,&target).await?;
    }
    r["status"]=json!("applied");r["message"]=json!("중지 상태에서 적용 완료. 직접 실행하세요.");
    save(state,r)?;
    state.store.audit("plugin.scheduled_update.applied",json!({"pluginId":id,"image":image}));
    Ok(())
}
pub(crate) fn start(state:AppState){
    tokio::spawn(async move{
        let mut timer=tokio::time::interval(Duration::from_secs(15));
        timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop{
            timer.tick().await;
            if state.maintenance_mode(){continue}
            let ids=state.store.all(KIND).into_iter().filter(|r|matches!(r["status"].as_str(),Some("queued"|"applying"))).filter_map(|r|r["id"].as_str().map(str::to_owned)).take(32).collect::<Vec<_>>();
            for id in ids{
                let Ok(_guard)=state.integrations.plugin_mutations.try_lock()else{continue};
                let Some(mut r)=state.store.get(KIND,&id)else{continue};
                if r["status"]=="applying"{
                    r["status"]=json!("needs_review");r["message"]=json!("이전 적용 결과를 확인하세요. 자동 재시도하지 않습니다.");
                    let _=save(&state,&r);continue
                }
                if r["status"]!="queued"{continue}
                if let Err(e)=apply(&state,&mut r).await{
                    r["status"]=json!("needs_review");r["message"]=json!(e);let _=save(&state,&r);
                    state.store.audit("plugin.scheduled_update.review",json!({"pluginId":id}));
                }
            }
        }
    });
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn background_updates_only_apply_exact_owner_reviewed_digest(){
        assert!(!pinned("alpine:3.22"));assert!(!pinned("alpine:latest"));
        assert!(pinned(&format!("alpine@sha256:{}","a".repeat(64))));
        let grant=json!({"expectedImageId":"sha256:a","expectedContainerId":"old"});
        assert!(same_target(&grant,&json!({"imageId":"sha256:a","containerId":"old"})));
        assert!(!same_target(&grant,&json!({"imageId":"sha256:a","containerId":"new"})));
    }
}
