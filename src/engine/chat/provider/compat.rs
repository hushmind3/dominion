//! Provider-owned compat responsibilities. The harness and provider selection stay in the parent.
use super::*;

pub(super) fn compat_messages(messages: &[Value]) -> Vec<Value> {
    messages.iter().map(|message| {
        match message["role"].as_str() {
            Some("tool") => json!({
                "role":"user",
                "content": format!(
                    "Tool observation (tool_call_id={}): {}",
                    message["tool_call_id"].as_str().unwrap_or("unknown"),
                    message["content"].as_str().unwrap_or(""),
                ),
            }),
            Some("assistant") if message["tool_calls"].is_array() => {
                let calls = message["tool_calls"].as_array().unwrap();
                let content = message["content"].as_str().unwrap_or("");
                json!({
                    "role":"assistant",
                    "content": format!(
                        "{content}\nTool requests issued: {}",
                        serde_json::to_string(calls).unwrap_or_default()
                    ),
                })
            }
            _ => message.clone(),
        }
    }).collect()
}


/// Preserve an incomplete native stream long enough to account for model usage,
/// then reject it instead of silently discarding missing indexed tool calls.
pub(super) fn finalize_streamed_tool_calls(calls:Vec<ToolCall>)->(Vec<ToolCall>,bool){
    let incomplete=calls.iter().any(|call|call.name.trim().is_empty());
    let calls=calls.into_iter().map(|mut call|{
        if call.id.is_empty(){call.id=uuid::Uuid::new_v4().to_string();}
        call
    }).collect();
    (calls,incomplete)
}

