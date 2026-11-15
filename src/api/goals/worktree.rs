use super::*;

/// Read-only diff for files attributed to this goal by observed tool receipts.
/// Returns no unrelated workspace changes when the goal has no file outputs.
pub async fn diff_goal(State(state):State<AppState>,Path(id):Path<String>)->Response{
    let Some(goal)=state.store.get("goal",&id) else{
        return error(StatusCode::NOT_FOUND,"목표를 찾을 수 없습니다.");
    };
    if goal["isolateWorktree"]==true {
        return match engine::goal_git::changes(&state,&id).await{
            Ok(value)=>Json(value).into_response(),
            Err(message)=>error(StatusCode::CONFLICT,&message),
        };
    }
    let paths=goal["artifacts"].as_array().into_iter().flatten()
        .filter_map(|row|row["path"].as_str().map(str::to_string))
        .take(20).collect::<Vec<_>>();
    if paths.is_empty(){
        return Json(json!({"goalId":id,"paths":[],"status":"",
            "unstaged":"","staged":"","observed":false,
            "note":"관측된 파일 수정 산출물이 없어 이 목표의 diff를 구분할 수 없습니다."})).into_response();
    }
    match crate::engine::agent_diff::inspect(&state.config.workspace,&paths).await{
        Ok(mut value)=>{
            value["goalId"]=json!(id);
            Json(value).into_response()
        }
        Err(message)=>error(StatusCode::BAD_REQUEST,&message),
    }
}

/// Exact, bounded patch for reproducible external evaluation. Never export
/// clipped UI diffs, pending writes, or silently omitted untracked files.
pub async fn export_goal_patch(State(state):State<AppState>,Path(id):Path<String>)->Response{
    match crate::engine::goal_git::export_patch(&state,&id).await{
        Ok(patch)=>Json(patch).into_response(),
        Err(message)=>error(StatusCode::CONFLICT,&message),
    }
}

/// Explicit owner discard: only the goal-owned linked worktree is removed.
pub async fn rollback_goal(State(state):State<AppState>,Path(id):Path<String>)->Response{
    match engine::goal_git::rollback(&state,&id).await{
        Ok(receipt)=>Json(receipt).into_response(),
        Err(message)=>error(StatusCode::CONFLICT,&message),
    }
}

/// Only an owner-submitted digest of the previously reviewed patch can be
/// applied to the clean original checkout.
pub async fn apply_goal(State(state):State<AppState>,Path(id):Path<String>,
    Json(body):Json<Value>)->Response{
    if body.as_object().is_none_or(|obj|obj.len()!=1 || !obj.contains_key("expectedPatchSha256")){
        return error(StatusCode::BAD_REQUEST,"expectedPatchSha256 is required.");
    }
    let Some(digest)=body["expectedPatchSha256"].as_str() else{
        return error(StatusCode::BAD_REQUEST,"Expected patch digest must be a string.");
    };
    match engine::goal_git::apply_reviewed(&state,&id,digest).await{
        Ok(receipt)=>Json(receipt).into_response(),
        Err(message)=>error(StatusCode::CONFLICT,&message),
    }
}

/// Shared-checkout preview is independent from isolated worktree /diff.
/// It only enumerates files with successful goal edit receipts.
pub async fn shared_changes(State(state):State<AppState>,Path(id):Path<String>)->Response{
    let Some(goal)=state.store.get("goal",&id) else{return error(StatusCode::NOT_FOUND,"Goal not found");};
    if goal["isolateWorktree"]==true{return error(StatusCode::CONFLICT,"Use isolated /diff instead");}
    match engine::goal_changes::preview(&state,&goal).await{
        Ok(v)=>Json(v).into_response(),
        Err(e)=>error(StatusCode::CONFLICT,&e),
    }
}
/// Restore one SHA-pinned, goal-edited tracked file from the frozen baseline.
pub async fn shared_undo(State(state):State<AppState>,Path(id):Path<String>,
    Json(body):Json<Value>)->Response{
    let Some(goal)=state.store.get("goal",&id) else{return error(StatusCode::NOT_FOUND,"Goal not found");};
    if goal["isolateWorktree"]==true{return error(StatusCode::CONFLICT,"Isolated goals use rollback");}
    if body.as_object().is_none_or(|map|map.len()!=2 ||
        !map.contains_key("path")||!map.contains_key("expectedSha256")){
        return error(StatusCode::BAD_REQUEST,"path and expectedSha256 are required.");
    }
    let Some(path)=body["path"].as_str() else{return error(StatusCode::BAD_REQUEST,"Invalid path");};
    let Some(sha)=body["expectedSha256"].as_str() else{return error(StatusCode::BAD_REQUEST,"Invalid digest");};
    match engine::goal_changes::undo(&state,&goal,path,sha).await{
        Ok(v)=>Json(v).into_response(),
        Err(e)=>error(StatusCode::CONFLICT,&e),
    }
}
