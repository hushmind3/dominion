//! Fixed package-to-container recipes, with explicit package version and CLI.
//! Search results are untrusted data; they never become Dockerfile syntax.
use super::{owner,valid_id,inspect,inventory,ensure_volume,create_managed,docker,call,container_id,Install,MAX_PLUGINS,versions};
use crate::{state::AppState,api::response::error};
use axum::{extract::State,http::StatusCode,response::{IntoResponse,Response},Json};
use serde::{Deserialize,Serialize};
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::path::PathBuf;

const KIND:&str="plugin_package";
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Spec{
    runtime:String,package:String,version:String,entry:String,
    #[serde(default)] args:Vec<String>,
}
#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Request{
    id:String,name:String,spec:Spec,network:bool,port:Option<u16>,confirmed:bool,
}
fn atom(value:&str,max:usize)->bool{
    !value.is_empty()&&value.len()<=max&&value.as_bytes()[0].is_ascii_alphanumeric()
        &&value.bytes().all(|c|c.is_ascii_alphanumeric()||matches!(c,b'_'|b'-'|b'.'))
        &&value!="."&&value!=".."
}
fn version(value:&str)->bool{
    !value.is_empty()&&value.len()<=96&&value.as_bytes()[0].is_ascii_digit()
        &&value.bytes().all(|c|c.is_ascii_alphanumeric()||matches!(c,b'.'|b'-'|b'+'|b'!'))
}
fn npm_version(value:&str)->bool{
    if !version(value)||value.contains('!'){return false}
    let core=value.split(['-','+']).next().unwrap_or("");
    let parts=core.split('.').collect::<Vec<_>>();
    parts.len()==3&&parts.iter().all(|p|!p.is_empty()&&p.bytes().all(|c|c.is_ascii_digit()))
}
impl Spec{
    pub(super) fn valid(&self)->bool{
        let pkg=match self.runtime.as_str(){
            "npm"=>if let Some(s)=self.package.strip_prefix('@'){
                s.split_once('/').is_some_and(|(a,b)|atom(a,100)&&atom(b,100))
            }else{atom(&self.package,214)},
            "pypi"|"crates"=>atom(&self.package,160),
            _=>false,
        };
        pkg&&atom(&self.entry,100)&&version(&self.version)
            &&(self.runtime!="npm"&&self.runtime!="crates"||npm_version(&self.version))
            &&self.args.len()<=32&&self.args.iter().all(|a|a.len()<=1000&&!a.chars().any(char::is_control))
    }
    fn image(&self)->String{
        let hash=Sha256::digest(serde_json::to_vec(self).unwrap_or_default());
        format!("dominion-package:{}",format!("{hash:x}"))
    }
    fn dockerfile(&self)->Result<String,String>{
        if !self.valid(){return Err("지원하는 패키지 이름·명시적 버전·CLI 실행 파일을 확인하세요.".into())}
        let (base,commands,binary)=match self.runtime.as_str(){
            "npm"=>("node:24-bookworm-slim",vec![
                vec!["npm".to_owned(),"install".into(),"--prefix".into(),"/opt/plugin".into(),
                     "--ignore-scripts".into(),"--no-audit".into(),"--no-fund".into(),"--save-exact".into(),
                     format!("{}@{}",self.package,self.version)]],
                 format!("/opt/plugin/node_modules/.bin/{}",self.entry)),
            "pypi"=>("python:3.12-slim-bookworm",vec![
                vec!["python".into(),"-m".into(),"venv".into(),"/opt/venv".into()],
                vec!["/opt/venv/bin/pip".into(),"install".into(),"--no-cache-dir".into(),
                     "--only-binary=:all:".into(),format!("{}=={}",self.package,self.version)]],
                format!("/opt/venv/bin/{}",self.entry)),
            "crates"=>("rust:1-slim-bookworm",vec![
                vec!["cargo".into(),"install".into(),"--locked".into(),"--root".into(),"/opt/plugin".into(),
                     "--version".into(),format!("={}",self.version),self.package.clone()]],
                format!("/opt/plugin/bin/{}",self.entry)),
            _=>return Err("Unsupported package runtime".into())
        };
        let mut out=format!("FROM {base}\nENV HOME=/tmp PYTHONDONTWRITEBYTECODE=1 PYTHONUNBUFFERED=1\nWORKDIR /data\n");
        // Dockerfile exec-form JSON, never shell interpolation, no host files.
        for command in commands{
            out.push_str("RUN ");out.push_str(&serde_json::to_string(&command).map_err(|e|e.to_string())?);out.push('\n');
        }
        out.push_str("RUN ");out.push_str(&serde_json::to_string(&["test","-x",binary.as_str()]).map_err(|e|e.to_string())?);out.push('\n');
        let mut args=vec![binary];args.extend(self.args.clone());
        out.push_str("USER 10001:10001\nCMD ");out.push_str(&serde_json::to_string(&args).map_err(|e|e.to_string())?);out.push('\n');
        Ok(out)
    }
}
pub(crate) fn local_tag(image:&str)->bool{image.starts_with("dominion-package:")}
pub(crate) fn attach(state:&AppState,row:&mut Value){
    let Some(id)=row["id"].as_str()else{return};
    if let Some(record)=state.store.get(KIND,id).filter(|r|r["image"]==row["image"]){
        row["packageSpec"]=record["spec"].clone();
    }
}
struct Context(PathBuf);
impl Drop for Context{fn drop(&mut self){let _=std::fs::remove_dir_all(&self.0);}}
fn context(state:&AppState,dockerfile:&str)->Result<Context,String>{
    let dir=state.config.data_dir.join(format!("package-build-{}",uuid::Uuid::new_v4()));
    std::fs::create_dir(&dir).map_err(|e|e.to_string())?;
    let dir=Context(dir);
    std::fs::write(dir.0.join("Dockerfile"),dockerfile).map_err(|e|e.to_string())?;
    Ok(dir)
}
pub async fn install(State(state):State<AppState>,Json(request):Json<Request>)->Response{
    if !request.confirmed||!valid_id(&request.id)||!request.spec.valid()
        ||request.name.trim().is_empty()||request.name.len()>120||request.name.chars().any(char::is_control)
        ||request.port==Some(0)||request.port.is_some()&&!request.network{
        return error(StatusCode::BAD_REQUEST,"패키지 버전·실행 파일·권한과 설치 승인을 확인하세요.")
    }
    let _guard=match state.integrations.plugin_mutations.try_lock(){Ok(g)=>g,Err(_)=>return error(StatusCode::CONFLICT,"다른 플러그인 변경이 진행 중입니다.")};
    let result:Result<Value,String>=async{
        let scope=owner(&state)?;
        if versions::pending(&state,&request.id){return Err("이전 버전 변경을 먼저 복구하세요.".into())}
        let image=request.spec.image();
        let spec=json!(request.spec);
        let record=json!({"id":request.id,"image":image,"spec":spec});
        if let Some(mut row)=inspect(&request.id,&scope).await?{
            if row["image"]==image&&row["name"]==request.name&&row["network"]==request.network
                &&row["requestedPort"]==json!(request.port){
                attach(&state,&mut row);return Ok(row)
            }
            return Err("같은 설치 ID에 다른 프로그램이 있습니다.".into())
        }
        let rows=inventory(&scope).await?;
        if rows.len()>=MAX_PLUGINS&&!rows.iter().any(|r|r["id"]==request.id){return Err("플러그인 등록 한도에 도달했습니다.".into())}
        let context=context(&state,&request.spec.dockerfile()?)?;
        let path=context.0.to_str().ok_or("Invalid build directory")?;
        let args=vec!["build".into(),"--quiet".into(),"--pull".into(),"--tag".into(),image.clone(),path.into()];
        docker(&args,900).await?;
        let output=call(&["image","inspect","--format","{{.Id}}",&image],15).await?;
        let exact=output.trim();
        if !exact.strip_prefix("sha256:").is_some_and(container_id){return Err("Invalid package image identity".into())}
        let input=Install{id:request.id.clone(),name:request.name,image,network:request.network,port:request.port,confirmed:true};
        // Save the exact package recipe before creating the container.
        if state.store.put(KIND,&record).is_none(){return Err("패키지 설치 설정을 저장하지 못했습니다.".into())}
        ensure_volume(&scope,&input).await?;
        create_managed(&state,&scope,&input,exact).await?;
        let mut row=inspect(&request.id,&scope).await?.ok_or("패키지 설치 결과를 확인하지 못했습니다.")?;
        attach(&state,&mut row);
        Ok(row)
    }.await;
    match result{Ok(row)=>Json(row).into_response(),Err(e)=>error(StatusCode::BAD_GATEWAY,&e)}
}
#[cfg(test)]
mod tests{
    use super::*;
    fn spec(runtime:&str,package:&str,version:&str)->Spec{Spec{runtime:runtime.into(),package:package.into(),version:version.into(),entry:"app".into(),args:vec!["--port".into(),"8080".into()]}}
    #[test]
    fn recipes_are_explicit_and_never_embed_shell(){
        for s in [spec("npm","@scope/app","1.2.3"),spec("pypi","app-cli","2.0.1"),spec("crates","app_cli","3.0.0")]{
            let f=s.dockerfile().unwrap();assert!(f.contains("USER 10001:10001"));assert!(f.contains("CMD ["));assert!(local_tag(&s.image()));
        }
        for s in [spec("npm","app","latest"),spec("npm","app","1"),spec("pypi","app;evil","1.2"),spec("npm","@x/../evil","1.2.3"),spec("pypi","app","1\nRUN evil")]{assert!(!s.valid());}
        let mut s=spec("pypi","app","1.2");s.entry="../bin";assert!(!s.valid());
        let mut s=spec("npm","app","1.2.3");s.args=vec!["x\nRUN evil".into()];assert!(!s.valid());
    }
}