pub(super) async fn stream_completion_once_body(
    state:&AppState,client:&reqwest::Client,provider:&ProviderRef,messages:&[Value],tools:&[Value],
    request:&TurnRequest,session_id:&str,tx:&UnboundedSender<Value>,convert_native_history:bool,
    continuity:&mut Value,
) -> Result<Completion,String> {
    if provider.kind == "cli" {
        let started=std::time::Instant::now();
        let source=provider.base_url.strip_prefix("cli://").ok_or("CLI 제공자 주소가 올바르지 않습니다.")?;
        let mut projected=messages.to_vec();
        if !tools.is_empty(){
            let manual=super::super::protocol::tool_manual(tools,&request.input).await;
            projected.insert(0,json!({"role":"system","content":manual}));
        }
        let content=crate::engine::cli_model::generate(source,&provider.model,&projected).await?;
        if state.is_cancelled(session_id){return Err("실행이 중단되었습니다.".into());}
        let (calls,visible)=super::super::protocol::parse_text_tool_calls(&content);
        let mut completed=Completion::default();
        completed.content=visible;
        completed.tool_calls=calls.into_iter().map(|(name,arguments)|ToolCall{id:uuid::Uuid::new_v4().to_string(),name,arguments}).collect();
        if !completed.content.is_empty(){let _=tx.send(json!({"type":"text","text":completed.content}));}
        let (prompt_tokens,completion_tokens)=estimate_usage(&projected,&[],&completed);
        completed.prompt_tokens=prompt_tokens;
        completed.completion_tokens=completion_tokens;
        completed.usage=prompt_tokens+completion_tokens;
        completed.first_delta_ms=Some(started.elapsed().as_millis() as u64);
        crate::engine::gateway::record_usage_tokens(state,&provider.id,&provider.model,false,"chat",Some(session_id),
            prompt_tokens,completion_tokens,completed.usage,true);
        return Ok(completed);
    }
    if provider.kind == "codex" {
        return stream_codex_once(
            state, client, provider, messages, tools, request, session_id, tx, continuity,
        )
        .await;
    }
    if provider.reasoning_protocol == "anthropic" {
        return stream_anthropic_once(
            state, client, provider, messages, tools, request, session_id, tx, continuity,
        )
        .await;
    }
    let started = std::time::Instant::now();
    let compatible = provider.tool_mode == "compat";
    let manual = if compatible && !tools.is_empty() {
        Some(super::super::protocol::tool_manual(tools, &request.input).await)
    } else {
        None
    };
    let schema_chars = if let Some(text) = manual.as_deref() {
        escaped_content_chars(text)
    } else {
        tools.iter().map(|tool| tool.to_string().chars().count()).sum::<usize>()
    };
    let mut provider_messages = messages.to_vec();
    let reclaimed = fit_tool_context(provider, &mut provider_messages, schema_chars);
    if reclaimed > 0 {
        state.store.audit("chat.context_edit_prepared", json!({
            "sessionId":session_id,"providerId":provider.id,"model":provider.model,
            "wire":if compatible {"openai-compat"} else {"openai-native"},
            "removedJsonCharsEstimate":reclaimed,
            "billedTokenSavings":Value::Null
        }));
    }
    if convert_native_history {
        provider_messages = compat_messages(&provider_messages);
    }
    if let Some(manual) = manual {
        if let Some(first_system) = provider_messages.iter_mut()
            .find(|message| message["role"] == "system")
        {
            let existing = first_system["content"].as_str().unwrap_or("");
            first_system["content"] = json!(format!("{existing}\n\n{manual}"));
        }
    }
    // DeepSeek and Reflection require `reasoning_content` on every earlier
    // assistant message. Most other OpenAI-compatible endpoints reject
    // unknown fields, so only send it where the provider requires it.
    let reflection_provider = provider.base_url.contains("api.reflection.ai");
    let requires_reasoning_content =
        provider.reasoning_protocol == "deepseek" || reflection_provider;
    provider_messages = order_tool_exchanges(provider_messages);
    normalize_assistant_reasoning(&mut provider_messages, requires_reasoning_content);
    if reflection_provider {
        let (preserved, quoted_legacy) = preserve_reflection_history(provider_messages);
        provider_messages = preserved;
        if quoted_legacy {
            state.store.audit(
                "reflection.history_preserved",
                json!({
                    "sessionId": session_id,
                    "reason": "assistant reasoning missing; sent placeholder to preserve roles and tool pairing",
                }),
            );
        }
    }
    let mut body = json!({
        "model": provider.model,
        "messages": provider_messages,
        "stream": true,
        "stream_options": { "include_usage": true },
    });
    if !compatible && !tools.is_empty() {
        body["tools"] = json!(tools);
    }
    if let Some(limit) = provider.max_output_tokens {
        body["max_tokens"] = json!(limit);
    }
    apply_reasoning_options(
        &mut body,
        &provider.reasoning_protocol,
        request.thinking,
        request.reasoning_effort.as_deref(),
    );

    let wire_input=super::super::room_context::input_estimate(
        body["messages"].as_array().map(Vec::as_slice).unwrap_or(&[]),
        body["tools"].as_array().map(Vec::as_slice).unwrap_or(&[]),
    );
    state.store.audit("chat.provider_wire_input",json!({
        "sessionId":session_id,"providerId":provider.id,"model":provider.model,
        "toolMode":provider.tool_mode,"estimatedInputTokens":wire_input,
        "contextTokens":provider.context_tokens,
        "reservedOutputTokens":provider.max_output_tokens
            .unwrap_or(4096).min((provider.context_tokens.unwrap_or(32768)/4).max(128)),
        "toolSchemaCount":if compatible {0}else{tools.len()},
        "estimateOnly":true
    }));
    validate_wire_context(wire_input,provider.context_tokens,provider.max_output_tokens)?;
    // The same admission slot is shared with helper (non-streaming) calls, so
    // one local model instance is never asked to run two generations at once.
    crate::engine::gateway::reserve_generation(
        &provider.base_url,
        &provider.model,
        provider.requests_per_minute,
    ).map_err(|e|format!("[model_not_started] {e}"))?;
    let _permit = crate::engine::gateway::acquire(&provider.base_url, provider.local).await;
    let builder = crate::engine::gateway::benchmark_request(
        client.post(format!("{}/chat/completions", provider.base_url)),
    )
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {}", provider.api_key));
    let response=super::super::continuity::send(state,provider,builder,&body,continuity,super::super::continuity::CacheWire::Key).await?;

    crate::engine::gateway::verify_benchmark_route(&response)?;

    if !response.status().is_success() {
        let status = response.status();
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            crate::engine::gateway::mark_rate_limited(&provider.base_url, &provider.model);
        }
        let text = response.text().await.unwrap_or_default();
        let lower = text.to_ascii_lowercase();
        if text.contains("1305") || lower.contains("temporarily overloaded") {
            return Err(format!(
                "{}가 혼잡합니다(1305). 다른 무료 모델을 선택하거나 잠시 후 다시 시도하세요.",
                provider.name
            ));
        }
        if matches!(status.as_u16(), 401 | 403) && provider.base_url.contains("cloudflare") {
            return Err(format!(
                "{} 키가 거부되었습니다. Workers AI 권한이 있는 Cloudflare API 토큰인지 확인하세요.",
                provider.name
            ));
        }
        return Err(format!(
            "{} 호출 실패 (HTTP {status}): {}",
            provider.name,
            text.chars().take(600).collect::<String>()
        ));
    }

    let mut completion = Completion::default();
    let mut calls: Vec<ToolCall> = Vec::new();
    let mut buffer = SseBuffer::default();
    let mut stream = response.bytes_stream();
    let mut terminated = false;
    let mut text_pending = String::new();
    let mut in_text_tool_call = false;
    let mut final_message: Option<Value> = None;
    let mut final_message_ms: Option<u64> = None;

    'stream: while let Some(chunk) = stream.next().await {
        if state.is_cancelled(session_id) {
            return Err("실행이 중단되었습니다.".into());
        }
        let chunk = chunk.map_err(|error| format!("스트림 오류: {error}"))?;
        buffer.push(&chunk)?;

        while let Some(block) = buffer.pop()? {
            let payload = block
                .lines()
                .filter(|line| line.starts_with("data:"))
                .map(|line| line[5..].trim_start())
                .collect::<Vec<_>>()
                .join("\n");
            if payload == "[DONE]" {
                terminated = true;
                break 'stream;
            }
            if payload.is_empty() {
                continue;
            }
            let piece: Value = serde_json::from_str(&payload)
                .map_err(|_| "모델 스트림 JSON이 올바르지 않습니다.")?;
            if let Some(error) = piece.get("error") {
                return Err(format!("모델 오류: {error}"));
            }
            if let Some(usage) = piece.get("usage").filter(|value| value.is_object()) {
                completion.cached_prompt_tokens = cached_prompt_tokens(usage);
                let prompt = usage.get("prompt_tokens").and_then(Value::as_u64);
                let generated = usage.get("completion_tokens").and_then(Value::as_u64);
                if let Some(total) = usage.get("total_tokens").and_then(Value::as_u64) {
                    completion.usage = total;
                }
                if let (Some(prompt), Some(generated)) = (prompt, generated) {
                    completion.prompt_tokens = prompt;
                    completion.completion_tokens = generated;
                    if completion.usage == 0 {
                        completion.usage = prompt + generated;
                    }
                    completion.usage_exact = true;
                }
            }
            if let Some(message) = piece.pointer("/choices/0/message").filter(|value| value.is_object()) {
                if final_message_ms.is_none() {
                    final_message_ms = Some(started.elapsed().as_millis() as u64);
                }
                final_message = Some(message.clone());
            }
            if let Some(reason) = piece["choices"][0]["finish_reason"].as_str() {
                // Keep both the partial content and any final usage receipt.
                // A length/content_filter terminal is not a verified answer:
                // the product host records the usage before surfacing failure.
                completion.finish_reason = Some(reason.to_string());
                terminated = true;
            }
            let Some(delta) = piece
                .get("choices")
                .and_then(|choices| choices.get(0))
                .and_then(|choice| choice.get("delta"))
            else {
                continue;
            };
            if completion.first_delta_ms.is_none()
                && (delta["content"]
                    .as_str()
                    .is_some_and(|text| !text.is_empty())
                    || delta["reasoning_content"]
                        .as_str()
                        .or_else(|| delta["reasoning"].as_str())
                        .is_some_and(|text| !text.is_empty())
                    || delta["tool_calls"]
                        .as_array()
                        .is_some_and(|calls| !calls.is_empty()))
            {
                completion.first_delta_ms = Some(started.elapsed().as_millis() as u64);
            }

            if let Some(content) = delta.get("content").and_then(|value| value.as_str()) {
                if !content.is_empty() {
                    completion.content.push_str(content);
                    stream_visible_text(tx, content, &mut text_pending, &mut in_text_tool_call);
                }
            }

            let reasoning = delta
                .get("reasoning_content")
                .or_else(|| delta.get("reasoning"))
                .and_then(|value| value.as_str())
                .unwrap_or("");
            if !reasoning.is_empty() {
                completion.reasoning.push_str(reasoning);
                let _ = tx.send(json!({ "type": "reasoning", "text": reasoning }));
            }

            if let Some(tool_calls) = delta.get("tool_calls").and_then(|value| value.as_array()) {
                for call in tool_calls {
                    let index = call
                        .get("index")
                        .and_then(|value| value.as_u64())
                        .unwrap_or(calls.len() as u64) as usize;
                    if index >= MAX_TOOL_CALLS {
                        return Err("모델이 너무 많은 도구 호출을 반환했습니다.".into());
                    }
                    while calls.len() <= index {
                        calls.push(ToolCall {
                            id: String::new(),
                            name: String::new(),
                            arguments: String::new(),
                        });
                    }
                    let slot = &mut calls[index];
                    if let Some(id) = call.get("id").and_then(|value| value.as_str()) {
                        if !id.is_empty() {
                            slot.id = id.to_string();
                        }
                    }
                    if let Some(function) = call.get("function") {
                        if let Some(name) = function.get("name").and_then(|value| value.as_str()) {
                            slot.name.push_str(name);
                        }
                        if let Some(arguments) =
                            function.get("arguments").and_then(|value| value.as_str())
                        {
                            slot.arguments.push_str(arguments);
                        }
                    }
                }
            }
        }
        if completion.content.len()
            + completion.reasoning.len()
            + calls.iter().map(|call| call.arguments.len()).sum::<usize>()
            > 2_000_000
        {
            return Err("모델 응답 크기 제한을 초과했습니다.".into());
        }
    }
    if let Some(message) = final_message.as_ref() {
        let (text, reasoning) = merge_final_chat_message(&mut completion, &mut calls, message)?;
        if !text.is_empty() {
            stream_visible_text(tx, &text, &mut text_pending, &mut in_text_tool_call);
        }
        if !reasoning.is_empty() {
            let _ = tx.send(json!({"type":"reasoning","text":reasoning}));
        }
        if completion.first_delta_ms.is_none()
            && (!text.is_empty() || !reasoning.is_empty() || !calls.is_empty())
        {
            completion.first_delta_ms = final_message_ms;
        }
    }
    if completion.content.len() + completion.reasoning.len()
        + calls.iter().map(|call|call.arguments.len()).sum::<usize>() > 2_000_000
    {
        return Err("모델 응답 크기 제한을 초과했습니다.".into());
    }
    // Flush text held for a possible tag, unless it is a dangling prefix.
    if !in_text_tool_call
        && !text_pending.is_empty()
        && !TEXT_TOOL_OPEN.starts_with(&text_pending)
    {
        let _ = tx.send(json!({ "type": "text", "text": text_pending }));
    }
    // On an EOF without a terminal marker, settle any reported paid usage
    // before rejecting the incomplete completion.
    // An indexed gap (e.g. only index 1 arrived) must not become a
    // partial successful tool round. Still account for the paid model call.
    let (settled_calls,incomplete_tool_calls)=finalize_streamed_tool_calls(calls);
    completion.tool_calls=settled_calls;

    let (prompt_tokens, completion_tokens) = if completion.usage_exact {
        (completion.prompt_tokens, completion.completion_tokens)
    } else {
        estimate_usage(&provider_messages, if compatible { &[] } else { tools }, &completion)
    };
    let total_tokens=finalize_usage_estimate(&mut completion,prompt_tokens,completion_tokens);
    crate::engine::gateway::record_usage_tokens_with_cache(
        state,
        &provider.id,
        &provider.model,
        provider.local,
        "chat",
        Some(session_id),
        prompt_tokens,
        completion_tokens,
        total_tokens,
        completion.cached_prompt_tokens,
        None,
        !completion.usage_exact,
    );
    if !terminated {
        state.store.audit("chat.provider_stream_interrupted",json!({
            "sessionId":session_id,"providerId":provider.id,"model":provider.model,
            "usageExact":completion.usage_exact,
            "reportedUsage":if completion.usage_exact {json!(completion.usage)} else {Value::Null}
        }));
        return Err("모델 스트림이 완료 신호 없이 종료되었습니다.".into());
    }
    if incomplete_tool_calls {
        return Err("모델 도구 호출 스트림의 인덱스 또는 이름이 누락되었습니다.".into());
    }
    Ok(completion)
}

