//! Owner-managed OCI extensions. Docker, not the agent kernel, owns process
//! state. Every effect uses a scoped container identity, never a shell command.
use crate::state::AppState;
use axum::{extract::{Path,State},http::StatusCode,response::{IntoResponse,Response},Json};
use serde::Deserialize;
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::{path::PathBuf,process::Stdio,time::Duration};
use tokio::{io::{AsyncRead,AsyncReadExt},process::Command};
use super::response::error;
pub mod bridge;
pub mod versions;
pub mod recipe;
pub mod package;
pub mod agent;
mod data;
pub mod scheduled;
pub mod personal_voice;
pub mod market;

const LIMIT:usize=1024*1024;
const OWNER:&str="io.dominion.plugin.owner";
const IDENT:&str="io.dominion.plugin.id";
const TITLE:&str="io.dominion.plugin.name";
const SOURCE:&str="io.dominion.plugin.source";
const NET:&str="io.dominion.plugin.network";
const PORT:&str="io.dominion.plugin.port";
const MAX_PLUGINS:usize=32;

#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Install{
    id:String,name:String,image:String,network:bool,port:Option<u16>,confirmed:bool,
}

fn valid_id(id:&str)->bool{
    uuid::Uuid::parse_str(id).is_ok_and(|u|u.to_string()==id)
}
fn valid_image(image:&str)->bool{
    if image.len()>255 || image.is_empty() || !image.as_bytes()[0].is_ascii_alphanumeric()
        || !image.bytes().all(|b|b.is_ascii_alphanumeric()||matches!(b,b'.'|b'_'|b'-'|b'/'|b':'|b'@')){return false}
    let (repo,digest)=image.split_once('@').unwrap_or((image,""));
    if image.contains('@') && (!digest.starts_with("sha256:")||digest.len()!=71
        || !digest[7..].bytes().all(|b|b.is_ascii_hexdigit())){return false}
    let leaf=repo.rsplit('/').next().unwrap_or("");
    if repo.split('/').any(|p|p.is_empty()||p=="."||p==".."){return false}
    // Explicit tag or digest; never silently choose a moving latest tag.
    let (name,tag)=leaf.split_once(':').unwrap_or((leaf,""));
    !name.is_empty() && (image.contains('@') || !tag.is_empty())
        && !tag.contains(':') && tag!="latest"
}
fn owner(state:&AppState)->Result<String,String>{
    let path=std::fs::canonicalize(&state.config.data_dir).map_err(|e|e.to_string())?;
    let digest=Sha256::digest(format!("{}:{}",path.display(),state.config.port).as_bytes());
    Ok(format!("{digest:x}")[..20].to_owned())
}
fn container_name(owner:&str,id:&str)->String{format!("dominion-{owner}-{id}")}

