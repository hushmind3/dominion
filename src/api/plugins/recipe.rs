//! A reviewed local build recipe; never build a registry search result or a
//! user-supplied Dockerfile. The context contains only compiled-in source files.
use super::{owner,valid_id,inspect,inventory,ensure_volume,create_managed,docker,call,container_id,Install,MAX_PLUGINS,versions};
use crate::state::AppState;
use crate::api::response::error;
use axum::{extract::State,http::StatusCode,response::{IntoResponse,Response},Json};
use serde::Deserialize;
use std::path::PathBuf;

pub(crate) const VOICE_IMAGE:&str="dominion-voice:0.2.0";
pub(crate) fn voice_tag(image:&str)->bool{matches!(image,"dominion-voice:0.1.0"|"dominion-voice:0.2.0")}
const FILES:[(&str,&str);6]=[
    ("Dockerfile",include_str!(concat!(env!("CARGO_MANIFEST_DIR"),"/plugins/dominion-voice/Dockerfile"))),
    ("session.py",include_str!(concat!(env!("CARGO_MANIFEST_DIR"),"/plugins/dominion-voice/session.py"))),
    ("app.py",include_str!(concat!(env!("CARGO_MANIFEST_DIR"),"/plugins/dominion-voice/app.py"))),
    ("voice.py",include_str!(concat!(env!("CARGO_MANIFEST_DIR"),"/plugins/dominion-voice/voice.py"))),
    ("index.html",include_str!(concat!(env!("CARGO_MANIFEST_DIR"),"/plugins/dominion-voice/index.html"))),
    ("app.js",include_str!(concat!(env!("CARGO_MANIFEST_DIR"),"/plugins/dominion-voice/app.js"))),
];
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request{id:String,confirmed:bool}
struct Context(PathBuf);
impl Drop for Context{fn drop(&mut self){let _=std::fs::remove_dir_all(&self.0);}}
fn context(state:&AppState)->Result<Context,String>{
    let dir=state.config.data_dir.join(format!("voice-build-{}",uuid::Uuid::new_v4()));
    std::fs::create_dir(&dir).map_err(|e|e.to_string())?;
    let dir=Context(dir);
    for (name,content) in FILES{std::fs::write(dir.0.join(name),content).map_err(|e|e.to_string())?;}
    Ok(dir)
}
fn key(id:&str)->String{format!("plugin:voice:{id}:bridge")}
pub(crate) fn bridge_token(state:&AppState,id:&str)->Option<String>{
    if !valid_id(id){return None}
    state.read_secret(&key(id)).filter(|v|v.len()==64&&v.bytes().all(|b|b.is_ascii_hexdigit()))
}
fn ensure_token(state:&AppState,id:&str)->Result<(),String>{
    if bridge_token(state,id).is_some(){return Ok(())}
    let token=format!("{}{}",uuid::Uuid::new_v4().simple(),uuid::Uuid::new_v4().simple());
    state.save_secret(&key(id),&token);
    if bridge_token(state,id).as_deref()!=Some(token.as_str()){return Err("음성 비서 인증정보를 저장하지 못했습니다.".into())}
    Ok(())
}
pub async fn install_voice(State(state):State<AppState>,Json(request):Json<Request>)->Response{
    if !request.confirmed||!valid_id(&request.id){return error(StatusCode::BAD_REQUEST,"음성 비서 빌드와 실행 환경을 승인하세요.")}
    let _guard=match state.integrations.plugin_mutations.try_lock(){Ok(g)=>g,Err(_)=>return error(StatusCode::CONFLICT,"다른 플러그인 변경이 진행 중입니다.")};
    let result:Result<serde_json::Value,String>=async{
        let scope=owner(&state)?;
        if versions::pending(&state,&request.id){return Err("이전 버전 변경을 먼저 복구하세요.".into())}
        let old=inspect(&request.id,&scope).await?;
        if let Some(row)=&old{
            if row["image"]==VOICE_IMAGE&&bridge_token(&state,&request.id).is_some(){return Ok(row.clone())}
            if row["image"]!="dominion-voice:0.1.0"||row["running"]!=false{
                return Err("기존 음성 비서를 중지하거나 다른 설치 ID를 사용하세요.".into())
            }
        }
        let rows=inventory(&scope).await?;
        if rows.len()>=MAX_PLUGINS&&!rows.iter().any(|r|r["id"]==request.id){return Err("플러그인 등록 한도에 도달했습니다.".into())}
        let input=Install{id:request.id.clone(),name:"DOMINION Voice".into(),image:VOICE_IMAGE.into(),
            network:true,port:Some(8080),confirmed:true};
        let context=context(&state)?;
        let path=context.0.to_str().ok_or("Invalid build directory")?;
        let args=vec!["build".into(),"--quiet".into(),"--pull".into(),"--tag".into(),VOICE_IMAGE.into(),path.into()];
        docker(&args,900).await?;
        let output=call(&["image","inspect","--format","{{.Id}}",VOICE_IMAGE],15).await?;
        let exact=output.trim();
        if !exact.strip_prefix("sha256:").is_some_and(container_id){return Err("Invalid voice image identity".into())}
        ensure_token(&state,&request.id)?;
        if let Some(row)=old{
            return versions::replace_stopped(&state,&scope,row,&input,exact).await
        }
        ensure_volume(&scope,&input).await?;
        create_managed(&state,&scope,&input,exact).await?;
        inspect(&request.id,&scope).await?.ok_or("음성 비서 설치 결과를 확인할 수 없습니다.".into())
    }.await;
    match result{Ok(row)=>Json(row).into_response(),Err(e)=>error(StatusCode::BAD_GATEWAY,&e)}
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn recipe_has_fixed_context_and_no_host_mount(){
        assert_eq!(FILES.len(),6);
        assert!(voice_tag(VOICE_IMAGE));assert!(voice_tag("dominion-voice:0.1.0"));
        assert!(FILES[0].1.contains("pipecat-ai[livekit,deepgram,cartesia,silero]==1.17.0"));
        assert!(FILES[0].1.contains("USER 10001"));
        assert!(!FILES.iter().any(|(name,_)|name.contains('/')||name.contains("..")));
    }
}
