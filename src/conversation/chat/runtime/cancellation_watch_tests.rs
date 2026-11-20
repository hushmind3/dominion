    use super::*;

    fn fixture()->(AppState,std::path::PathBuf) {
        let dir=std::env::temp_dir().join(format!("dominion-cancel-watch-{}",
            uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config=crate::config::Config{port:0,data_dir:dir.clone(),
            web_dir:dir.join("web"),workspace:dir.clone()};
        let state=AppState::new(config.clone(),
            crate::db::Store::open(&config.db_path()).unwrap(),
            crate::vault::Vault::open(&config.key_path()));
        (state,dir)
    }

    #[tokio::test]
    async fn local_cancel_notifies_a_running_turn_without_waiting_for_poll() {
        let (state,dir)=fixture();
        let generation=state.cancellation_generation("s");
        let waiting=state.clone();
        let task=tokio::spawn(async move {
            wait_cancelled(&waiting,"s",generation).await;
        });
        tokio::task::yield_now().await;
        state.cancel_session("s");
        tokio::time::timeout(Duration::from_secs(2),task).await
            .expect("live cancellation must wake").unwrap();
        let _=std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn durable_cancel_without_local_signal_uses_bounded_reconciliation() {
        let (state,dir)=fixture();
        let generation=state.cancellation_generation("external");
        let waiting=state.clone();
        let task=tokio::spawn(async move {
            wait_cancelled(&waiting,"external",generation).await;
        });
        tokio::task::yield_now().await;
        state.store.cancel_session_epoch("external").unwrap();
        tokio::time::timeout(Duration::from_secs(2),task).await
            .expect("external cancellation must be reconciled").unwrap();
        let _=std::fs::remove_dir_all(dir);
    }
