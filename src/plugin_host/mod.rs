//! OpenClaw's real SDK/loader runs in a dedicated, owner-managed profile.
//! Dominion owns the process, credentials, installation receipts and agent tool approvals.
use crate::state::AppState;
use futures_util::{SinkExt,StreamExt};
use serde_json::{json,Value};
use std::{path::{Path,PathBuf},process::Stdio,time::Duration};
use tokio::{process::Command,io::AsyncWriteExt};
use tokio_tungstenite::tungstenite::Message;
pub mod provision;
pub const VERSION:&str="2026.11.10";
pub const MANAGE_TOOL:&str="mcp__dominion_plugin_host__manage";
pub const TOOL:&str="mcp__dominion_plugin_host__invoke";
#[derive(Default)]
pub struct Host { child:Option<tokio::process::Child>,port:u16,token:String }
fn root(state:&AppState)->PathBuf{state.config.data_dir.join("plugin-host")}
fn installed_runtime()->Option<(PathBuf,PathBuf)>{
    let home=std::env::var_os("HOME").or_else(||std::env::var_os("USERPROFILE")).map(PathBuf::from)?;
    let tools=home.join(".openclaw/tools");
    let mut prefixes=vec![tools.join("node")];
    if let Ok(entries)=std::fs::read_dir(&tools){prefixes.extend(entries.flatten().map(|entry|entry.path()));}
    if let Some(paths)=std::env::var_os("PATH"){for bin in std::env::split_paths(&paths){if let Some(prefix)=bin.parent(){prefixes.push(prefix.to_path_buf());}}}
    for prefix in prefixes{
        let executable=if cfg!(windows){prefix.join("node.exe")}else{prefix.join("bin/node")};
        for package in [prefix.join("lib/node_modules/openclaw"),prefix.join("node_modules/openclaw")]{
            if executable.is_file()&&package.join("dist/entry.js").is_file(){return Some((package,executable));}
        }
    }
    None
}
pub fn package_root(state:&AppState)->PathBuf{
    std::env::var_os("DOMINION_OPENCLAW_ROOT").map(PathBuf::from)
        .or_else(||{let managed=root(state).join("runtime/node_modules/openclaw");managed.join("dist/entry.js").is_file().then_some(managed)})
        .or_else(||installed_runtime().map(|(package,_)|package))
        .unwrap_or_else(||root(state).join("runtime/node_modules/openclaw"))
}
fn node(state:&AppState)->PathBuf{
    std::env::var_os("DOMINION_PLUGIN_NODE").map(PathBuf::from)
        .or_else(||installed_runtime().map(|(_,node)|node))
        .unwrap_or_else(||provision::node_path(&root(state)))
}
fn progress(state:&AppState,operation:&str,label:&str){
    if let Some(mut row)=state.store.get("plugin_install",operation){
        let mut activities=row["activities"].as_array().cloned().unwrap_or_default();
        for activity in &mut activities{if activity["stage"]=="host"&&activity["status"]=="started"{activity["status"]=json!("completed");}}
        activities.push(json!({"label":label,"stage":"host","status":"started","at":chrono::Utc::now().timestamp_millis()}));
        row["activities"]=json!(activities);row["updatedAt"]=json!(chrono::Utc::now().timestamp_millis());
        if let Some(saved)=state.store.put("plugin_install",&row){state.bus.record("plugin_install",&saved);}
    }
}
fn install_activity(state:&AppState,operation:&str,event:&Value){
    let stage=event["stage"].as_str().unwrap_or("");let status=event["status"].as_str().unwrap_or("");
    let subject=match stage{"resolve"=>"패키지 확인","download"=>"플러그인 다운로드","extract"=>"압축 파일 해제","files"=>"플러그인 파일 준비","dependencies"=>"플러그인 종속성 설치","runtime"=>"플러그인 적용",_=>return};
    let suffix=match status{"started"=>"중","completed"=>"완료","failed"=>"실패",_=>return};
    if let Some(mut row)=state.store.get("plugin_install",operation){
        let mut activities=row["activities"].as_array().cloned().unwrap_or_default();
        let entry=json!({"activityId":event["activityId"],"stage":stage,"status":status,"label":format!("{subject} {suffix}"),"at":chrono::Utc::now().timestamp_millis()});
        if let Some(previous)=activities.iter_mut().find(|a|a["activityId"]==event["activityId"]&&a["stage"]==stage){*previous=entry;}else{activities.push(entry);}
        row["activities"]=json!(activities);
        if let Some(saved)=state.store.put("plugin_install",&row){state.bus.record("plugin_install",&saved);}
    }
}
pub async fn status(state:&AppState)->Value{
    let mut host=state.plugin_host.lock().await;
    let running=host.child.as_mut().is_some_and(|child|matches!(child.try_wait(),Ok(None)));
    if !running{host.child=None;}
    json!({"installed":package_root(state).join("dist/entry.js").is_file(),"running":running,"version":std::fs::read(package_root(state).join("package.json")).ok().and_then(|bytes|serde_json::from_slice::<Value>(&bytes).ok()).and_then(|value|value["version"].as_str().map(str::to_owned))})
}
pub async fn start(state:&AppState,operation:&str)->Result<(),String>{
    if status(state).await["running"]==true{return Ok(())}
    let package=package_root(state);
    progress(state,operation,"실행 환경 준비 중");
    if !node(state).is_file(){provision::node(&root(state)).await?;}
    if !package.join("dist/entry.js").is_file(){
        progress(state,operation,"플러그인 실행 환경 준비 중");
        provision::sdk(&root(state),&node(state),VERSION).await?;
    }
    let profile=root(state).join("profile");tokio::fs::create_dir_all(&profile).await.map_err(|e|e.to_string())?;
    let config_path=profile.join("openclaw.json");
    let port=std::net::TcpListener::bind(("127.0.0.1",0)).map_err(|e|e.to_string())?.local_addr().map_err(|e|e.to_string())?.port();
    let token=format!("{}{}",uuid::Uuid::new_v4().simple(),uuid::Uuid::new_v4().simple());
    // Retain only this profile's existing settings. Never read the user's OpenClaw config.
    let mut config=tokio::fs::read(&config_path).await.ok().and_then(|b|serde_json::from_slice::<Value>(&b).ok())
        .unwrap_or_else(||json!({"plugins":{"slots":{"memory":"none"}},"channels":{},"discovery":{"mdns":{"mode":"off"}},"agents":{"defaults":{"workspace":root(state).join("workspace")}}}));
    config["gateway"]=json!({"mode":"local","bind":"loopback","port":port,"auth":{"mode":"token"},"controlUi":{"enabled":false}});
    private_write(&config_path,&serde_json::to_vec_pretty(&config).map_err(|e|e.to_string())?).await?;
    let mut log_options=std::fs::OpenOptions::new();log_options.create(true).append(true);
    #[cfg(unix)] {use std::os::unix::fs::OpenOptionsExt;log_options.mode(0o600);}
    let log=log_options.open(profile.join("host.log")).map_err(|e|e.to_string())?;
    let mut command=Command::new(node(state));
    command.arg(package.join("dist/entry.js")).args(["gateway","run","--bind","loopback","--port",&port.to_string()])
        .env_clear().env("OPENCLAW_STATE_DIR",&profile).env("OPENCLAW_CONFIG_PATH",&config_path)
        .env("OPENCLAW_GATEWAY_TOKEN",&token).env("OPENCLAW_NO_RESPAWN","1").env("NODE_DISABLE_COMPILE_CACHE","1")
        .stdin(Stdio::null()).stdout(Stdio::from(log.try_clone().map_err(|e|e.to_string())?)).stderr(Stdio::from(log)).kill_on_drop(true);
    // OS plumbing only: do not inherit ambient channel/model credentials or NODE_OPTIONS.
    for key in ["PATH","HOME","USERPROFILE","SystemRoot","SYSTEMROOT","TEMP","TMP","TMPDIR","LANG"]{
        if let Some(value)=std::env::var_os(key){command.env(key,value);}
    }
    let child=command.spawn().map_err(|e|format!("플러그인 호스트를 시작하지 못했습니다: {e}"))?;
    {let mut host=state.plugin_host.lock().await;host.child=Some(child);host.port=port;host.token=token;}
    progress(state,operation,"플러그인 호스트 시작 중");
    for _ in 0..180{
        if status(state).await["running"]!=true{return Err("플러그인 호스트가 시작 중 종료됐습니다. 호스트 로그를 확인하세요.".into())}
        if rpc(state,"health",json!({}),None,Duration::from_secs(2)).await.is_ok(){return Ok(())}
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    stop(state).await?;Err("플러그인 호스트 시작 시간이 초과됐습니다.".into())
}
pub async fn stop(state:&AppState)->Result<(),String>{
    let child={let mut host=state.plugin_host.lock().await;host.token.clear();host.child.take()};
    if let Some(mut child)=child{
        if matches!(child.try_wait(),Ok(Some(_))){return Ok(())}
        #[cfg(unix)] if let Some(pid)=child.id(){let _=Command::new("/bin/kill").args(["-TERM",&pid.to_string()]).status().await;}
        #[cfg(windows)] if let Some(pid)=child.id(){let _=Command::new("taskkill.exe").args(["/PID",&pid.to_string(),"/T"]).status().await;}
        if tokio::time::timeout(Duration::from_secs(15),child.wait()).await.is_err(){let _=child.kill().await;let _=child.wait().await;}
    }
    Ok(())
}

async fn private_write(path:&Path,bytes:&[u8])->Result<(),String>{
    let mut options=tokio::fs::OpenOptions::new();options.create(true).write(true).truncate(true);
    #[cfg(unix)] options.mode(0o600);
    let mut file=options.open(path).await.map_err(|e|e.to_string())?;
    file.write_all(bytes).await.map_err(|e|e.to_string())?;file.sync_all().await.map_err(|e|e.to_string())
}
pub async fn rpc(state:&AppState,method:&str,params:Value,operation:Option<&str>,limit:Duration)->Result<Value,String>{
    let (port,token)={let host=state.plugin_host.lock().await;
        if host.child.is_none(){return Err("플러그인 호스트가 실행 중이 아닙니다.".into())}(host.port,host.token.clone())};
    let work=async {
        let (mut socket,_)=tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}")).await.map_err(|e|e.to_string())?;
        let mut requested=false;let mut admission_retries=0u8;
        while let Some(frame)=socket.next().await{
            let frame=frame.map_err(|e|e.to_string())?;
            if let Message::Text(text)=frame{
                if text.len()>8*1024*1024{return Err("호스트 응답이 너무 큽니다.".into())}
                let value:Value=serde_json::from_str(&text).map_err(|e|e.to_string())?;
                if value["event"]=="connect.challenge"{
                    socket.send(Message::Text(json!({"type":"req","id":"connect","method":"connect","params":{
                        "minProtocol":3,"maxProtocol":4,"client":{"id":"cli","version":env!("CARGO_PKG_VERSION"),"platform":std::env::consts::OS,"mode":"cli"},
                        "role":"operator","scopes":["operator.admin"],"auth":{"token":token}
                    }}).to_string())).await.map_err(|e|e.to_string())?;
                }
                if value["type"]=="res"&&value["id"]=="connect"{
                    if value["ok"]!=true{return Err(value["error"].to_string())}
                    socket.send(Message::Text(json!({"type":"req","id":"operation","method":method,"params":params}).to_string())).await.map_err(|e|e.to_string())?;requested=true;
                }
                if requested&&value["event"]=="plugins.install.progress"{
                    if let Some(id)=operation{if value["payload"]["requestId"]=="operation"{install_activity(state,id,&value["payload"]);}}
                }
                if value["type"]=="res"&&value["id"]=="operation"{
                    if value["ok"]!=true&&value["error"]["details"]["mutationAccepted"]==false&&value["error"]["retryable"]==true&&admission_retries<8{
                        admission_retries+=1;
                        tokio::time::sleep(Duration::from_millis(value["error"]["retryAfterMs"].as_u64().unwrap_or(1000).clamp(100,5000))).await;
                        socket.send(Message::Text(json!({"type":"req","id":"operation","method":method,"params":params}).to_string())).await.map_err(|e|e.to_string())?;
                        continue;
                    }
                    let _=socket.close(None).await;
                    if value["ok"]==true{
                        if method=="plugins.list"{let _=state.store.put("plugin_inventory",&json!({"id":"default","plugins":value["payload"]["plugins"],"updatedAt":chrono::Utc::now().timestamp_millis()}));}
                        return Ok(value["payload"].clone())
                    }
                    return Err(value["error"].to_string());
                }
            }
        }
        Err("호스트 연결이 결과 확인 전에 종료됐습니다. 설치 상태를 확인하세요.".into())
    };
    tokio::time::timeout(limit,work).await.map_err(|_|"호스트 응답 시간이 초과됐습니다. 작업을 자동 재실행하지 않습니다. 현재 상태를 확인하세요.".to_string())?
}
pub async fn install(state:AppState,id:String,request:Value){
    let _lease=state.plugin_host_operation.lock().await;
    let result=async {
        start(&state,&id).await?;progress(&state,&id,"패키지 확인 및 설치 중");
        rpc(&state,"plugins.install",request,Some(&id),Duration::from_secs(300)).await
    }.await;
    if let Some(mut row)=state.store.get("plugin_install",&id){
        row["finishedAt"]=json!(chrono::Utc::now().timestamp_millis());
        match result{Ok(value)=>{let _=rpc(&state,"plugins.list",json!({}),None,Duration::from_secs(30)).await;row["status"]=json!("completed");row["result"]=value;
            if let Some(activities)=row["activities"].as_array_mut(){for activity in activities{if activity["stage"]=="host"&&activity["status"]=="started"{activity["status"]=json!("completed");}}}},Err(message)=>{row["status"]=json!("failed");row["error"]=json!(message);}}
        if let Some(saved)=state.store.put("plugin_install",&row){state.bus.record("plugin_install",&saved);}
    }
}
// The existing durable MCP tool gateway also gates calls into the plugin host.
// One generic invocation preserves the upstream tool's arbitrary JSON contract.
pub async fn definitions(state:&AppState)->Vec<Value>{
    let management=json!({"type":"function","function":{"name":MANAGE_TOOL,"description":"Inspect, install, enable, disable, configure or remove plugins using the real compatibility host. Mutations require the normal owner tool approval. Use the exact package version and plugin identity from catalog/context. Configuration changes apply only to this plugin.","parameters":{"type":"object","properties":{"action":{"type":"string","enum":["status","install","enable","disable","configure","remove"]},"packageName":{"type":"string"},"version":{"type":"string"},"pluginId":{"type":"string"},"config":{"type":"object","additionalProperties":true}},"required":["action"],"additionalProperties":false}}});
    if status(state).await["running"]!=true{return vec![management]}
    let Ok(catalog)=rpc(state,"tools.catalog",json!({}),None,Duration::from_secs(10)).await else{return vec![management]};
    let tools=plugin_tools(&catalog);
    if tools.is_empty(){return vec![management]}
    vec![management,json!({"type":"function","function":{"name":TOOL,"description":format!("Call an installed plugin tool. Available tool contracts: {}",json!(tools)),"parameters":{"type":"object","properties":{"tool":{"type":"string","enum":tools.iter().filter_map(|v|v["name"].as_str()).collect::<Vec<_>>()} ,"arguments":{"type":"object","additionalProperties":true}},"required":["tool","arguments"],"additionalProperties":false}}})]
}
fn plugin_tools(catalog:&Value)->Vec<Value>{
    catalog["groups"].as_array().into_iter().flatten().flat_map(|group|group["tools"].as_array().into_iter().flatten())
        .filter(|tool|tool["source"]=="plugin"||tool["source"]=="channel").cloned().map(|mut tool|{if tool.get("name").is_none(){tool["name"]=tool["id"].clone();}tool}).collect()
}
pub async fn invoke(state:&AppState,args:&Value)->Result<Value,String>{
    let name=args["tool"].as_str().ok_or("Missing plugin tool")?;
    let catalog=rpc(state,"tools.catalog",json!({}),None,Duration::from_secs(10)).await?;
    if !plugin_tools(&catalog).iter().any(|tool|tool["name"]==name){return Err("활성 플러그인이 제공하는 도구가 아닙니다.".into())}
    rpc(state,"tools.invoke",json!({"name":name,"args":args["arguments"],"idempotencyKey":uuid::Uuid::new_v4().to_string()}),None,Duration::from_secs(120)).await
}

