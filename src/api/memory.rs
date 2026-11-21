use crate::state::AppState;
use axum::{extract::{State},Json};
use serde_json::{json,Value};

pub async fn memory_overview(State(state): State<AppState>) -> Json<Value> {
    let mut by_type = std::collections::BTreeMap::<String, usize>::new();
    let mut by_source = std::collections::BTreeMap::<String, usize>::new();
    let mut by_status = std::collections::BTreeMap::<String, usize>::new();
    let mut by_scope = std::collections::BTreeMap::<String, usize>::new();
    for (field,key,count) in state.store.memory_overview_groups().unwrap_or_default() {
        let map=match field.as_str() {
            "type"=>&mut by_type,
            "source"=>&mut by_source,
            "status"=>&mut by_status,
            "scope"=>&mut by_scope,
            _=>continue,
        };
        map.insert(key,count);
    }
    let jobs=state.store.recent_learning_jobs(50).unwrap_or_default().into_iter()
        .map(|job|json!({"id":job["id"],"status":job["status"],"sessionId":job["input"]["sessionId"],"agentId":job["input"]["agentId"],"result":job["result"],"error":job["error"],"throughMessageId":job["input"]["throughMessageId"],"queuedAt":job["queuedAt"],"finishedAt":job["finishedAt"]})).collect::<Vec<_>>();
    let drafts=state.store.recent_learning_draft_skills(20).unwrap_or_default();
    let path = state.config.db_path();
    let path = if path.is_absolute() {
        path
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    };
    Json(
        json!({"total":state.store.count("memory"),"path":path,"table":"records (kind=memory)","byType":by_type,"bySource":by_source,"byStatus":by_status,"scopes":by_scope,"byScope":by_scope,
        "learningJobs":jobs,"checkpoints":state.store.recent_records("learning_checkpoint",50).unwrap_or_default(),
        "feedback":state.store.recent_records("learning_feedback",30).unwrap_or_default(),"toolReliability":state.store.recent_records("tool_stat",200).unwrap_or_default(),"draftSkills":drafts,
        "capabilities":{"storage":"SQLite WAL","retrieval":"local lexical words and Korean bigrams","evidence":"exact source quotes, successful tool provenance","corrections":"scoped stable keys with superseded history","modelWeightTraining":false,"automaticSkillActivation":false},
        "updatedAt":chrono::Utc::now().to_rfc3339()}),
    )
}
