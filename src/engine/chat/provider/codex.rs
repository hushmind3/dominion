//! Provider-owned codex responsibilities. The harness and provider selection stay in the parent.
use super::*;

pub(super) fn codex_tools(tools: &[Value]) -> Vec<Value> {
    tools
        .iter()
        .filter_map(|tool| {
            let function = tool.get("function").unwrap_or(tool);
            let name = function["name"].as_str()?;
            Some(json!({
                "type":"function",
                "name":name,
                "description":function["description"].as_str().unwrap_or(""),
                "parameters":function["parameters"].clone(),
            }))
        })
        .collect()
}

/// Normalizes a Responses `function_call` output item into the shape the
/// harness rebuilds from persisted messages. Extra response fields (`id`,
/// `status`) are deliberately dropped so chain signatures stay stable.
pub(in crate::engine::chat) fn codex_function_call_item(item: &Value) -> Option<Value> {
    if item["type"].as_str() != Some("function_call") {
        return None;
    }
    let call_id = item["call_id"]
        .as_str()
        .or_else(|| item["id"].as_str())
        .filter(|value| !value.is_empty())?;
    let name = item["name"].as_str().filter(|value| !value.is_empty())?;
    let arguments = item["arguments"]
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| item["arguments"].to_string());
    Some(json!({
        "type":"function_call",
        "call_id":call_id,
        "name":name,
        "arguments":arguments,
    }))
}

/// Extracts structured Codex tool calls from a final response output or from
/// one completed output item.
pub(in crate::engine::chat) fn codex_tool_calls(output: &[Value]) -> Vec<super::ToolCall> {
    let mut calls = Vec::new();
    for item in output {
        let Some(normalized) = codex_function_call_item(item) else {
            continue;
        };
        let id = normalized["call_id"].as_str().unwrap_or("").to_string();
        if calls.iter().any(|call: &super::ToolCall| call.id == id) {
            continue;
        }
        calls.push(super::ToolCall {
            id,
            name: normalized["name"].as_str().unwrap_or("").to_string(),
            arguments: normalized["arguments"]
                .as_str()
                .unwrap_or("{}")
                .to_string(),
        });
    }
    calls
}

