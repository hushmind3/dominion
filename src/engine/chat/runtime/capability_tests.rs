//! Live native dispatch regression for progressive discovery. No model API.
use super::*;
use crate::{config::Config,db::Store,vault::Vault};

fn fixture(read_only:bool,discovery_enabled:bool)
    ->(AppState,std::path::PathBuf,TurnRequest){
    let root=std::env::temp_dir()
        .join(format!("dominion-discovery-{}",uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("create workspace");
    let config=Config{port:0,data_dir:root.clone(),
        web_dir:root.join("web"),workspace:root.clone()};
    let store=Store::open(&config.db_path()).expect("open sqlite");
    let vault=Vault::open(&config.key_path());
    let state=AppState::new(config,store,vault);
    state.store.put("session",&json!({
        "id":"search-owner","parallelReadOnly":read_only,
    })).expect("persist session");
    state.store.put("execution",&json!({
        "id":"search-run","sessionId":"search-owner",
        "taskId":"search-task","status":"running",
        "progressiveToolDiscovery":discovery_enabled
    })).expect("persist execution");
    state.store.put("policy",&json!({
        "id":"default","autoApproveAll":true,
    })).expect("persist approval fixture");
    let request=TurnRequest::from_value(&json!({
        "input":"Search for a file tool and inspect a file",
        "sessionId":"search-owner","toolsEnabled":true,
        "toolsExecutionEnabled":true
    })).expect("valid owner request");
    (state,root,request)
}

#[tokio::test]
async fn discover_then_execute_real_file_tool_via_same_native_gateway(){
    let (state,root,request)=fixture(false,true);
    let target=root.join("example.txt");
    std::fs::write(&target,"actual native file").unwrap();
    let (tx,_rx)=tokio::sync::mpsc::unbounded_channel();
    let metrics=std::sync::Mutex::new(Metrics::default());
    let original=tools::definitions();
    assert!(original.len()>18,"discovery fixture needs a real large catalog");
    let mut visible=capability::project(original.clone(),"코드 변경 테스트");
    assert!(!visible.iter().any(|item|tool_name(item)=="file_read"));
    visible.insert(0,capability::definition());
    assert!(super::super::tool_projection::advertised_tool(
        &visible,capability::SEARCH));

    let search_args=json!({"query":"file_read","limit":3});
    let search_call=ToolCall{id:"find-file-reader".into(),
        name:capability::SEARCH.into(),arguments:search_args.to_string()};
    let (result,ok,_)=dispatch_tool_call(
        &state,&tx,&json!({"id":"search-owner"}),&request,
        "search-owner","search-run",&search_call,search_args,
        &metrics,1,0,0,false,"model",None,None,None
    ).await.expect("native discovery");
    assert!(ok,"{result}");
    assert!(result["matches"].as_array().unwrap()
        .iter().any(|item|item["name"]=="file_read"),"{result}");
    assert!(capability::promote(&mut visible,&original,&result)>0);
    assert!(super::super::tool_projection::advertised_tool(&visible,"file_read"));

    let arguments=json!({"path":target});
    let read_call=ToolCall{id:"read-discovered".into(),
        name:"file_read".into(),arguments:arguments.to_string()};
    let (read,allowed,_)=dispatch_tool_call(
        &state,&tx,&json!({"id":"search-owner"}),&request,
        "search-owner","search-run",&read_call,arguments,
        &metrics,2,1,0,false,"model",None,None,None
    ).await.expect("native file read");
    assert!(allowed,"{read}");
    assert_eq!(read["content"],"actual native file");
    let operations=state.store.execution_operations("search-run").unwrap();
    assert_eq!(operations.len(),2);
    assert_eq!(operations[0]["tool"],capability::SEARCH);
    assert_eq!(operations[0]["status"],"completed");
    assert_eq!(operations[1]["tool"],"file_read");
    assert_eq!(operations[1]["status"],"completed");
    assert_eq!(metrics.lock().unwrap().tool_calls,2);
    let messages=state.store.messages("search-owner");
    assert_eq!(messages.len(),2);
    assert!(messages.iter().all(|msg|msg["role"]=="tool"));
    let _=std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn discovery_rejects_foreign_execution_and_read_only_mutations(){
    let (state,root,_request)=fixture(true,true);
    state.store.put("execution",&json!({
        "id":"other-run","sessionId":"another-user",
        "progressiveToolDiscovery":true
    })).unwrap();
    assert!(capability::search(&state,"search-owner","other-run",
        &json!({"query":"workspace_read"})).await.is_err());
    let (tx,_rx)=tokio::sync::mpsc::unbounded_channel();
    let metrics=std::sync::Mutex::new(Metrics::default());
    let request=TurnRequest::from_value(&json!({
        "input":"Find a read-only tool",
        "sessionId":"search-owner","toolsEnabled":true,
        "toolsExecutionEnabled":true
    })).unwrap();
    let args=json!({"query":"workspace_write read","limit":8});
    let call=ToolCall{id:"safe-search".into(),
        name:capability::SEARCH.into(),arguments:args.to_string()};
    // This must traverse the actual dispatcher, which checks the same
    // read-only admission policy as an ordinary subagent tool.
    let (results,ok,_)=dispatch_tool_call(
        &state,&tx,&json!({"id":"search-owner"}),&request,
        "search-owner","search-run",&call,args,&metrics,
        1,0,0,false,"model",None,None,None,
    ).await.expect("read-only discovery through native gate");
    assert!(ok,"{results}");
    assert!(results["matches"].as_array().unwrap().iter().all(|result|
        result["name"]!="workspace_write"
        && result["name"]!="shell_exec"),"{results}");

    let target=root.join("must-not-write.txt");
    let write_args=json!({"path":"must-not-write.txt","content":"unsafe"});
    let write=ToolCall{id:"unsafe-write".into(),
        name:"workspace_write".into(),arguments:write_args.to_string()};
    let (denied,allowed,_)=dispatch_tool_call(
        &state,&tx,&json!({"id":"search-owner"}),&request,
        "search-owner","search-run",&write,write_args,&metrics,
        2,0,1,false,"model",None,None,None,
    ).await.expect("native read-only rejection receipt");
    assert!(!allowed,"{denied}");
    assert!(!target.exists());
    assert!(capability::search(&state,"search-owner","search-run",
        &json!({"query":"", "limit":9})).await.is_err());
    let _=std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn progressive_discovery_must_be_enabled_for_this_execution(){
    let (state,root,_request)=fixture(false,false);
    let rejected=capability::search(&state,"search-owner","search-run",
        &json!({"query":"file_read"})).await;
    assert!(rejected.is_err());
    assert!(state.store.execution_operations("search-run")
        .unwrap().is_empty());
    let _=std::fs::remove_dir_all(root);
}
