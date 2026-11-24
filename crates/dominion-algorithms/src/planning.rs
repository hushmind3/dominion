//! Provider-neutral planning decisions.
use serde_json::Value;
/// Legacy default planning heuristic, now owned by the replaceable
/// strategy layer rather than the native execution/finalization code.
pub fn needs_plan(input:&str,acceptance:&Option<(String,Value)>)->bool{
    let text=input.trim();
    if text.chars().count()>240 || text.contains('\n'){return true;}
    if ["그리고","그 다음","다음으로","한 뒤","후에","then","after",";"]
        .iter().any(|marker|text.contains(marker)){return true;}
    match acceptance {
        Some((_,checks))=>checks.as_array()
            .map(|items|items.is_empty()).unwrap_or(true),
        None=>true,
    }
}

#[cfg(test)]
mod tests{
    use super::*;
    use serde_json::json;
    #[test]fn legacy_planning_contract(){
        let checks=Some(("file".to_string(),json!([{"type":"file_exists","path":"x"}])));
        assert!(!needs_plan("report.txt 파일을 만들어줘",&checks));
        assert!(needs_plan("파일을 만들고 그리고 내용도 확인해",&checks));
        assert!(needs_plan("report.txt 파일을 만들어줘",&None));
        assert!(needs_plan(&"가".repeat(300),&checks));
    }
}