/// Converts chat messages into the Responses API shape used by the Codex
/// backend. System text becomes `instructions`; assistant tool calls and tool
/// observations stay structured so the model can continue a real tool loop.
pub(super) fn codex_input(messages: &[Value]) -> (String, Vec<Value>) {
    let mut instructions = String::new();
    let mut input = Vec::new();
    for message in messages {
        let role = message["role"].as_str().unwrap_or("");
        let content = anthropic_message_text(&message["content"]);
        let has_image = message["content"].as_array().is_some_and(|parts| {
            parts.iter().any(|part| part["type"] == "image_url")
        });
        let tool_calls = message["tool_calls"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        if content.trim().is_empty() && !has_image && tool_calls.is_empty() && role != "tool" {
            continue;
        }
        match role {
            "system" => {
                if !instructions.is_empty() {
                    instructions.push_str("\n\n");
                }
                instructions.push_str(&content);
            }
            "assistant" => {
                if !content.trim().is_empty() {
                    input.push(json!({
                        "role":"assistant",
                        "content":[{"type":"output_text","text":content}]
                    }));
                }
                for call in tool_calls {
                    let Some(name) = call["function"]["name"]
                        .as_str()
                        .filter(|value| !value.is_empty())
                    else {
                        continue;
                    };
                    let Some(call_id) = call["id"].as_str().filter(|value| !value.is_empty())
                    else {
                        continue;
                    };
                    input.push(json!({
                        "type":"function_call",
                        "call_id":call_id,
                        "name":name,
                        "arguments":call["function"]["arguments"].as_str().unwrap_or("{}"),
                    }));
                }
            }
            "tool" => {
                let Some(call_id) = message["tool_call_id"].as_str().filter(|value| !value.is_empty())
                else {
                    continue;
                };
                input.push(json!({
                    "type":"function_call_output",
                    "call_id":call_id,
                    "output":content,
                }));
            }
            _ => {
                let parts = if let Some(parts) = message["content"].as_array() {
                    parts.iter().filter_map(|part| match part["type"].as_str() {
                        Some("text") => Some(json!({"type":"input_text","text":part["text"]})),
                        Some("image_url") => {
                            let mut image = json!({"type":"input_image","image_url":part["image_url"]["url"]});
                            if let Some(detail) = part["image_url"].get("detail") {
                                image["detail"] = detail.clone();
                            }
                            Some(image)
                        }
                        _ => None,
                    }).collect::<Vec<_>>()
                } else {
                    vec![json!({"type":"input_text","text":content})]
                };
                input.push(json!({"role":"user","content":parts}));
            }
        }
    }
    if input.is_empty() {
        input.push(json!({
            "role":"user",
            "content":[{"type":"input_text","text":"ping"}]
        }));
    }
    (instructions, input)
}

/// Persistent Responses/WebSocket input: keep the full immutable room
/// contract in a high-priority input message, not top-level instructions.
/// Responses previous_response_id inherits earlier input messages, while it
/// does NOT inherit top-level instructions. After a verified prefix match,
/// the socket sends only newly appended items. An edit/reconnect resends this
/// complete anchor on a fresh chain. HTTP fallback keeps the original full
/// top-level instructions to preserve compatibility with the Codex gateway.
pub(super) fn codex_ws_persistent_input(instructions: &str, input: &[Value]) -> Vec<Value> {
    let mut persisted=Vec::with_capacity(input.len()+1);
    if !instructions.trim().is_empty() {
        persisted.push(json!({
            "role":"developer",
            "content":[{"type":"input_text","text":instructions}]
        }));
    }
    persisted.extend_from_slice(input);
    persisted
}

/// Terminal failure events have identical meaning on HTTP and WebSocket.
pub(in crate::engine::chat) fn codex_terminal_error(piece: &Value) -> Option<String> {
    let message = match piece["type"].as_str()? {
        "response.failed" => piece["response"]["error"]["message"]
            .as_str().unwrap_or("응답이 실패했습니다.").to_string(),
        "response.incomplete" => format!(
            "응답이 완료되지 않았습니다: {}",
            piece["response"]["incomplete_details"]["reason"].as_str().unwrap_or("unknown")
        ),
        "error" => piece["error"]["message"].as_str().unwrap_or("알 수 없는 오류").to_string(),
        _ => return None,
    };
    Some(format!("Codex: {message}"))
}

/// Codex/ChatGPT subscription models speak the Responses API on a different
/// host and require the OAuth account id, not an API key.
pub(super) async fn stream_codex_once(
    state: &AppState,
    client: &reqwest::Client,
    provider: &ProviderRef,
    messages: &[Value],
    tools: &[Value],
    request: &TurnRequest,
    session_id: &str,
    tx: &UnboundedSender<Value>,
    continuity: &mut Value,
) -> Result<Completion, String> {
    let account_id = provider
        .account_id
        .as_deref()
        .filter(|value| !value.is_empty())
        .ok_or("Codex OAuth 계정 ID가 없습니다. 모델 관리에서 Codex를 다시 가져오세요.")?;
    let started = std::time::Instant::now();
    let (instructions, input) = codex_input(messages);
    let instructions=if instructions.is_empty() {
        "You are a helpful assistant. Answer in Korean unless the user writes in another language.".to_string()
    } else { instructions };
    // Assemble the persistent developer item while the immutable persona
    // string is still borrowed; the fallback HTTP body owns the full string.
    let ws_input=codex_ws_persistent_input(&instructions, &input);
    let mut body = json!({
        "model": provider.model,
        "instructions": instructions,
        "input": input.clone(),
        "stream": true,
        "store": false,
    });
    if let Some(limit) = provider.max_output_tokens {
        body["max_output_tokens"] = json!(limit);
    }
    let codex_tools = codex_tools(tools);
    if !codex_tools.is_empty() {
        body["tools"] = json!(codex_tools);
        body["tool_choice"] = json!("auto");
    }
    let catalog = state.store.get("codex_model_catalog", &provider.id)
        .filter(|row|row["accountId"].as_str() == Some(account_id));
    // An explicit tier wins over a legacy inherited boolean setting.
    let effort = request.reasoning_effort.as_deref()
        .or_else(|| (request.thinking == Some(false)).then_some("none"));
    if let Some(reasoning) = crate::engine::reasoning::codex_options(catalog.as_ref(), &provider.model, effort)? {
        body["reasoning"] = reasoning;
    }
    let access = crate::engine::codex_oauth::fresh_access(
        state,
        client,
        &provider.id,
        &provider.api_key,
    )
    .await?;
    // Preserve the full persona/system contract once in the Responses input
    // chain. A top-level 'instructions' field is per-call and would otherwise
    // retransmit the same 4k prompt on *every* WebSocket response.create.
    // Codex's HTTP fallback below keeps the unmodified, fully compatible
    // top-level instructions and full explicit history.
    let mut ws_body=body.clone();
    ws_body["instructions"]=json!("Follow the developer guidance in the conversation input.");
    let ws_body=super::super::continuity::hinted_body(state,provider,&ws_body,continuity,super::super::continuity::CacheWire::Key);
    let websocket = super::super::codex_ws::stream(
        state,
        provider,
        account_id,
        &access,
        continuity["transportSessionId"].as_str().unwrap_or(""),
        &ws_body,
        ws_input,
        session_id,
        tx,
    )
    .await;
    match websocket {
        Ok(outcome) => {
            if continuity["wireHintSent"]==true {
                super::super::continuity::note_hint(state,provider,super::super::continuity::CacheWire::Key,true,200);
                continuity["cacheHintAcceptance"]=json!(true);
            }
            let mut completion = outcome.completion;
            fill_estimated_usage(messages,tools,&mut completion);
            continuity["incrementalTransport"] = outcome.transport;
            crate::engine::gateway::record_usage_tokens_with_cache(
                state,
                &provider.id,
                &provider.model,
                provider.local,
                "chat",
                Some(session_id),
                completion.prompt_tokens,
                completion.completion_tokens,
                completion.usage,
                completion.cached_prompt_tokens,
                None,
                !completion.usage_exact,
            );
            return Ok(completion);
        }
        Err(super::super::codex_ws::WsError::Fatal(message)) => return Err(message),
        Err(super::super::codex_ws::WsError::HintRejected(reason)) => {
            super::super::continuity::note_hint(state,provider,super::super::continuity::CacheWire::Key,false,400);
            continuity["incrementalTransport"]=json!({"wire":"http","mode":"full-fallback","fallbackReason":reason});
        }
        Err(super::super::codex_ws::WsError::Fallback(reason)) => {
            continuity["incrementalTransport"] = json!({
                "wire":"http",
                "mode":"full-fallback",
                "fallbackReason":reason,
                "sentInputTokens":super::super::room_context::estimate(&json!(body["input"]).to_string()),
                "fullInputTokens":super::super::room_context::estimate(&json!(body["input"]).to_string()),
                "instructionTokens":super::super::room_context::estimate(&json!(body["instructions"]).to_string()),
                "modelCalls":0
            });
            state.store.audit(
                "codex.ws.fallback",
                json!({"sessionId":session_id,"reason":reason}),
            );
        }
    }
    let builder = client
        .post(format!("{}/responses", provider.base_url.trim_end_matches('/')))
        .header("Authorization", format!("Bearer {access}"))
        .header("chatgpt-account-id", account_id)
        .header("OpenAI-Beta", "responses=experimental")
        .header("originator", "codex_cli_rs")
        .header("User-Agent", "codex_cli_rs/0.0.0")
        .header("session_id", continuity["transportSessionId"].as_str().unwrap_or(""))
        .header("x-client-request-id", continuity["cacheKey"].as_str().unwrap_or(""))
        .header("Content-Type", "application/json");
    let response=super::super::continuity::send(state,provider,builder,&body,continuity,super::super::continuity::CacheWire::Key).await?;
    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        if matches!(status.as_u16(), 401 | 403) {
            return Err("Codex OAuth 토큰이 만료되었거나 거부되었습니다. Codex 앱에서 다시 로그인한 뒤 모델 관리에서 다시 가져오세요.".into());
        }
        return Err(format!(
            "{} 호출 실패 (HTTP {status}): {}",
            provider.name,
            text.chars().take(600).collect::<String>()
        ));
    }

    let mut completion = Completion::default();
    let mut buffer = SseBuffer::default();
    let mut stream = response.bytes_stream();
    let mut terminated = false;
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
                break 'stream;
            }
            if payload.is_empty() {
                continue;
            }
            let piece: Value = match serde_json::from_str(&payload) {
                Ok(value) => value,
                Err(_) => continue,
            };
            match piece["type"].as_str().unwrap_or("") {
                "response.output_text.delta" => {
                    let delta = piece["delta"].as_str().unwrap_or("");
                    if delta.is_empty() {
                        continue;
                    }
                    if completion.first_delta_ms.is_none() {
                        completion.first_delta_ms = Some(started.elapsed().as_millis() as u64);
                    }
                    completion.content.push_str(delta);
                    let _ = tx.send(json!({"type":"text","text":delta}));
                }
                "response.reasoning_summary_text.delta" | "response.reasoning_text.delta" => {
                    let delta = piece["delta"].as_str().unwrap_or("");
                    if !delta.is_empty() {
                        if completion.first_delta_ms.is_none() {
                            completion.first_delta_ms = Some(started.elapsed().as_millis() as u64);
                        }
                        completion.reasoning.push_str(delta);
                        let _ = tx.send(json!({"type":"reasoning","text":delta}));
                    }
                }
                "response.output_item.done" => {
                    if let Some(item) = piece.get("item") {
                        for call in codex_tool_calls(std::slice::from_ref(item)) {
                            if !completion
                                .tool_calls
                                .iter()
                                .any(|existing| existing.id == call.id)
                            {
                                completion.tool_calls.push(call);
                            }
                        }
                    }
                }
                "response.completed" => {
                    if let Some(usage) = piece["response"]["usage"].as_object() {
                        completion.cached_prompt_tokens = cached_prompt_tokens(&json!(usage));
                        completion.prompt_tokens =
                            usage.get("input_tokens").and_then(Value::as_u64).unwrap_or(0);
                        completion.completion_tokens =
                            usage.get("output_tokens").and_then(Value::as_u64).unwrap_or(0);
                        completion.usage = usage
                            .get("total_tokens")
                            .and_then(Value::as_u64)
                            .unwrap_or(completion.prompt_tokens + completion.completion_tokens);
                        completion.usage_exact = true;
                    }
                    let output = piece["response"]["output"]
                        .as_array()
                        .cloned()
                        .unwrap_or_default();
                    for call in codex_tool_calls(&output) {
                        if !completion
                            .tool_calls
                            .iter()
                            .any(|existing| existing.id == call.id)
                        {
                            completion.tool_calls.push(call);
                        }
                    }
                    terminated = true;
                    break 'stream;
                }
                "response.failed" | "response.incomplete" => {
                    return Err(codex_terminal_error(&piece).unwrap());
                }
                "error" => {
                    let message = piece["error"]["message"].as_str().unwrap_or("알 수 없는 오류");
                    return Err(format!("Codex: {message}"));
                }
                _ => {}
            }
        }
    }
    if !terminated {
        return Err("Codex 스트림이 완료 신호 없이 종료되었습니다.".into());
    }
    fill_estimated_usage(messages,tools,&mut completion);
    crate::engine::gateway::record_usage_tokens_with_cache(
        state,
        &provider.id,
        &provider.model,
        provider.local,
        "chat",
        Some(session_id),
        completion.prompt_tokens,
        completion.completion_tokens,
        completion.usage,
        completion.cached_prompt_tokens,
        None,
        !completion.usage_exact,
    );
    Ok(completion)
}

