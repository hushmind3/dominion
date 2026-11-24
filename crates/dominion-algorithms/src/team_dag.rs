//! Sibling dependency evaluation; host supplies authoritative job/session records.
use serde_json::Value;
use std::collections::HashSet;
const MAX_EDGES:usize=16;

#[derive(Debug,PartialEq,Eq)]
pub enum Gate { Ready, Waiting, Blocked(String) }

/// Reject foreign, missing, repeated, future and self dependencies before
/// reserving budget or preparing isolated worktrees.
pub fn validate_edges(
    mut get_job:impl FnMut(&str)->Option<Value>,parent:&str,new_job:&str,value:&Value,
)->Result<Vec<String>,String>{
    if value.is_null(){return Ok(Vec::new());}
    let ids=value.as_array().ok_or("dependsOnJobIds는 작업 ID 배열이어야 합니다.")?;
    if ids.len()>MAX_EDGES{return Err("작업 의존성은 최대 16개입니다.".into());}
    let mut unique=HashSet::new();
    let mut out=Vec::with_capacity(ids.len());
    for raw in ids{
        let id=raw.as_str().filter(|x|!x.is_empty()&&x.len()<=256)
            .ok_or("작업 의존성 ID 형식이 잘못되었습니다.")?;
        if id==new_job||!unique.insert(id){return Err("자기 참조 또는 중복된 작업 의존성입니다.".into());}
        let prior=get_job(id).ok_or("존재하지 않는 선행 작업입니다.")?;
        if prior["kind"]!="prompt"||prior["input"]["delegation"]!=true
            ||prior["input"]["parentSessionId"]!=parent {
            return Err("다른 사용 세션이나 비위임 작업을 선행 작업으로 지정할 수 없습니다.".into());
        }
        out.push(id.to_string());
    }
    Ok(out)
}

/// All edges must resolve inside the same parent session, and an isolated
/// writer is consumable only after its result diff has been frozen.
pub fn gate(mut get_job:impl FnMut(&str)->Option<Value>,mut get_session:impl FnMut(&str)->Option<Value>,job:&Value)->Gate{
    let Some(dependencies)=job["input"]["dependsOnJobIds"].as_array() else {
        return if job["input"]["dependsOnJobIds"].is_null(){
            Gate::Ready
        }else{Gate::Blocked("의존성 ID 목록이 손상되었습니다.".into())};
    };
    if dependencies.is_empty(){return Gate::Ready;}
    let Some(parent)=job["input"]["parentSessionId"].as_str() else{
        return Gate::Blocked("상위 세션 식별자가 없습니다.".into());
    };
    for raw in dependencies{
        let Some(id)=raw.as_str() else{
            return Gate::Blocked("선행 작업 ID가 문자열이 아닙니다.".into());
        };
        if job["id"].as_str()==Some(id){
            return Gate::Blocked("작업이 자신을 기다리는 순환입니다.".into());
        }
        let Some(dep)=get_job(id) else{
            return Gate::Blocked(format!("선행 작업이 삭제되었습니다: {id}"));
        };
        if dep["kind"]!="prompt"||dep["input"]["delegation"]!=true
            ||dep["input"]["parentSessionId"]!=parent {
            return Gate::Blocked(format!("선행 작업 접근 권한이 없습니다: {id}"));
        }
        match dep["status"].as_str(){
            Some("completed")=>{
                if let Some(session)=dep["input"]["sessionId"].as_str()
                    .and_then(|session|get_session(session)) {
                    if session["delegationWorkspace"].is_object()
                        && dep["delegationPatchSha256"].as_str().is_none(){
                        return Gate::Waiting;
                    }
                }else{
                    return Gate::Blocked(format!("선행 작업 세션이 없습니다: {id}"));
                }
            }
            Some("queued"|"running")=>return Gate::Waiting,
            Some("failed"|"cancelled"|"needs_review")=>{
                return Gate::Blocked(format!("선행 작업이 완료되지 않았습니다: {id}"));
            }
            _=>return Gate::Blocked(format!("알 수 없는 선행 작업 상태: {id}")),
        }
    }
    Gate::Ready
}


#[cfg(test)]
mod tests{
 use super::*;
 use serde_json::json;
 #[test]
 fn isolated_writer_requires_frozen_patch_before_consumption(){
  let dependent=json!({"id":"b","input":{"parentSessionId":"parent","dependsOnJobIds":["a"]}});
  let mut prior=json!({"id":"a","kind":"prompt","status":"completed","input":{"delegation":true,"parentSessionId":"parent","sessionId":"worker"}});
  let session=json!({"delegationWorkspace":{}});
  assert_eq!(gate(|_|Some(prior.clone()),|_|Some(session.clone()),&dependent),Gate::Waiting);
  prior["delegationPatchSha256"]=json!("sha");
  assert_eq!(gate(|_|Some(prior.clone()),|_|Some(session.clone()),&dependent),Gate::Ready);
 }
}
