//! Real Axum API + real independent package process + scripted model.
use super::*;
use base64::Engine as _;

#[test]
fn independent_extension_import_group_execution_and_removal_without_mcp(){
    let _serial=serial();
    let (port,requests)=scripted_provider(|request|{
        let messages=request["messages"].as_array().unwrap();
        if messages.iter().any(|m|m["role"]=="tool"){
            return vec![json!({"choices":[{"delta":{"content":"EXTENSION_RECEIPT_CONFIRMED"},"finish_reason":"stop"}]})];
        }
        let selected=request["tools"].as_array().and_then(|tools|tools.iter()
            .find(|tool|tool["function"]["name"].as_str().is_some_and(|n|n.starts_with("ext__fixture__echo__"))));
        if let Some(tool)=selected{
            vec![json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"extension-call","type":"function","function":{
                "name":tool["function"]["name"],"arguments":"{\"text\":\"EXTENSION_ACTUAL_PROCESS\"}"
            }}]},"finish_reason":"tool_calls"}]})]
        }else{
            vec![json!({"choices":[{"delta":{"content":"NO_EXTENSION_ADVERTISED"},"finish_reason":"stop"}]})]
        }
    });
    let app=TestApp::new(port,"native");
    app.post("/api/extensions/registries",&json!({"urls":[],"githubDiscovery":false}));
    assert_eq!(app.get("/api/extensions/catalog?q=fixture")["items"],json!([]));
    assert_eq!(app.get("/api/extensions"),json!([]));
    assert_eq!(app.get("/api/mcp"),json!([]));
    let bytes=b"import sys,json\nr=json.load(sys.stdin)\nprint(json.dumps(dict(protocol=r['protocol'],id=r['id'],ok=True,result=r['arguments'])))\n";
    let mut bundle=json!({"bundleVersion":1,"confirmed":false,"artifactBase64":base64::engine::general_purpose::STANDARD.encode(bytes),
        "manifest":{"sdkVersion":1,"id":"fixture","name":"문서 도우미","version":"1.0.0","description":"Independent test package","runtime":"python",
        "artifact":{"url":"https://example.invalid/entry.py","sha256":format!("{:x}",Sha256::digest(bytes))},
        "tools":[{"name":"echo","description":"Echo an input","parameters":{"type":"object","properties":{"text":{"type":"string"}},"required":["text"]}}]}});
    let rejected=app.client.post(format!("{}/api/extensions/import",app.base)).json(&bundle).send().unwrap();
    assert_eq!(rejected.status(),400);
    bundle["confirmed"]=json!(true);
    let row=app.post("/api/extensions/import",&bundle);
    assert_eq!(row["enabled"],false);
    assert_eq!(app.get("/api/extensions/fixture/status")["integrityOk"],true);
    let name=format!("ext__fixture__echo__{}",row["revision"].as_str().unwrap());
    let advertised=|tools:Value|tools.as_array().unwrap().iter().any(|tool|tool["name"]==name);
    assert!(!advertised(app.get("/api/tools")));
    app.post("/api/extensions/fixture/enable",&json!({"enabled":true,"confirmed":true}));
    assert!(advertised(app.get("/api/tools")));
    app.post("/api/agents",&json!({"id":"a","name":"Extension tester","systemPrompt":"Use available tools and report observed results."}));
    let room=app.post("/api/rooms",&json!({"name":"independent extension","agentIds":["a"],"rounds":1,"maxTurns":1,"toolsEnabled":true,"autoLearning":false}));
    let sent=app.post(&format!("/api/rooms/{}/messages",room["id"].as_str().unwrap()),&json!({"input":"문서 도우미로 실행해"}));
    let approval=app.wait_pending_approval(20);
    assert_eq!(approval["tool"],name);
    app.post("/api/approvals",&json!({"id":approval["id"],"allow":true}));
    let deadline=Instant::now()+Duration::from_secs(25);
    loop{
        let job=app.get(&format!("/api/jobs/{}",sent["jobId"].as_str().unwrap()));
        if job["status"]=="completed"{break;}
        assert!(matches!(job["status"].as_str(),Some("queued"|"running")),"{job}");
        assert!(Instant::now()<deadline,"{job}");
        std::thread::sleep(Duration::from_millis(30));
    }
    let seen=requests.lock().unwrap();
    assert!(seen.iter().any(|r|r["messages"].as_array().unwrap().iter().any(|m|
        m["role"]=="tool"&&m["content"].as_str().is_some_and(|v|v.contains("EXTENSION_ACTUAL_PROCESS")&&v.contains("success")))),"{seen:?}");
    drop(seen);
    app.post("/api/extensions/fixture/enable",&json!({"enabled":false}));
    assert!(!advertised(app.get("/api/tools")));
    app.delete("/api/extensions/fixture");
    assert_eq!(app.get("/api/extensions"),json!([]));
    assert_eq!(app.get("/api/mcp"),json!([]));
}
