//! Real host + public OCI images + actual SDK transport.
//! The model alone is scripted; no cloud keys, fake Docker or mocked HTTP APIs.
use super::*;
fn install(app:&TestApp,query:&str)->String{
 let rows=app.get(&format!("/api/plugins/market?q={query}"));
 let items=rows["items"].as_array().expect("market items");
 assert_eq!(items.len(),1,"{rows}");
 let item=&items[0];
 assert!(item["manifest"].is_object(),"Missing published manifest: {item}");
 let installed=app.post("/api/plugins/market/install",&json!({
  "repository":item["repository"],"commit":item["commit"],"manifestDigest":item["manifestDigest"],"confirmed":true
 }));
 assert_eq!(installed["id"],item["installId"]);
 assert_eq!(installed["running"],false);
 assert_eq!(installed["image"],item["manifest"]["image"]);
 installed["id"].as_str().unwrap().to_owned()
}
fn capabilities(app:&TestApp,id:&str)->Value{
 let path=format!("/api/plugins/managed/{id}/capabilities");
 let deadline=Instant::now()+Duration::from_secs(45);
 loop{
  let res=app.client.get(format!("{}{}",app.base,path)).send().unwrap();
  let status=res.status();let body=res.text().unwrap_or_default();
  if status.is_success(){return serde_json::from_str(&body).unwrap()}
  assert!(Instant::now()<deadline,"SDK did not start: {status}: {body}");
  std::thread::sleep(Duration::from_secs(1));
 }
}
#[test]
#[ignore="Requires a clean Docker runner and access to public GitHub/GHCR"]
fn published_extensions_install_register_and_execute_live(){
 let _serial=serial();
 let (port,requests)=scripted_provider(|request|{
  let messages=request["messages"].as_array().unwrap();
  if messages.iter().any(|m|m["role"]=="tool"){
   return vec![json!({"choices":[{"delta":{"content":"Live extension tool completed."},"finish_reason":"stop"}]})]
  }
  let name=request["tools"].as_array().into_iter().flatten()
   .filter_map(|t|t["function"]["name"].as_str()).find(|n|n.starts_with("ext__"));
  match name{
   Some(name)=>vec![json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"live-extension-call","type":"function","function":{"name":name,"arguments":"{\"text\":\"live-verification\"}"}}]},"finish_reason":"tool_calls"}]})],
   None=>vec![json!({"choices":[{"delta":{"content":"No extension tool was advertised."},"finish_reason":"stop"}]})]
  }
 });
 let app=TestApp::new(port,"native");
 assert_eq!(app.get("/api/mcp/runtime")["available"],false,"Run default host build only");
 assert_eq!(app.get("/api/plugins/managed")["available"],true,"Docker required");
 let indexed=app.get("/api/plugins/market?q=MCP");
 assert!(indexed["items"].as_array().unwrap().iter().any(|x|x["sourceRef"]=="dominion-sub3"&&x["indexed"]==true),"{indexed}");
 let id=install(&app,"hushmind3%2Fdominion-rust%40dominion-sub3");
 let path=format!("/api/plugins/managed/{id}");
 assert_eq!(app.post(&format!("{path}/start"),&json!({}))["running"],true);
 let caps=capabilities(&app,&id);
 assert_eq!(caps["tools"],true);assert_eq!(caps["configuration"],true);assert_eq!(caps["events"],false);
 println!("PASS: public MCP package installed and started through real market API");
 let script=include_str!("../../extensions/mcp-connector/scripts/mcp-echo-modern-server.mjs");
 let settings=app.post(&format!("{path}/configuration"),&json!({"confirmed":true,"values":{"servers":[
  {"id":"fixture","name":"Echo","transport":"stdio","command":"node","args":["--input-type=module","-e",script],"enabled":true,"protocolMode":"modern"}
 ]}}));
 assert_eq!(settings["values"]["servers"][0]["name"],"Echo");
 assert_eq!(settings["schema"]["type"],"object");
 let catalog=app.get(&format!("{path}/tools/discover"));
 assert_eq!(catalog["tools"].as_array().unwrap().len(),1,"{catalog}");
 let local=catalog["tools"][0]["name"].as_str().unwrap();
 let tool=format!("ext__{}__{}",id.replace('-',""),local);
 let grant=app.post(&format!("{path}/tools"),&json!({"enabled":true,"confirmed":true,"digest":catalog["digest"]}));
 assert_eq!(grant["enabled"],true);
 println!("PASS: real configuration and catalog approval");
 app.post("/api/agents",&json!({"id":"extension-agent","name":"Researcher","systemPrompt":"Use the Echo tool to echo the supplied text."}));
 let room=app.post("/api/rooms",&json!({"name":"Live extension","agentIds":["extension-agent"],"rounds":1,"maxTurns":1,"toolsEnabled":true,"autoLearning":false}));
 let sent=app.post(&format!("/api/rooms/{}/messages",room["id"].as_str().unwrap()),&json!({"input":"Use the Modern stdio echo tool to echo live-verification."}));
 let approval=app.wait_pending_approval(40);
 assert_eq!(approval["tool"],tool,"{approval}");
 app.post("/api/approvals",&json!({"id":approval["id"],"allow":true}));
 let deadline=Instant::now()+Duration::from_secs(60);
 loop {
  let job=app.get(&format!("/api/jobs/{}",sent["jobId"].as_str().unwrap()));
  if job["status"]=="completed"{break}
  assert!(matches!(job["status"].as_str(),Some("queued"|"running")),"{job}");
  assert!(Instant::now()<deadline,"{job}");
  std::thread::sleep(Duration::from_millis(100));
 }
 assert!(requests.lock().unwrap().iter().any(|r|r["messages"].as_array().unwrap().iter().any(|m|m["role"]=="tool"&&m["content"].as_str().is_some_and(|s|s.contains("MCP_MODERN: live-verification")))),"Actual extension result must reach model");
 println!("PASS: owner-approved real tool invocation and returned evidence");
 assert_eq!(app.post(&format!("{path}/tools"),&json!({"enabled":false,"confirmed":true}))["enabled"],false);
 assert_eq!(app.post(&format!("{path}/stop"),&json!({}))["running"],false);
 assert_eq!(app.post(&format!("{path}/start"),&json!({}))["running"],true);
 capabilities(&app,&id);
 assert_eq!(app.get(&format!("{path}/configuration"))["values"]["servers"][0]["name"],"Echo");
 assert_eq!(app.get(&format!("{path}/tools"))["enabled"],false);
 assert_eq!(app.post(&format!("{path}/stop"),&json!({}))["running"],false);
 let removed=app.client.delete(format!("{}{path}",app.base)).send().unwrap().error_for_status().unwrap().json::<Value>().unwrap();
 assert_eq!(removed["dataRetained"],true);
 println!("PASS: revoke, restart, persistence, stop and remove");
 let assistant=install(&app,"hushmind3%2Fdominion-assistant");
 let path=format!("/api/plugins/managed/{assistant}");
 assert_eq!(app.post(&format!("{path}/start"),&json!({}))["running"],true);
 assert_eq!(capabilities(&app,&assistant)["events"],true);
 let connected=app.post(&format!("{path}/connection"),&json!({"enabled":true,"confirmed":true,"mode":"chat"}));
 assert_eq!(connected["enabled"],true);
 assert_eq!(app.post(&format!("{path}/connection"),&json!({"enabled":false,"confirmed":true}))["enabled"],false);
 assert_eq!(app.post(&format!("{path}/stop"),&json!({}))["running"],false);
 app.delete(&path);
 println!("PASS: public assistant package install, start, common SDK connection and disconnect");
}
