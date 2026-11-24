//! Model-independent owner-correction classification. Authority and actual
//! execution evidence remain the responsibility of the product host.
use serde_json::{json,Value};

pub fn looks_like_correction(text: &str) -> bool {
    let lower = text.to_lowercase();
    let direct = [
        "그게 아니라",
        "그게아니라",
        "잘못",
        "틀렸",
        "틀린",
        "왜 이렇게",
        "왜이렇게",
        "다시 해",
        "다시해",
        "수정해",
        "고쳐",
        "고치",
        "실패했",
        "하지 마",
        "하지마",
        "그러지 마",
        "그러지마",
        "그만해",
        "not what",
        "that's wrong",
        "thats wrong",
        "wrong",
        "fix it",
        "do it again",
        "don't do",
        "dont do",
        "do not do",
        "stop doing",
    ]
    .iter()
    .any(|signal| lower.contains(signal));
    if direct {
        return true;
    }
    // "앞으로 이런 식으로 하지 마", "이제 그렇게 하지 말고 ..."처럼
    // 부정 명령 + 지속 의도가 결합된 교정도 잡는다.
    let negative = ["하지 말", "하지말", "안 돼", "안돼", "말고", "대신"];
    let intent = ["앞으로", "이제", "다음부터", "from now", "always", "never"];
    negative.iter().any(|signal| lower.contains(signal))
        && (intent.iter().any(|signal| lower.contains(signal))
            || lower.contains("이런 식")
            || lower.contains("그런 식")
            || lower.contains("그렇게"))
}

/// Separates an explicit owner directive from an ambiguous complaint. String
/// patterns are only a fast signal; the surrounding execution outcome decides
/// whether the correction is verified evidence or a hypothesis.
pub fn classify_correction(text: &str, transcript: &str, operations: &[Value]) -> Value {
    let lower = text.to_lowercase();
    let direct = looks_like_correction(text);
    let claims_success = ["완료", "했습니다", "생성", "성공", "created", "done", "written"]
        .iter()
        .any(|marker| transcript.to_lowercase().contains(marker));
    let reports_missing = ["없는데", "없어", "안 됐", "안됐", "실패", "not there", "missing", "didn't", "did not"]
        .iter()
        .any(|marker| lower.contains(marker));
    let persistent_preference = ["앞으로", "다음부터", "항상", "from now", "always", "never"]
        .iter()
        .any(|marker| lower.contains(marker));
    let failed_operations = operations
        .iter()
        .filter(|operation| operation["status"].as_str() == Some("failed"))
        .count();
    let (kind, confidence) = if persistent_preference && direct {
        ("explicit_directive", 0.85)
    } else if direct && claims_success && (reports_missing || failed_operations > 0) {
        ("outcome_mismatch", 0.8)
    } else if direct && failed_operations > 0 {
        ("outcome_mismatch", 0.7)
    } else if direct {
        ("explicit_directive", 0.6)
    } else {
        ("ambiguous", 0.3)
    };
    json!({
        "kind": kind,
        "confidence": confidence,
        "explicitSignal": direct,
        "claimsSuccess": claims_success,
        "reportsMissing": reports_missing,
        "failedOperations": failed_operations,
        "verified": kind != "ambiguous",
    })
}


#[cfg(test)]
mod tests{
 use super::*;
 #[test]
 fn separates_owner_directives_and_observed_outcome_mismatch(){
  assert_eq!(classify_correction("앞으로 그렇게 하지 마","",&[])["kind"],"explicit_directive");
  assert_eq!(classify_correction("잘못했어 파일이 없는데","완료",&[json!({"status":"failed"})])["kind"],"outcome_mismatch");
  assert_eq!(classify_correction("안녕하세요","",&[])["kind"],"ambiguous");
 }
}
