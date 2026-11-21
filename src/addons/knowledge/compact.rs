use crate::state::AppState;
use serde_json::{json, Value};
use std::time::Duration;

const KEEP_RECENT: usize = 8;

/// Automatic compaction only fires once the unsummarized tail is large enough
/// that a summary replaces real input. Below this, the normal window already
/// covers the conversation and the model call would be pure overhead.
const AUTO_MIN_MESSAGES: usize = 24;
const AUTO_MIN_BYTES: usize = 48_000;
/// A failed attempt must not retry on every following turn.
const AUTO_RETRY_COOLDOWN: Duration = Duration::from_secs(600);

/// Index range of rows the next summary would absorb: everything after the
/// previous checkpoint, minus the verbatim recent window. Manual and automatic
/// compaction share this boundary so they never summarize different ranges.
fn compaction_window(state: &AppState, history: &[Value], session_id: &str) -> (usize, usize) {
    let prior_id = state
        .store
        .get("session_compaction", session_id)
        .and_then(|value| value.get("throughMessageId")?.as_str().map(str::to_string));
    let prior_index = prior_id
        .as_ref()
        .and_then(|id| {
            history
                .iter()
                .position(|row| row.get("id").and_then(|value| value.as_str()) == Some(id.as_str()))
        })
        .map(|index| index as isize)
        .unwrap_or(-1);
    let start = (prior_index + 1).max(0) as usize;

    let mut cut = history.len().saturating_sub(KEEP_RECENT).max(1);
    while cut > start
        && history
            .get(cut.saturating_sub(1))
            .and_then(|row| row.get("role"))
            .and_then(|value| value.as_str())
            == Some("tool")
    {
        cut -= 1;
    }
    (start, cut.max(start))
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
    let (start, cut) = compaction_window(state, &history, session_id);
    if cut <= start {
        return Err("압축할 만큼 새 메시지가 충분하지 않습니다.".into());
    }

    let candidates = &history[start..cut];
    let transcript = candidates
        .iter()
        .map(|row| {
            let role = row
                .get("role")
                .and_then(|value| value.as_str())
                .unwrap_or("?");
            let content = row
                .get("content")
                .and_then(|value| value.as_str())
                .unwrap_or("")
                .chars()
                .take(3200)
                .collect::<String>();
            format!("[{role}] {content}")
        })
        .collect::<Vec<_>>()
        .join("\n");
    let transcript: String = transcript
        .chars()
        .rev()
        .take(110_000)
        .collect::<String>()
        .chars()
        .rev()
        .collect();

    let previous_summary = prior
        .as_ref()
        .and_then(|value| value.get("summary"))
        .and_then(|value| value.as_str())
        .unwrap_or("");
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

    let client = reqwest::Client::new();
    // This independent summary prompt should never displace the main
    // participant's cached persona, but OpenCode Go still requires a
    // deterministic session header. Keep one compaction affinity per room.
    let affinity=state.store.get("provider",&route.id)
        .filter(|provider|provider["integration"]=="opencode-go")
        .map(|_|format!("dominion-compact-{}",crate::engine::memory::stable_id(&[
            "opencode-compaction-v1",session_id,&route.id,&route.model,
        ])));
    let response = crate::engine::gateway::send_with_affinity(
        &route,
        &client,
        &request,
        std::time::Duration::from_secs(180),
        affinity.as_deref(),
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

/// Admission rule for the automatic path. The model call is only worth it when
/// a large block of unsummarized messages would otherwise be dropped from the
/// prompt: enough rows and enough bytes.
fn auto_eligible(state: &AppState, session_id: &str, history: &[Value]) -> bool {
    let (start, cut) = compaction_window(state, history, session_id);
    if cut <= start || cut - start < AUTO_MIN_MESSAGES {
        return false;
    }
    let bytes: usize = history[start..cut]
        .iter()
        .map(|row| row.get("content").and_then(Value::as_str).map_or(0, str::len))
        .sum();
    bytes >= AUTO_MIN_BYTES
}

/// Schedules one background summary after a completed turn, so the next turn
/// keeps the older decisions without replaying every message. Room sessions
/// are skipped: room turns select relevant history themselves and never read
/// `session_compaction`.
pub fn spawn_auto(state: &AppState, session_id: &str) {
    let Some(session) = state.store.get("session", session_id) else {
        return;
    };
    if session["roomId"].is_string() {
        return;
    }
    let provider_id = session["providerId"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let model = session["model"].as_str().unwrap_or_default().to_string();
    if provider_id.is_empty() || model.is_empty() {
        return;
    }
    if state.store.message_count(session_id) < AUTO_MIN_MESSAGES + KEEP_RECENT {
        return;
    }
    let history = state.store.messages(session_id);
    if !auto_eligible(state, session_id, &history) {
        return;
    }
    if !state.begin_auto_compact(session_id, AUTO_RETRY_COOLDOWN) {
        return;
    }
    let state = state.clone();
    let id = session_id.to_string();
    tokio::spawn(async move {
        let outcome = run(&state, &id, &provider_id, &model).await;
        state.finish_auto_compact(&id, outcome.is_ok());
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(prefix: &str, count: usize, bytes_each: usize) -> Vec<Value> {
        (0..count)
            .map(|index| {
                json!({
                    "id": format!("{prefix}{index}"),
                    "role": "user",
                    "content": "x".repeat(bytes_each),
                })
            })
            .collect()
    }

    fn state() -> AppState {
        let dir = std::env::temp_dir().join(format!(
            "dominion-compact-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let config = crate::config::Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir,
        };
        let state = AppState::new(
            config.clone(),
            crate::db::Store::open(&config.db_path()).unwrap(),
            crate::vault::Vault::open(&config.key_path()),
        );
        state
            .store
            .put("session", &json!({"id": "s"}))
            .unwrap();
        state
    }

    #[test]
    fn automatic_compaction_needs_both_enough_rows_and_bytes() {
        let state = state();
        // Plenty of rows, but each is tiny: the window already covers them.
        assert!(!auto_eligible(&state, "s", &rows("a", 40, 10)));
        // Plenty of bytes, but too few rows to summarize.
        assert!(!auto_eligible(&state, "s", &rows("b", 10, 10_000)));
        // The recent window is never summarized, so it does not count.
        let mut history = rows("c", AUTO_MIN_MESSAGES, 2_500);
        history.extend(rows("d", KEEP_RECENT, 2_500));
        assert_eq!(history.len(), AUTO_MIN_MESSAGES + KEEP_RECENT);
        assert!(auto_eligible(&state, "s", &history));
    }

    #[test]
    fn automatic_compaction_skips_what_the_checkpoint_already_absorbed() {
        let state = state();
        let mut history = rows("m", AUTO_MIN_MESSAGES + KEEP_RECENT, 2_500);
        state
            .store
            .put(
                "session_compaction",
                &json!({"id": "s", "throughMessageId": "m19", "summary": "이전 요약"}),
            )
            .unwrap();
        // Only the rows after m19 remain, and that tail is already small.
        assert!(!auto_eligible(&state, "s", &history));
        history.extend(rows("n", AUTO_MIN_MESSAGES, 2_500));
        assert!(auto_eligible(&state, "s", &history));
    }

    #[test]
    fn compaction_slot_blocks_a_second_call_until_success() {
        let state = state();
        let cooldown = std::time::Duration::from_secs(600);
        assert!(state.begin_auto_compact("s", cooldown));
        // A running or failed attempt stays blocked inside the cooldown.
        state.finish_auto_compact("s", false);
        assert!(!state.begin_auto_compact("s", cooldown));
        state.finish_auto_compact("s", true);
        assert!(state.begin_auto_compact("s", cooldown));
    }
}
