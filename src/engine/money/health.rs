//! Money-specific process samples. A sampled dispatcher heartbeat proves
//! only that the scheduler passed this code path, not that a model was online.
use crate::state::AppState;
use serde_json::json;

fn bucket_id(at:i64)->String{
    format!("money:heartbeat:{}",at.div_euclid(300))
}
pub fn sample(state:&AppState){
    let timestamp=chrono::Utc::now();
    let id=bucket_id(timestamp.timestamp());
    // Preserve one row per 5-minute bucket across restarts and competing workers.
    if state.store.get("money_heartbeat",&id).is_none(){
        let _=state.store.put("money_heartbeat",&json!({
            "id":id,"sampledAt":timestamp.to_rfc3339(),
            "periodSeconds":300,"source":"money_scheduler_process_only"
        }));
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn heartbeat_id_is_stable_per_bucket_even_after_restart(){
        assert_eq!(bucket_id(300),bucket_id(599));
        assert_ne!(bucket_id(299),bucket_id(300));
    }
}
