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
        workspace:root.clone(),model_name:"mock".into()
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
async fn wrong_multifile_repair_receives_real_test_failure_and_fixes_it(){
    let traffic=Arc::new(Mutex::new(Vec::<Value>::new()));
    let inspected=traffic.clone();
    let (url,server)=server_for(move |turn,body|{
        inspected.lock().unwrap().push(body);
        match turn{
            0=>model_message(vec![
                function_call("read-source","workspace_read",json!({"path":"calc.js"})),
                function_call("read-test","workspace_read",json!({"path":"calc.test.js"}))
            ]),
            1=>edit("wrong-fix","x + 1","x + 2"),
            2=>final_message("The code is finished."),
            3=>edit("fix-diagnostic","x + 2","x + 3"),
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
