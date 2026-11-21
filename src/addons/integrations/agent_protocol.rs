//! Bounded local JSON-RPC bridge. Never a second native Harness.
//! The external process owns its own loop. Owner approvals are routed through
//! DOMINION's existing durable approval store and live one-shot decision API.
use super::*;
use std::sync::Arc;
use tokio::{
    io::{AsyncBufReadExt,AsyncWriteExt,BufReader},
    process::ChildStdin,
    sync::{Mutex,oneshot},
    time::{interval,timeout},
};
type Wire=Arc<Mutex<ChildStdin>>;
const MAX_LINE:usize=256*1024;
const MAX_NOTIFICATIONS:usize=2_000;
const MAX_TEXT:usize=12_000;

async fn send(writer:&Wire,kind:ExternalAgent,mut msg:Value)->Result<(),String>{
    if kind==ExternalAgent::CodexServer{msg.as_object_mut().map(|m|m.remove("jsonrpc"));}
    let mut bytes=serde_json::to_vec(&msg).map_err(|e|e.to_string())?;
    if bytes.len()>MAX_LINE{return Err("external RPC frame too large".into());}
    bytes.push(b'\n');
    let mut stdin=writer.lock().await;
    stdin.write_all(&bytes).await.map_err(|e|e.to_string())?;
    stdin.flush().await.map_err(|e|e.to_string())
}
async fn request(writer:&Wire,kind:ExternalAgent,id:u64,method:&str,params:Value)
    ->Result<(),String>{
    send(writer,kind,json!({"jsonrpc":"2.0","id":id,
        "method":method,"params":params})).await
}
async fn notify(writer:&Wire,kind:ExternalAgent,method:&str,params:Value)
    ->Result<(),String>{
    send(writer,kind,json!({"jsonrpc":"2.0","method":method,"params":params})).await
}
fn response_for_permission(kind:ExternalAgent,method:&str,options:&Value,allow:bool)->Value{
    if kind==ExternalAgent::QwenAcp{
        let option=options.as_array().into_iter().flatten().find(|o|{
            let kind=o["kind"].as_str().unwrap_or("");
            let id=o["optionId"].as_str().unwrap_or("").to_ascii_lowercase();
            let name=o["name"].as_str().unwrap_or("").to_ascii_lowercase();
            // v1 ACP uses kind=allow_once, while Qwen can advertise a
            // plain {optionId:"allow",name:"Allow"} instead. Do not
            // accidentally select allow_always/session/persistent grants.
            allow && (kind=="allow_once"
                || matches!(id.as_str(),"allow"|"allow_once"|"proceed_once")
                || matches!(name.as_str(),"allow"|"allow once"))
                &&o["optionId"].is_string()
        });
        let outcome=option.map(|o|json!({"outcome":"selected","optionId":o["optionId"]}))
            .unwrap_or(json!({"outcome":"cancelled"}));
        json!({"outcome":outcome})
    }else if matches!(method,"item/commandExecution/requestApproval"
        |"item/fileChange/requestApproval"){
        json!({"decision":if allow{"accept"}else{"decline"}})
    }else{
        // Unsupported privilege escalation is never interpreted as permission.
        json!({"decision":"decline"})
    }
}
fn approval_method(kind:ExternalAgent,method:&str)->bool{
    if kind==ExternalAgent::QwenAcp{method=="session/request_permission"}
    else{matches!(method,"item/commandExecution/requestApproval"
        |"item/fileChange/requestApproval")}
}
async fn permission(
    context:&JobContext,kind:ExternalAgent,writer:&Wire,session:&str,
    parent_execution:Option<&str>,external_run_id:&str,
    req:&Value,
)->Result<(),String>{
    let method=req["method"].as_str().unwrap_or("");
    let rpc_id=req.get("id").cloned().ok_or("server approval without RPC id")?;
    let method=method.to_owned();
    if !approval_method(kind,&method){
        return send(writer,kind,json!({"jsonrpc":"2.0","id":rpc_id,
            "error":{"code":-32601,"message":"permission method unavailable"}})).await;
    }
    let approval_id=format!("external-approval:{}",uuid::Uuid::new_v4());
    let (tx,rx)=oneshot::channel::<bool>();
    let options=req["params"]["options"].clone();
    let pending=context.state.store.put("approval",&json!({
        "id":approval_id,"status":"pending","sessionId":session,
        "jobId":context.job_id,"executionId":parent_execution,
        "externalRunId":external_run_id,
        "tool":method,"source":"external_agent_rpc",
        "approvalKind":"external_agent","runtime":kind.key(),
        "request":req["params"],"createdAt":chrono::Utc::now().to_rfc3339()
    })).ok_or("external approval could not be persisted")?;
    context.state.approvals.lock().unwrap().insert(approval_id.clone(),tx);
    context.state.bus.approval(&pending);
    let writer=writer.clone();
    let state=context.state.clone();
    let key=approval_id.clone();
    tokio::spawn(async move{
        // Never silently grant. The existing POST /api/approvals/resolve
        // resumes this one-shot decision only after the owner explicitly acts.
        let answer=timeout(Duration::from_secs(110),rx).await;
        let allow=matches!(answer,Ok(Ok(true)));
        state.approvals.lock().unwrap().remove(&key);
        if let Some(saved)=state.store.patch_if(
            "approval",&key,"status",&json!("pending"),
            &json!({"status":"interrupted","decision":"timeout",
                "decidedAt":chrono::Utc::now().to_rfc3339()})){
            state.bus.approval(&saved);
        }
        let result=response_for_permission(kind,&method,&options,allow);
        let _=send(&writer,kind,json!({"jsonrpc":"2.0","id":rpc_id,"result":result})).await;
    });
    Ok(())
}
fn append_answer(answer:&mut String,text:&str){
    if answer.len()>=MAX_TEXT{return;}
    let remain=MAX_TEXT-answer.len();
    let excerpt=text.chars().take(remain).collect::<String>();
    answer.push_str(&excerpt);
}
fn forward(context:&JobContext,kind:ExternalAgent,session:&str,
    execution:Option<&str>,external_id:&str,method:&str,text:Option<&str>){
    let mut event=json!({"type":"external_agent_event","jobId":context.job_id,
        "sessionId":session,"externalRunId":external_id,
        "runtime":kind.key(),"method":method});
    if let Some(id)=execution{event["executionId"]=json!(id);}
    if let Some(text)=text{event["text"]=json!(text.chars().take(2000).collect::<String>());}
    context.state.bus.emit(event);
}

