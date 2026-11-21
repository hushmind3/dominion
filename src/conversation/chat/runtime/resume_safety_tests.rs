    use super::*;
    use serde_json::json;

    fn fixture()->(AppState,std::path::PathBuf){
        let root=std::env::temp_dir().join(format!("sub3-resume-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let config=crate::config::Config{port:0,data_dir:root.join("data"),
            web_dir:root.join("web"),workspace:root.join("work")};
        let state=AppState::new(config.clone(),
            crate::db::Store::open(&config.db_path()).unwrap(),
            crate::vault::Vault::open(&config.key_path()));
        (state,root)
    }

    #[test]
    fn paid_model_unknown_is_not_relaunched_by_chat_task_resume(){
        let (state,root)=fixture();
        state.store.put("execution",&json!({
            "id":"old-paid","taskId":"task-A","sessionId":"s",
            "status":"failed","kernelState":{"modelInFlight":true,
                "modelCalls":1,"chargedTokens":18,"inFlight":[]}
        })).unwrap();
        assert!(ensure_task_resume_safe(&state,"task-A").is_err());
        assert!(ensure_task_resume_safe(&state,"missing-task").is_err());
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn unknown_tool_operation_blocks_resume_even_with_a_clean_snapshot(){
        let (state,root)=fixture();
        state.store.put("execution",&json!({
            "id":"old-effect","taskId":"task-B","sessionId":"s",
            "status":"failed","kernelState":{"modelInFlight":false,"inFlight":[]}
        })).unwrap();
        assert!(ensure_task_resume_safe(&state,"task-B").is_ok());
        state.store.put("execution_operation",&json!({
            "id":"old-effect-op","runId":"old-effect","status":"interrupted",
            "tool":"workspace_write","outcomeUnknown":true
        })).unwrap();
        assert!(ensure_task_resume_safe(&state,"task-B").is_err(),
            "The operation journal must override a clean-looking snapshot");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn prior_in_flight_tool_blocks_replay_without_a_host_operation(){
        let (state,root)=fixture();
        state.store.put("execution",&json!({
            "id":"old-tool","taskId":"task-C","sessionId":"s",
            "status":"cancelled","kernelState":{"modelInFlight":false,
                "inFlight":[{"id":"pending","tool":"workspace_write","arguments":{}}]}
        })).unwrap();
        assert!(ensure_task_resume_safe(&state,"task-C").is_err());
        let _=std::fs::remove_dir_all(root);
    }
