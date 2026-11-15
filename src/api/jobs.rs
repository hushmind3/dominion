use crate::{engine,state::AppState};
use axum::{extract::{Path,State},http::StatusCode,response::{IntoResponse,Response},Json};
use serde_json::{json,Value};
use super::response::error;
pub async fn list(State(state):State<AppState>)->Json<Value>{Json(json!(state.store.all("job")))}
pub async fn one(State(state):State<AppState>,Path(id):Path<String>)->Response{match state.store.get("job",&id){Some(j)=>Json(j).into_response(),None=>error(StatusCode::NOT_FOUND,"작업을 찾을 수 없습니다.")}}
/// Client requests describe work, never mutable runtime status or evidence.
pub async fn create(State(state):State<AppState>,Json(body):Json<Value>)->Response{
    let kind=body["kind"].as_str().unwrap_or("prompt");let input=&body["input"];
    let clean=match kind{
        "prompt"=>{
            let Some(prompt)=input["prompt"].as_str().filter(|p|!p.trim().is_empty()&&p.len()<=64000)else{return error(StatusCode::BAD_REQUEST,"실행할 작업 지시가 필요합니다.")};
            // Never reinterpret invalid owner controls as silent defaults.
            for key in ["toolsEnabled","toolsExecutionEnabled"] {
                if input.get(key).is_some() && !input[key].is_boolean() {
                    return error(StatusCode::BAD_REQUEST,"도구 활성화 설정은 true 또는 false여야 합니다.");
                }
            }
            let mut value=json!({"prompt":prompt,
                "toolsEnabled":input["toolsEnabled"].as_bool().unwrap_or(true),
                "toolsExecutionEnabled":input["toolsExecutionEnabled"].as_bool().unwrap_or(true)});
            for key in ["agentId","providerId","model","reasoningEffort"]{if let Some(v)=input[key].as_str().filter(|s|!s.is_empty()){value[key]=json!(v);}}
            if let Some(v)=input["thinking"].as_bool(){value["thinking"]=json!(v);}
            if let Some(id)=value["agentId"].as_str(){if state.store.get("agent",id).is_none(){return error(StatusCode::BAD_REQUEST,"실행 에이전트를 찾을 수 없습니다.");}}
            value
        },
        "workflow"=>{
            let Some(id)=input["workflowId"].as_str().filter(|id|state.store.get("workflow",id).is_some())else{return error(StatusCode::BAD_REQUEST,"워크플로를 찾을 수 없습니다.")};json!({"workflowId":id,"input":input["input"]})
        },
        _=>return error(StatusCode::BAD_REQUEST,"해당 작업은 대화·회의·영상의 실행 API에서 시작하세요."),
    };
    // A retry ID never enters the worker input or model prompt. It only
    // selects an existing immutable task after an ambiguous HTTP outcome.
    if kind=="prompt" {
        if let Some(raw)=body.get("requestId"){
            let Some(key)=raw.as_str().filter(|s|
                (8..=128).contains(&s.len())
                && s.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'-'||b==b'_'))
            else {return error(StatusCode::BAD_REQUEST,
                "요청 ID는 8~128자 영문·숫자·하이픈·밑줄만 가능합니다.");};
            return match engine::jobs::enqueue_owner_prompt(&state,key,clean) {
                Ok(job)=>(StatusCode::ACCEPTED,Json(job)).into_response(),
                Err(message)=>error(StatusCode::CONFLICT,&message),
            };
        }
    }
    match engine::jobs::enqueue(&state,kind,clean){Some(job)=>(StatusCode::ACCEPTED,Json(job)).into_response(),None=>error(StatusCode::INTERNAL_SERVER_ERROR,"작업을 대기열에 저장할 수 없습니다.")}
}
pub async fn cancel(State(state):State<AppState>,Path(id):Path<String>)->Response{
    let Some(job)=state.store.get("job",&id)else{return error(StatusCode::NOT_FOUND,"작업이 없습니다.")};
    if job["kind"]=="rsi_evolve" {
        // Also clean up descendants when a client repeats Stop on a terminal parent.
        engine::jobs::cancel_rsi_children(&state,&id);
        if job["status"]=="cancelled" {return Json(job).into_response();}
    }
    if !matches!(job["status"].as_str(),Some("queued"|"running")){return error(StatusCode::CONFLICT,"진행 중인 작업만 중단할 수 있습니다.");}
    if let Some(session)=job["sessionId"].as_str(){state.cancel_session(session);}
    match state.store.patch_if("job",&id,"status",&job["status"],&json!({"status":"cancelled","cancelRequested":true,"finishedAt":chrono::Utc::now().to_rfc3339()})){
        Some(saved)=>{state.bus.record("job",&saved);Json(saved).into_response()},None=>state.store.get("job",&id).map(|j|Json(j).into_response()).unwrap_or_else(||error(StatusCode::CONFLICT,"작업 상태가 변경되었습니다."))
    }
}


