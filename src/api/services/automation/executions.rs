use crate::state::AppState;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

/// Safe, compact projections for the existing Activity UI.
/// Never return a managed Team's prompt, system instructions or remote source bytes.
fn team_activity_view(team:&Value, members:&[Value], remote_tasks:&[Value])->Value{
    let id=team["id"].as_str().unwrap_or("");
    let plan=team["tasks"].as_array().cloned().unwrap_or_default();
    let mut blocked=false;
    let mut all_done=!plan.is_empty();
    let mut pending_artifacts=Vec::<String>::new();
    let mut tasks=Vec::with_capacity(plan.len());
    for node in plan.iter().take(8){
        let node_id=node["id"].as_str().unwrap_or("");
        let row=members.iter().find(|row|
            row["teamId"]==id && row["nodeId"]==node_id);
        let status=row.and_then(|r|r["status"].as_str()).unwrap_or("queued");
        if status!="completed"{all_done=false;}
        if matches!(status,"needs_review"|"failed"|"cancelled"|"interrupted"){blocked=true;}
        let remote_id=row.and_then(|r|r["remoteTaskId"].as_str());
        let remote=remote_id.and_then(|rid|remote_tasks.iter().find(|t|
            t["id"]==rid && t["teamId"]==id));
        let apply=remote.and_then(|t|t["artifactApplyStatus"].as_str());
        let has_artifacts=remote.is_some_and(|t|
            t["artifactPaths"].as_array().is_some_and(|p|!p.is_empty()));
        if status=="completed" && has_artifacts && apply!=Some("applied"){
            blocked=true;
            if apply==Some("pending"){
                if let Some(rid)=remote_id{pending_artifacts.push(rid.to_string());}
            }
        }
        tasks.push(json!({
            "id":node_id,
            "agentId":row.map(|r|r["agentId"].clone()).unwrap_or_else(||node["agentId"].clone()),
            "status":status,
            "readOnly":node["readOnly"]!=false,
            "dependsOn":node["dependsOn"].as_array().cloned().unwrap_or_default(),
            "verified":row.is_some_and(|r|r["verified"]==true),
            "remoteWorkerId":node["remoteWorkerId"],
            "remoteTaskId":remote_id,
            "artifactApplyStatus":apply,
            "jobId":row.and_then(|r|r["jobId"].as_str()),
            "error":row.and_then(|r|r["error"].as_str())
                .map(|s|s.chars().take(240).collect::<String>()),
        }));
    }
    let status=if team["status"]=="cancelled"{"cancelled"}
        else if blocked{"needs_review"}else if all_done{"completed"}else{"running"};
    json!({
        "id":id,"status":status,"leadSessionId":team["leadSessionId"],
        "leadAgentId":team["leadAgentId"],
        "createdAt":team["createdAt"],"updatedAt":team["updatedAt"],
        "verified":all_done && !blocked && status=="completed"
            && tasks.iter().all(|t|t["verified"]==true),
        "pendingRemoteArtifactTaskIds":pending_artifacts,
        "tasks":tasks,
    })
}

/// Read-only visibility for the existing execution-records screen.
/// Run IDs are public to the local user, not authorization to apply effects.
pub async fn team_activity(State(state):State<AppState>)->Json<Value>{
    let mut teams=state.store.all("managed_team");
    teams.sort_by(|a,b|b["createdAt"].as_str().unwrap_or("")
        .cmp(a["createdAt"].as_str().unwrap_or("")));
    let members=state.store.all("managed_team_task");
    let remote_tasks=state.store.all("remote_task");
    let now=chrono::Utc::now().timestamp_millis();
    let mut workers=state.store.all("remote_worker").into_iter().map(|node|{
        let heartbeat=node["lastHeartbeatMs"].as_i64().unwrap_or(0);
        json!({
            "id":node["id"],"online":node["status"]=="online"
                && heartbeat>0 && now.saturating_sub(heartbeat)<=90_000,
            "allowWrites":node["allowWrites"]==true,
            "agentCount":node["agents"].as_array().map_or(0,Vec::len),
            "agents":node["agents"],
            "lastHeartbeatMs":node["lastHeartbeatMs"],
        })
    }).collect::<Vec<Value>>();
    workers.sort_by(|a,b|a["id"].as_str().cmp(&b["id"].as_str()));
    workers.truncate(64);
    Json(json!({
        "teams":teams.iter().take(30).map(|team|
            team_activity_view(team,&members,&remote_tasks)).collect::<Vec<_>>(),
        "remoteWorkers":workers,
        "remoteConfigured":crate::remote::control_plane_configured(),
    }))
}


