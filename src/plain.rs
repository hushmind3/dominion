//! Owner-facing sentences for upstream failures.
//!
//! Channel probes, provider calls and job runners all produce machine text
//! ("Discord HTTP 401: invalid token", "Failed to fetch"). That text is stored
//! on the record and later read by the UI, the API and the audit log, so it is
//! normalized here, once, before it is saved.

pub fn error(message: &str) -> String {
    let raw = message.trim();
    if raw.is_empty() {
        return "문제가 생겼습니다. 다시 시도해 주세요.".to_string();
    }
    let lower = raw.to_lowercase();
    let has = |needle: &str| lower.contains(needle);
    if has("failed to fetch")
        || has("networkerror")
        || has("network request")
        || has("dns")
        || has("load failed")
    {
        return "인터넷 연결을 확인한 뒤 다시 시도해 주세요.".to_string();
    }
    if has("timeout") || has("timed out") || has("timedout") {
        return "응답이 너무 늦습니다. 잠시 후 다시 시도해 주세요.".to_string();
    }
    if has("connection refused") || has("econnrefused") || has("connection reset") {
        return "서버에 연결할 수 없습니다. 잠시 후 다시 시도해 주세요.".to_string();
    }
    if has("401") || has("unauthorized") || has("invalid token") || has("invalid auth") {
        return "인증 정보가 올바르지 않습니다. 키나 토큰을 다시 확인해 주세요.".to_string();
    }
    if has("403") || has("forbidden") {
        return "이 작업을 할 권한이 없습니다.".to_string();
    }
    if has("404") || has("not found") {
        return "찾을 수 없는 항목입니다. 새로고침한 뒤 다시 시도해 주세요.".to_string();
    }
    if has("429") || has("rate limit") {
        return "요청이 너무 많습니다. 잠시 후 다시 시도해 주세요.".to_string();
    }
    if has_http_5xx(&lower) || has("internal server error") || has("bad gateway") {
        return "서버가 잠시 응답하지 않습니다. 잠시 후 다시 시도해 주세요.".to_string();
    }
    if has("quota") || has("insufficient") || has("billing") {
        return "사용 한도를 넘었습니다. 요금제나 한도를 확인해 주세요.".to_string();
    }
    // A sentence the engine already wrote for the owner passes through.
    if raw.chars().any(|ch| ('가'..='힣').contains(&ch)) {
        return raw.to_string();
    }
    "문제가 생겼습니다. 다시 시도해 주세요.".to_string()
}

fn has_http_5xx(lower: &str) -> bool {
    let bytes = lower.as_bytes();
    for index in 0..bytes.len().saturating_sub(2) {
        if bytes[index] == b'5' && bytes[index + 1].is_ascii_digit() && bytes[index + 2].is_ascii_digit() {
            let before_ok = index == 0 || !bytes[index - 1].is_ascii_digit();
            return before_ok;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::error;

    #[test]
    fn upstream_codes_become_owner_sentences() {
        assert_eq!(
            error("Discord HTTP 401: invalid token"),
            "인증 정보가 올바르지 않습니다. 키나 토큰을 다시 확인해 주세요."
        );
        assert_eq!(
            error("Telegram getUpdates HTTP 502"),
            "서버가 잠시 응답하지 않습니다. 잠시 후 다시 시도해 주세요."
        );
        assert_eq!(
            error("TypeError: Failed to fetch"),
            "인터넷 연결을 확인한 뒤 다시 시도해 주세요."
        );
        assert_eq!(
            error("request timed out after 30s"),
            "응답이 너무 늦습니다. 잠시 후 다시 시도해 주세요."
        );
        assert_eq!(
            error("Telegram HTTP 401 / API 401: 권한·설정 또는 요청 제한을 확인하세요."),
            "인증 정보가 올바르지 않습니다. 키나 토큰을 다시 확인해 주세요."
        );
    }

    #[test]
    fn korean_messages_and_empty_values_stay_readable() {
        assert_eq!(error("회의실을 찾을 수 없습니다."), "회의실을 찾을 수 없습니다.");
        assert_eq!(error(""), "문제가 생겼습니다. 다시 시도해 주세요.");
        assert_eq!(error("segfault"), "문제가 생겼습니다. 다시 시도해 주세요.");
    }
}
