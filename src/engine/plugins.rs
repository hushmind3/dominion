//! Optional domain plug-in boundary: host owns execution, storage, permissions,
//! budgets and restarts. Money is a toolset, not another runtime or agent loop.
//! Adding a domain must not attach its collectors to the global startup path.
use crate::{state::AppState,engine::jobs::JobContext};
use serde_json::{json,Value};

const MONEY_SCHEDULE_ID:&str="money:scheduler";

/// The domain is not activated by opening the general product.
pub fn start(state:&AppState){
    // Upgrade a pre-existing, explicitly enabled Money autopilot without
    // silently enabling Money collection for other users on restart.
    if state.store.get("money_plugin",MONEY_SCHEDULE_ID).is_none() {
        let legacy_active=state.store.all("money_autopilot").iter()
            .any(|item|item["enabled"]==true);
        if legacy_active {let _=sync_money_schedule(state);}
    }
    maintenance_tick(state);
}

/// This is called by the existing host heartbeat; it is a no-op when off.
pub fn maintenance_tick(state:&AppState){
    if !state.store.get("money_plugin",MONEY_SCHEDULE_ID)
        .is_some_and(|row|row["enabled"]==true){return;}
    // The stored plug-in marker is a hint, not independent authority.
    // A stopped/expired campaign must not leave a perpetual collector alive.
    if !state.store.all("money_autopilot").iter().any(|row|row["enabled"]==true){
        let _=sync_money_schedule(state);
        return;
    }
    // The scheduler may stop the last active campaign on a deadline, job
    // failure or round quota. Check again before requesting any new sources.
    crate::engine::money::schedule_autopilot(state);
    if state.store.all("money_autopilot").iter().any(|row|row["enabled"]==true){
        refresh_money(state);
    }else{
        let _=sync_money_schedule(state);
    }
}

/// Explicit, read-only research acquisition on a user's Money button click.
/// All work uses the existing durable job queue, not a plug-in worker loop.
pub fn refresh_money(state:&AppState){
    crate::engine::money_health::sample(state);
    crate::engine::money_sources::schedule(state);
    crate::engine::money_catalog::schedule(state);
    crate::engine::money_folding::schedule(state);
}

/// Only opt-in autopilots may keep Money research alive in the background.
/// Update this inexpensive marker on user enable/disable, not every tick.
pub fn sync_money_schedule(state:&AppState)->Result<(),String>{
    let enabled=state.store.all("money_autopilot").iter()
        .any(|row|row["enabled"]==true);
    state.store.put("money_plugin",&json!({
        "id":MONEY_SCHEDULE_ID,"enabled":enabled,
        "updatedAt":chrono::Utc::now().to_rfc3339()
    })).ok_or("Could not persist Money plug-in scheduler state".into())
        .map(|_|())
}

/// Host job types are domain-registered; the domain never owns job workers.
pub fn supports(kind:&str)->bool {
    matches!(kind,"money_discover"|"money_ingest"|"money_catalog"|"money_fold")
}
pub async fn run(context:&JobContext,job:&Value)->Result<Value,String>{
    let kind=job["kind"].as_str().ok_or("Money job kind missing")?;
    let result=match kind {
        "money_discover"=>crate::engine::money::run(context,job).await,
        "money_ingest"=>crate::engine::money_sources::run(context,job).await,
        "money_catalog"=>crate::engine::money_catalog::run(context,job).await,
        "money_fold"=>crate::engine::money_folding::run(context,job).await,
        _=>return Err(format!("Unsupported Money tool: {kind}")),
    }?;
    // Event-driven processing: newly collected evidence is folded without
    // requiring a permanent second scanner or waiting for an auto-loop.
    if matches!(kind,"money_ingest"|"money_catalog"){
        crate::engine::money_folding::schedule(&context.state);
    }
    Ok(result)
}

#[cfg(test)]
mod tests{
    use super::*;
    use crate::{config::Config,db::Store,vault::Vault};
    fn fixture()->AppState{
        let root=std::env::temp_dir().join(format!("money-plugin-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let config=Config{port:0,data_dir:root.join("data"),web_dir:root.join("web"),
            workspace:root};
        std::fs::create_dir_all(&config.data_dir).unwrap();
        AppState::new(config.clone(),Store::open(&config.db_path()).unwrap(),
            Vault::open(&config.key_path()))
    }
    #[test]
    fn dormant_money_plugin_never_enqueues_on_host_start_or_maintenance(){
        let state=fixture();
        start(&state);
        maintenance_tick(&state);
        assert!(state.store.all("job").into_iter().all(|j|
            !j["kind"].as_str().unwrap_or("").starts_with("money_")));
        assert!(state.store.get("money_plugin",MONEY_SCHEDULE_ID).is_none());
        assert!(supports("money_discover"));
        assert!(!supports("prompt"));
    }
    #[test]
    fn explicit_money_research_uses_existing_queue_and_deduplicates(){
        let state=fixture();
        refresh_money(&state);
        refresh_money(&state);
        let jobs=state.store.all("job");
        assert_eq!(jobs.iter().filter(|j|j["kind"]=="money_ingest").count(),1);
        assert_eq!(jobs.iter().filter(|j|j["kind"]=="money_catalog").count(),1);
    }
    #[test]
    fn stale_scheduler_marker_self_disables_without_launching_collectors(){
        let state=fixture();
        state.store.put("money_plugin",&json!({"id":MONEY_SCHEDULE_ID,
            "enabled":true})).unwrap();
        state.store.put("money_autopilot",&json!({"id":"c1",
            "enabled":false,"runId":"stopped"})).unwrap();
        maintenance_tick(&state);
        assert_eq!(state.store.get("money_plugin",MONEY_SCHEDULE_ID).unwrap()["enabled"],false);
        assert!(state.store.all("job").iter().all(|j|
            !j["kind"].as_str().unwrap_or("").starts_with("money_")));
    }
    #[test]
    fn expired_campaign_stops_without_rearming_money_collection(){
        let state=fixture();
        state.store.put("money_campaign",&json!({"id":"c1","name":"expired"})).unwrap();
        state.store.put("money_autopilot",&json!({"id":"c1","enabled":true,
            "continuous":false,"runId":"run-1",
            "endsAt":(chrono::Utc::now()-chrono::Duration::minutes(1)).to_rfc3339()})).unwrap();
        sync_money_schedule(&state).unwrap();
        maintenance_tick(&state);
        assert_eq!(state.store.get("money_autopilot","c1").unwrap()["stopReason"],"deadline");
        assert_eq!(state.store.get("money_plugin",MONEY_SCHEDULE_ID).unwrap()["enabled"],false);
        assert!(state.store.all("job").iter().all(|j|
            !j["kind"].as_str().unwrap_or("").starts_with("money_")));
    }
    #[test]
    fn only_explicitly_enabled_autopilot_activates_money_scheduler(){
        let state=fixture();
        state.store.put("money_autopilot",&json!({"id":"c1","enabled":true,
            "runId":"r1"})).unwrap();
        sync_money_schedule(&state).unwrap();
        assert_eq!(state.store.get("money_plugin",MONEY_SCHEDULE_ID).unwrap()["enabled"],true);
        state.store.put("money_autopilot",&json!({"id":"c1","enabled":false,
            "runId":"r1"})).unwrap();
        sync_money_schedule(&state).unwrap();
        assert_eq!(state.store.get("money_plugin",MONEY_SCHEDULE_ID).unwrap()["enabled"],false);
    }
}
