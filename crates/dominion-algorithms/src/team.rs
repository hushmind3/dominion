//! Validate model-authored team plans against an owner-authorized roster.
//! This does not launch workers, reserve budgets or approve remote execution.
use serde_json::Value;
use std::collections::HashSet;
#[derive(Clone)]
pub struct Step{ pub id:String, pub agent:String, pub profile:String, pub prompt:String,
    pub depends:Vec<String>, pub external:Option<String>, pub remote:bool }

pub fn validate(plan:&Value,roster:&[Value])->Result<Vec<Step>,String>{
    let rows=plan["steps"].as_array().ok_or("steps 배열이 없습니다.")?;
    if rows.is_empty()||rows.len()>8||roster.is_empty(){return Err("팀 계획은 1~8단계와 실제 Worker가 필요합니다.".into());}
    let mut steps=Vec::<Step>::new();
    let mut keys=HashSet::<String>::new();
    for (i,item) in rows.iter().enumerate(){
        let id=item["id"].as_str().ok_or("단계 ID가 없습니다.")?;
        if id.is_empty()||id.len()>40||!id.bytes().all(|ch|
            ch.is_ascii_alphanumeric()||matches!(ch,b'-'|b'_'))||!keys.insert(id.into()){
            return Err("단계 ID가 중복되었거나 유효하지 않습니다.".into());
        }
        let profile=item["profile"].as_str().unwrap_or("general");
        if !matches!(profile,"general"|"inspection"|"implementation"|"verification"){
            return Err("지원하지 않는 Worker 역할입니다.".into());
        }
        let prompt=item["prompt"].as_str().filter(|s|!s.trim().is_empty()&&s.len()<=3000)
            .ok_or("Worker의 구체적인 작업과 검증 조건을 3000바이트 이하로 적으세요.")?;
        let agent=match item["agentId"].as_str(){
            Some(id) if !id.is_empty() => {
                if !roster.iter().any(|a|a["id"]==id){return Err("팀 외부 에이전트 지정이 거부되었습니다.".into());}
                id.to_string()
            }
            _=>roster[i%roster.len()]["id"].as_str().unwrap_or("").to_string()
        };
        let raw=item["dependsOn"].as_array();
        if raw.is_none()&&!item["dependsOn"].is_null(){return Err("dependsOn은 배열이어야 합니다.".into());}
        let mut depends=Vec::new();
        for previous in raw.map(Vec::as_slice).unwrap_or(&[]){
            let id=previous.as_str().ok_or("선행 단계 ID가 문자열이 아닙니다.")?;
            if depends.contains(&id.to_string())||!steps.iter().any(|step|step.id==id){
                return Err("단계가 미래, 중복, 순환 의존성을 참조했습니다.".into());
            }
            depends.push(id.to_string());
        }
        if depends.len()>4{return Err("단계당 의존성은 최대 4개입니다.".into());}
        let external=item["externalRuntime"].as_str().map(str::to_string);
        if external.as_deref().is_some_and(|id|
            profile!="inspection"||!matches!(id,"codex"|"claude"|"qwen"|"codex-app-server"|"qwen-acp")){
            return Err("외부 Agent는 격리된 inspection 역할에서만 실행합니다.".into());
        }
        if item.get("remoteWorker").is_some_and(|v|!v.is_boolean()){
            return Err("remoteWorker must be boolean".into());
        }
        let remote=item["remoteWorker"]==true;
        if remote&&(profile!="general"||external.is_some()){
            return Err("remote SDK worker supports general research only".into());
        }
        steps.push(Step{id:id.into(),agent,profile:profile.into(),prompt:prompt.into(),depends,external,remote});
    }
    for step in steps.iter().filter(|step|step.profile=="implementation"){
        if !steps.iter().any(|later|later.profile=="verification"
            && later.depends.contains(&step.id)){
            return Err(format!("구현 단계 {}에 연결된 독립 검증 단계가 없습니다.",step.id));
        }
    }
    Ok(steps)
}

#[cfg(test)]
mod tests {
 use super::*;
 use serde_json::json;
 #[test]
 fn team_plan_requires_a_verifier_and_rejects_foreign_agent() {
  let roster=vec![json!({"id":"a"}),json!({"id":"b"})];
  let mut plan=json!({"steps":[{"id":"write","agentId":"a","profile":"implementation","prompt":"make a file"}]});
  assert!(validate(&plan,&roster).is_err());
  plan["steps"].as_array_mut().unwrap().push(json!({"id":"verify","agentId":"b","profile":"verification","prompt":"verify file","dependsOn":["write"]}));
  assert_eq!(validate(&plan,&roster).unwrap().len(),2);
  plan["steps"][1]["agentId"]=json!("foreign");
  assert!(validate(&plan,&roster).is_err());
 }
}
