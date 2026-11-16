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
/// Wan on consumer GPUs is rendered as short bounded clips, never as
/// one unbounded 30-second latent graph. Preserve scene order and total time.
fn bounded_ai_shots(scenes:&[Value])->Result<Vec<Value>,String>{
    let mut output=Vec::new();
    for (index,scene) in scenes.iter().enumerate(){
        let duration=scene["seconds"].as_u64().unwrap_or(0);
        if duration==0{return Err("장면 길이가 올바르지 않습니다.".into())}
        let pieces=duration.div_ceil(5);
        if output.len()+pieces as usize>12{
            return Err("이 모델의 자율 AI 영상은 최대 12장면 × 5초(60초)입니다. 장면을 줄이거나 FFmpeg 모드를 선택하세요.".into());
        }
        let mut remaining=duration;
        for piece in 0..pieces {
            let length=remaining.min(5);
            remaining-=length;
            let mut part=scene.clone();
            part["id"]=json!(format!("scene-{}-clip-{}",index+1,piece+1));
            part["seconds"]=json!(length);
            if pieces>1{
                let visual=scene["visualPrompt"].as_str().unwrap_or("");
                part["visualPrompt"]=json!(format!("{visual}. Visual continuity across clip {} of {}; matching subjects, location and lighting.",piece+1,pieces));
            }
            output.push(part);
        }
    }
    Ok(output)
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
/// The same project keeps the same base seed across edit revisions.
/// Without this, video::enqueue would randomize each job's seed, making
/// unchanged scenes miss the clip cache and spend GPU time unnecessarily.
fn project_seed(id:&str)->i64{
 let hash=Sha256::digest(format!("dominion-content-scene-seed:{id}"));
 i64::from(u32::from_be_bytes(hash[..4].try_into().unwrap()))
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
/// Explicitly retry a failed *planning* job without replaying an immutable job ID.
pub fn retry_plan(state:&AppState,id:&str,expected_revision:u64,acknowledge_unknown:bool)->Result<Value,String>{
 let previous=get(state,id)?;
 if previous["revision"].as_u64()!=Some(expected_revision){
  return Err("기획 버전이 바뀌었습니다. 다시 불러오세요.".into())
 }
 if !matches!(previous["status"].as_str(),Some("failed"|"needs_review"))||
    previous["scenes"].as_array().is_some_and(|items|!items.is_empty()){
  return Err("실패한 최초 기획만 다시 실행할 수 있습니다. 영상 오류는 렌더링을 다시 실행하세요.".into())
 }
 let old_id=previous["planJobId"].as_str().ok_or("이전 기획 작업이 없습니다.")?;
 let old_job=state.store.get("job",old_id).ok_or("이전 기획 기록이 없습니다.")?;
 if old_job["kind"]!="video_plan"||old_job["input"]["projectId"]!=id{
  return Err("기획 작업 연결이 올바르지 않습니다.".into())
 }
 if !matches!(old_job["status"].as_str(),Some("failed"|"cancelled"|"interrupted"|"needs_review")){
  return Err("이전 기획 작업의 결과를 검토한 뒤 재시도하세요.".into())
 }
 if old_job["status"]=="needs_review"&&!acknowledge_unknown{
  return Err("이전 유료 모델 호출의 결과가 불확실합니다. 중복 비용 가능성을 확인하고 다시 요청하세요.".into())
 }
 let attempt=previous["planAttempts"].as_u64().unwrap_or(1).saturating_add(1);
 let job_id=uuid::Uuid::new_v4().to_string();
 let mut updated=previous.clone();
 updated["status"]=json!("planning");
 updated["planJobId"]=json!(job_id);
 updated["planAttempts"]=json!(attempt);
 updated["planningError"]=Value::Null;
 updated["updatedAt"]=json!(chrono::Utc::now().to_rfc3339());
 let job=json!({"id":job_id,"kind":"video_plan","input":{"projectId":id},
  "status":"queued","queuedAt":chrono::Utc::now().to_rfc3339_opts(
   chrono::SecondsFormat::Micros,true),
  "attempts":0,"stepIndex":0,"stepTotal":0,"events":[]});
 // Commit the new work and its owner-visible link in one SQLite transaction.
 // The failed-status CAS makes simultaneous retry button presses single-winner.
 let records=vec![("video_project".to_string(),updated),
                   ("job".to_string(),job.clone())];
 if !state.store.transaction_if_status("video_project",id,
   previous["status"].as_str().unwrap_or("failed"),&records)?{
  return Err("다른 창에서 재시도했습니다. 다시 불러오세요.".into())
 }
 let saved=get(state,id)?;
 state.bus.record("job",&job);
 state.bus.record("video_project",&saved);
 state.store.audit("video.plan_retried",json!({"projectId":id,
  "previousJobId":old_id,"jobId":job_id,"attempt":attempt}));
 Ok(saved)
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
 if engine=="comfyui"&&seconds>60{
  return Err("현재 ComfyUI 턴키 제작은 최대 60초를 5초 이하 장면으로 분할합니다.".into())
 }
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
  "revision":0,"scenes":[],"planJobId":null,"planAttempts":1,"renderJobId":null,
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
 let editable=validate_scenes(&project["scenes"])?;
 let scenes=if project["engine"]=="comfyui"{bounded_ai_shots(&editable)?}else{editable};
 let brief=scenes.iter().filter_map(|v|v["script"].as_str()).collect::<Vec<_>>().join("\n");
 let duration=scenes.iter().map(|v|v["seconds"].as_u64().unwrap_or(0)).sum::<u64>();
 let mut params=json!({"title":project["title"],"brief":brief,
  "duration":duration,"aspectRatio":project["aspectRatio"],"engine":project["engine"],
  "economy":project["economy"],"projectId":id,"projectRevision":rev,"scenes":scenes,
  "seed":project_seed(id),
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
 // A superseded or orphaned planning job must never change the active project.
 if project["planJobId"]!=job["id"] {
  return Err("다른 기획 작업으로 교체된 실행입니다.".into())
 }
 if project["revision"].as_u64().unwrap_or(0)>0{
  if project["autoRender"]==true&&project["renderJobId"].is_null(){
   render(&ctx.state,id,project["revision"].as_u64())?;
  }
  return Ok(json!({"projectId":id,"reused":true}))
 }
 ctx.checkpoint(json!({"phase":"scriptwriting","progress":15}))?;
 let ai_planning=project["useAI"]==true;
 if ai_planning {
  // Persist the uncertainty BEFORE a paid provider request. After a crash,
  // workers must not quietly replay a paid model call with unknown outcome.
  ctx.checkpoint(json!({"videoPlanInFlight":true,"phase":"scriptwriting","progress":15}))?;
 }
 let mut scenes=if project["useAI"]==true{
  let prompt=format!("주제:{}\n타깃:{}\n스타일:{}\n영상 {}초\n플랫폼:{}",
   project["brief"],project["audience"],project["style"],
   project["targetSeconds"],project["platform"]);
  let answer=llm::complete(&ctx.state,project["providerId"].as_str().unwrap_or(""),
   project["model"].as_str().unwrap_or(""),
   "영상 연출자입니다. 사실을 지어내지 마세요. JSON 객체만 응답하세요. 형식: {\"scenes\":[{\"script\":\"나레이션\",\"visualPrompt\":\"장면 지시\",\"overlay\":\"화면 자막\",\"seconds\":5}]}. 도입, 본론, 마무리를 포함한 2~8개 장면, 총길이 3~180초.",&prompt).await?;
  let raw=answer.trim().trim_start_matches("```json")
   .trim_start_matches("```")
   .trim_end_matches("```").trim();
  let parsed:Value=serde_json::from_str(raw)
   .map_err(|_|"기획 모델이 유효한 JSON을 반환하지 않았습니다.")?;
  validate_scenes(&parsed["scenes"])?
 }else{validate_scenes(&json!(offline(&project)))?};
 // Normalize AI-scored scene lengths to the owner's requested runtime
 // without dropping a scene or creating zero-second frames.
 let target=project["targetSeconds"].as_u64().unwrap_or(30);
 let original:u64=scenes.iter().map(|scene|scene["seconds"].as_u64().unwrap_or(0)).sum();
 if original!=target {
  if scenes.len()>target as usize{return Err("장면 수가 요청 영상 길이보다 많습니다.".into())}
  let mut left=target;
  for i in 0..scenes.len() {
   let remaining=(scenes.len()-i-1) as u64;
   let duration=if i==scenes.len()-1{left}else{
    ((scenes[i]["seconds"].as_u64().unwrap_or(1)*target)/original)
     .max(1).min(left-remaining)
   };
   scenes[i]["seconds"]=json!(duration);
   left-=duration;
  }
 }
 if project["engine"]=="comfyui"{
  scenes=bounded_ai_shots(&scenes)?;
 }
 let source=if ai_planning{"provider"}else{"offline"};
 let saved=ctx.state.store.patch_if("video_project",id,"revision",&json!(0),
  &json!({"revision":1,"status":"planned","scenes":scenes,
   "planningSource":source,"updatedAt":chrono::Utc::now().to_rfc3339()}))
  .ok_or("기획 저장 충돌")?;
 ctx.state.bus.record("video_project",&saved);
 if ai_planning {ctx.checkpoint(json!({"videoPlanInFlight":false,"progress":80}))?;}
 if project["autoRender"]==true{render(&ctx.state,id,Some(1))?;}
 Ok(json!({"projectId":id,"sceneCount":scenes.len(),"planningSource":source}))
}
/// Only the currently linked planning job may change the project's error state.
pub fn finish_plan(state:&AppState,job:&Value){
 if job["status"]!="failed"{return}
 let Some(id)=job["input"]["projectId"].as_str()else{return};
 let Some(job_id)=job["id"].as_str()else{return};
 let Some(project)=state.store.get("video_project",id)else{return};
 if project["planJobId"]!=job_id{return}
 if let Some(saved)=state.store.patch_if("video_project",id,"status",
  &json!("planning"),&json!({"status":"failed","planningError":job["error"]})){
  state.bus.record("video_project",&saved);
 }
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
 async fn failed_planning_retry_has_atomic_new_job_and_stale_job_is_rejected(){
  let state=test_state();
  let created=create(&state,&json!({"brief":"First. Second.","targetSeconds":15,
    "useAI":false,"autoRender":false})).unwrap();
  let id=created["id"].as_str().unwrap();
  let prior_id=created["planJobId"].as_str().unwrap();
  let prior_job=state.store.patch_if("job",prior_id,"status",&json!("queued"),
   &json!({"status":"failed","error":"model returned invalid JSON"})).unwrap();
  let failed=state.store.patch_if("video_project",id,"status",&json!("planning"),
   &json!({"status":"failed","planningError":"model returned invalid JSON"})).unwrap();
  assert_eq!(failed["revision"],0);
  assert!(retry_plan(&state,id,1,false).is_err());
  let retried=retry_plan(&state,id,0,false).unwrap();
  assert_eq!(retried["status"],"planning");
  assert_eq!(retried["planAttempts"],2);
  assert!(retried["planningError"].is_null());
  assert_ne!(retried["planJobId"],prior_job["id"]);
  // An old worker may finish late: it must not fail the new planning attempt.
  finish_plan(&state,&prior_job);
  assert_eq!(get(&state,id).unwrap()["status"],"planning");
  assert!(retry_plan(&state,id,0,false).is_err());
  let orphan_ctx=JobContext::new(state.clone(),prior_id.into());
  assert!(run_plan(&orphan_ctx,&prior_job).await.is_err());
  let job_id=retried["planJobId"].as_str().unwrap();
  let next=state.store.get("job",job_id).unwrap();
  assert_eq!(next["status"],"queued");
  let ctx=JobContext::new(state.clone(),job_id.into());
  run_plan(&ctx,&next).await.unwrap();
  assert_eq!(get(&state,id).unwrap()["status"],"planned");
  assert_eq!(get(&state,id).unwrap()["planJobId"],next["id"]);
  crate::test_support::cleanup_dir(state.config.data_dir.clone());
 }
 #[test]
 fn only_current_planning_job_can_mark_a_project_failed(){
  let state=test_state();
  let project=create(&state,&json!({"brief":"Plan me","autoRender":false})).unwrap();
  let id=project["id"].as_str().unwrap();
  let first=project["planJobId"].as_str().unwrap();
  let failed=state.store.patch_if("job",first,"status",&json!("queued"),
   &json!({"status":"failed","error":"schema failure"})).unwrap();
  finish_plan(&state,&failed);
  assert_eq!(get(&state,id).unwrap()["status"],"failed");
  let retried=retry_plan(&state,id,0,false).unwrap();
  finish_plan(&state,&failed);
  assert_eq!(get(&state,id).unwrap()["status"],"planning");
  assert_ne!(retried["planJobId"],failed["id"]);
  crate::test_support::cleanup_dir(state.config.data_dir.clone());
 }
 #[test]
 fn recovered_paid_plan_requires_explicit_acknowledgement_before_retry(){
  let state=test_state();
  let project=create(&state,&json!({"brief":"One. Two.","autoRender":false})).unwrap();
  let id=project["id"].as_str().unwrap();
  let first=project["planJobId"].as_str().unwrap();
  state.store.patch_if("job",first,"status",&json!("queued"),
   &json!({"status":"running","videoPlanInFlight":true})).unwrap();
  jobs::recover(&state);
  let interrupted=state.store.get("job",first).unwrap();
  assert_eq!(interrupted["status"],"needs_review");
  let unresolved=get(&state,id).unwrap();
  assert_eq!(unresolved["status"],"needs_review");
  assert!(retry_plan(&state,id,0,false).is_err());
  let retried=retry_plan(&state,id,0,true).unwrap();
  assert_eq!(retried["status"],"planning");
  assert_ne!(retried["planJobId"],interrupted["id"]);
  assert_eq!(state.store.get("job",first).unwrap()["status"],"needs_review");
  crate::test_support::cleanup_dir(state.config.data_dir.clone());
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
 #[tokio::test]
 async fn editor_changes_flow_into_a_real_encoded_mp4_and_downloadable_srt(){
  let state=test_state();
  if video::ffmpeg(&state).is_none(){return}
  let project=create(&state,&json!({"brief":"two short scenes.","targetSeconds":3,
    "engine":"native","useAI":false,"autoRender":false})).unwrap();
  let id=project["id"].as_str().unwrap();
  let plan_id=project["planJobId"].as_str().unwrap();
  let plan=state.store.get("job",plan_id).unwrap();
  let plan_ctx=JobContext::new(state.clone(),plan_id.into());
  run_plan(&plan_ctx,&plan).await.unwrap();
  let edited=edit(&state,id,&json!({"revision":1,
    "title":"Edited producer title",
    "scenes":[{"script":"Spoken sentence one","overlay":"Visible title one",
      "visualPrompt":"Wide angle street","seconds":1},
      {"script":"Spoken sentence two","overlay":"Visible title two",
       "visualPrompt":"City skyline","seconds":2}]})).unwrap();
  let render_project=render(&state,id,edited["revision"].as_u64()).unwrap();
  let render_id=render_project["renderJobId"].as_str().unwrap();
  let job=state.store.get("job",render_id).unwrap();
  assert_eq!(job["input"]["scenes"][0]["script"],"Spoken sentence one");
  assert_eq!(job["input"]["scenes"][0]["overlay"],"Visible title one");
  assert_eq!(job["input"]["scenes"][1]["seconds"],2);
  assert_eq!(job["input"]["render"]["seed"],project_seed(id));
  assert_eq!(project_seed(id),project_seed(id));
  assert_ne!(project_seed(id),project_seed("other-video-project"));
  let ctx=JobContext::new(state.clone(),render_id.into());
  let output=video::run(&ctx,&job).await.unwrap();
  assert_eq!(output["qa"]["containerVerified"],true);
  assert_eq!(output["qa"]["captionsReady"],true);
  let dir=video::directory(&state,render_id).unwrap();
  let srt=std::fs::read_to_string(dir.join("captions.srt")).unwrap();
  assert!(srt.contains("Visible title one"));
  assert!(srt.contains("00:00:01,000 --> 00:00:03,000"));
  let manifest=std::fs::read_to_string(dir.join("production.json")).unwrap();
  assert!(manifest.contains("Spoken sentence two"));
  let saved=state.store.patch_if("job",render_id,"status",&json!("queued"),
   &json!({"status":"completed","result":output})).unwrap();
  finish_render(&state,&saved);
  assert_eq!(get(&state,id).unwrap()["status"],"ready");
  assert!(video::output_path(&state,render_id).is_ok());
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
 #[test]
 fn ai_video_is_split_into_bounded_scene_clips_with_correct_total_duration(){
  let original=json!([{"script":"Take a journey through the city",
   "visualPrompt":"City at night","overlay":"City trip","seconds":30}]);
  let output=bounded_ai_shots(original.as_array().unwrap()).unwrap();
  assert_eq!(output.len(),6);
  assert_eq!(output.iter().map(|s|s["seconds"].as_u64().unwrap()).sum::<u64>(),30);
  assert!(output.iter().all(|s|s["seconds"].as_u64().unwrap()<=5));
  assert_eq!(output[0]["script"],original[0]["script"]);
  assert!(bounded_ai_shots(&json!([{"script":"too long","seconds":61}])
    .as_array().unwrap()).is_err());
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
