//! Durable autonomous content production on the existing Rust job kernel.
use crate::{engine::{jobs::{self,JobContext},llm,video},state::AppState};
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
fn field(v:&Value,key:&str,max:usize,required:bool)->Result<String,String>{
 let text=v[key].as_str().unwrap_or("").trim();
 if text.len()>max||(required&&text.is_empty()){return Err(format!("{key}: 값 또는 길이를 확인하세요."))}
 Ok(text.to_string())
}
pub fn validate_scenes(v:&Value)->Result<Vec<Value>,String>{
 let items=v.as_array().ok_or("장면 배열이 필요합니다.")?;
 if items.is_empty()||items.len()>12{return Err("장면은 1~12개까지 지원합니다.".into())}
 let mut scenes=Vec::new();
 for (i,item) in items.iter().enumerate(){
  let script=field(item,"script",1800,true)?;
  let visual=field(item,"visualPrompt",1800,false)?;
  let overlay=field(item,"overlay",180,false)?;
  let seconds=item["seconds"].as_u64().unwrap_or(0);
  if !(1..=180).contains(&seconds){return Err("장면은 1~180초여야 합니다.".into())}
  scenes.push(json!({"id":format!("scene-{}",i+1),"script":script,
   "visualPrompt":visual,"overlay":overlay,"seconds":seconds}));
 }
 let total:u64=scenes.iter().map(|v|v["seconds"].as_u64().unwrap_or(0)).sum();
 if !(3..=180).contains(&total){return Err("총 길이는 3~180초여야 합니다.".into())}
 Ok(scenes)
}
fn offline(project:&Value)->Vec<Value>{
 let brief=project["brief"].as_str().unwrap_or("");
 let pieces=brief.split(['\n','.','!','?','。']).map(str::trim)
  .filter(|p|!p.is_empty()).take(8).collect::<Vec<_>>();
 let pieces=if pieces.is_empty(){vec![brief]}else{pieces};
 let total=project["targetSeconds"].as_u64().unwrap_or(30);
 let n=pieces.len() as u64;
 pieces.iter().enumerate().map(|(i,s)|json!({"script":s,
  "visualPrompt":s,"overlay":"","seconds":total/n+u64::from((i as u64)<total%n)})).collect()
}
fn render_id(id:&str,version:u64)->String{
 let digest=Sha256::digest(format!("dominion-content:{id}:{version}"));
 let mut bytes=[0u8;16];bytes.copy_from_slice(&digest[..16]);
 uuid::Uuid::from_bytes(bytes).to_string()
}
pub fn get(state:&AppState,id:&str)->Result<Value,String>{
 uuid::Uuid::parse_str(id).map_err(|_|"잘못된 프로젝트 ID")?;
 state.store.get("video_project",id).ok_or("프로젝트가 없습니다.".into())
}
pub fn list(state:&AppState)->Vec<Value>{
 let mut rows=state.store.all("video_project");
 rows.sort_by(|a,b|b["createdAt"].as_str().unwrap_or("").cmp(a["createdAt"].as_str().unwrap_or("")));
 rows.truncate(80);rows
}
pub fn create(state:&AppState,body:&Value)->Result<Value,String>{
 let brief=field(body,"brief",16000,true)?;
 let title=field(body,"title",180,false)?;
 let audience=field(body,"audience",180,false)?;
 let style=field(body,"style",180,false)?;
 let provider=field(body,"providerId",180,false)?;
 let model=field(body,"model",180,false)?;
 let platform=body["platform"].as_str().unwrap_or("youtube");
 if !matches!(platform,"youtube"|"shorts"|"reels"|"tiktok"|"local"){
  return Err("지원하지 않는 플랫폼입니다.".into())
 }
 let engine=body["engine"].as_str().unwrap_or("native");
 if !matches!(engine,"native"|"comfyui"){return Err("지원하지 않는 엔진입니다.".into())}
 let seconds=body["targetSeconds"].as_u64().unwrap_or(30);
 if !(3..=180).contains(&seconds){return Err("영상은 3~180초여야 합니다.".into())}
 let aspect=body["aspectRatio"].as_str().unwrap_or("9:16");
 if !matches!(aspect,"9:16"|"16:9"|"1:1"){return Err("화면 비율을 확인하세요.".into())}
 let ai=body["useAI"]==true;
 let auto=body["autoRender"]==true;
 let upload=body["autoUpload"]==true;
 let privacy=body["privacyStatus"].as_str().unwrap_or("private");
 if !matches!(privacy,"private"|"unlisted"|"public"){return Err("공개 범위를 확인하세요.".into())}
 if ai&&provider.is_empty(){return Err("AI 기획용 Provider를 선택하세요.".into())}
 if upload&&(!auto||!matches!(platform,"youtube"|"shorts")||privacy=="public"){
  return Err("자동 배포는 YouTube 비공개/일부공개 및 자동 렌더링만 지원합니다.".into())
 }
 if upload&&crate::engine::oauth::access_token(state,"youtube").is_none(){
  return Err("YouTube 계정 연결이 필요합니다.".into())
 }
 if auto&&engine=="native"&&video::ffmpeg(state).is_none(){
  return Err("자동 렌더링에는 FFmpeg가 필요합니다.".into())
 }
 if auto&&engine=="comfyui"{
  let cfg=video::config(state);
  if cfg["comfyUrl"].as_str().unwrap_or("").is_empty()||
   (!cfg["comfyWorkflow"].is_object()&&!video::video_models(state).iter().any(|m|
    m["id"]==cfg["videoModel"]&&m["available"]==true)){
   return Err("ComfyUI 서버·모델·워크플로를 먼저 설정하세요.".into())
  }
 }
 let id=uuid::Uuid::new_v4().to_string();
 let project=json!({"id":id,"kind":"content_project","status":"planning",
  "brief":brief,"title":if title.is_empty(){brief.chars().take(70).collect::<String>()}else{title},
  "audience":audience,"style":style,"platform":platform,
  "targetSeconds":seconds,"aspectRatio":aspect,"engine":engine,"useAI":ai,
  "providerId":provider,"model":model,"autoRender":auto,"autoUpload":upload,
  "privacyStatus":privacy,"economy":body["economy"].as_bool().unwrap_or(true),
  "revision":0,"scenes":[],"planJobId":null,"renderJobId":null,
  "createdAt":chrono::Utc::now().to_rfc3339()});
 state.store.put_if_absent("video_project",&project).ok_or("프로젝트 저장 실패")?;
 let job=jobs::enqueue_once(state,&render_id(&id,0),"video_plan",json!({"projectId":id}))
  .ok_or("기획 작업 등록 실패")?;
 let saved=state.store.patch_if("video_project",&id,"planJobId",&Value::Null,
  &json!({"planJobId":job["id"]})).ok_or("기획 작업 연결 실패")?;
 state.bus.record("video_project",&saved);Ok(saved)
}
pub fn edit(state:&AppState,id:&str,body:&Value)->Result<Value,String>{
 let prior=get(state,id)?;
 let revision=prior["revision"].as_u64().unwrap_or(0);
 if body["revision"].as_u64()!=Some(revision){return Err("다른 창에서 변경되었습니다. 다시 불러오세요.".into())}
 if !matches!(prior["status"].as_str(),Some("planned"|"ready"|"failed")){
  return Err("기획 또는 렌더링이 끝난 후 수정하세요.".into())
 }
 let new_scenes=validate_scenes(&body["scenes"])?;
 let title=if body["title"].is_string(){field(body,"title",180,true)?}
  else{prior["title"].as_str().unwrap_or("").to_owned()};
 let saved=state.store.patch_if("video_project",id,"revision",&json!(revision),
  &json!({"revision":revision+1,"scenes":new_scenes,"title":title,
   "status":"planned","renderJobId":null,"updatedAt":chrono::Utc::now().to_rfc3339()}))
  .ok_or("편집 충돌이 발생했습니다.")?;
 state.bus.record("video_project",&saved);Ok(saved)
}
pub fn render(state:&AppState,id:&str,expected:Option<u64>)->Result<Value,String>{
 let mut project=get(state,id)?;
 let mut rev=project["revision"].as_u64().unwrap_or(0);
 if expected.is_some_and(|v|v!=rev){return Err("기획 버전이 바뀌었습니다.".into())}
 if !matches!(project["status"].as_str(),Some("planned"|"ready"|"failed"|"rendering")){
  return Err("기획 완료 후 렌더링할 수 있습니다.".into())
 }
 if matches!(project["status"].as_str(),Some("ready"|"failed"))
    && project["renderJobId"].is_string(){
  if let Some(old)=project["renderJobId"].as_str()
    .and_then(|job_id|state.store.get("job",job_id)){
    if old["status"]=="needs_review"{
      return Err("이전 영상의 원격 생성 결과가 불확실합니다. 먼저 작업 기록을 확인하세요.".into())
    }
  }
  // Explicit re-render is a new immutable revision, not a replay of a
  // completed/failed child with the same idempotency key.
  project=state.store.patch_if("video_project",id,"revision",&json!(rev),
    &json!({"revision":rev+1,"status":"planned",
      "renderJobId":null,"renderError":null}))
    .ok_or("새 렌더링 버전을 생성하지 못했습니다.")?;
  rev+=1;
 }
 let scenes=validate_scenes(&project["scenes"])?;
 let brief=scenes.iter().filter_map(|v|v["script"].as_str()).collect::<Vec<_>>().join("\n");
 let duration=scenes.iter().map(|v|v["seconds"].as_u64().unwrap_or(0)).sum::<u64>();
 let mut params=json!({"title":project["title"],"brief":brief,
  "duration":duration,"aspectRatio":project["aspectRatio"],"engine":project["engine"],
  "economy":project["economy"],"projectId":id,"projectRevision":rev,"scenes":scenes,
  "studioAutoUpload":project["autoUpload"],"studioPrivacyStatus":project["privacyStatus"]});
 if project["engine"]=="comfyui"{params["brief"]=project["brief"].clone()}
 let queued=video::enqueue_project(state,&params,&render_id(id,rev))?;
 let saved=state.store.patch_if("video_project",id,"revision",&json!(rev),
  &json!({"status":"rendering","renderJobId":queued["id"]}))
  .ok_or("렌더링 작업 연결 충돌")?;
 state.bus.record("video_project",&saved);Ok(saved)
}
pub async fn run_plan(ctx:&JobContext,job:&Value)->Result<Value,String>{
 let id=job["input"]["projectId"].as_str().ok_or("프로젝트 ID가 없습니다.")?;
 let project=get(&ctx.state,id)?;
 if project["revision"].as_u64().unwrap_or(0)>0{
  if project["autoRender"]==true&&project["renderJobId"].is_null(){
   render(&ctx.state,id,project["revision"].as_u64())?;
  }
  return Ok(json!({"projectId":id,"reused":true}))
 }
 ctx.checkpoint(json!({"phase":"scriptwriting","progress":15}))?;
 let scenes=if project["useAI"]==true{
  let prompt=format!("주제:{}\n타깃:{}\n스타일:{}\n영상 {}초\n플랫폼:{}",
   project["brief"],project["audience"],project["style"],
   project["targetSeconds"],project["platform"]);
  let answer=llm::complete(&ctx.state,project["providerId"].as_str().unwrap_or(""),
   project["model"].as_str().unwrap_or(""),
   "영상 연출자입니다. 사실을 지어내지 마세요. JSON 객체만 응답하세요. 형식: {\"scenes\":[{\"script\":\"나레이션\",\"visualPrompt\":\"장면 지시\",\"overlay\":\"화면 자막\",\"seconds\":5}]}. 도입, 본론, 마무리를 포함한 2~8개 장면, 총길이 3~180초.",&prompt).await?;
  let parsed:Value=serde_json::from_str(answer.trim())
   .map_err(|_|"기획 모델이 유효한 JSON을 반환하지 않았습니다.")?;
  validate_scenes(&parsed["scenes"])?
 }else{validate_scenes(&json!(offline(&project)))?};
 let source=if project["useAI"]==true{"provider"}else{"offline"};
 let saved=ctx.state.store.patch_if("video_project",id,"revision",&json!(0),
  &json!({"revision":1,"status":"planned","scenes":scenes,
   "planningSource":source,"updatedAt":chrono::Utc::now().to_rfc3339()}))
  .ok_or("기획 저장 충돌")?;
 ctx.state.bus.record("video_project",&saved);
 if project["autoRender"]==true{render(&ctx.state,id,Some(1))?;}
 Ok(json!({"projectId":id,"sceneCount":scenes.len(),"planningSource":source}))
}
pub fn finish_render(state:&AppState,job:&Value){
 let Some(id)=job["input"]["projectId"].as_str()else{return};
 let rev=job["input"]["projectRevision"].as_u64().unwrap_or(0);
 if let Ok(prior)=get(state,id){
  if prior["revision"].as_u64()!=Some(rev){return}
  let status=if job["status"]=="completed"{"ready"}else{"failed"};
  if let Some(saved)=state.store.patch_if("video_project",id,"revision",&json!(rev),
   &json!({"status":status,"renderJobId":job["id"],
    "renderStatus":job["status"],"renderError":job["error"]})){
   state.bus.record("video_project",&saved);
  }
 }
}
#[cfg(test)]
mod tests{
 use super::*;
 fn test_state()->AppState{
  let dir=std::env::temp_dir().join(format!("dominion-video-project-{}",uuid::Uuid::new_v4()));
  let cfg=crate::config::Config{port:0,data_dir:dir.join("data"),
   web_dir:dir.join("web"),workspace:dir.join("workspace")};
  AppState::new(cfg.clone(),crate::db::Store::open(&cfg.db_path()).unwrap(),
   crate::vault::Vault::open(&cfg.key_path()))
 }
 #[tokio::test]
 async fn real_project_plan_is_persistent_editable_and_revision_safe(){
  let state=test_state();
  let project=create(&state,&json!({"brief":"intro. facts. final.","targetSeconds":30,
   "platform":"shorts","autoRender":false,"useAI":false})).unwrap();
  assert_eq!(project["status"],"planning");
  let id=project["id"].as_str().unwrap();
  let plan_id=project["planJobId"].as_str().unwrap();
  let job=state.store.get("job",plan_id).unwrap();
  assert_eq!(job["kind"],"video_plan");
  let ctx=JobContext::new(state.clone(),plan_id.into());
  let result=run_plan(&ctx,&job).await.unwrap();
  assert_eq!(result["sceneCount"],3);
  let stored=get(&state,id).unwrap();
  assert_eq!(stored["status"],"planned");
  assert_eq!(stored["revision"],1);
  let edit_result=edit(&state,id,&json!({"revision":1,"title":"Edited",
    "scenes":[{"script":"new script","visualPrompt":"a street at night",
        "overlay":"Text on screen","seconds":5}]})).unwrap();
  assert_eq!(edit_result["revision"],2);
  assert_eq!(edit_result["scenes"][0]["overlay"],"Text on screen");
  assert!(edit(&state,id,&json!({"revision":1,"scenes":stored["scenes"]})).is_err());
  assert_eq!(list(&state).len(),1);
  crate::test_support::cleanup_dir(state.config.data_dir.clone());
 }
 #[test]
 fn auto_upload_requires_explicit_non_public_authorization(){
  let state=test_state();
  let body=json!({"brief":"video","autoRender":true,"autoUpload":true,
   "privacyStatus":"public","platform":"shorts"});
  assert!(create(&state,&body).is_err());
  assert!(state.store.all("video_project").is_empty());
  crate::test_support::cleanup_dir(state.config.data_dir.clone());
 }
 #[test]fn scene_edits_are_validated(){
  assert!(validate_scenes(&json!([])).is_err());
  assert!(validate_scenes(&json!([{"script":"one","seconds":3}])).is_ok());
  assert!(validate_scenes(&json!([{"script":"","seconds":3}])).is_err());
  let p=json!({"brief":"intro. body. end.","targetSeconds":31});
  let s=offline(&p);
  assert_eq!(s.iter().map(|v|v["seconds"].as_u64().unwrap()).sum::<u64>(),31);
  assert_eq!(render_id("id",1),render_id("id",1));
  assert_ne!(render_id("id",1),render_id("id",2));
 }
}
