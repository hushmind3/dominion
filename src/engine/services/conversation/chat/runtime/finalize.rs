//! Final-answer commit boundary. No model or algorithm decisions occur here;
//! a cancelled epoch can never commit its late generated output.
use super::*;

pub(super) struct FinalAnswer {
    pub metadata:Value,
    pub content:String,
    pub reasoning:String,
    pub provider_reasoning:String,
    pub acceptance_passed:bool,
    pub acceptance_text:Option<String>,
    pub tools_used:Vec<String>,
    pub budget_snapshot:Value,
    pub prompt_tokens:u64,
    pub cached_prompt_tokens:u64,
    pub completion_tokens:u64,
    pub total_tokens:u64,
    pub cache_reporting_complete:bool,
    pub usage_estimated:bool,
    pub provider_id:String,
    pub model:String,
    pub projection:Option<super::continuity::Projection>,
}

pub(super) fn finish(
    state:&AppState,request:&TurnRequest,session_id:&str,
    execution_id:&str,generation:u64,answer:FinalAnswer,
)->Result<Option<Value>,String>{
    let FinalAnswer{
        mut metadata,content:final_content,reasoning:final_reasoning,
        provider_reasoning:last_reasoning,acceptance_passed,acceptance_text,
        tools_used,budget_snapshot,prompt_tokens:prompt_tokens_total,
        cached_prompt_tokens:cached_prompt_tokens_total,
        completion_tokens:completion_tokens_total,total_tokens,
        cache_reporting_complete,usage_estimated,provider_id,model,
        projection:continuation_projection,
    }=answer;
    // Preserve provider reasoning as received. Display language requests are
    // best effort; never send private reasoning to a hidden translation API.
    metadata["providerReasoning"] = json!(last_reasoning);

    if final_content.trim().is_empty() && final_reasoning.trim().is_empty() {
        return Err("모델이 빈 응답을 반환했습니다.".into());
    }

    if let Some(mut execution) = state.store.get("execution", execution_id) {
        execution["acceptancePassed"] = json!(acceptance_passed);
        execution["acceptanceText"] = acceptance_text.as_deref()
            .map(|text|json!(text)).unwrap_or(Value::Null);
        execution["finalAnswer"] = json!(final_content.chars().take(600).collect::<String>());
        execution["toolsUsed"] = json!(tools_used);
        execution["budget"] = budget_snapshot;
        state.store.put("execution", &execution);
    }
    // Attach the usage to the message so every client can show what the turn
    // cost without a second lookup.
    // Reject a late provider completion after Stop, even if its final delta
    // raced the supervisor's polling tick. Never persist a new successful
    // assistant reply from a cancelled browser epoch.
    if state.is_cancelled(session_id)
        || state.cancellation_generation(session_id)!=generation
        || super::super::transport::request_cancelled(
            state,session_id,request.request_id.as_deref())
    {
        return Err("실행이 중단되었습니다.".into());
    }
    metadata["usage"] = json!({
        "promptTokens": prompt_tokens_total,
        "cachedPromptTokens": if cache_reporting_complete { json!(cached_prompt_tokens_total) } else { Value::Null },
        "uncachedPromptTokens": if cache_reporting_complete { json!(prompt_tokens_total.saturating_sub(cached_prompt_tokens_total)) } else { Value::Null },
        "completionTokens": completion_tokens_total,
        "totalTokens": total_tokens,
        "estimated": usage_estimated,
        "providerId": provider_id,
        "model": model,
    });
    let (saved,linked) = state
        .store
        .write_final_message_and_link(execution_id,&json!({
            "session_id": session_id,
            "role": "assistant",
            "content": final_content,
            "reasoning": final_reasoning,
            "agent_id": request.agent_id,
            "metadata": metadata,
        }))
        .ok_or("실행이 종료되었거나 최종 응답이 이미 저장되었습니다.")?;
    state.bus.message(&saved);
    state.bus.record("execution",&linked);
    super::continuity::commit(state, &metadata["continuity"], &saved);
    if let Some(projection) = continuation_projection {
        super::continuity::commit_projection(state, &metadata["continuity"], &saved, projection);
    }
    Ok(Some(saved))
}
