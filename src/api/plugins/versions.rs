//! Stopped-container replacement with a durable rollback descriptor.
//! Rollback restores the immutable executable image, not mutable /data.
use super::{call,create_managed,inspect,owner,ensure_volume,container_id,valid_image,valid_id,Install};
use crate::state::AppState;
use crate::api::response::error;
use axum::{extract::{Path,State},http::StatusCode,response::{IntoResponse,Response},Json};
use serde::Deserialize;
use serde_json::{json,Value};

const KIND:&str="plugin_version_change";
#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Update{image:String,expected_image_id:String,confirmed:bool}
#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Rollback{confirmed:bool}
fn failed(s:&str)->Response{error(StatusCode::BAD_GATEWAY,s)}
fn conflict(s:&str)->Response{error(StatusCode::CONFLICT,s)}
fn immutable(s:&str)->bool{s.strip_prefix("sha256:").is_some_and(container_id)}
fn spec(row:&Value,image:&str)->Option<Install>{
    let id=row["id"].as_str()?;
    let name=row["name"].as_str()?;
    if !valid_id(id)||!valid_image(image)||name.is_empty()||name.len()>120||name.chars().any(char::is_control){return None}
    let network=row["network"].as_bool()?;
    let port=row["requestedPort"].as_u64().and_then(|n|u16::try_from(n).ok()).filter(|n|*n>0);
    if port.is_some()&&!network{return None}
    Some(Install{id:id.into(),name:name.into(),image:image.into(),network,port,confirmed:true})
}
fn image_id(row:&Value)->Option<&str>{row["imageId"].as_str().filter(|s|immutable(s))}
pub(crate) fn pending(state:&AppState,id:&str)->bool{
    state.store.get(KIND,id).is_some_and(|r|r["phase"]=="prepared")
}
pub(crate) fn attach(state:&AppState,row:&mut Value){
    let Some(id)=row["id"].as_str()else{return};
    let Some(r)=state.store.get(KIND,id)else{return};
    if matches!(r["phase"].as_str(),Some("prepared"|"ready")){
        row["versionChange"]=json!({"phase":r["phase"],"previousImage":r["old"]["image"],
            "previousImageId":r["old"]["imageId"],"targetImage":r["targetImage"]});
    }
}
async fn pull(image:&str)->Result<String,String>{
    call(&["image","pull","--quiet",image],300).await?;
    let output=call(&["image","inspect","--format","{{.Id}}",image],15).await?;
    let id=output.trim();
    if !immutable(id){return Err("Invalid immutable image identity".into())}
    Ok(id.into())
}
async fn create(state:&AppState,scope:&str,input:&Install,exact:&str)->Result<Value,String>{
    // Do not fall back to mutable tags or fetch a different image on rollback.
    let output=call(&["image","inspect","--format","{{.Id}}",exact],15).await?;
    if output.trim()!=exact{return Err("Original image is not available locally".into())}
    ensure_volume(scope,input).await?;
    create_managed(state,scope,input,exact).await?;
    let row=inspect(&input.id,scope).await?.ok_or("Replacement container not found")?;
    if image_id(&row)!=Some(exact)||row["running"]!=false{return Err("Replacement image/status verification failed".into())}
    Ok(row)
}
fn save(state:&AppState,r:&Value)->Result<(),String>{
    state.store.put(KIND,r).map(|_|()).ok_or("버전 복구 정보를 저장할 수 없습니다.".into())
}
pub(super) async fn replace_stopped(state:&AppState,scope:&str,old:Value,next:&Install,target:&str)->Result<Value,String>{
    let cid=removal_id(&old)?.to_owned();
    if !immutable(target)||image_id(&old).is_none()||old["id"]!=next.id{return Err("Invalid replacement identity".into())}
    ensure_volume(scope,next).await?;
    let mut record=json!({"id":next.id,"phase":"prepared","old":old,
        "targetImage":next.image,"targetImageId":target});
    save(state,&record)?;
    call(&["container","rm",&cid],30).await?;
    let mut installed=create(state,scope,next,target).await?;
    record["phase"]=json!("ready");save(state,&record)?;
    attach(state,&mut installed);
    Ok(installed)
}
fn removal_id(row:&Value)->Result<&str,String>{
    if row["running"]!=false{return Err("플러그인을 먼저 중지하세요.".into())}
    row["containerId"].as_str().filter(|s|container_id(s)).ok_or("Invalid container identity".into())
}
pub async fn update(State(state):State<AppState>,Path(id):Path<String>,Json(input):Json<Update>)->Response{
    if super::recipe::voice_tag(&input.image)||super::package::local_tag(&input.image){return error(StatusCode::BAD_REQUEST,"로컬 빌드 프로그램은 전용 설치를 이용하세요.")}
    if !input.confirmed||!valid_id(&id)||!valid_image(&input.image)||!immutable(&input.expected_image_id){
        return error(StatusCode::BAD_REQUEST,"새 이미지 버전과 현재 버전을 확인하세요.")
    }
    let _guard=match state.integrations.plugin_mutations.try_lock(){Ok(g)=>g,Err(_)=>return conflict("다른 플러그인 변경이 진행 중입니다.")};
    if pending(&state,&id){return conflict("이전 버전 변경을 먼저 복구하세요.")}
    let scope=match owner(&state){Ok(s)=>s,Err(e)=>return failed(&e)};
    let result:Result<Value,String>=async{
        let old=inspect(&id,&scope).await?.ok_or("설치된 플러그인이 없습니다.")?;
        if image_id(&old)!=Some(input.expected_image_id.as_str()){return Err("현재 이미지가 변경되었습니다. 새로고침하세요.".into())}
        removal_id(&old)?;
        let next=spec(&old,&input.image).ok_or("Invalid installed plugin metadata")?;
        // Download before replacing the working version. An unavailable
        // registry or image leaves the original container untouched.
        let target=pull(&input.image).await?;
        if target==input.expected_image_id{return Ok(old)}
        replace_stopped(&state,&scope,old,&next,&target).await
    }.await;
    match result{Ok(row)=>Json(row).into_response(),Err(e)=>failed(&e)}
}
pub async fn rollback(State(state):State<AppState>,Path(id):Path<String>,Json(input):Json<Rollback>)->Response{
    if !input.confirmed||!valid_id(&id){return error(StatusCode::BAD_REQUEST,"이전 실행 이미지로 복구를 확인하세요.")}
    let _guard=match state.integrations.plugin_mutations.try_lock(){Ok(g)=>g,Err(_)=>return conflict("다른 플러그인 변경이 진행 중입니다.")};
    let scope=match owner(&state){Ok(s)=>s,Err(e)=>return failed(&e)};
    let result:Result<Value,String>=async{
        let mut record=state.store.get(KIND,&id).filter(|r|matches!(r["phase"].as_str(),Some("ready"|"prepared")))
            .ok_or("복구할 이전 이미지가 없습니다.")?;
        let old=&record["old"];
        let exact=image_id(old).ok_or("Invalid previous image")?.to_owned();
        let target=record["targetImageId"].as_str().filter(|s|immutable(s)).ok_or("Invalid target image")?.to_owned();
        let previous=spec(old,old["image"].as_str().unwrap_or_default()).ok_or("Invalid previous plugin metadata")?;
        if previous.id!=id{return Err("Rollback identity mismatch".into())}
        // Confirm the previous image is still present before touching a
        // replacement. Docker image prune may make rollback unavailable.
        let available=call(&["image","inspect","--format","{{.Id}}",&exact],15).await?;
        if available.trim()!=exact{return Err("이전 실행 이미지가 로컬에 없습니다.".into())}
        let current=inspect(&id,&scope).await?;
        if let Some(row)=&current{
            if image_id(row)==Some(exact.as_str()) {
                record["phase"]=json!("rolled_back");save(&state,&record)?;
                return Ok(row.clone())
            }
            if image_id(row)!=Some(target.as_str()){return Err("등록된 버전과 현재 이미지가 일치하지 않습니다.".into())}
            let cid=removal_id(row)?.to_owned();
            record["phase"]=json!("prepared");save(&state,&record)?;
            call(&["container","rm",&cid],30).await?;
        }
        let installed=create(&state,&scope,&previous,&exact).await?;
        record["phase"]=json!("rolled_back");save(&state,&record)?;
        Ok(installed)
    }.await;
    match result{Ok(row)=>Json(row).into_response(),Err(e)=>failed(&e)}
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn rollback_uses_exact_image_and_preserves_owner_settings(){
        let id="01234567-89ab-4def-8123-456789abcdef";
        let old=json!({"id":id,"name":"Voice","image":"voice:1.0","imageId":format!("sha256:{}","a".repeat(64)),
            "network":true,"requestedPort":8080,"running":false,"containerId":"b".repeat(64)});
        let next=spec(&old,"voice:2.0").unwrap();
        assert_eq!(next.id,id);assert_eq!(next.port,Some(8080));assert!(next.network);
        assert!(image_id(&old).is_some());assert!(spec(&old,"voice:latest").is_none());
        let mut running=old;running["running"]=json!(true);assert!(removal_id(&running).is_err());
    }
}