fn local_socket()->Result<String,String>{
    #[cfg(windows)]
    {return Ok("npipe:////./pipe/docker_engine".into());}
    #[cfg(unix)]
    {
        let mut paths=Vec::new();
        if let Some(home)=std::env::var_os("HOME"){
            let home=PathBuf::from(home);
            paths.push(home.join(".docker/run/docker.sock"));
            paths.push(home.join(".colima/default/docker.sock"));
        }
        paths.push(PathBuf::from("/var/run/docker.sock"));
        use std::os::unix::fs::FileTypeExt;
        for path in paths{
            if std::fs::metadata(&path).is_ok_and(|m|m.file_type().is_socket()){
                return Ok(format!("unix://{}",path.display()));
            }
        }
        Err("로컬 Docker가 실행 중이지 않습니다. Docker Desktop 또는 Docker Engine을 실행하세요.".into())
    }
    #[cfg(not(any(unix,windows)))]
    {Err("지원하지 않는 Docker 실행 환경입니다.".into())}
}
fn docker_bin()->PathBuf{
    let mut paths=vec![PathBuf::from("/usr/local/bin/docker"),PathBuf::from("/opt/homebrew/bin/docker")];
    if let Some(home)=std::env::var_os("HOME"){paths.push(PathBuf::from(home).join(".docker/bin/docker"));}
    paths.into_iter().find(|p|p.is_file()).unwrap_or_else(||PathBuf::from("docker"))
}
async fn drain<R:AsyncRead+Unpin>(mut r:R)->std::io::Result<(Vec<u8>,bool)>{
    let mut out=Vec::new();let mut buf=[0u8;8192];let mut truncated=false;
    loop{
        let n=r.read(&mut buf).await?;if n==0{break}
        let keep=n.min(LIMIT.saturating_sub(out.len()));
        out.extend_from_slice(&buf[..keep]);truncated|=keep<n;
    }
    Ok((out,truncated))
}
async fn docker(args:&[String],seconds:u64)->Result<(String,String,bool),String>{
    docker_with_token(args,seconds,None).await
}
async fn docker_with_token(args:&[String],seconds:u64,token:Option<&str>)->Result<(String,String,bool),String>{
    let socket=local_socket()?;
    let mut command=Command::new(docker_bin());
    command.env_remove("DOMINION_BRIDGE_TOKEN");
    if let Some(value)=token{command.env("DOMINION_BRIDGE_TOKEN",value);}
    let mut child=command.arg("--host").arg(socket)
        .env_remove("DOCKER_HOST").env_remove("DOCKER_CONTEXT").env_remove("DOCKER_TLS_VERIFY").env_remove("DOCKER_CERT_PATH").env_remove("DOCKER_TLS")
        .args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped())
        .kill_on_drop(true).spawn().map_err(|e|format!("Docker 실행 실패: {e}"))?;
    let stdout=child.stdout.take().ok_or("Docker stdout unavailable")?;
    let stderr=child.stderr.take().ok_or("Docker stderr unavailable")?;
    let (status,(out,a),(err,b))=tokio::time::timeout(Duration::from_secs(seconds),async{
        tokio::try_join!(child.wait(),drain(stdout),drain(stderr))
    }).await.map_err(|_|"Docker 응답 시간 초과. 새로고침하여 실제 상태를 확인하세요.".to_owned())?
        .map_err(|e|e.to_string())?;
    let out=String::from_utf8_lossy(&out).into_owned();
    let err=String::from_utf8_lossy(&err).into_owned();
    if !status.success(){return Err(format!("Docker: {}",err.chars().take(1800).collect::<String>()))}
    Ok((out,err,a||b))
}
async fn call(args:&[&str],seconds:u64)->Result<String,String>{
    let (out,_,truncated)=docker(&args.iter().map(|s|s.to_string()).collect::<Vec<_>>(),seconds).await?;
    if truncated{return Err("Docker 응답 크기 제한을 초과했습니다.".into())}
    Ok(out)
}
fn container_id(s:&str)->bool{s.len()==64&&s.bytes().all(|b|b.is_ascii_hexdigit())}
fn project(raw:&Value,scope:&str)->Option<Value>{
    let labels=&raw["Config"]["Labels"];
    let id=labels[IDENT].as_str()?;
    let docker_id=raw["Id"].as_str()?;
    let expected_name=format!("/{}",container_name(scope,id));
    if labels[OWNER].as_str()!=Some(scope)||!valid_id(id)||!container_id(docker_id)
        ||raw["Name"].as_str()!=Some(expected_name.as_str()){return None}
    let ports=raw["NetworkSettings"]["Ports"].as_object().into_iter().flat_map(|map|map.values())
        .filter_map(Value::as_array).flatten().filter_map(|v|{
            if v["HostIp"]!="127.0.0.1"{return None}
            let p=v["HostPort"].as_str()?.parse::<u16>().ok()?;
            if p==0{return None}Some(p)
        }).collect::<Vec<_>>();
    let volume=raw["Mounts"].as_array().and_then(|m|m.iter().find(|m|m["Type"]=="volume"&&m["Destination"]=="/data")).and_then(|m|m["Name"].as_str());
    Some(json!({"id":id,"name":labels[TITLE],"image":labels[SOURCE],"imageId":raw["Image"],"dataVolume":volume,
        "state":raw["State"]["Status"],"exitCode":raw["State"]["ExitCode"],"running":raw["State"]["Running"],
        "network":labels[NET]=="true","requestedPort":labels[PORT].as_str().and_then(|p|p.parse::<u16>().ok()),"ports":ports,"containerId":docker_id}))
}
async fn inspect(id:&str,scope:&str)->Result<Option<Value>,String>{
    if !valid_id(id){return Err("Invalid plugin identity".into())}
    let name=container_name(scope,id);
    let filter=format!("name=^/{name}$");
    let ids=call(&["container","ls","-a","--no-trunc","--filter",&filter,"--format","{{.ID}}"],12).await?;
    let ids=ids.lines().filter(|s|!s.is_empty()).collect::<Vec<_>>();
    if ids.is_empty(){return Ok(None)}
    if ids.len()!=1||!container_id(ids[0]){return Err("Ambiguous plugin identity".into())}
    let raw:Value=serde_json::from_str(&call(&["container","inspect",ids[0]],12).await?).map_err(|e|e.to_string())?;
    let row=raw.as_array().and_then(|a|a.first()).and_then(|r|project(r,scope)).ok_or("Foreign container or invalid plugin metadata")?;
    Ok(Some(row))
}
async fn list_owned(scope:&str)->Result<Vec<Value>,String>{
    let filter=format!("label={OWNER}={scope}");
    let out=call(&["container","ls","-a","--no-trunc","--filter",&filter,"--format","{{.ID}}"],12).await?;
    let ids=out.lines().filter(|x|!x.is_empty()).collect::<Vec<_>>();
    if ids.len()>MAX_PLUGINS||ids.iter().any(|s|!container_id(s)){return Err("Invalid or oversized plugin inventory".into())}
    if ids.is_empty(){return Ok(vec![])}
    let mut args=vec!["container","inspect"];args.extend(ids);
    let raw:Vec<Value>=serde_json::from_str(&call(&args,15).await?).map_err(|e|e.to_string())?;
    raw.iter().map(|r|project(r,scope).ok_or_else(||"Invalid plugin ownership".into())).collect()
}
fn volume_name(scope:&str,id:&str)->String{format!("{}-data",container_name(scope,id))}
fn project_volume(raw:&Value,scope:&str)->Option<Value>{
    let labels=&raw["Labels"];
    let id=labels[IDENT].as_str()?;
    let expected=volume_name(scope,id);
    if labels[OWNER].as_str()!=Some(scope)||!valid_id(id)||raw["Name"].as_str()!=Some(expected.as_str())
        ||raw["Driver"]!="local"||raw["Options"].as_object().is_some_and(|o|!o.is_empty()){return None}
    Some(json!({"id":id,"name":labels[TITLE],"image":labels[SOURCE],"network":labels[NET]=="true",
        "requestedPort":labels[PORT].as_str().and_then(|p|p.parse::<u16>().ok()),
        "state":"retained","running":false,"ports":[],"exitCode":null,"imageId":null}))
}
async fn volume_rows(scope:&str)->Result<Vec<Value>,String>{
    let filter=format!("label={OWNER}={scope}");
    let out=call(&["volume","ls","--filter",&filter,"--format","{{.Name}}"],12).await?;
    let names=out.lines().filter(|s|!s.is_empty()).collect::<Vec<_>>();
    if names.len()>MAX_PLUGINS{return Err("Retained volume limit exceeded".into())}
    if names.is_empty(){return Ok(vec![])}
    let prefix=format!("dominion-{scope}-");
    if names.iter().any(|n|!n.strip_prefix(&prefix).and_then(|x|x.strip_suffix("-data")).is_some_and(valid_id)){
        return Err("Invalid managed volume identity".into())
    }
    let mut args=vec!["volume","inspect"];args.extend(names);
    let raw:Vec<Value>=serde_json::from_str(&call(&args,15).await?).map_err(|e|e.to_string())?;
    raw.iter().map(|r|project_volume(r,scope).ok_or_else(||"Invalid managed volume ownership".into())).collect()
}
async fn ensure_volume(scope:&str,input:&Install)->Result<(),String>{
    let name=volume_name(scope,&input.id);
    let filter=format!("name={name}");
    let out=call(&["volume","ls","--filter",&filter,"--format","{{.Name}}"],12).await?;
    if !out.lines().any(|s|s==name){
        let args=vec!["volume".into(),"create".into(),"--driver".into(),"local".into(),
            "--name".into(),name.clone(),"--label".into(),format!("{OWNER}={scope}"),"--label".into(),format!("{IDENT}={}",input.id),
            "--label".into(),format!("{TITLE}={}",input.name),"--label".into(),format!("{SOURCE}={}",input.image),
            "--label".into(),format!("{NET}={}",input.network),"--label".into(),format!("{PORT}={}",input.port.map(|p|p.to_string()).unwrap_or_default())];
        docker(&args,20).await?;
    }
    let raw:Vec<Value>=serde_json::from_str(&call(&["volume","inspect",&name],12).await?).map_err(|e|e.to_string())?;
    if raw.first().and_then(|r|project_volume(r,scope)).is_none(){return Err("Foreign volume or unsupported volume driver".into())}
    Ok(())
}
async fn inventory(scope:&str)->Result<Vec<Value>,String>{
    let mut installed=list_owned(scope).await?;
    for row in volume_rows(scope).await?{
        if !installed.iter().any(|p|p["id"]==row["id"]){installed.push(row)}
    }
    Ok(installed)
}
fn creation_args(scope:&str,input:&Install,image_id:&str)->Vec<String>{creation_args_for_volume(scope,input,image_id,&volume_name(scope,&input.id))}
fn creation_args_for_volume(scope:&str,input:&Install,image_id:&str,volume:&str)->Vec<String>{
    let name=container_name(scope,&input.id);
    let mut args=vec!["container".into(),"create".into(),"--name".into(),name.clone(),
        "--label".into(),format!("{OWNER}={scope}"),"--label".into(),format!("{IDENT}={}",input.id),
        "--label".into(),format!("{TITLE}={}",input.name),"--label".into(),format!("{SOURCE}={}",input.image),
        "--label".into(),format!("{NET}={}",input.network),"--label".into(),format!("{PORT}={}",input.port.map(|p|p.to_string()).unwrap_or_default()),
        "--pull=never".into(),"--restart=no".into(),"--cap-drop=ALL".into(),
        "--security-opt=no-new-privileges:true".into(),"--pids-limit=256".into(),
        "--memory=2g".into(),"--cpus=2".into(),"--read-only".into(),"--tmpfs".into(),"/tmp:rw,nosuid,nodev,size=128m".into(),
        "--log-driver=local".into(),"--log-opt=max-size=10m".into(),"--log-opt=max-file=3".into(),
        "--network".into(),if input.network{"bridge".into()}else{"none".into()},
        "--mount".into(),format!("type=volume,source={volume},target=/data{}",if package::local_tag(&input.image)||recipe::voice_tag(&input.image){""}else{",volume-nocopy"})];
    if let Some(port)=input.port{args.extend(["--publish".into(),format!("127.0.0.1::{port}/tcp")]);}
    args.push(image_id.into());args
}
async fn create_managed(state:&AppState,scope:&str,input:&Install,image_id:&str)->Result<(),String>{
    let volume=data::active(state,scope,&input.id)?;
    create_managed_in(state,scope,input,image_id,&volume).await
}
async fn create_managed_in(state:&AppState,scope:&str,input:&Install,image_id:&str,volume:&str)->Result<(),String>{
    data::verify(scope,&input.id,volume).await?;
    let mut args=creation_args_for_volume(scope,input,image_id,volume);
    if recipe::voice_tag(&input.image){
        let token=recipe::bridge_token(state,&input.id).ok_or("Voice bridge credential missing")?;
        let pos=args.len()-1;args.splice(pos..pos,["--env".into(),"DOMINION_BRIDGE_TOKEN".into()]);
        docker_with_token(&args,45,Some(&token)).await?;
    }else{docker(&args,45).await?;}
    Ok(())
}
fn bad(message:&str)->Response{error(StatusCode::BAD_REQUEST,message)}
fn failed(message:&str)->Response{error(StatusCode::BAD_GATEWAY,message)}
pub async fn list(State(state):State<AppState>)->Response{
    let scope=match owner(&state){Ok(s)=>s,Err(e)=>return failed(&e)};
    match inventory(&scope).await{
        Ok(mut items)=>{
            for row in &mut items { data::attach(&state,row); versions::attach(&state,row); package::attach(&state,row); scheduled::attach(&state,row); }
            Json(json!({"available":true,"items":items})).into_response()
        },
        Err(e)=>Json(json!({"available":false,"items":null,"error":e})).into_response()
    }
}
pub async fn install(State(state):State<AppState>,Json(input):Json<Install>)->Response{
    if recipe::voice_tag(&input.image)||package::local_tag(&input.image){return bad("로컬 빌드 프로그램은 전용 설치 버튼을 이용하세요.")}
    if !input.confirmed||!valid_id(&input.id)||!valid_image(&input.image)||input.name.trim().is_empty()
        ||input.name.len()>120||input.name.chars().any(char::is_control)
        ||input.port==Some(0)||input.port.is_some()&&!input.network{
        return bad("플러그인 이름·명시적 이미지 버전·네트워크 설정과 설치 승인을 확인하세요.");
    }
    let _lock=match state.integrations.plugin_mutations.try_lock(){Ok(g)=>g,Err(_)=>return error(StatusCode::CONFLICT,"다른 플러그인 변경이 진행 중입니다.")};
    let scope=match owner(&state){Ok(s)=>s,Err(e)=>return failed(&e)};
    let result:Result<Value,String>=async{
        if versions::pending(&state,&input.id){return Err("진행 중인 버전 변경을 먼저 복구하세요.".into())}
        if let Some(existing)=inspect(&input.id,&scope).await?{
            if existing["image"]==input.image && existing["name"]==input.name && existing["network"]==input.network && existing["requestedPort"]==json!(input.port){
                return Ok(existing)
            }
            return Err("같은 설치 요청 ID가 이미 다른 설정에 사용되었습니다.".into())
        }
        let rows=inventory(&scope).await?;
        if rows.len()>=MAX_PLUGINS&&!rows.iter().any(|r|r["id"]==input.id){return Err("플러그인 등록 한도에 도달했습니다.".into())}
        call(&["image","pull","--quiet",&input.image],300).await?;
        let image_id=call(&["image","inspect","--format","{{.Id}}",&input.image],15).await?;
        let image_id=image_id.trim();
        if !image_id.strip_prefix("sha256:").is_some_and(container_id){return Err("Invalid immutable image identity".into())}
        ensure_volume(&scope,&input).await?;
        // Do not launch at install time. A separate owner action starts it.
        create_managed(&state,&scope,&input,image_id).await?;
        let volume=data::active(&state,&scope,&input.id)?;
        data::bind(&state,&scope,&input,&volume)?;
        inspect(&input.id,&scope).await?.ok_or_else(||"설치 결과를 확인하지 못했습니다. 새로고침하세요.".into())
    }.await;
    match result{Ok(row)=>Json(row).into_response(),Err(e)=>failed(&e)}
}
async fn mutate(state:AppState,id:String,action:&str)->Response{mutate_checked(state,id,action,None).await}
async fn mutate_checked(state:AppState,id:String,action:&str,expected:Option<&str>)->Response{
    let _lock=match state.integrations.plugin_mutations.try_lock(){Ok(g)=>g,Err(_)=>return error(StatusCode::CONFLICT,"다른 플러그인 변경이 진행 중입니다.")};
    let scope=match owner(&state){Ok(s)=>s,Err(e)=>return failed(&e)};
    let row=match inspect(&id,&scope).await{Ok(Some(r))=>r,Ok(None)=>return error(StatusCode::NOT_FOUND,"플러그인이 없습니다."),Err(e)=>return failed(&e)};
    let cid=row["containerId"].as_str().unwrap_or_default();
    if !container_id(cid){return failed("Invalid container identity")}
    if expected.is_some_and(|e|e!=cid){return error(StatusCode::CONFLICT,"승인 대상 컨테이너가 변경되었습니다.")}
    if matches!(action,"start"|"remove")&&versions::pending(&state,&id){return error(StatusCode::CONFLICT,"진행 중인 버전 변경을 먼저 복구하세요.")}
    if action=="start"{
        let active=match data::active(&state,&scope,&id){Ok(v)=>v,Err(e)=>return failed(&e)};
        if row["dataVolume"]!=active{return error(StatusCode::CONFLICT,"실행 데이터와 저장된 데이터 연결이 다릅니다. 복구가 필요합니다.")}
    }
    if action=="remove"&&row["running"]==true{return error(StatusCode::CONFLICT,"실행 중인 플러그인은 먼저 중지하세요.")}
    let args=match action{
        "start"=>vec!["container","start",cid],
        "stop"=>vec!["container","stop","--time","10",cid],
        "remove"=>vec!["container","rm",cid],
        _=>return bad("Invalid action")
    };
    if let Err(e)=call(&args,30).await{return failed(&e)}
    match inspect(&id,&scope).await{
        Ok(None) if action=="remove"=>Json(json!({"removed":true,"dataRetained":true,"volume":row["dataVolume"]})).into_response(),
        Ok(Some(r)) if action!="remove"=>Json(r).into_response(),
        Ok(_)=>failed("변경 결과가 예상과 다릅니다. 새로고침하세요."),
        Err(e)=>failed(&e)
    }
}
pub async fn start(State(s):State<AppState>,Path(id):Path<String>)->Response{mutate(s,id,"start").await}
pub async fn stop(State(s):State<AppState>,Path(id):Path<String>)->Response{mutate(s,id,"stop").await}
pub async fn remove(State(s):State<AppState>,Path(id):Path<String>)->Response{mutate(s,id,"remove").await}
pub async fn logs(State(state):State<AppState>,Path(id):Path<String>)->Response{
    let scope=match owner(&state){Ok(s)=>s,Err(e)=>return failed(&e)};
    let row=match inspect(&id,&scope).await{Ok(Some(r))=>r,Ok(None)=>return error(StatusCode::NOT_FOUND,"플러그인이 없습니다."),Err(e)=>return failed(&e)};
    let cid=row["containerId"].as_str().unwrap_or_default();
    match docker(&["container".into(),"logs".into(),"--tail".into(),"100".into(),cid.into()],15).await{
        Ok((stdout,stderr,truncated))=>Json(json!({"stdout":stdout,"stderr":stderr,"truncated":truncated})).into_response(),
        Err(e)=>failed(&e)
    }
}
pub(crate) async fn discover_images(query:&str)->Result<Vec<Value>,String>{
    let out=call(&["search","--limit","20","--format","{{json .}}","--",query],20).await?;
    let mut rows=Vec::new();
    for line in out.lines().take(20){
        let r:Value=serde_json::from_str(line).map_err(|e|e.to_string())?;
        let Some(name)=r["Name"].as_str()else{continue};
        if !valid_image(&format!("{name}:1")){continue}
        let path=if name.contains('/') {format!("r/{name}")}else{format!("_/{name}")};
        rows.push(json!({"id":format!("docker:{name}"),"source":"docker","name":name,"kind":"repository",
            "version":null,"description":r["Description"].as_str().unwrap_or("").chars().take(500).collect::<String>(),
            "license":"","url":format!("https://hub.docker.com/{path}"),"compatibility":"unverified"}));
    }
    Ok(rows)
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn image_must_be_explicit_and_not_cli_syntax(){
        for s in ["alpine:3.22","ghcr.io/org/app:v1.2","localhost:5000/x:1"]{assert!(valid_image(s),"{s}")}
        for s in ["alpine","alpine:latest","--privileged","a:1;echo","a/../b:1","a@sha256:bad","a:1\n"]{assert!(!valid_image(s),"{s}")}
    }
    #[test]
    fn create_is_stopped_bounded_and_no_host_mounts(){
        let i=Install{id:"01234567-89ab-4def-8123-456789abcdef".into(),name:"Voice".into(),image:"app:1".into(),network:false,port:None,confirmed:true};
        let args=creation_args("scope",&i,"sha256:abc");
        assert_eq!(&args[..2],&["container","create"]);
        for flag in ["--pull=never","--restart=no","--read-only","--cap-drop=ALL","--security-opt=no-new-privileges:true","none"]{assert!(args.iter().any(|a|a==flag))}
        assert!(!args.iter().any(|s|s.contains("type=bind")||s.contains("docker.sock")||s=="--privileged"));
        assert_eq!(args.last().unwrap(),"sha256:abc");
    }
    #[test]
    fn retained_data_requires_owned_local_volume(){
        let id="01234567-89ab-4def-8123-456789abcdef";
        let mut v=json!({"Name":volume_name("scope",id),"Driver":"local","Labels":{(OWNER):"scope",(IDENT):id}});
        assert_eq!(project_volume(&v,"scope").unwrap()["state"],"retained");
        assert!(project_volume(&v,"other").is_none());
        v["Options"]=json!({"device":"/host","type":"none","o":"bind"});
        assert!(project_volume(&v,"scope").is_none());
    }
    #[test]
    fn foreign_container_cannot_be_projected(){
        let id="01234567-89ab-4def-8123-456789abcdef";
        let mut r=json!({"Id":"a".repeat(64),"Name":format!("/{}",container_name("scope",id)),"Config":{"Labels":{(OWNER):"scope",(IDENT):id}}});
        assert!(project(&r,"scope").is_some());
        assert!(project(&r,"other").is_none());
        r["Name"]=json!("/different");
        assert!(project(&r,"scope").is_none());
    }
}
