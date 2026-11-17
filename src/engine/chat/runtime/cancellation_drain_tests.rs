//! Co-located tests for the decomposed chat execution path.
use super::*;
    use std::sync::{Arc,atomic::{AtomicBool,Ordering}};
    fn state()->(AppState,std::path::PathBuf){
        let dir=std::env::temp_dir().join(format!("dominion-cancel-{}",uuid::Uuid::new_v4()));
        let config=crate::config::Config{
            port:0,data_dir:dir.join("db"),web_dir:dir.join("web"),workspace:dir.clone(),
        };
        let state=AppState::new(config.clone(),
            crate::db::Store::open(&config.db_path()).unwrap(),
            crate::vault::Vault::open(&config.key_path()));
        (state,dir)
    }
    #[tokio::test]
    async fn cancellation_drains_inflight_tool_and_preserves_completed_receipt(){
        let (state,dir)=state();
        let ready=Arc::new(AtomicBool::new(false));
        let started=ready.clone();
        let update=state.clone();
        let (cancelled_tx,cancelled_rx)=tokio::sync::oneshot::channel();
        let future=async move{
            update.store.put("execution_operation",&json!({
                "id":"operation-1","runId":"run-1","status":"running",
                "effectState":"started","tool":"github_api",
            }));
            started.store(true,Ordering::SeqCst);
            // Must not complete before cancellation is actually recorded.
            let _=cancelled_rx.await;
            tokio::time::sleep(Duration::from_millis(80)).await;
            update.store.patch_if("execution_operation","operation-1",
                "status",&json!("running"),&json!({
                    "status":"completed","effectState":"applied","result":{"ok":true},
                }));
            Ok(None)
        };
        let cancel=state.clone();
        let seen=ready.clone();
        // An explicit handshake guarantees cancellation was committed
        // before the synthetic tool settles on slow Windows runners.
        // Without this barrier an overloaded scheduler may finish the 80ms
        // tool before polling the cancellation task at all.
        tokio::spawn(async move{
            while !seen.load(Ordering::SeqCst){
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
            cancel.cancel_session("session-1");
            let _=cancelled_tx.send(());
        });
        let generation=state.cancellation_generation("session-1");
        let result=supervise_tool_cancellation(
            &state,"session-1","run-1",generation,Duration::from_secs(2),future,
        ).await;
        assert!(result.unwrap_err().contains("중단"));
        let operation=state.store.get("execution_operation","operation-1").unwrap();
        assert_eq!(operation["status"],"completed");
        assert_ne!(operation["outcomeUnknown"],true);
        let _=std::fs::remove_dir_all(dir);
    }
    #[tokio::test]
    async fn idle_model_cancel_is_immediate_and_does_not_fabricate_tool_results(){
        let (state,dir)=state();
        let cancel=state.clone();
        let generation=state.cancellation_generation("session-1");
        tokio::spawn(async move{
            tokio::time::sleep(Duration::from_millis(12)).await;
            cancel.cancel_session("session-1");
        });
        let start=std::time::Instant::now();
        let result=supervise_tool_cancellation(
            &state,"session-1","run-2",generation,
            Duration::from_secs(2),
            std::future::pending::<Result<Option<Value>,String>>(),
        ).await;
        assert!(result.unwrap_err().contains("중단"));
        assert!(start.elapsed()<Duration::from_secs(1));
        assert!(state.store.all("execution_operation").is_empty());
        let _=std::fs::remove_dir_all(dir);
    }
    #[tokio::test]
    async fn timeout_seals_unknown_outcomes_without_replaying_effects(){
        let (state,dir)=state();
        let future=async{
            state.store.put("execution_operation",&json!({
                "id":"operation-3","runId":"run-3","status":"running",
                "effectState":"started","tool":"mcp__external__write",
            }));
            std::future::pending::<Result<Option<Value>,String>>().await
        };
        let generation=state.cancellation_generation("session-3");
        let error=supervise_tool_cancellation(
            &state,"session-3","run-3",generation,
            Duration::from_millis(20),future,
        ).await.unwrap_err();
        assert!(error.contains("시간 제한"));
        let op=state.store.get("execution_operation","operation-3").unwrap();
        assert_eq!(op["status"],"needs_review");
        assert_eq!(op["outcomeUnknown"],true);
        assert_eq!(op["effectState"],"effect_unknown");
        let _=std::fs::remove_dir_all(dir);
    }

