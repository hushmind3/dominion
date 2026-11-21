//! OpenAI-compatible provider resolution, native thinking controls and HTTP streaming.
use super::{protocol::SseBuffer, Completion, ProviderRef, ToolCall, TurnRequest, MAX_TOOL_CALLS};
use crate::state::AppState;
use futures_util::StreamExt;
use serde_json::{json, Value};
use tokio::sync::mpsc::UnboundedSender;

#[path = "provider/context_budget.rs"]
mod context_budget;
#[path = "provider/compat.rs"]
mod compat;
#[path = "provider/anthropic.rs"]
mod anthropic;
#[path = "provider/codex.rs"]
mod codex;

use context_budget::{input_char_capacity, fixed_prompt_chars, normalize_assistant_reasoning, order_tool_exchanges, preserve_reflection_history, validate_wire_context, escaped_content_chars, clip_tool_for_provider, edit_stale_read_context, fit_tool_context};
pub(super) use context_budget::cached_prompt_tokens;
use compat::{compat_messages, stream_completion_once_body};
use anthropic::{anthropic_message_text, stream_anthropic_once};
use codex::{codex_tools, codex_input, codex_ws_persistent_input, stream_codex_once};
pub(super) use codex::{codex_function_call_item, codex_tool_calls, codex_terminal_error};

const TEXT_TOOL_OPEN: &str = "<tool_call>";
const TEXT_TOOL_CLOSE: &str = "</tool_call>";

/// Streams visible text while hiding text-protocol tool calls. Small local
/// models emit `<tool_call>...` as ordinary content; without this buffer the
/// user sees the raw JSON before the tool actually runs.
fn stream_visible_text(
    tx: &UnboundedSender<Value>,
    content: &str,
    pending: &mut String,
    in_tool: &mut bool,
) {
    pending.push_str(content);
    loop {
        if *in_tool {
            if let Some(end) = pending.find(TEXT_TOOL_CLOSE) {
                pending.drain(..end + TEXT_TOOL_CLOSE.len());
                *in_tool = false;
                continue;
            }
            break;
        }
        if let Some(start) = pending.find(TEXT_TOOL_OPEN) {
            let before = pending[..start].to_string();
            if !before.is_empty() {
                let _ = tx.send(json!({ "type": "text", "text": before }));
            }
            pending.drain(..start + TEXT_TOOL_OPEN.len());
            *in_tool = true;
            continue;
        }
        // A tag may be split across deltas. Hold only a suffix that can still
        // become `<tool_call>`.
        let hold = (1..TEXT_TOOL_OPEN.len())
            .rev()
            .find(|length| {
                pending.len() >= *length && pending.ends_with(&TEXT_TOOL_OPEN[..*length])
            })
            .unwrap_or(0);
        let emit = pending.len() - hold;
        if emit > 0 {
            let text = pending[..emit].to_string();
            let _ = tx.send(json!({ "type": "text", "text": text }));
            pending.drain(..emit);
        }
        break;
    }
}

