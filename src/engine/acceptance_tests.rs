//! Real-process coding acceptance tests. Mock only the model's decisions;
//! edits, Node test runs, feedback, SQLite receipts and termination are real.
use super::*;
use axum::{routing::post,Json,Router};
use std::sync::{Arc,Mutex,atomic::{AtomicUsize,Ordering}};

fn function_call(id:&str,name:&str,args:Value)->Value{
    json!({"id":id,"type":"function","function":{
        "name":name,"arguments":args.to_string()
    }})
}
fn model_message(calls:Vec<Value>)->Value{
    json!({"role":"assistant","content":null,"tool_calls":calls})
}
fn final_message(text:&str)->Value{
    json!({"role":"assistant","content":text})
}
async fn server_for<F>(reply:F)->(String,tokio::task::JoinHandle<()>)
where F:Fn(usize,Value)->Value+Clone+Send+Sync+'static {
    let called=Arc::new(AtomicUsize::new(0));
    let service=Router::new().route("/v1/chat/completions",post(
        move |Json(body):Json<Value>|{
            let called=called.clone();
            let reply=reply.clone();
            async move{
                let turn=called.fetch_add(1,Ordering::SeqCst);
                Json(json!({"choices":[{"message":reply(turn,body)}],
                    "usage":{"prompt_tokens":240,"completion_tokens":70}}))
            }
        }
    ));
    let socket=tokio::net::TcpListener::bind(("127.0.0.1",0)).await.unwrap();
    let url=format!("http://{}/v1/chat/completions",socket.local_addr().unwrap());
    let handle=tokio::spawn(async move{axum::serve(socket,service).await.unwrap()});
    (url,handle)
}
async fn fixture(url:String,initial:&str)->(AppState,std::path::PathBuf){
    let root=std::env::temp_dir().join(format!("dominion-code-verify-{}",uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("calc.js"),
        format!("module.exports = x => x + {initial};\n")).unwrap();
    std::fs::write(root.join("calc.test.js"),
        "const test = require('node:test');\n\
         const assert = require('node:assert/strict');\n\
         const calc = require('./calc.js');\n\
         test('calculator should add three', () => assert.equal(calc(2), 5));\n").unwrap();
    let store=crate::store::Store::open(&root.join("native.sqlite")).unwrap();
    let (bus,_)=tokio::sync::broadcast::channel(128);
    let state=AppState{
        store,bus,
        mcp:Arc::new(crate::mcp::Mcp::new(vec![]).unwrap()),
        groups:Arc::new(crate::group::Groups::open(root.join("groups")).await.unwrap()),
        videos:Arc::new(crate::video::Videos::open(root.join("video")).await.unwrap()),
        improvements:Arc::new(crate::improvement::Improvements::open(
            root.join("improvements"),root.clone()).await.unwrap()),
        provider:crate::provider::ModelClient::new(
            crate::provider::ModelConfig::basic(url,"mock".into(),None),None).unwrap(),
        workspace:root.clone(),workspace_write_lock:Arc::new(tokio::sync::Mutex::new(())),
        model_name:"mock".into()
    };
    (state,root)
}
fn input(max_steps:usize)->Value{
    json!({
        "prompt":"Fix calc.js","allowWrite":true,"allowShell":true,
        "requireEvidence":true,"planningMode":"never",
        "modelTier":"quality","maxSteps":max_steps,"tokenBudget":12000,
        "acceptanceChecks":[{"program":"node","args":["--test","calc.test.js"]}]
    })
}
fn edit(id:&str,from:&str,to:&str)->Value{
    model_message(vec![function_call(id,"workspace_replace",
        json!({"path":"calc.js","old":from,"new":to}))])
}
fn trace<'a>(state:&'a AppState,id:&str)->Vec<Value>{
    state.store.events_after(id,0,300)
}
#[tokio::test]
async fn concurrent_coding_tasks_hold_the_workspace_lease_until_source_acceptance(){
    use tokio::time::{sleep,Duration};
    let active=Arc::new(AtomicUsize::new(0));
    let peak=Arc::new(AtomicUsize::new(0));
    let observed=active.clone();
    let max_seen=peak.clone();
    let app=Router::new().route("/v1/chat/completions",post(
        move |Json(body):Json<Value>|{
            let observed=observed.clone();
            let max_seen=max_seen.clone();
            async move{
                let running=observed.fetch_add(1,Ordering::SeqCst)+1;
                max_seen.fetch_max(running,Ordering::SeqCst);
                sleep(Duration::from_millis(55)).await;
                observed.fetch_sub(1,Ordering::SeqCst);
                let messages=body["messages"].as_array().cloned().unwrap_or_default();
                let finished=messages.iter().any(|item|item["role"]=="tool");
                let prompt=messages.iter().find(|m|m["role"]=="user")
                    .and_then(|m|m["content"].as_str()).unwrap_or("");
                let file=if prompt.contains("one.txt"){"one.txt"}else{"two.txt"};
                let message=if finished{
                    final_message("Wrote requested workspace output.")
                }else{model_message(vec![function_call(file,"workspace_write",
                    json!({"path":file,"content":file}))])};
                Json(json!({"choices":[{"message":message}],
                    "usage":{"prompt_tokens":150,"completion_tokens":30}}))
            }
        }
    ));
    let listener=tokio::net::TcpListener::bind(("127.0.0.1",0)).await.unwrap();
    let url=format!("http://{}/v1/chat/completions",listener.local_addr().unwrap());
    let server=tokio::spawn(async move{axum::serve(listener,app).await.unwrap()});
    let (state,root)=fixture(url,"1").await;
    let (first,_)=state.store.submit("lease-one-2026",&json!({
        "prompt":"Create one.txt","allowWrite":true,
        "requireEvidence":true,"planningMode":"never","maxSteps":4
    })).unwrap();
    let (second,_)=state.store.submit("lease-two-2026",&json!({
        "prompt":"Create two.txt","allowWrite":true,
        "requireEvidence":true,"planningMode":"never","maxSteps":4
    })).unwrap();
    let a=first["id"].as_str().unwrap().to_string();
    let b=second["id"].as_str().unwrap().to_string();
    tokio::join!(run(state.clone(),a.clone()),run(state.clone(),b.clone()));
    assert_eq!(state.store.get(&a).unwrap()["status"],"completed");
    assert_eq!(state.store.get(&b).unwrap()["status"],"completed");
    assert_eq!(peak.load(Ordering::SeqCst),1,
        "No model/edit/verification of a second writer may interleave");
    assert_eq!(std::fs::read_to_string(root.join("one.txt")).unwrap(),"one.txt");
    assert_eq!(std::fs::read_to_string(root.join("two.txt")).unwrap(),"two.txt");
    server.abort();
    let _=std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn native_agent_can_change_two_files_then_pass_real_joint_acceptance(){
    let (url,server)=server_for(move |turn,_body|match turn{
        0=>model_message(vec![
            function_call("read-a","workspace_read_lines",json!({
                "path":"a.js","startLine":1,"maxLines":40
            })),
            function_call("read-b","workspace_read_lines",json!({
                "path":"b.js","startLine":1,"maxLines":40
            }))
        ]),
        1=>model_message(vec![function_call("atomic-update","workspace_replace_many",
            json!({"edits":[
                {"path":"a.js","old":"x + 1","new":"x + 3"},
                {"path":"b.js","old":"x * 2","new":"x * 4"}
            ]}))]),
        _=>final_message("Both source modules were corrected and tested.")
    }).await;
    let (state,root)=fixture(url,"1").await;
    std::fs::write(root.join("a.js"),"module.exports = x => x + 1;\n").unwrap();
    std::fs::write(root.join("b.js"),"module.exports = x => x * 2;\n").unwrap();
    std::fs::write(root.join("joint.test.js"),
        "const test = require('node:test');\n\
         const assert = require('node:assert/strict');\n\
         const a = require('./a.js'); const b = require('./b.js');\n\
         test('both source edits are valid',()=>{assert.equal(a(2),5);assert.equal(b(2),8)});\n"
    ).unwrap();
    let mut request=input(5);
    request["acceptanceChecks"]=json!([{
        "program":"node","args":["--test","joint.test.js"]
    }]);
    let (task,_)=state.store.submit("multi-file-coding-2026",&request).unwrap();
    let id=task["id"].as_str().unwrap().to_string();
    run(state.clone(),id.clone()).await;
    let record=state.store.get(&id).unwrap();
    assert_eq!(record["status"],"completed","{record}");
    assert_eq!(record["result"]["verifiedWrites"],2);
    assert_eq!(record["result"]["acceptance"]["passed"],true);
    assert_eq!(record["result"]["verification"],
        "source_sha256_and_fresh_command_checks");
    assert!(std::fs::read_to_string(root.join("a.js")).unwrap().contains("x + 3"));
    assert!(std::fs::read_to_string(root.join("b.js")).unwrap().contains("x * 4"));
    let events=trace(&state,&id);
    assert!(events.iter().any(|e|e["type"]=="tool_result" &&
        e["detail"]["name"]=="workspace_replace_many" &&
        e["detail"]["outcome"]["receipt"]["fileCount"]==2));
    assert!(events.iter().any(|e|e["type"]=="acceptance_result" &&
        e["detail"]["success"]==true));
    assert!(!std::fs::read_dir(&root).unwrap().filter_map(Result::ok)
        .any(|f|f.file_name().to_string_lossy().starts_with(".dominion-stage-")));
    server.abort();
    let _=std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn wrong_multifile_repair_receives_real_test_failure_and_fixes_it(){
    let traffic=Arc::new(Mutex::new(Vec::<Value>::new()));
    let inspected=traffic.clone();
    let (url,server)=server_for(move |turn,body|{
        let diagnostic=body["messages"].as_array().is_some_and(|history|
            history.iter().any(|entry|entry["role"]=="user" &&
                entry["content"].as_str().is_some_and(|text|
                    text.contains("NATIVE CODE VERIFICATION FAILED") &&
                    (text.contains("ERR_ASSERTION")||text.contains("not ok")))));
        inspected.lock().unwrap().push(body);
        match turn{
            0=>model_message(vec![
                function_call("read-source","workspace_read",json!({"path":"calc.js"})),
                function_call("read-test","workspace_read",json!({"path":"calc.test.js"}))
            ]),
            1=>edit("wrong-fix","x + 1","x + 2"),
            2=>final_message("The code is finished."),
            3 if diagnostic=>edit("fix-diagnostic","x + 2","x + 3"),
            3=>final_message("I could not diagnose the failing check."),
            _=>final_message("The corrected source passed its real tests.")
        }
    }).await;
    let (state,root)=fixture(url,"1").await;
    let (task,_)=state.store.submit("real-node-correction-2026",&input(7)).unwrap();
    let id=task["id"].as_str().unwrap().to_owned();
    run(state.clone(),id.clone()).await;
    let record=state.store.get(&id).unwrap();
    assert_eq!(record["status"],"completed","{record}");
    assert_eq!(std::fs::read_to_string(root.join("calc.js")).unwrap(),
        "module.exports = x => x + 3;\n");
    let events=trace(&state,&id);
    let results=events.iter().filter(|event|
        event["type"]=="acceptance_result").collect::<Vec<_>>();
    assert_eq!(results.len(),2);
    assert_eq!(results[0]["detail"]["success"],false);
    assert_eq!(results[1]["detail"]["success"],true);
    assert!(events.iter().any(|event|event["type"]=="repair_required"));
    assert!(events.iter().any(|event|event["type"]=="acceptance_complete"));
    let prompts=traffic.lock().unwrap();
    assert_eq!(prompts.len(),5,"should fix in the same task");
    let repair=prompts[3]["messages"].as_array().unwrap().iter()
        .filter_map(|message|message["content"].as_str())
        .collect::<Vec<_>>().join("\n");
    assert!(repair.contains("NATIVE CODE VERIFICATION FAILED"));
    assert!(repair.contains("calc.test.js"));
    assert!(repair.contains("ERR_ASSERTION")||repair.contains("not ok"));
    server.abort();
    let _=std::fs::remove_dir_all(root);
}
#[tokio::test]
async fn repeated_final_claim_cannot_recheck_unchanged_failing_source(){
    let turns=Arc::new(AtomicUsize::new(0));
    let calls=turns.clone();
    let (url,server)=server_for(move |turn,_body|{
        calls.fetch_add(1,Ordering::SeqCst);
        if turn==0{edit("bad-once","x + 1","x + 2")}
        else{final_message("I have fixed everything.")}
    }).await;
    let (state,root)=fixture(url,"1").await;
    let (task,_)=state.store.submit("unchanged-bad-check-2026",&input(4)).unwrap();
    let id=task["id"].as_str().unwrap().to_string();
    run(state.clone(),id.clone()).await;
    let record=state.store.get(&id).unwrap();
    assert_eq!(record["status"],"needs_review","{record}");
    assert_eq!(std::fs::read_to_string(root.join("calc.js")).unwrap(),
        "module.exports = x => x + 2;\n");
    let results=trace(&state,&id).into_iter()
        .filter(|event|event["type"]=="acceptance_result").collect::<Vec<_>>();
    assert_eq!(results.len(),1,
        "same failing revision must not repeat an expensive test");
    assert_eq!(results[0]["detail"]["success"],false);
    assert_eq!(turns.load(Ordering::SeqCst),4);
    server.abort();
    let _=std::fs::remove_dir_all(root);
}
#[tokio::test]
async fn old_green_test_is_invalidated_by_newer_bad_write(){
    let (url,server)=server_for(move |turn,_body|match turn{
        0=>model_message(vec![function_call("green-before-edit","workspace_check",
            json!({"program":"node","args":["--test","calc.test.js"]}))]),
        1=>edit("break-after-green","x + 3","x + 2"),
        _=>final_message("The earlier test passed; we are done.")
    }).await;
    let (state,root)=fixture(url,"3").await;
    let (task,_)=state.store.submit("stale-green-2026",&input(3)).unwrap();
    let id=task["id"].as_str().unwrap().to_string();
    run(state.clone(),id.clone()).await;
    let record=state.store.get(&id).unwrap();
    assert_eq!(record["status"],"needs_review","{record}");
    let events=trace(&state,&id);
    assert!(events.iter().any(|event|event["type"]=="acceptance_result" &&
        event["detail"]["success"]==false));
    let green=events.iter().find(|event|event["type"]=="tool_result" &&
        event["detail"]["callId"]=="green-before-edit").unwrap();
    assert_eq!(green["detail"]["outcome"]["receipt"]["success"],true);
    server.abort();
    let _=std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn green_test_that_rewrites_source_is_rejected_at_real_native_completion(){
    let (url,server)=server_for(move |turn,_body|match turn{
        0=>edit("repair","x + 1","x + 3"),
        _=>final_message("All tests passed; task complete.")
    }).await;
    let (state,root)=fixture(url,"1").await;
    std::fs::write(root.join("calc.test.js"),
        "const fs=require('node:fs');\n\
         const test=require('node:test');\n\
         fs.writeFileSync('calc.js','module.exports = x => x + 99;\\n');\n\
         test('fake-green',()=>{});\n").unwrap();
    let (task,_)=state.store.submit("fake-green-2026",&input(3)).unwrap();
    let id=task["id"].as_str().unwrap().to_string();
    run(state.clone(),id.clone()).await;
    let record=state.store.get(&id).unwrap();
    assert_eq!(record["status"],"needs_review","{record}");
    assert_ne!(record["result"]["verification"],
        "source_sha256_and_fresh_command_checks");
    let events=trace(&state,&id);
    let proof=events.iter().find(|event|
        event["type"]=="acceptance_result").expect("real node test ran");
    assert_eq!(proof["detail"]["success"],false);
    assert_eq!(proof["detail"]["exitCode"],0);
    let receipt=events.iter().find(|event|event["type"]=="tool_result" &&
        event["detail"]["phase"]=="native_acceptance").unwrap();
    assert_eq!(receipt["detail"]["outcome"]["receipt"]["sourceUnchanged"],false);
    assert!(receipt["detail"]["outcome"]["receipt"]["stderr"]
        .as_str().unwrap().contains("SOURCE INTEGRITY FAILURE"));
    server.abort();
    let _=std::fs::remove_dir_all(root);
}
