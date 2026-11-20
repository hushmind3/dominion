//! Background extension jobs enter through the application host, rather
//! than the core job queue importing memory and messenger implementations.
use crate::{engine::{discord,learning,reflect,jobs::JobContext},state::AppState};
use serde_json::Value;
use super::{enabled,globally_enabled,Extension};

fn memory_admitted(state:&AppState,job:&Value)->bool{
    if !globally_enabled(state,Extension::Memory){return false;}
    let input=&job["input"];
    let Some(session)=input["sessionId"].as_str() else {return true};
    enabled(state,session,input["agentId"].as_str(),Extension::Memory)
}
pub async fn run(context:&JobContext,job:&Value)->Result<Value,String>{
    match job["kind"].as_str().unwrap_or_default(){
        "learn"|"reflect" if !memory_admitted(&context.state,job) =>
            Err("needs_review: 장기기억 부가기능이 비활성화되어 학습·회상 작업을 실행하지 않았습니다.".into()),
        "learn"=>learning::run(context,job).await,
        "reflect"=>reflect::run(context,job).await,
        "discord" if !globally_enabled(&context.state,Extension::Channels) =>
            Err("needs_review: 메시지 채널 부가기능이 비활성화되어 외부 전송을 수행하지 않았습니다.".into()),
        "discord"=>discord::run(context,job).await,
        _=>Err("등록되지 않은 부가기능 작업입니다.".into())
    }
}
