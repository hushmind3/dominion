//! Direct model conversation. Never inject tool schemas, agent identity,
//! policy decomposition, memory search or task-execution programs.
use super::*;
use dominion_agent_sdk::{Journal, ModelAttempt, Status};

pub(super) async fn run_plain_body(
    state: &AppState,
    request: &TurnRequest,
    tx: &UnboundedSender<Value>,
    session_id: &str,
    metrics: &std::sync::Mutex<Metrics>,
    execution_id: &str,
    generation: u64,
) -> Result<Option<Value>, String> {
    // Save the exact owner input once, before any billable model request.
    let attachments = crate::engine::attachments::validate_ids(
        state, session_id, &request.attachment_ids
    )?;
    let source = state.store.write_message(&json!({
        "session_id": session_id,
        "role": "user",
        "content": request.input,
        "agent_id": Value::Null,
        "metadata": {"authorKind":"owner", "attachments": attachments},
    })).ok_or("사용자 메시지를 저장할 수 없습니다.")?;
    state.bus.message(&source);
    if let Some(session) = state.store.get("session", session_id) {
        retitle_if_placeholder(state, &session, &request.input);
    }

    let started = std::time::Instant::now();
    let mut system = String::from(
        "당신은 DOMINION의 일반 대화 AI입니다. 사용자 질문에 직접 답하십시오. \
         도구 실행, 웹 검색, 파일 변경, 에이전트 작업을 수행하거나 수행했다고 주장하지 마십시오. \
         사용자가 별도의 언어를 지정하지 않는 한 한국어로 답하십시오.",
    );
    // Owner policy is still authoritative in chat mode; agent persona,
    // autonomous procedures and memory retrieval are not.
    if let Some(instructions) = state.store.get("assistant_settings", "global")
        .filter(|settings| settings["enabled"] != false)
        .and_then(|settings| settings["instructions"].as_str().map(str::to_string))
        .filter(|instructions| !instructions.trim().is_empty())
    {
        system.push_str("\n사용자 지정 지침:\n");
        system.push_str(&instructions);
    }
    if request.effective_reasoning_language() == Some("ko") {
        system.push_str("\n최종 응답과 사용자에게 표시되는 추론 설명은 한국어로 작성하십시오.");
    }
    let provider = select_provider(
        state, request, &state.store.get("session", session_id).unwrap_or(Value::Null)
    )?;
    let mut messages = vec![json!({"role":"system", "content":system})];
    messages.extend(history(
        state, session_id, None,
        provider.context_tokens.map(|tokens|
            ((tokens as usize).saturating_mul(3)).clamp(8_000, 48_000)
        ),
    )?);
    metrics.lock().unwrap().context_elapsed_ms = started.elapsed().as_millis() as u64;

    let client = state.provider_http_client(300)?;
    // One paid generation, zero tools. Keep the same durable SDK model-call
    // fence as agent runs so interruptions never silently retry a paid call.
    let mut snapshot = dominion_agent_sdk::Snapshot::from_request(
        &dominion_agent_sdk::RunRequest {
            id: execution_id.to_string(),
            instruction: request.input.clone(),
            owner_instructions: String::new(),
            strategy: dominion_agent_sdk::Strategy::Balanced,
            limits: dominion_agent_sdk::Limits {
                max_steps:1, max_model_calls:1, max_tool_calls:1,
                max_estimated_tokens:MAX_TOKENS, max_parallel_reads:1,
                max_result_chars:3000,
            },
            allowed_tools:Vec::new(), checks:vec![], require_evidence:false,
        }
    );
    let journal = ChatKernelJournal {
        state, execution_id, session_id, generation,
    };
    Journal::save(&journal, &snapshot)?;
    {
        let mut counters = metrics.lock().unwrap();
        counters.model_calls = 1;
        counters.iterations = 1;
    }
    let model_started = std::time::Instant::now();
    let (completion, provider) = match dominion_agent_sdk::dispatch_model_turn(
        &journal, &mut snapshot, None,
        || stream_completion(
            state, &client, &provider, &messages, &[],
            request, session_id, tx, false
        ),
    ).await? {
        ModelAttempt::Completed(reply) => reply,
        ModelAttempt::Cancelled => return Err("모델 호출이 중단되었습니다.".into()),
        ModelAttempt::OwnerChanged => return Err("사용자 지시 변경으로 호출을 폐기했습니다.".into()),
        ModelAttempt::Uncertain(error) => return Err(error),
    };
    {
        let mut counters = metrics.lock().unwrap();
        counters.model_elapsed_ms = model_started.elapsed().as_millis() as u64;
        counters.first_token_ms = completion.first_delta_ms;
    }

    let (prompt_tokens, completion_tokens) = if completion.usage_exact {
        (completion.prompt_tokens, completion.completion_tokens)
    } else {
        (
            super::super::room_context::input_estimate(&messages, &[]) as u64,
            super::super::room_context::estimate(
                &format!("{}{}", completion.content, completion.reasoning)
            ).max(1) as u64,
        )
    };
    let charged = if completion.usage > 0 {
        completion.usage
    } else {
        prompt_tokens.saturating_add(completion_tokens)
    };
    metrics.lock().unwrap().tokens = charged;
    let rejection = if charged > MAX_TOKENS {
        Some("모델 응답이 토큰 제한을 초과했습니다.")
    } else if !completion.tool_calls.is_empty()
        || completion.content.contains("<tool_call>")
        || completion.content.contains("invoke name=\"")
    {
        Some("일반 채팅에서 모델이 허용되지 않은 도구 호출을 반환했습니다.")
    } else if completion.content.trim().is_empty() {
        Some("모델이 빈 답변을 반환했습니다.")
    } else {
        None
    };
    if let Some(reason) = rejection {
        snapshot.step = 1;
        snapshot.model_calls = 1;
        snapshot.charged_tokens = charged;
        snapshot.stop(Status::NeedsReview, reason);
        dominion_agent_sdk::settle_rejected_model_turn(&journal, &mut snapshot)?;
        return Err(reason.into());
    }
    dominion_agent_sdk::settle_model_turn(&journal, &mut snapshot,
        dominion_agent_sdk::Decision {
            plan:Vec::new(), actions:Vec::new(),
            answer:Some(completion.content.clone()),
        }
    )?;
    if state.is_cancelled(session_id) || state.cancellation_generation(session_id)!=generation {
        return Err("실행이 중단되었습니다.".into());
    }
    snapshot.status = Status::Completed;
    snapshot.answer = Some(completion.content.clone());
    Journal::save(&journal, &snapshot)?;

    let metadata = json!({
        "runtime":"plain-chat",
        "authorKind":"assistant",
        "executionId":execution_id,
        "replyToMessageId":source["id"],
        "usage":{
            "promptTokens":prompt_tokens,
            "cachedPromptTokens":if completion.usage_exact {
                json!(completion.cached_prompt_tokens)
            } else {Value::Null},
            "uncachedPromptTokens":if completion.usage_exact {
                json!(completion.cached_prompt_tokens.map(|cached|
                    prompt_tokens.saturating_sub(cached.min(prompt_tokens))))
            } else {Value::Null},
            "completionTokens":completion_tokens,
            "totalTokens":charged,
            "estimated":!completion.usage_exact,
            "providerId":provider.id,
            "model":provider.model,
        },
    });
    let saved = state.store.write_message(&json!({
        "session_id":session_id,
        "role":"assistant",
        "content":completion.content,
        "reasoning":completion.reasoning,
        "agent_id":Value::Null,
        "metadata":metadata,
    })).ok_or("일반 채팅 응답을 저장할 수 없습니다.")?;
    state.bus.message(&saved);
    if let Some(mut execution) = state.store.get("execution", execution_id) {
        execution["finalMessageId"] = saved["id"].clone();
        execution["finalAnswer"] = json!(saved["content"].as_str().unwrap_or("")
            .chars().take(600).collect::<String>());
        execution["runtime"] = json!("plain-chat");
        execution["toolsUsed"] = json!([]);
        state.store.put("execution", &execution);
    }
    Ok(Some(saved))
}
