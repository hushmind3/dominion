use crate::state::AppState;
use serde_json::{json, Value};

const KEEP_RECENT: usize = 8;

const MAX_TRANSCRIPT_CHARS: usize = 48_000;

/// Extract only the evidence needed for a continuation checkpoint. Full tool
/// responses and identifiers remain in SQLite, not in this model request.
fn excerpt(text: &str, limit: usize) -> String {
    let count = text.chars().count();
    if count <= limit { return text.to_string(); }
    let head = limit.saturating_mul(3) / 5;
    let tail = limit.saturating_sub(head).saturating_sub(56);
    let prefix: String = text.chars().take(head).collect();
    let suffix: String = text.chars().rev().take(tail).collect::<String>().chars().rev().collect();
    format!("{prefix}\n[{} chars omitted; full record retained]\n{suffix}",
        count.saturating_sub(head + tail))
}

/// Latest observations stay prominent. Exact duplicate tool replies should not
/// cost another model call's input allocation during checkpoint creation.
fn checkpoint_transcript(candidates: &[Value], budget: usize) -> String {
    let mut recent_tools = 0usize;
    let mut seen_tool_results = std::collections::HashSet::<&str>::new();
    let mut chunks = Vec::with_capacity(candidates.len());
    for row in candidates.iter().rev() {
        let role = row.get("role").and_then(Value::as_str).unwrap_or("?");
        let content = row.get("content").and_then(Value::as_str).unwrap_or("");
        let output = if role == "tool" {
            recent_tools += 1;
            let duplicate = content.chars().count() > 512 && !seen_tool_results.insert(content);
            if duplicate {
                "[Exact duplicate of a later tool result; original preserved in SQLite]".to_string()
            } else {
                let failure = content.starts_with("Error:")
                    || serde_json::from_str::<Value>(content).ok().is_some_and(|value|
                        value.get("error").is_some() || value.get("ok") == Some(&json!(false)));
                excerpt(content, if recent_tools <= 3 || failure { 3_200 } else { 800 })
            }
        } else {
            excerpt(content, 3_200)
        };
        if output.is_empty() && role == "tool" { continue; }
        chunks.push(format!("[{role}] {output}"));
    }
    chunks.reverse();
    let joined = chunks.join("\n");
    joined.chars().rev().take(budget).collect::<String>().chars().rev().collect()
}