#[derive(Deserialize, Default)]
pub struct Page {
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

/// Executions are engine-owned audit records. Clients may read and cancel
/// them, never create or overwrite status/evidence. The list is bounded by
/// default because individual execution records include large evidence blobs.
pub async fn list(State(state): State<AppState>, Query(page): Query<Page>) -> Json<Value> {
    let mut items = state.store.all("execution");
    items.sort_by(|a, b| {
        b["startedAt"]
            .as_str()
            .or_else(|| b["createdAt"].as_str())
            .unwrap_or("")
            .cmp(
                a["startedAt"]
                    .as_str()
                    .or_else(|| a["createdAt"].as_str())
                    .unwrap_or(""),
            )
    });
    let limit = page.limit.unwrap_or(200).clamp(1, 1000);
    let offset = page.offset.unwrap_or(0);
    Json(json!(
        items
            .into_iter()
            .skip(offset)
            .take(limit)
            .collect::<Vec<Value>>()
    ))
}

pub async fn one(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match state.store.get("execution", &id) {
        Some(execution) => Json(execution).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "실행 기록을 찾을 수 없습니다." })),
        )
            .into_response(),
    }
}

pub async fn cancel(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let Some(execution) = state.store.get("execution", &id) else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "실행 기록을 찾을 수 없습니다." })),
        )
            .into_response();
    };
    if execution["status"].as_str() != Some("running") {
        return (
            StatusCode::CONFLICT,
            Json(json!({ "error": "실행 중인 기록만 중단할 수 있습니다." })),
        )
            .into_response();
    }
    if let Some(session) = execution["sessionId"].as_str() {
        state.cancel_session(session);
    }
    let now = chrono::Utc::now().to_rfc3339();
    match state.store.patch_if(
        "execution",
        &id,
        "status",
        &json!("running"),
        &json!({"status":"cancelled","reason":"사용자 요청으로 중단","finishedAt":now}),
    ) {
        Some(saved) => {
            state.bus.record("execution", &saved);
            Json(saved).into_response()
        }
        None => (
            StatusCode::CONFLICT,
            Json(json!({ "error": "실행 상태가 이미 변경되었습니다." })),
        )
            .into_response(),
    }
}

pub async fn active_executions(State(state): State<AppState>) -> Json<Value> {
    let items = state
        .store
        .all("execution")
        .into_iter()
        .filter(|execution| {
            execution.get("status").and_then(|value| value.as_str()) == Some("running")
        })
        .collect::<Vec<_>>();
    Json(json!(items))
}

pub async fn execution_operations(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Json<Value> {
    let items = state
        .store
        .all("execution_operation")
        .into_iter()
        .filter(|record| record.get("runId").and_then(|value| value.as_str()) == Some(id.as_str()))
        .collect::<Vec<_>>();
    Json(json!(items))
}

#[cfg(test)]
mod team_activity_tests{
    use super::*;
    #[test]
    fn remote_pending_merge_blocks_completion_without_exposing_prompts_or_source(){
        let team=json!({"id":"team-1","leadSessionId":"s1",
            "leadAgentId":"lead","status":"running",
            "tasks":[{"id":"impl","agentId":"coder","prompt":"SECRET_PROMPT",
                "readOnly":false,"remoteWorkerId":"remote-1"}]});
        let members=vec![json!({"teamId":"team-1","nodeId":"impl",
            "agentId":"coder","status":"completed","verified":true,
            "remoteTaskId":"task-1"})];
        let remote=vec![json!({"id":"task-1","teamId":"team-1",
            "artifactPaths":["src/lib.rs"],"artifactApplyStatus":"pending",
            "artifacts":[{"path":"src/lib.rs","content":"SECRET_SOURCE"}]})];
        let visible=team_activity_view(&team,&members,&remote);
        assert_eq!(visible["status"],"needs_review");
        assert_eq!(visible["pendingRemoteArtifactTaskIds"],json!(["task-1"]));
        assert_eq!(visible["verified"],false);
        assert!(!visible.to_string().contains("SECRET_"));
        let mut applied=remote;
        applied[0]["artifactApplyStatus"]=json!("applied");
        let done=team_activity_view(&team,&members,&applied);
        assert_eq!(done["status"],"completed");
        assert_eq!(done["verified"],true);
    }
}
