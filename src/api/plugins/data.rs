//! Copy-on-update plugin volumes. Old data is never overwritten by upgrades
//! or rollback; replacement runs against a verified new volume copy.
use super::{call,docker,volume_name,container_name,valid_id,container_id,Install,OWNER,IDENT,project_volume};
use crate::state::AppState;
use serde_json::{json,Value};
const KIND:&str="plugin_data_binding";
const COPY_OWNER:&str="io.dominion.plugin.copy.owner";
const COPY_ID:&str="io.dominion.plugin.copy.id";
const MAX_COPIES:usize=4;
const MAX_KIB:u64=2*1024*1024;
const HELPER:&str="docker.io/library/busybox:1.37.0";

fn copy_prefix(scope:&str,id:&str)->String{format!("{}-copy-",container_name(scope,id))}
fn valid_volume(scope:&str,id:&str,name:&str)->bool{
    valid_id(id)&&(name==volume_name(scope,id)||name.strip_prefix(&copy_prefix(scope,id)).is_some_and(valid_id))
}
pub(super) fn active(state:&AppState,scope:&str,id:&str)->Result<String,String>{
    if !valid_id(id){return Err("Invalid plugin identity".into())}
    let name=if let Some(r)=state.store.get(KIND,id){
        r["volume"].as_str().ok_or("Invalid data binding")?.to_owned()
    }else{volume_name(scope,id)};
    if !valid_volume(scope,id,&name){return Err("Invalid data volume identity".into())}
    Ok(name)
}
pub(super) fn attach(state:&AppState,row:&mut Value){
    let Some(id)=row["id"].as_str()else{return};
    if let Some(r)=state.store.get(KIND,id){
        if row["state"]=="retained"{
            for key in ["name","image","network","requestedPort"]{row[key]=r[key].clone();}
            row["dataVolume"]=r["volume"].clone();
        }
    }
}
pub(super) fn bind(state:&AppState,scope:&str,input:&Install,volume:&str)->Result<(),String>{
    if !valid_volume(scope,&input.id,volume){return Err("Invalid data volume identity".into())}
    let r=json!({"id":input.id,"volume":volume,"name":input.name,"image":input.image,
        "network":input.network,"requestedPort":input.port});
    state.store.put(KIND,&r).map(|_|()).ok_or("데이터 연결 정보를 저장하지 못했습니다.".into())
}
fn owned_volume(scope:&str,id:&str,name:&str,raw:&Value)->bool{
    if !valid_volume(scope,id,name)||raw["Name"]!=name||raw["Driver"]!="local"
        ||raw["Options"].as_object().is_some_and(|o|!o.is_empty()){return false}
    if name==volume_name(scope,id){
        return project_volume(raw,scope).is_some_and(|r|r["id"]==id)
    }
    raw["Labels"][COPY_OWNER]==scope&&raw["Labels"][COPY_ID]==id
        &&raw["Labels"].get(OWNER).is_none()&&raw["Labels"].get(IDENT).is_none()
}
pub(super) async fn verify(scope:&str,id:&str,name:&str)->Result<(),String>{
    if !valid_volume(scope,id,name){return Err("Invalid data volume identity".into())}
    let rows:Vec<Value>=serde_json::from_str(&call(&["volume","inspect",name],12).await?).map_err(|e|e.to_string())?;
    if rows.len()!=1||!owned_volume(scope,id,name,&rows[0]){return Err("플러그인 데이터 볼륨 소유권을 확인하지 못했습니다.".into())}
    Ok(())
}
fn helper_args(source:&str,target:Option<&str>,image:&str)->Vec<String>{
    let mut args=vec!["run".into(),"--rm".into(),"--pull=never".into(),"--network=none".into(),
        "--read-only".into(),"--cap-drop=ALL".into(),"--cap-add=DAC_OVERRIDE".into(),
        "--cap-add=CHOWN".into(),"--cap-add=FOWNER".into(),"--cap-add=FSETID".into(),
        "--security-opt=no-new-privileges:true".into(),"--pids-limit=32".into(),
        "--memory=256m".into(),"--cpus=1".into(),"--user=0:0".into(),
        "--mount".into(),format!("type=volume,source={source},target=/source,readonly,volume-nocopy")];
    if let Some(target)=target{
        args.extend(["--mount".into(),format!("type=volume,source={target},target=/target,volume-nocopy"),
            image.into(),"cp".into(),"-a".into(),"/source/.".into(),"/target/".into()]);
    }else{args.extend([image.into(),"du".into(),"-sk".into(),"/source".into()]);}
    args
}
pub(super) async fn clone_for_update(state:&AppState,scope:&str,id:&str,source:&str)->Result<String,String>{
    if active(state,scope,id)?!=source{return Err("실행 데이터 연결이 변경되었습니다.".into())}
    verify(scope,id,source).await?;
    let volume_filter=format!("volume={source}");
    if !call(&["container","ls","--filter",&volume_filter,"--format","{{.ID}}"],12).await?.trim().is_empty(){
        return Err("데이터를 사용하는 실행 중인 컨테이너를 먼저 중지하세요.".into())
    }
    let owner_filter=format!("label={COPY_OWNER}={scope}");
    let id_filter=format!("label={COPY_ID}={id}");
    let names=call(&["volume","ls","--filter",&owner_filter,"--filter",&id_filter,"--format","{{.Name}}"],12).await?;
    if names.lines().filter(|s|!s.is_empty()).count()>=MAX_COPIES{
        return Err("보존 데이터 복사본 한도(4개)에 도달했습니다. 이전 데이터를 삭제하지 않았습니다.".into())
    }
    // Pull a fixed official utility image, then execute only its exact image ID.
    call(&["image","pull","--quiet",HELPER],120).await?;
    let image=call(&["image","inspect","--format","{{.Id}}",HELPER],15).await?;
    let image=image.trim();
    if !image.strip_prefix("sha256:").is_some_and(container_id){return Err("Invalid copy helper image".into())}
    let (size,_,truncated)=docker(&helper_args(source,None,image),45).await?;
    let kib=size.split_whitespace().next().and_then(|s|s.parse::<u64>().ok()).ok_or("데이터 크기를 확인할 수 없습니다.")?;
    if truncated||kib>MAX_KIB{return Err("2GB를 초과하는 데이터는 자동 복사하지 않습니다.".into())}
    let target=format!("{}{}",copy_prefix(scope,id),uuid::Uuid::new_v4());
    let args=vec!["volume".into(),"create".into(),"--driver".into(),"local".into(),"--name".into(),target.clone(),
        "--label".into(),format!("{COPY_OWNER}={scope}"),"--label".into(),format!("{COPY_ID}={id}")];
    docker(&args,20).await?;
    verify(scope,id,&target).await?;
    // No host mount, shell interpolation or deletion. On a failed copy,
    // retain the source and leave the incomplete clone unbound.
    docker(&helper_args(source,Some(&target),image),240).await?;
    verify(scope,id,&target).await?;
    Ok(target)
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn clone_is_owned_and_source_is_readonly(){
        let id="01234567-89ab-4def-8123-456789abcdef";
        let target=format!("{}{}",copy_prefix("scope",id),id);
        assert!(valid_volume("scope",id,&target));
        assert!(!valid_volume("another",id,&target));
        let r=json!({"Name":target,"Driver":"local","Labels":{(COPY_OWNER):"scope",(COPY_ID):id}});
        assert!(owned_volume("scope",id,&target,&r));
        let args=helper_args("old",Some("new"),"sha256:exact");
        assert!(args.iter().any(|s|s.contains("source=old,target=/source,readonly")));
        assert!(args.iter().any(|s|s.contains("source=new,target=/target")));
        assert!(args.iter().any(|s|s=="--network=none"));
        assert!(!args.iter().any(|s|s.contains("type=bind")||s=="--privileged"));
    }
}
