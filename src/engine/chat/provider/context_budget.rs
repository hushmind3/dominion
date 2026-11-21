//! Provider-owned context responsibilities. The harness and provider selection stay in the parent.
use super::*;

pub(super) fn input_char_capacity(tokens: u64, max_output_tokens: Option<u64>) -> usize {
    let reserved = max_output_tokens.unwrap_or(tokens / 8)
        .min(tokens / 4).max(128).min(tokens);
    (tokens.saturating_sub(reserved) as usize).saturating_mul(4)
}

/// Count the fixed prompt without the bodies of prior tool observations.
/// Those are dynamically reduced for each candidate provider at request time.
pub(super) fn fixed_prompt_chars(messages: &[Value]) -> usize {
    messages.iter().map(|message| {
        if message["role"] == "tool" {
            let mut envelope = message.clone();
            envelope["content"] = json!("");
            envelope.to_string().chars().count()
        } else {
            message.to_string().chars().count()
        }
    }).sum::<usize>()
}

/// Count serialized JSON string characters (without the surrounding quotes).
/// Escaping matters for structured tool results containing JSON or newlines.
pub(super) fn normalize_assistant_reasoning(messages: &mut [Value], required: bool) {
    for message in messages {
        if message["role"] != "assistant" {
            continue;
        }
        if required {
            if !message["reasoning_content"].is_string() {
                message["reasoning_content"] = json!("");
            }
        } else if let Some(map) = message.as_object_mut() {
            map.remove("reasoning_content");
        }
    }
}

/// Provider protocols require every assistant tool call to be followed
/// immediately by its tool result. Runtime hints are sometimes inserted as
/// system messages while a round is still executing; move those hints after
/// the completed tool exchange without dropping them.
pub(super) fn order_tool_exchanges(messages: Vec<Value>) -> Vec<Value> {
    let mut out = Vec::with_capacity(messages.len());
    let mut index = 0;
    while index < messages.len() {
        let message = &messages[index];
        let calls = message["tool_calls"]
            .as_array()
            .filter(|calls| !calls.is_empty());
        if message["role"] != "assistant" || calls.is_none() {
            out.push(message.clone());
            index += 1;
            continue;
        }
        let mut expected = calls
            .unwrap()
            .iter()
            .filter_map(|call| call["id"].as_str().map(str::to_string))
            .collect::<std::collections::BTreeSet<_>>();
        let mut tools = Vec::new();
        let mut hints = Vec::new();
        let mut cursor = index + 1;
        while cursor < messages.len()
            && (messages[cursor]["role"] == "tool" || messages[cursor]["role"] == "system")
        {
            if messages[cursor]["role"] == "tool" {
                if let Some(id) = messages[cursor]["tool_call_id"].as_str() {
                    expected.remove(id);
                }
                tools.push(messages[cursor].clone());
            } else {
                hints.push(messages[cursor].clone());
            }
            cursor += 1;
            if expected.is_empty() {
                break;
            }
        }
        out.push(message.clone());
        out.extend(tools);
        out.extend(hints);
        index = cursor;
    }
    out
}

/// Reflection Beam requires reasoning_content for assistant turns. If an
/// earlier model/legacy record has no original reasoning, keep its *visible
/// evidence* as an explicitly quoted user record, never invent private
/// reasoning and never erase the entire transcript.
pub(super) fn preserve_reflection_history(messages: Vec<Value>) -> (Vec<Value>, bool) {
    let mut filled = false;
    let mut out = Vec::with_capacity(messages.len());
    for mut message in messages {
        if message["role"] == "assistant"
            && message["reasoning_content"]
                .as_str()
                .is_none_or(|text| text.is_empty())
        {
            // Reflection requires the field on every earlier assistant turn.
            // A space preserves roles, tool IDs and pairing without inventing
            // a private rationale. Real reasoning is never overwritten.
            message["reasoning_content"] = json!(" ");
            filled = true;
        }
        out.push(message);
    }
    (out, filled)
}