fn interrupt_orphan_approvals(context:&JobContext,external_id:&str){
    // A protocol disconnect invalidates all still-pending decisions, even if
    // the owner browser will later submit a stale approval button.
    for row in context.state.store.all("approval").into_iter().filter(|r|
        r["source"]=="external_agent_rpc"
        &&r["externalRunId"]==external_id
        &&r["status"]=="pending"){
        let Some(id)=row["id"].as_str()else{continue};
        context.state.approvals.lock().unwrap().remove(id);
        if let Some(saved)=context.state.store.patch_if(
            "approval",id,"status",&json!("pending"),
            &json!({"status":"interrupted","decision":"connection_closed",
                "decidedAt":chrono::Utc::now().to_rfc3339()})){
            context.state.bus.approval(&saved);
        }
    }
}

pub(super) async fn run(
    context:&JobContext,kind:ExternalAgent,session:&str,workspace:&Path,
    prompt:&str,resume:Option<&str>,generation:u64,parent:&str,
    parent_execution:Option<&str>,external_id:&str
)->Result<(String,String,Option<String>),String>{
    let (binary,argv)=kind.command(prompt);
    let mut cmd=Command::new(binary);
    cmd.args(argv);
    run_with_command(context,kind,session,workspace,prompt,resume,generation,
        parent,parent_execution,external_id,cmd).await
}
async fn run_with_command(
    context:&JobContext,kind:ExternalAgent,session:&str,workspace:&Path,
    prompt:&str,resume:Option<&str>,generation:u64,parent:&str,
    parent_execution:Option<&str>,external_id:&str,
    mut cmd:Command,
)->Result<(String,String,Option<String>),String>{
    cmd.current_dir(workspace)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    let mut child=cmd.spawn().map_err(|e|format!("외부 RPC 실행 시작 실패: {e}"))?;
    let writer=Arc::new(Mutex::new(child.stdin.take().ok_or("외부 RPC stdin이 없습니다.")?));
    let stdout=child.stdout.take().ok_or("외부 RPC stdout이 없습니다.")?;
    let mut lines=BufReader::new(stdout).lines();
    let is_acp=kind==ExternalAgent::QwenAcp;
    let init=if is_acp{json!({
        "protocolVersion":1,
        "clientCapabilities":{"fs":{"readTextFile":false,"writeTextFile":false},"terminal":false},
        "clientInfo":{"name":"dominion","version":"0.4.4"}
    })}else{json!({
        "clientInfo":{"name":"dominion","title":"DOMINION","version":"0.4.4"},
        "capabilities":{"experimentalApi":false}
    })};
    request(&writer,kind,1,"initialize",init).await?;
    let mut stage=0u8;
    let mut agent_session:Option<String>=None;
    let mut answer=String::new();
    let mut seen=0usize;
    let mut ticker=interval(Duration::from_millis(250));
    let loop_result=timeout(Duration::from_secs(180),async{
        loop {
            tokio::select! {
                _=ticker.tick()=>{
                    if context.state.is_cancelled(session)
                        ||context.state.is_cancelled(parent)
                        ||context.state.cancellation_generation(parent)!=generation
                        ||context.state.store.get("job",&context.job_id)
                            .is_none_or(|job|job["cancelRequested"]==true
                                ||job["status"]=="cancelled"){
                        if let Some(ref session_id)=agent_session{
                            if is_acp{
                                let _=notify(&writer,kind,"session/cancel",
                                    json!({"sessionId":session_id})).await;
                            }else{
                                let _=request(&writer,kind,4,"turn/interrupt",
                                    json!({"threadId":session_id})).await;
                            }
                        }
                        return Err("외부 에이전트 RPC 작업이 취소되었습니다.".into());
                    }
                }
                line=lines.next_line()=>{
                    let line=line.map_err(|e|e.to_string())?
                        .ok_or("외부 에이전트 RPC 연결이 완료 전에 종료됐습니다.")?;
                    if line.len()>MAX_LINE{return Err("외부 RPC 응답 한도를 초과했습니다.".into());}
                    seen+=1;
                    if seen>MAX_NOTIFICATIONS{return Err("외부 RPC 이벤트 수 한도를 초과했습니다.".into());}
                    let msg:Value=serde_json::from_str(&line)
                        .map_err(|_|"외부 에이전트 RPC JSON 형식이 올바르지 않습니다.")?;
                    if let Some(method)=msg["method"].as_str(){
                        if msg.get("id").is_some() {
                            permission(context,kind,&writer,session,parent_execution,
                                external_id,&msg).await?;
                            continue;
                        }
                        let params=&msg["params"];
                        let text=if !is_acp {
                            match method {
                                "item/agentMessage/delta"=>params["delta"].as_str(),
                                "item/completed" if params["item"]["type"]=="agentMessage"=>
                                    params["item"]["text"].as_str(),
                                _=>None
                            }
                        }else if method=="session/update"
                            &&params["update"]["sessionUpdate"]=="agent_message_chunk"{
                            params["update"]["content"]["text"].as_str()
                        }else{None};
                        if let Some(text)=text{
                            if method=="item/completed" {answer.clear();}
                            append_answer(&mut answer,text);
                        }
                        forward(context,kind,session,parent_execution,external_id,method,text);
                        if !is_acp&&method=="turn/completed"{
                            let status=params["turn"]["status"].as_str().unwrap_or("");
                            if status!="completed" {
                                return Err(format!("Codex App Server turn ended: {status}"));
                            }
                            if answer.trim().is_empty(){
                                return Err("Codex App Server produced no final agent message".into());
                            }
                            return Ok(());
                        }
                        if method=="turn/failed"||method=="error"{
                            return Err("외부 에이전트가 실패 이벤트를 반환했습니다.".into());
                        }
                        continue;
                    }
                    let id=msg["id"].as_u64();
                    if msg.get("error").is_some(){
                        let detail=msg["error"]["message"].as_str().unwrap_or("remote RPC error");
                        return Err(format!("외부 RPC 요청 {id:?} 실패: {}",
                            detail.chars().take(300).collect::<String>()));
                    }
                    if id==Some(1)&&stage==0{
                        stage=1;
                        if !is_acp{notify(&writer,kind,"initialized",json!({})).await?;}
                        if is_acp{
                            let (method,params)=if let Some(id)=resume{
                                ("session/load",json!({"cwd":workspace.to_string_lossy(),
                                    "mcpServers":[],"sessionId":id}))
                            }else{
                                ("session/new",json!({"cwd":workspace.to_string_lossy(),
                                    "mcpServers":[]}))
                            };
                            request(&writer,kind,2,method,params).await?;
                        }else{
                            let method=if resume.is_some(){"thread/resume"}else{"thread/start"};
                            let params=if let Some(id)=resume{
                                json!({"threadId":id,"cwd":workspace.to_string_lossy()})
                            }else{
                                json!({"cwd":workspace.to_string_lossy(),
                                    "sandbox":"read-only","approvalPolicy":"on-request"})
                            };
                            request(&writer,kind,2,method,params).await?;
                        }
                    }else if id==Some(2)&&stage==1{
                        stage=2;
                        let token=if is_acp{
                            msg["result"]["sessionId"].as_str()
                        }else{
                            msg["result"]["thread"]["id"].as_str().or_else(||
                                msg["result"]["threadId"].as_str())
                        }.ok_or("외부 RPC 세션 ID가 없습니다.")?;
                        if let Some(expected)=resume{
                            if token!=expected{return Err("외부 RPC 재개 세션 ID가 일치하지 않습니다.".into());}
                        }
                        agent_session=Some(token.to_string());
                        if is_acp {
                            request(&writer,kind,3,"session/prompt",json!({
                                "sessionId":token,"prompt":[{"type":"text","text":prompt}]
                            })).await?;
                        }else{
                            request(&writer,kind,3,"turn/start",json!({
                                "threadId":token,"input":[{"type":"text","text":prompt}]
                            })).await?;
                        }
                    }else if id==Some(3)&&stage==2&&is_acp{
                        if msg["result"]["stopReason"]!="end_turn" {
                            return Err("ACP 작업이 정상적으로 끝나지 않았습니다.".into());
                        }
                        if answer.trim().is_empty(){
                            return Err("ACP 실행의 최종 에이전트 응답이 없습니다.".into());
                        }
                        return Ok(());
                    }
                }
            }
        }
    }).await;
    if loop_result.is_err()||loop_result.as_ref().is_ok_and(|result|result.is_err()){
        let _=child.start_kill();
    }
    let _=timeout(Duration::from_secs(2),child.wait()).await;
    interrupt_orphan_approvals(context,external_id);
    loop_result.map_err(|_|"외부 에이전트 RPC 실행 제한 180초를 초과했습니다.".to_string())??;
    let answer=answer.trim().chars().take(MAX_ANSWER).collect::<String>();
    if answer.is_empty(){return Err("외부 에이전트의 검증 가능한 답변이 없습니다.".into());}
    let hash=format!("{:x}",Sha256::digest(answer.as_bytes()));
    Ok((answer,hash,agent_session))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mock stdio peer, not a model mock: the actual bidirectional JSON-RPC
    /// transport, live event ingestion, owner approval route and completion
    /// handling are exercised without requiring an installed paid CLI.
    #[cfg(unix)]
    #[tokio::test]
    async fn codex_and_acp_live_stdio_contracts_complete_with_real_owner_approval(){
        use crate::{config::Config,db::Store,vault::Vault};
        use axum::{extract::State,Json,http::StatusCode};
        let root=std::env::temp_dir()
            .join(format!("dominion-rpc-smoke-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let cfg=Config{port:0,data_dir:root.clone(),workspace:root.clone(),
            web_dir:root.join("web")};
        let state=AppState::new(cfg.clone(),
            Store::open(&cfg.db_path()).unwrap(),Vault::open(&cfg.key_path()));
        state.store.put("session",&json!({"id":"parent"})).unwrap();
        state.store.put("session",&json!({"id":"child"})).unwrap();
        let script=r#"
import json,sys
kind=sys.argv[1]
def recv(): return json.loads(sys.stdin.readline())
def send(payload):
    sys.stdout.write(json.dumps(payload)+'\n')
    sys.stdout.flush()
assert recv()['method']=='initialize'
send({'id':1,'result':{}})
if kind=='codex':
    assert recv()['method']=='initialized'
    assert recv()['method']=='thread/start'
    send({'id':2,'result':{'thread':{'id':'123e4567-e89b-12d3-a456-426614174000'}}})
    assert recv()['method']=='turn/start'
    send({'method':'item/agentMessage/delta','params':{'delta':'Codex result'}})
    send({'method':'turn/completed','params':{'turn':{'status':'completed'}}})
else:
    assert recv()['method']=='session/new'
    send({'jsonrpc':'2.0','id':2,'result':{'sessionId':'qwen-mock-1'}})
    assert recv()['method']=='session/prompt'
    send({'jsonrpc':'2.0','id':17,'method':'session/request_permission',
        'params':{'options':[{'optionId':'allow','name':'Allow'}]}})
    voted=recv()
    assert voted['id']==17 and voted['result']['outcome']['optionId']=='allow'
    # A thinking chunk MUST NOT enter the final public answer.
    send({'jsonrpc':'2.0','method':'session/update',
        'params':{'update':{'sessionUpdate':'agent_thought_chunk',
            'content':{'type':'text','text':'private thought'}}}})
    send({'jsonrpc':'2.0','method':'session/update',
        'params':{'update':{'sessionUpdate':'agent_message_chunk',
            'content':{'type':'text','text':'ACP final answer'}}}})
    send({'jsonrpc':'2.0','id':3,'result':{'stopReason':'end_turn'}})
"#;
        for (kind,arg,expected) in [
            (ExternalAgent::CodexServer,"codex","Codex result"),
            (ExternalAgent::QwenAcp,"acp","ACP final answer")
        ]{
            let job_id=format!("rpc-{arg}");
            state.store.put("job",&json!({"id":job_id,"kind":"prompt",
                "status":"running"})).unwrap();
            let ctx=JobContext::new(state.clone(),job_id);
            let mut cmd=Command::new("python3");
            cmd.args(["-u","-c",script,arg]);
            let root_owned=root.clone();
            let run_id=format!("external:rpc-{arg}");
            let task=tokio::spawn(async move{
                run_with_command(&ctx,kind,"child",&root_owned,"inspect",None,
                    0,"parent",None,&run_id,cmd).await
            });
            if kind==ExternalAgent::QwenAcp{
                let pending=tokio::time::timeout(Duration::from_secs(8),async{
                    loop{
                        if let Some(row)=state.store.all("approval").into_iter()
                            .find(|a|a["source"]=="external_agent_rpc"
                                &&a["status"]=="pending"){
                            break row;
                        }
                        tokio::time::sleep(Duration::from_millis(20)).await;
                    }
                }).await.expect("live approval request");
                let response=crate::api::approvals::resolve_approval(
                    State(state.clone()),Json(json!({
                        "id":pending["id"],"allow":true}))).await;
                assert_eq!(response.status(),StatusCode::OK);
            }
            let result=tokio::time::timeout(Duration::from_secs(10),task)
                .await.expect("RPC mock timed out").unwrap().unwrap();
            assert_eq!(result.0,expected);
            assert_eq!(result.1.len(),64);
            assert!(result.2.is_some());
            assert!(state.bus.replay(0,500).iter().any(|e|
                e["type"]=="external_agent_event"
                &&e["method"].as_str().is_some()));
        }
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn approval_reply_is_protocol_specific_and_never_grants_unknown_permissions(){
        let opts=json!([{"kind":"allow_once","optionId":"opt-1"},
            {"kind":"reject_once","optionId":"opt-2"}]);
        let acp=response_for_permission(ExternalAgent::QwenAcp,
            "session/request_permission",&opts,true);
        assert_eq!(acp["outcome"]["optionId"],"opt-1");
        let qwen=response_for_permission(ExternalAgent::QwenAcp,
            "session/request_permission",
            &json!([{"optionId":"allow","name":"Allow"},
                    {"optionId":"allow_always","name":"Always Allow"}]),true);
        assert_eq!(qwen["outcome"]["optionId"],"allow");
        let persist=response_for_permission(ExternalAgent::QwenAcp,
            "session/request_permission",
            &json!([{"optionId":"allow_always","name":"Always Allow"}]),true);
        assert_eq!(persist["outcome"]["outcome"],"cancelled");
        let deny=response_for_permission(ExternalAgent::QwenAcp,
            "session/request_permission",&opts,false);
        assert_eq!(deny["outcome"]["outcome"],"cancelled");
        assert_eq!(response_for_permission(ExternalAgent::CodexServer,
            "item/commandExecution/requestApproval",&Value::Null,true)["decision"],"accept");
        assert_eq!(response_for_permission(ExternalAgent::CodexServer,
            "item/permissions/requestApproval",&Value::Null,true)["decision"],"decline");
    }
}
