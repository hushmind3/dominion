//! Pure, replaceable planning and action-selection strategies.
//! No provider, runtime, state, MCP, network, database or UI dependencies.
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolOrder { ModelOrder, ReadFirst }
impl ToolOrder {
    pub fn from_implementation(s:Option<&str>)->Self{
        if s==Some("read-first") {Self::ReadFirst} else {Self::ModelOrder}
    }
    pub fn wants_read_first(self)->bool {self==Self::ReadFirst}
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCombination { Sequential, ParallelReads }
impl ToolCombination{
    pub fn from_implementation(s:Option<&str>)->Self{
        if s==Some("parallel-reads") {Self::ParallelReads} else {Self::Sequential}
    }
    pub fn parallel_reads(self)->bool {self==Self::ParallelReads}
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteChoice { RecommendRead, RequireRead, NoHint }
impl WriteChoice{
    pub fn from_implementation(s:Option<&str>)->Self{
        match s {
            Some("block-write-without-read")=>Self::RequireRead,
            Some("none")=>Self::NoHint,
            _=>Self::RecommendRead,
        }
    }
    pub fn blocks_without_read(self)->bool {self==Self::RequireRead}
    pub fn recommends_read(self)->bool {self==Self::RecommendRead}
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryChoice { Fingerprint, BlockIdentical, AllowAll }
impl RetryChoice{
    pub fn from_implementation(s:Option<&str>)->Self{
        match s{
            Some("allow-all")=>Self::AllowAll,
            Some("block-identical")=>Self::BlockIdentical,
            _=>Self::Fingerprint,
        }
    }
    pub fn repeat_allowed(self,previous:Option<&str>,current:&str)->bool{
        match self{
            Self::AllowAll=>true,
            Self::BlockIdentical=>false,
            Self::Fingerprint=>previous.is_some_and(|old|old!=current),
        }
    }
    pub fn allows_unsafe_repeat(self)->bool{self==Self::AllowAll}
}
/// Owner-facing planning settings take precedence over the selected algorithm.
pub fn admit_plan(explicit:Option<bool>,heuristic:bool,
    previous_panel_answer:bool,native_contract_plan:bool,mode:Option<&str>)->bool{
    match mode{
        Some("always")=>true,
        Some("never")=>false,
        _=>explicit.unwrap_or(heuristic)&&!previous_panel_answer&&!native_contract_plan,
    }
}

/// Pure extra-planner admission: a bounded single-file task usually needs no
/// additional model call, whereas a multi-step request does.
pub fn needs_plan(input:&str,acceptance:&Option<(String,Value)>)->bool{
    let text=input.trim();
    if text.chars().count()>240 || text.contains('\n'){return true;}
    if ["그리고","그 다음","다음으로","한 뒤","후에","then","after",";"]
        .iter().any(|marker|text.contains(marker)){return true;}
    match acceptance{
        Some((_,checks))=>checks.as_array().map(|items|items.is_empty()).unwrap_or(true),
        None=>true,
    }
}
/// A multi-file acceptance contract is a complete execution target, unless
/// the owner specifically requested a separate plan before the actions.
pub fn local_multifile_skips_extra_planner(
    input:&str,acceptance:&Option<(String,Value)>
)->bool{
    let lower=input.to_lowercase();
    if ["계획을","계획부터","실행 계획","plan first","make a plan",
        "design a plan","계획 수립"].iter().any(|word|lower.contains(word)){
        return false;
    }
    let Some((_,checks))=acceptance else{return false};
    let Some(items)=checks.as_array() else{return false};
    (2..=20).contains(&items.len())&&items.iter().all(|check|{
        matches!(check["type"].as_str(),
            Some("file_exists"|"file_changed"|"file_contains"|"file_equals"))
            && check["path"].as_str().is_some_and(|path|!path.trim().is_empty())
    })
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn unknown_versions_keep_conservative_existing_defaults(){
        assert_eq!(ToolOrder::from_implementation(None),ToolOrder::ModelOrder);
        assert_eq!(ToolCombination::from_implementation(Some("unknown")),ToolCombination::Sequential);
        assert_eq!(WriteChoice::from_implementation(None),WriteChoice::RecommendRead);
        assert_eq!(RetryChoice::from_implementation(Some("unknown")),RetryChoice::Fingerprint);
    }
    #[test]
    fn typed_behavior_matches_active_algorithm_names(){
        assert!(ToolOrder::from_implementation(Some("read-first")).wants_read_first());
        assert!(ToolCombination::from_implementation(Some("parallel-reads")).parallel_reads());
        assert!(WriteChoice::from_implementation(Some("block-write-without-read")).blocks_without_read());
        assert!(!WriteChoice::NoHint.recommends_read());
    }
    #[test]
    fn identical_failed_mutations_remain_blocked_unless_original_policy_allows(){
        let retry=RetryChoice::Fingerprint;
        assert!(!retry.repeat_allowed(None,"next"));
        assert!(!retry.repeat_allowed(Some("same"),"same"));
        assert!(retry.repeat_allowed(Some("before"),"after"));
        assert!(!RetryChoice::BlockIdentical.repeat_allowed(Some("before"),"after"));
        assert!(RetryChoice::AllowAll.allows_unsafe_repeat());
    }
    #[test]
    fn explicit_planning_mode_always_wins(){
        assert!(admit_plan(Some(false),false,true,true,Some("always")));
        assert!(!admit_plan(Some(true),true,false,false,Some("never")));
        assert!(!admit_plan(None,true,true,false,None));
        assert!(!admit_plan(None,true,false,true,None));
        assert!(admit_plan(None,true,false,false,None));
        assert!(!admit_plan(Some(false),true,false,false,None));
    }
    #[test]
    fn plan_heuristics_respect_owner_requested_plans_and_existing_contracts(){
        let single=Some(("owner".into(),serde_json::json!([
            {"type":"file_exists","path":"src/main.rs"}
        ])));
        assert!(!needs_plan("파일 생성",&single));
        assert!(needs_plan("파일 작성 그리고 테스트",&single));
        let many=Some(("owner".into(),serde_json::json!([
            {"type":"file_changed","path":"src/a.rs"},
            {"type":"file_changed","path":"src/b.rs"}
        ])));
        assert!(local_multifile_skips_extra_planner("두 파일 수정",&many));
        assert!(!local_multifile_skips_extra_planner("계획부터 세워 주세요",&many));
        assert!(needs_plan("제안해 주세요",&None));
    }

}