pub(super) fn cached_prompt_tokens(usage: &Value) -> Option<u64> {
    usage.pointer("/prompt_tokens_details/cached_tokens").and_then(Value::as_u64)
        .or_else(|| usage.pointer("/input_tokens_details/cached_tokens").and_then(Value::as_u64))
        .or_else(|| usage["prompt_cache_hit_tokens"].as_u64())
        .or_else(|| usage["cached_tokens"].as_u64())
        .or_else(|| usage["cached_input_tokens"].as_u64())
        .or_else(|| usage["cache_read_input_tokens"].as_u64())
}

/// Check the final rendered HTTP request: compat mode has already inserted
/// the text tool manual and the native mode has its real schema array.
pub(super) fn validate_wire_context(input:usize,context:Option<u64>,output:Option<u64>)
    ->Result<(),String>{
    if let Some(capacity)=context.filter(|value|*value>0){
        let reserve=output.unwrap_or(4096).min((capacity/4).max(128));
        if (input as u64).saturating_add(reserve)>capacity {
            return Err(format!(
                "[model_not_started] 최종 모델 입력({input} 추정 토큰)과 출력 예약({reserve})이 컨텍스트 한도({capacity})를 초과합니다."
            ));
        }
    }
    Ok(())
}

pub(super) fn escaped_content_chars(text: &str) -> usize {
    text.chars().map(|ch| match ch {
        '"' | '\\' | '\n' | '\r' | '\t' | '\u{0008}' | '\u{000c}' => 2,
        '\u{0000}'..='\u{001f}' => 6,
        _ => 1,
    }).sum()
}

/// Trim just the provider-visible copy, retaining the newest observations
/// first. All raw results remain in the runtime/DB and are available when
/// a larger context provider takes over a later round.
pub(super) fn clip_tool_for_provider(text: &str, allowed_encoded_chars: usize) -> String {
    if escaped_content_chars(text) <= allowed_encoded_chars {
        return text.to_owned();
    }
    const NOTICE: &str = "\n… [this model's context is full; see stored execution_operation]";
    let notice_size = escaped_content_chars(NOTICE);
    if allowed_encoded_chars < notice_size {
        return if allowed_encoded_chars > 0 { "…".to_string() } else { String::new() };
    }
    let mut prefix = String::new();
    let mut used = 0usize;
    for ch in text.chars() {
        let width = match ch {
            '"' | '\\' | '\n' | '\r' | '\t' | '\u{0008}' | '\u{000c}' => 2,
            '\u{0000}'..='\u{001f}' => 6,
            _ => 1,
        };
        if used.saturating_add(width) > allowed_encoded_chars - notice_size {
            break;
        }
        prefix.push(ch);
        used += width;
    }
    prefix.push_str(NOTICE);
    prefix
}