#[cfg(test)]
mod owner_request_tests {
    use super::*;
    use crate::{config::Config,db::Store,vault::Vault};

    fn state()->(AppState,std::path::PathBuf){
        let dir=std::env::temp_dir().join(format!("dominion-owner-once-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config=Config{port:0,data_dir:dir.clone(),web_dir:dir.join("web"),workspace:dir.clone()};
        let store=Store::open(&config.db_path()).unwrap();
        let vault=Vault::open(&config.key_path());
        (AppState::new(config,store,vault),dir)
    }

    #[tokio::test]
    async fn same_key_and_payload_reuse_one_job_but_changed_payload_conflicts(){
        let (state,dir)=state();
        let request=|text:&str|json!({"kind":"prompt","requestId":"sdk-owner-retry-2026",
            "input":{"prompt":text,"toolsEnabled":true,"toolsExecutionEnabled":false}});
        for _ in 0..3 {
            let reply=create(State(state.clone()),Json(request("edit file"))).await;
            assert_eq!(reply.status(),StatusCode::ACCEPTED);
        }
        let queued=state.store.all("job");
        assert_eq!(queued.len(),1);
        assert_eq!(queued[0]["input"]["toolsExecutionEnabled"],false);
        assert_eq!(queued[0]["input"]["requestId"],Value::Null);
        let different=create(State(state.clone()),Json(request("delete file"))).await;
        assert_eq!(different.status(),StatusCode::CONFLICT);
        assert_eq!(state.store.count("job"),1);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test(flavor="multi_thread",worker_threads=4)]
    async fn concurrent_owner_retries_are_an_atomic_sqlite_insert(){
        let (state,dir)=state();
        let tasks=(0..12).map(|_|{
            let state=state.clone();
            tokio::spawn(async move {
                create(State(state),Json(json!({"kind":"prompt",
                    "requestId":"parallel-owner-retry-2026",
                    "input":{"prompt":"fix tests","toolsExecutionEnabled":false}})))
                    .await.status()
            })
        }).collect::<Vec<_>>();
        for task in tasks {assert_eq!(task.await.unwrap(),StatusCode::ACCEPTED);}
        assert_eq!(state.store.count("job"),1);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn malformed_owner_controls_and_retry_ids_are_rejected_before_enqueue(){
        let (state,dir)=state();
        for key in [json!("short"),json!("contains spaces"),json!(42),Value::Null] {
            let reply=create(State(state.clone()),Json(json!({"kind":"prompt",
                "requestId":key,"input":{"prompt":"test"}}))).await;
            assert_eq!(reply.status(),StatusCode::BAD_REQUEST);
        }
        for invalid in [json!("false"),json!(1),Value::Null]{
            let reply=create(State(state.clone()),Json(json!({"kind":"prompt",
                "input":{"prompt":"test","toolsExecutionEnabled":invalid}}))).await;
            assert_eq!(reply.status(),StatusCode::BAD_REQUEST);
        }
        assert_eq!(state.store.count("job"),0);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