pub(crate) fn select_provider(
    state: &AppState,
    request: &TurnRequest,
    session: &Value,
) -> Result<ProviderRef, String> {
    let agent = request
        .agent_id
        .as_deref()
        .and_then(|id| state.store.get("agent", id));

    let provider_id = request
        .provider_id
        .clone()
        .or_else(|| {
            agent
                .as_ref()
                .and_then(|agent| agent.get("providerId"))
                .and_then(|value| value.as_str())
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
        .or_else(|| {
            if session["roomId"].is_string() {
                return None;
            }
            session
                .get("providerId")
                .and_then(|value| value.as_str())
                .filter(|id| state.store.get("provider", id)
                    .is_some_and(|provider| provider["enabled"] != false))
                .map(str::to_string)
        });

    let provider = match provider_id {
        Some(id) => state.store.get("provider", &id),
        None => {
            // Auto mode: no explicit provider/agent/session selection. Pick a
            // deterministic tier from the request instead of the first record.
            let messages = session["id"]
                .as_str()
                .map(|id| state.store.messages(id).len())
                .unwrap_or(0);
            let complexity = crate::engine::router::classify(
                &request.input,
                request.attachment_ids.len(),
                messages,
            );
            crate::engine::router::select_provider(state, complexity).or_else(|| {
                state.store.all("provider").into_iter().find(|provider| {
                    provider
                        .get("enabled")
                        .and_then(|value| value.as_bool())
                        .unwrap_or(true)
                })
            })
        }
    };

    let Some(provider) = provider else {
        return Err(
            "모델 제공자가 설정되지 않았습니다. 모델 관리에서 OpenAI 호환 제공자를 추가하세요."
                .into(),
        );
    };

    if provider.get("enabled").and_then(|value| value.as_bool()) == Some(false) {
        return Err("선택한 제공자가 비활성 상태입니다.".into());
    }

    let id = provider
        .get("id")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .to_string();
    let base_url = provider
        .get("baseUrl")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .trim_end_matches('/')
        .to_string();
    if base_url.is_empty() {
        return Err("제공자에 baseUrl 이 없습니다.".into());
    }

    let model = request
        .model
        .clone()
        .filter(|value| !value.is_empty())
        .or_else(|| {
            agent
                .as_ref()
                .filter(|agent| {
                    agent["providerId"]
                        .as_str()
                        .is_none_or(|provider_id| provider_id == id)
                })
                .and_then(|agent| agent.get("model"))
                .and_then(|value| value.as_str())
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
        .or_else(|| {
            (session["providerId"].as_str() == Some(id.as_str())
                && session["agentId"].as_str() == request.agent_id.as_deref())
            .then(|| {
                session["model"]
                    .as_str()
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
            })
            .flatten()
        })
        .or_else(|| {
            provider
                .get("model")
                .and_then(|value| value.as_str())
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
        .ok_or("이 API에서 사용할 모델을 선택하거나 모델 ID를 입력하세요.")?;

    let route = crate::engine::gateway::route_from_provider(state, &provider, Some(&model))?;
    Ok(ProviderRef {
        id: route.id,
        name: route.name,
        base_url: route.base_url,
        api_key: route.api_key,
        model: route.model,
        tool_mode: route.tool_mode,
        reasoning_protocol: route.reasoning_protocol,
        local: route.local,
        context_tokens: route.context_tokens,
        max_output_tokens: route.max_output_tokens,
        requests_per_minute: route.requests_per_minute,
        kind: route.kind,
        account_id: route.account_id,
    })
}

pub(super) fn apply_reasoning_options(
    body: &mut Value,
    protocol: &str,
    thinking: Option<bool>,
    effort: Option<&str>,
) {
    if protocol == "deepseek" {
        let enabled = if thinking == Some(false) || effort == Some("none") {
            Some(false)
        } else {
            thinking
        };
        if let Some(enabled) = enabled {
            body["thinking"] = json!({"type":if enabled{"enabled"}else{"disabled"}});
        }
        if enabled != Some(false) {
            if let Some(effort) = effort {
                body["reasoning_effort"] = json!(match effort {
                    "minimal" | "low" => "low",
                    _ => "high",
                });
            }
        }
    } else {
        // Ollama and Qwen templates expose thinking through enable_thinking.
        // Sending OpenAI's reasoning_effort to those endpoints makes Ollama
        // reject non-thinking models with "does not support thinking".
        if let Some(effort) = effort {
            if !matches!(protocol, "chat_template" | "qwen") {
                body["reasoning_effort"] = json!(effort);
            }
        }
        if let Some(thinking) = thinking.or_else(|| (effort == Some("none")).then_some(false)) {
            match protocol {
                "chat_template" => {
                    body["chat_template_kwargs"] = json!({"enable_thinking":thinking})
                }
                "qwen" => body["enable_thinking"] = json!(thinking),
                _ => {}
            }
        }
    }
}

/// OpenAI-compatible servers may send the entire final choice as
/// choices[0].message rather than streaming it through delta. Only fill
/// fields that did not arrive through deltas; otherwise the final snapshot
/// duplicates content already visible to the user.
fn merge_final_chat_message(
    completion: &mut Completion,
    calls: &mut Vec<ToolCall>,
    message: &Value,
) -> Result<(String, String), String> {
    let mut new_text = String::new();
    let mut new_reasoning = String::new();
    if let Some(text) = message["content"].as_str().filter(|text| !text.is_empty()) {
        // Some relays send delta prefixes and later a complete message.
        // Append only a proven missing suffix, never replay an old prefix.
        let suffix = if completion.content.is_empty() {
            Some(text)
        } else {
            text.strip_prefix(completion.content.as_str())
        };
        if let Some(suffix) = suffix.filter(|value| !value.is_empty()) {
            completion.content.push_str(suffix);
            new_text.push_str(suffix);
        }
    }
    if let Some(text) = message["reasoning_content"]
        .as_str()
        .or_else(|| message["reasoning"].as_str())
        .filter(|text| !text.is_empty())
    {
        let suffix = if completion.reasoning.is_empty() {
            Some(text)
        } else {
            text.strip_prefix(completion.reasoning.as_str())
        };
        if let Some(suffix) = suffix.filter(|value| !value.is_empty()) {
            completion.reasoning.push_str(suffix);
            new_reasoning.push_str(suffix);
        }
    }
    if let Some(final_calls) = message["tool_calls"].as_array() {
        if final_calls.len() > MAX_TOOL_CALLS {
            return Err("모델이 너무 많은 도구 호출을 반환했습니다.".into());
        }
        if !final_calls.is_empty() {
            let mut parsed = Vec::with_capacity(final_calls.len());
            for (index, item) in final_calls.iter().enumerate() {
                let name = item["function"]["name"]
                    .as_str()
                    .filter(|name| !name.is_empty())
                    .ok_or("모델의 최종 도구 호출에 이름이 없습니다.")?;
                let id = item["id"].as_str().filter(|id| !id.is_empty())
                    .or_else(|| calls.get(index).map(|call| call.id.as_str())
                        .filter(|id| !id.is_empty()))
                    .map(str::to_string)
                    .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
                parsed.push(ToolCall {
                    id,
                    name: name.to_string(),
                    arguments: item["function"]["arguments"]
                        .as_str().unwrap_or("{}").to_string(),
                });
            }
            // Some compatible relays send the full snapshot after streaming
            // only the first tool, or while an argument is still incomplete.
            // Complete only a proven prefix; conflicting snapshots must never
            // replace an already observed tool request.
            let aligned = parsed.len() >= calls.len()
                && calls.iter().zip(parsed.iter()).all(|(partial, full)| {
                    (partial.id.is_empty() || partial.id == full.id)
                        && full.name.starts_with(partial.name.as_str())
                        && full.arguments.starts_with(partial.arguments.as_str())
                });
            if aligned {
                *calls = parsed;
            }
        }
    }
    Ok((new_text, new_reasoning))
}

/// Retry transient provider/stream failures only before any content was
/// published to the client. Replaying a partially streamed answer could repeat
/// visible text, reasoning, or a tool request, so that case always stops.
fn transient_provider_error(error: &str) -> bool {
    error.contains(" 연결 실패:")
        || error.contains("스트림 오류:")
        || error.contains("모델 스트림이 완료 신호 없이 종료")
        || error.contains("(HTTP 408")
        || error.contains("(HTTP 429")
        || error.contains("(HTTP 5")
}

/// Estimated input room for the declared model context, leaving its
/// configured generation capacity reserved. No universal character cap.
fn room_route_keys(state: &AppState, session_id: &str) -> Option<std::collections::HashSet<String>> {
    let session = state.store.get("session", session_id)?;
    let room = state.store.get("room", session["roomId"].as_str()?)?;
    let mut keys = std::collections::HashSet::new();
    for entry in room["models"].as_array().into_iter().flatten() {
        if let (Some(provider), Some(model)) =
            (entry["providerId"].as_str(), entry["model"].as_str())
        {
            keys.insert(format!("{provider}|{model}"));
        }
    }
    if !keys.is_empty() {
        return Some(keys);
    }
    for id in room["agentIds"].as_array()? {
        let Some(agent) = id.as_str().and_then(|id| state.store.get("agent", id)) else {
            continue;
        };
        if let (Some(provider), Some(model)) =
            (agent["providerId"].as_str(), agent["model"].as_str())
        {
            keys.insert(format!("{provider}|{model}"));
        }
    }
    Some(keys)
}

async fn fallback_candidates(
    state: &AppState,
    primary: &ProviderRef,
    request: &TurnRequest,
    messages: &[Value],
    tools: &[Value],
) -> Vec<ProviderRef> {
    let fixed_chars = fixed_prompt_chars(messages);
    let native_schema_chars = tools.iter()
        .map(|tool| tool.to_string().chars().count()).sum::<usize>();
    let compat_schema_chars = if tools.is_empty() {
        0
    } else {
        escaped_content_chars(&super::protocol::tool_manual(tools, &request.input).await)
    };
    // A room may only fall back to its own participants' routes. Otherwise a
    // dead free model silently escalates to whatever paid provider happens to
    // rank highest, which is exactly the surprise bill to avoid.
    let needs_vision=request.attachment_ids.iter().any(|id|
        state.store.get("attachment",id)
            .is_some_and(|attachment|attachment["isImage"]==true));
    let room_routes = request
        .session_id
        .as_deref()
        .and_then(|session_id| room_route_keys(state, session_id));
    crate::engine::gateway::ordered_alternatives(state, &primary.id)
        .into_iter()
        .filter_map(|mut route| {
            let record=state.store.get("provider",&route.id)?;
            crate::engine::gateway::require_streaming(
                &record,&route.model,route.kind!="cli").ok()?;
            route.tool_mode=crate::engine::gateway::negotiate_tool_mode(
                &record,&route.model,&route.tool_mode,!tools.is_empty()).ok()?;
            Some(route)
        })
        .filter(|route| {
            room_routes.as_ref().is_none_or(|allowed| {
                allowed.contains(&format!("{}|{}", route.id, route.model))
            })
        })
        // Native/API and compat/local models can take over the same turn.
        // A smaller context clips *only tool observations* for that attempt;
        // it does not prevent a local fallback when the tool output is large.
        .filter(|route| {
            route.context_tokens.is_none_or(|tokens| {
                let schema_chars = if route.tool_mode == "compat" {
                    compat_schema_chars
                } else {
                    native_schema_chars
                };
                fixed_chars.saturating_add(schema_chars).saturating_add(512)
                    <= input_char_capacity(tokens, route.max_output_tokens)
            })
        })
        .filter(|route| {
            // An image requires the exact endpoint + selected model to
            // declare vision. Text-only attachments do not trigger this gate.
            state.store.get("provider",&route.id).is_some_and(|provider|
                crate::engine::gateway::require_vision(
                    &provider,&route.model,needs_vision).is_ok())
        })
        .map(|route| ProviderRef {
            id: route.id,
            name: route.name,
            base_url: route.base_url,
            api_key: route.api_key,
            model: route.model,
            tool_mode: route.tool_mode,
            reasoning_protocol: route.reasoning_protocol,
            local: route.local,
            context_tokens: route.context_tokens,
            max_output_tokens: route.max_output_tokens,
            requests_per_minute: route.requests_per_minute,
            kind: route.kind,
            account_id: route.account_id,
        })
        .collect()
}

/// Returns the completed response and the route which actually generated it.
/// A private relay preserves streaming to the user while detecting whether any
/// output was emitted before a provider failed.
/// Providers that omit usage during streaming still need a cost signal, so
/// prompt/output characters provide a conservative estimate. The flag stored
/// with the message keeps the estimate distinguishable from exact usage.
fn estimate_usage(messages: &[Value], tools: &[Value], completion: &Completion) -> (u64, u64) {
    (
        super::room_context::input_estimate(messages, tools) as u64,
        super::room_context::estimate(&format!("{}{}",completion.content,completion.reasoning)).max(1) as u64,
    )
}

/// Final provider-wire estimate is returned to the execution journal as well
/// as the provider usage counters. The native/compat transport must not
/// silently fall back to the pre-serialization schema estimate.
fn finalize_usage_estimate(completion:&mut Completion,prompt:u64,output:u64)->u64{
    let total=if completion.usage>0 {completion.usage}
        else{prompt.saturating_add(output)};
    if !completion.usage_exact{
        completion.prompt_tokens=prompt;
        completion.completion_tokens=output;
        completion.usage=total;
    }
    total
}

fn fill_estimated_usage(messages:&[Value],tools:&[Value],completion:&mut Completion){
    if !completion.usage_exact {
        let(prompt,output)=estimate_usage(messages,tools,completion);
        completion.prompt_tokens=prompt;
        completion.completion_tokens=output;
        completion.usage=prompt.saturating_add(output);
        completion.cached_prompt_tokens=None;
    }
}

pub(super) async fn stream_completion(
    state: &AppState,
    client: &reqwest::Client,
    provider: &ProviderRef,
    messages: &[Value],
    tools: &[Value],
    request: &TurnRequest,
    session_id: &str,
    tx: &UnboundedSender<Value>,
    allow_fallback: bool,
) -> Result<(Completion, ProviderRef), String> {
    let mut candidates = vec![provider.clone()];
    if allow_fallback {
        candidates.extend(fallback_candidates(state, provider, request, messages, tools).await);
    }
    // Keep normal compat sessions untouched. Translate native tool history
    // only when a turn originally started in native mode and falls back to a
    // compat model (also on later rounds of that same turn).
    let original_mode = request.provider_id.as_deref()
        .and_then(|id| state.store.get("provider", id))
        .and_then(|item| crate::engine::gateway::route_from_provider(state, &item, request.model.as_deref()).ok())
        .map(|route| route.tool_mode)
        .unwrap_or_else(|| provider.tool_mode.clone());
    let mut failed = Vec::new();

    for candidate in candidates {
        if state.is_cancelled(session_id) {
            return Err("실행이 중단되었습니다.".into());
        }
        let (relay, mut relay_rx) = tokio::sync::mpsc::unbounded_channel();
        let candidate_started = std::time::Instant::now();
        let (result, exposed) = {
            let attempt = stream_completion_once(
                state, client, &candidate, messages, tools, request, session_id, &relay,
                candidate.tool_mode == "compat" && original_mode != "compat",
            );
            tokio::pin!(attempt);
            let mut exposed = false;
            let result = loop {
                tokio::select! {
                    biased;
                    event = relay_rx.recv() => {
                        if let Some(event) = event {
                            exposed = true;
                            let _ = tx.send(event);
                        }
                    }
                    complete = &mut attempt => break complete,
                }
            };
            // The model may finish and enqueue its last delta in the same tick.
            // Forward those deltas before deciding whether replay is safe.
            while let Ok(event) = relay_rx.try_recv() {
                exposed = true;
                let _ = tx.send(event);
            }
            (result, exposed)
        };
        // A two-minute answer is not a usable room participant. Remember it
        // so the next round does not pay the same price again.
        if candidate_started.elapsed() >= std::time::Duration::from_secs(60) {
            crate::engine::gateway::mark_slow(&candidate.base_url, &candidate.model);
        }
        match result {
            Ok(completion) => {
                if candidate.id != provider.id {
                    state.store.audit("chat.provider_failover", json!({
                        "sessionId":session_id, "from":provider.id, "to":candidate.id,
                        "model":candidate.model, "previousFailures":failed,
                    }));
                    let _ = tx.send(json!({
                        "type":"provider_failover",
                        "from":provider.id, "to":candidate.id,
                        "model":candidate.model,
                    }));
                }
                return Ok((completion, candidate));
            }
            Err(error) => {
                // No replay after partial visible content or a policy/cancel
                // error. The next provider is only allowed to retry a clean
                // transient failure with zero externally observed output.
                if exposed || !transient_provider_error(&error) || state.is_cancelled(session_id) {
                    return Err(error);
                }
                failed.push(json!({"providerId":candidate.id,"error":error}));
            }
        }
    }
    Err(format!("사용 가능한 제공자가 모두 실패했습니다: {}", json!(failed)))
}

/// Compatibility models consume text protocol rather than OpenAI tool-call
/// message roles. Convert native tool history into ordinary labeled evidence,
/// preserving it across cloud-to-local failover.
async fn stream_completion_once(
    state: &AppState,
    client: &reqwest::Client,
    provider: &ProviderRef,
    messages: &[Value],
    tools: &[Value],
    request: &TurnRequest,
    session_id: &str,
    tx: &UnboundedSender<Value>,
    convert_native_history: bool,
) -> Result<Completion, String> {
    // Every provider uses the same participant state and route binding.
    // Adapters only translate the optional cache hint into their wire schema.
    let instructions=messages.iter().filter(|row|row["role"]=="system")
        .map(|row|anthropic_message_text(&row["content"]))
        .collect::<Vec<_>>().join("\n\n");
    // Tool order is part of the execution contract: compatibility menus may
    // use that priority when their schema budget is limited.
    let ordered_tools=tools.to_vec();
    let mut selected_route=provider.clone();
    if let Some(record)=state.store.get("provider",&provider.id){
        crate::engine::gateway::require_streaming(
            &record,&provider.model,provider.kind!="cli")?;
        let needs_vision=request.attachment_ids.iter().any(|id|
            state.store.get("attachment",id)
                .is_some_and(|attachment|attachment["isImage"]==true));
        crate::engine::gateway::require_vision(
            &record,&provider.model,needs_vision)?;
        selected_route.tool_mode=crate::engine::gateway::negotiate_tool_mode(
            &record,&provider.model,&provider.tool_mode,!ordered_tools.is_empty())?;
        if selected_route.tool_mode!=provider.tool_mode {
            state.store.audit("model.capability_tool_mode_negotiated",json!({
                "providerId":provider.id,"model":provider.model,
                "requested":provider.tool_mode,"effective":selected_route.tool_mode,
                "executionSessionId":session_id}));
        }
    }
    let settings=json!({"thinking":request.thinking,"effort":request.reasoning_effort,
        "tools":ordered_tools,"toolMode":selected_route.tool_mode});
    let mut continuity=super::continuity::prepare(state,&selected_route,request,session_id,&instructions,&settings)?;
    let (projected, projection)=super::continuity::project(state,&selected_route,messages,&ordered_tools,session_id,&mut continuity);
    let input=super::room_context::input_estimate(&projected,&ordered_tools) as u64;
    let (budget_call,output)=crate::engine::execution_budget::prepare_call(state,session_id,
        &provider.id,&provider.model,provider.local,input,provider.max_output_tokens.unwrap_or(8192))?;
    let mut selected=selected_route;
    if output<provider.max_output_tokens.unwrap_or(8192){selected.max_output_tokens=Some(output);}
    let result=stream_completion_once_body(state,client,&selected,&projected,&ordered_tools,
        request,session_id,tx,convert_native_history,&mut continuity).await;
    crate::engine::execution_budget::finish_call(budget_call,result.is_ok(),result.as_ref().err().map(String::as_str))?;
    let mut completion=result?;
    completion.projection=projection;
    completion.continuity=Some(continuity);
    Ok(completion)
}

// Keep bytes until a whole SSE event arrives; network chunks may split a
// Korean UTF-8 character or the CRLF separator at any byte.

#[cfg(test)]
#[path = "provider/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "provider/final_chat_message_tests.rs"]
mod final_chat_message_tests;
#[cfg(test)]
mod cache_accounting_tests {
    use super::*;
    #[test]
    fn provider_cache_counts_are_optional_and_never_invent_zero_hits() {
        assert_eq!(cached_prompt_tokens(&json!({})),None);
        assert_eq!(cached_prompt_tokens(&json!({"prompt_tokens":1200})),None);
        assert_eq!(cached_prompt_tokens(&json!({"cached_tokens":512})),Some(512));
        assert_eq!(cached_prompt_tokens(&json!({"cache_read_input_tokens":256})),Some(256));
        assert_eq!(cached_prompt_tokens(&json!({"prompt_tokens_details":{"cached_tokens":0}})),Some(0));
        assert_eq!(cached_prompt_tokens(&json!({"prompt_tokens_details":{"cached_tokens":1024},
            "cached_tokens":900})),Some(1024));
    }
}

#[cfg(test)]
mod wire_context_admission_tests {
    use super::*;
    #[test]
    fn compat_manual_cannot_overflow_wire_before_http() {
        assert!(validate_wire_context(1388,Some(1024),Some(256)).is_err());
        assert!(validate_wire_context(1857,Some(1024),Some(256)).is_err());
        assert!(validate_wire_context(92,Some(1024),Some(256)).is_ok());
        assert!(validate_wire_context(2000,None,Some(256)).is_ok());
    }
    #[test]
    fn wire_fallback_usage_is_returned_to_execution_not_only_gateway_counters(){
        let mut completion=Completion::default();
        assert_eq!(finalize_usage_estimate(&mut completion,1857,45),1902);
        assert_eq!(completion.prompt_tokens,1857);
        assert_eq!(completion.completion_tokens,45);
        assert_eq!(completion.usage,1902);
        assert!(!completion.usage_exact);
        completion.usage_exact=true;
        completion.prompt_tokens=100;
        completion.completion_tokens=20;
        completion.usage=120;
        assert_eq!(finalize_usage_estimate(&mut completion,1857,45),120);
        assert_eq!(completion.prompt_tokens,100);
    }
}
