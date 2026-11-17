//! Model-free classification of observed errors in the existing Rust Cycle.
//! No tool calls, authorizations, new loops, or model invocations occur here.

#[derive(Debug,Clone,Copy,PartialEq,Eq)]
pub enum FailureClass{
    Permission, Compile, Test, MissingEvidence, Provider, Verify, Other,
}
impl FailureClass{
    pub fn label(self)->&'static str{
        match self{
            Self::Permission=>"permission",Self::Compile=>"compile",
            Self::Test=>"test",Self::MissingEvidence=>"missing_evidence",
            Self::Provider=>"provider",Self::Verify=>"verify",Self::Other=>"other",
        }
    }
    pub fn next_step(self)->&'static str{
        match self{
            Self::Permission=>"권한·승인 오류입니다. 승인 상태와 허용된 범위를 먼저 확인하고 승인을 우회하거나 같은 쓰기 호출을 반복하지 마세요.",
            Self::Compile=>"컴파일 오류입니다. 오류에 나타난 파일·함수·의존성만 조사해 최소 수정한 뒤 같은 검증 명령을 다시 실행하세요.",
            Self::Test=>"테스트 실패입니다. 실패한 테스트 입력·기대값·실제 결과를 비교하고 원인 코드만 수정한 뒤 해당 테스트부터 재검증하세요.",
            Self::MissingEvidence=>"자료 또는 파일이 부족합니다. 누락된 경로·근거를 먼저 찾아 필요한 범위만 읽고, 근거 없는 수정을 피하세요.",
            Self::Provider=>"모델·도구 제공자 오류입니다. 이미 실행된 도구의 상태부터 조회하고 남은 예산·재시도 조건을 확인하세요.",
            Self::Verify=>"실행 결과에 대한 검증 증거가 부족합니다. 최종 수정본에서 요구된 검증 명령이나 산출물 검사를 수행하세요.",
            Self::Other=>"관측된 실패 원인을 먼저 분리하세요. 기존에 실패한 도구와 인자를 검토 없이 반복하지 말고 최소 수정·검증만 하세요.",
        }
    }
}
pub fn classify(gaps:&[String],error:Option<&str>)->FailureClass{
    let mut text=error.unwrap_or("").to_lowercase();
    for gap in gaps.iter().take(8){
        text.push('\n');text.push_str(&gap.to_lowercase());
    }
    let has=|needles:&[&str]|needles.iter().any(|n|text.contains(n));
    if has(&["permission denied","access denied","authorization","unauthorized",
        "forbidden","http 401","http 403","권한","승인 거부","승인이 필요","eacces"]){
        return FailureClass::Permission;
    }
    if has(&["error[e","could not compile","compiler error","syntax error",
        "compile failed","컴파일 오류","타입 오류"]){
        return FailureClass::Compile;
    }
    if has(&["test failed","tests failed","test result: failed",
        "assertion failed","panicked at","테스트 실패"]){
        return FailureClass::Test;
    }
    if has(&["no such file","not found","file missing","missing file",
        "자료 부족","근거 부족","파일 없음","찾을 수 없"]){
        return FailureClass::MissingEvidence;
    }
    if has(&["rate limit","429","503","502","connection reset",
        "timed out","timeout","제공자 오류"]){
        return FailureClass::Provider;
    }
    if has(&["검증 명령","최종 검증","post-edit","post edit","verification",
        "acceptance","검증 증거"]){
        return FailureClass::Verify;
    }
    FailureClass::Other
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn classification_routes_distinct_observed_failures(){
        assert_eq!(classify(&[],Some("error[E0425]: cannot find value")),
            FailureClass::Compile);
        assert_eq!(classify(&["test failed: got 3".to_string()],None),
            FailureClass::Test);
        assert_eq!(classify(&[],Some("No such file: src/x.rs")),
            FailureClass::MissingEvidence);
        assert_eq!(classify(&[],Some("HTTP 429 rate limit")),FailureClass::Provider);
        assert_eq!(classify(&[],Some("permission denied")),FailureClass::Permission);
        assert_eq!(classify(&["최종 검증 명령 실패".into()],None),FailureClass::Verify);
    }
    #[test]
    fn repair_guidance_never_grants_permission(){
        assert!(FailureClass::Permission.next_step().contains("우회"));
        for class in [FailureClass::Compile,FailureClass::Test,
            FailureClass::MissingEvidence,FailureClass::Provider,
            FailureClass::Verify,FailureClass::Other]{
            assert!(!class.next_step().is_empty());
        }
    }
}