#[cfg(test)] mod tests;


pub async fn manage(state:&AppState,args:&Value)->Result<Value,String>{
    let action=args["action"].as_str().ok_or("Missing plugin action")?;
    if action=="status"{return if status(state).await["running"]==true{rpc(state,"plugins.list",json!({}),None,Duration::from_secs(30)).await}else{Ok(status(state).await)}}
    let _lease=state.plugin_host_operation.lock().await;
    if action=="install"{
        let package=args["packageName"].as_str().filter(|v|!v.is_empty()).ok_or("An exact package is required")?;
        let version=args["version"].as_str().filter(|v|!v.is_empty()).ok_or("An exact version is required")?;
        start(state,"").await?;
        return rpc(state,"plugins.install",json!({"source":"clawhub","packageName":package,"version":version,"enable":true}),None,Duration::from_secs(300)).await;
    }
    let id=args["pluginId"].as_str().filter(|v|!v.is_empty()).ok_or("Missing installed plugin identity")?;
    start(state,"").await?;
    if ["__proto__","constructor","prototype"].contains(&id){return Err("Invalid plugin identity".into())}
    match action{
        "enable"|"disable"=>rpc(state,"plugins.setEnabled",json!({"pluginId":id,"enabled":action=="enable"}),None,Duration::from_secs(120)).await,
        "remove"=>rpc(state,"plugins.uninstall",json!({"pluginId":id}),None,Duration::from_secs(120)).await,
        "configure"=>{
            if !args["config"].is_object(){return Err("Plugin configuration must be an object".into())}
            let snapshot=rpc(state,"config.get",json!({}),None,Duration::from_secs(30)).await?;
            let hash=snapshot["hash"].as_str().ok_or("No current configuration revision")?;
            let mut entries=serde_json::Map::new();entries.insert(id.to_string(),json!({"config":args["config"]}));
            rpc(state,"config.patch",json!({"baseHash":hash,"raw":json!({"plugins":{"entries":entries}}).to_string()}),None,Duration::from_secs(60)).await.map(|_|json!({"saved":true}))
        }
        _=>Err("Unknown plugin action".into()),
    }
}