/// Summarizes older messages into a checkpoint. The original messages stay in
/// the database (the UI still lists them); the engine uses the summary plus the
/// recent window on later turns.
pub async fn run(
    state: &AppState,
    session_id: &str,
    provider_id: &str,
    model: &str,
) -> Result<Value, String> {
    if state.store.get("session", session_id).is_none() {
        return Err("세션을 찾을 수 없습니다.".into());
    }
    if provider_id.is_empty() || model.is_empty() {
        return Err("컨텍스트 압축에는 연결된 모델이 필요합니다.".into());
    }
    let route = crate::engine::gateway::resolve(state, provider_id, Some(model))?;

    let history = state.store.messages(session_id);
    let prior = state.store.get("session_compaction", session_id);
    let prior_id = prior
        .as_ref()
        .and_then(|value| value.get("throughMessageId"))
        .and_then(|value| value.as_str())
        .map(str::to_string);
    let prior_index = prior_id
        .as_ref()
        .and_then(|id| {
            history
                .iter()
                .position(|row| row.get("id").and_then(|value| value.as_str()) == Some(id.as_str()))
        })
        .map(|index| index as isize)
        .unwrap_or(-1);

    let mut cut = history.len().saturating_sub(KEEP_RECENT).max(1);
    while cut > (prior_index + 1) as usize
        && history
            .get(cut.saturating_sub(1))
            .and_then(|row| row.get("role"))
            .and_then(|value| value.as_str())
            == Some("tool")
    {
        cut -= 1;
    }
    if cut <= (prior_index + 1) as usize {
        return Err("압축할 만큼 새 메시지가 충분하지 않습니다.".into());
    }

    let candidates = &history[(prior_index + 1) as usize..cut];
    let old_summary = prior
        .as_ref()
        .and_then(|value| value.get("summary"))
        .and_then(|value| value.as_str())
        .unwrap_or("");
    // A checkpoint should fit even a small local model. Leave output tokens
    // free and bound the new evidence separately from the carried checkpoint.
    // 3 chars/token is a conservative heuristic, not provider-billed usage.
    let context_tokens = route.context_tokens.unwrap_or(32_768).max(2_048);
    let output_reserve = route.max_output_tokens.unwrap_or(context_tokens / 8)
        .min(context_tokens / 4).max(128);
    let input_chars = (context_tokens.saturating_sub(output_reserve) as usize)
        .saturating_mul(3).min(70_000);
    let previous_summary = excerpt(old_summary, (input_chars / 4).min(12_000));
    let transcript_budget = input_chars.saturating_sub(previous_summary.chars().count())
        .saturating_sub(1_024).clamp(1_000, MAX_TRANSCRIPT_CHARS);
    let transcript = checkpoint_transcript(candidates, transcript_budget);
    let request = json!({
        "model": model,
        "stream": false,
        "messages": [
            {
                "role": "system",
                "content": "Summarize prior conversation for continuation. Preserve user intent, key decisions, files, constraints, open tasks, and important tool outcomes. Do not invent results. Be concise and write in the same language as the conversation."
            },
            {
                "role": "user",
                "content": format!(
                    "{}{}Conversation to compact:\n{transcript}",
                    if previous_summary.is_empty() { String::new() } else { format!("Previous summary:\n{previous_summary}\n\n") },
                    ""
                )
            }
        ]
    });

    let client=if route.kind=="codex"{
        state.account_http_client(180)?
    }else{
        state.provider_http_client(180)?
    };
    let response = crate::engine::gateway::send(
        &route,
        &client,
        &request,
        std::time::Duration::from_secs(180),
    )
    .await?;
    let response = response.response;
    if !response.status().is_success() {
        return Err(format!("모델 호출 실패: HTTP {}", response.status()));
    }
    let value: Value = response.json().await.map_err(|error| error.to_string())?;
    crate::engine::gateway::record_usage(
        state,
        &route,
        crate::engine::gateway::usage_from(&value),
        "compact",
        Some(session_id),
    );
    let summary = value
        .get("choices")
        .and_then(|choices| choices.get(0))
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"))
        .and_then(|content| content.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if summary.is_empty() {
        return Err("모델이 빈 요약을 반환했습니다.".into());
    }
    let summary: String = summary.chars().take(18_000).collect();

    let through = candidates
        .last()
        .and_then(|row| row.get("id"))
        .cloned()
        .unwrap_or(Value::Null);
    state.store.put(
        "session_compaction",
        &json!({
            "id": session_id,
            "throughMessageId": through,
            "summary": summary,
            "updatedAt": chrono::Utc::now().to_rfc3339(),
        }),
    );
    state.store.audit(
        "session.compacted",
        json!({ "sessionId": session_id, "summarized": candidates.len() }),
    );

    Ok(json!({
        "sessionId": session_id,
        "summarized": candidates.len(),
        "remaining": history.len() - cut,
        "summary": summary,
    }))
}


#[cfg(test)]
mod token_budget_tests {
    use super::*;
    #[test]
    fn checkpoint_bounds_repeated_tool_outputs_without_mutating_history() {
        let original = (0..24).map(|i| json!({
            "role": "tool", "content": if i == 23 {
                format!("LATEST_EVIDENCE {}", "N".repeat(8_000))
            } else {
                format!("FILE_RESULT {}", "A".repeat(9_000))
            },
        })).collect::<Vec<_>>();
        let before = serde_json::to_string(&original).unwrap();
        let prompt = checkpoint_transcript(&original, 8_000);
        assert!(prompt.chars().count() <= 8_000);
        assert!(prompt.contains("LATEST_EVIDENCE"));
        assert!(prompt.contains("Exact duplicate"));
        assert!(prompt.len() * 10 < before.len(), "repeated outputs must be materially smaller");
        assert_eq!(serde_json::to_string(&original).unwrap(), before);
    }
    #[test]
    fn checkpoint_preserves_recent_error_tail_and_owner_intent() {
        let messages = vec![
            json!({"role":"user","content":"중요한 사용자 제약 유지"}),
            json!({"role":"tool","content":format!("Error: build failed {}", "x".repeat(5_000)) + " CRITICAL_TAIL"}),
        ];
        let prompt = checkpoint_transcript(&messages, 10_000);
        assert!(prompt.contains("중요한 사용자 제약 유지"));
        assert!(prompt.contains("Error: build failed"));
        assert!(prompt.contains("CRITICAL_TAIL"));
    }
    #[test]
    fn excerpt_does_not_cut_unicode_characters() {
        let source = "가".repeat(10_000) + "끝";
        let output = excerpt(&source, 800);
        assert!(output.contains("끝"));
        assert!(output.chars().count() <= 800);
    }
}