/// Context editing for long tool-heavy sessions. Only provider-facing copies
/// are changed: execution_operation and durable chat history remain exact.
/// Inspired by 2026 context-editing practice, but uses no added model call.
///
/// Keep recent observations and all writes/errors intact. Older large reads
/// are reduced only near a declared context limit AND when the reduction is
/// large enough to justify losing a prompt-cache prefix.
pub(super) fn edit_stale_read_context(messages: &mut [Value], capacity: usize, schema_chars: usize) -> usize {
    if capacity < 4096 { return 0; }
    let mut total = fixed_prompt_chars(messages)
        .saturating_add(schema_chars)
        .saturating_add(512);
    total = total.saturating_add(messages.iter()
        .filter(|row| row["role"] == "tool")
        .filter_map(|row| row["content"].as_str())
        .map(escaped_content_chars).sum::<usize>());
    if total <= capacity.saturating_mul(3) / 4 { return 0; }

    // IDs are only unique within their assistant tool batch. Providers may
    // reuse an ID in a later turn: never reclassify an older WRITE as a READ.
    let mut pending = std::collections::HashMap::<String, String>::new();
    let mut names_by_result = std::collections::HashMap::<usize, String>::new();
    for (index, message) in messages.iter().enumerate() {
        match message["role"].as_str() {
            Some("assistant") => {
                pending.clear();
                for call in message["tool_calls"].as_array().into_iter().flatten() {
                    if let (Some(id), Some(name)) = (call["id"].as_str(),
                        call["function"]["name"].as_str().or_else(|| call["name"].as_str())) {
                        pending.insert(id.to_owned(), name.to_owned());
                    }
                }
            }
            Some("tool") => {
                if let Some(id) = message["tool_call_id"].as_str() {
                    if let Some(name) = pending.remove(id) {
                        names_by_result.insert(index, name);
                    }
                }
            }
            Some("user") => pending.clear(),
            _ => {}
        }
    }
    let mut recent = 0usize;
    let mut candidates = Vec::<(usize, String, usize)>::new();
    for (index, message) in messages.iter().enumerate().rev() {
        if message["role"] != "tool" { continue; }
        recent += 1;
        if recent <= 3 { continue; }
        let Some(id) = message["tool_call_id"].as_str() else { continue; };
        let Some(tool) = names_by_result.get(&index) else { continue; };
        if !matches!(tool.as_str(), "workspace_read" | "workspace_read_range"
            | "workspace_list" | "file_read" | "file_list" | "web_search" | "http_fetch") {
            continue;
        }
        let Some(content) = message["content"].as_str() else { continue; };
        let size = escaped_content_chars(content);
        if size < 2400 || content.starts_with("Error:")
            || serde_json::from_str::<Value>(content).ok().is_some_and(|value|
                value.get("error").is_some() || value.get("ok") == Some(&json!(false))) {
            continue;
        }
        let begin: String = content.chars().take(240).collect();
        let end: String = content.chars().rev().take(120).collect::<Vec<_>>()
            .into_iter().rev().collect();
        let compact = format!(
            "[Older {tool} result compacted, call={id}, original_chars={}. \
             Original observation remains in durable execution history; \
             re-read if exact bytes are needed.]\n{begin}\n…\n{end}",
            content.chars().count(),
        );
        let saving = size.saturating_sub(escaped_content_chars(&compact));
        if saving > 1000 { candidates.push((index, compact, saving)); }
    }
    let minimum = (capacity / 12).clamp(8_000, 24_000);
    if candidates.iter().map(|(_, _, saving)| saving).sum::<usize>() < minimum {
        return 0;
    }
    // Oldest-first, then stop. This avoids needless cache invalidation on
    // subsequent turns and leaves the most recent work available verbatim.
    candidates.reverse();
    let mut cleared = 0usize;
    for (index, compact, saving) in candidates {
        if total <= capacity.saturating_mul(3) / 5 && cleared >= minimum { break; }
        messages[index]["content"] = json!(compact);
        total = total.saturating_sub(saving);
        cleared = cleared.saturating_add(saving);
    }
    cleared
}

/// Prepare a different model's copy from the original, unabridged tool
/// observations. No 6k/20k/80k magic limits or local/cloud distinction.
pub(super) fn fit_tool_context(provider: &ProviderRef, messages: &mut [Value], schema_chars: usize) -> usize {
    let Some(tokens) = provider.context_tokens else { return 0 };
    let fixed = fixed_prompt_chars(messages)
        .saturating_add(schema_chars)
        .saturating_add(512);
    let capacity = input_char_capacity(tokens, provider.max_output_tokens);
    let reclaimed = edit_stale_read_context(messages, capacity, schema_chars);
    let mut available = capacity.saturating_sub(fixed);
    // Preserve the most recent observation; old tool outputs get the
    // remaining space, with each tool-call ID kept intact.
    for message in messages.iter_mut().rev() {
        if message["role"] != "tool" {
            continue;
        }
        let Some(original) = message["content"].as_str() else { continue };
        let clipped = clip_tool_for_provider(original, available);
        available = available.saturating_sub(escaped_content_chars(&clipped));
        message["content"] = json!(clipped);
    }
    reclaimed
}

