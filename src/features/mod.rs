//! Feature packages compose around the minimal executor; no agent loop here.
pub mod cognition;
pub mod collaboration;
pub mod providers;
pub mod platform;
pub mod forensics;

/// Non-core tasks are dispatched only when their corresponding job is enqueued.
/// The executor never embeds learning, reflection or group-meeting logic.
pub async fn dispatch_job(
    ctx:&crate::engine::jobs::JobContext,job:&serde_json::Value,kind:&str
)->Option<Result<serde_json::Value,String>>{
    match kind {
        "room"|"meeting" => Some(collaboration::rooms::run(ctx,job).await),
        "learn" => Some(cognition::learning::run(ctx,job).await),
        "reflect" => Some(cognition::reflect::run(ctx,job).await),
        _=>None,
    }
}

/// Optional capability startup belongs to host composition, not the core worker.
pub fn recover(state:&crate::state::AppState){
    platform::wake::recover(state);
    cognition::algorithms::seed(state);
}
pub fn start(state:crate::state::AppState){platform::maintenance::start(state);}
