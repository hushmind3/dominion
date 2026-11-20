//! Swappable agent intelligence/policies. No DB, network, authority or runtime loop.
//! The host can select policies; the Rust runtime remains authoritative for
//! tool permissions, effect admission, budgets, receipts and verification.
pub use dominion_agent_runtime::{Action,Receipt,Strategy,ToolSpec};
pub mod ranking;
pub mod context;
pub mod failure_guidance;
pub mod tool_discovery;
pub mod agent_loop;
pub use agent_loop::{Kernel,ReActLoop};
pub mod replay;
pub use dominion_agent_runtime::{BoxFuture,Limits,CriterionKind,Criterion,RunRequest,
    Decision,ModelReply,ModelTurn,Model,Tools,Evidence,Journal,DispatchResult,
    Dispatcher,Status,CheckResult,Snapshot};
pub use ranking::{tool_relevance,tool_relevance_with_schema};

#[derive(Clone,Copy,Debug,Default,PartialEq,Eq)]
pub struct Intent {
    pub requires_file_mutation: bool,
    pub requires_effect: bool,
}
impl Intent {
    /// Policies can demand more evidence, never waive baseline safety checks.
    pub fn union(self, other:Self)->Self{
        Self {requires_file_mutation:self.requires_file_mutation||other.requires_file_mutation,
            requires_effect:self.requires_effect||other.requires_effect}
    }
}

/// Model-facing decision policy, independent of the executable scheduler.
/// All output remains subject to hard kernel admission and owner approvals.
pub trait AgentAlgorithm: Send + Sync {
    fn id(&self)->&'static str;
    fn planner_instructions(&self,native_wire:bool,needs_discovery:bool,
        strategy:&str,owner_instructions:&str)->String;
    fn classify(&self,instruction:&str)->Intent;
    /// Choose model-visible context without mutating the durable receipt journal.
    fn project_context(&self,receipts:&[Receipt],strategy:Strategy,max_chars:usize)
        ->context::Projection{context::project(receipts,strategy,max_chars)}
    /// Rank only host-authorized tool descriptions; the runtime revalidates every
    /// returned schema, read-only classification and name before a model turn.
    fn select_tools(&self,catalog:&std::collections::HashMap<String,ToolSpec>,
        task:&str,owner:&str,directive:Option<&str>,plan:&[String],receipts:&[Receipt])
        ->Vec<ToolSpec>{tool_discovery::select(catalog,task,owner,directive,plan,receipts)}
    /// Supply diagnostic hints to the model; never authorize an effect.
    fn failure_guidance(&self,receipts:&[Receipt])
        ->Option<failure_guidance::FailureGuidance>{failure_guidance::latest(receipts)}
    /// Optional stricter plan constraint. The kernel always enforces its own limits.
    fn validate_plan(&self,_plan:&[String])->Result<(),String>{Ok(())}
}

#[derive(Clone,Copy,Debug,Default)]
pub struct StandardAlgorithm;
impl AgentAlgorithm for StandardAlgorithm {
    fn id(&self)->&'static str{"standard-v1"}
    fn planner_instructions(&self,native_wire:bool,needs_discovery:bool,
        strategy:&str,owner_instructions:&str)->String {
        let response_contract=if native_wire {
            "For each action use a provider-native function/tool call with valid JSON object arguments. \
             Never emit a JSON decision wrapper or impersonate a tool result in text. \
             When no more actions are needed, respond with the actual final answer as normal text."
        }else{
            "Reply with exactly one JSON object: \
             {\"plan\":[\"concise ordered steps\"],\"actions\":[{\"id\":\"unique-id\", \
             \"tool\":\"name\",\"arguments\":{},\"dependsOn\":[]}],\"answer\":null}. \
             On task completion return actions=[] and answer=\"actual result\"."
        };
        let discovery_hint=if needs_discovery {
            "If a capability is not advertised, call dominion_tool_search to read \
             its authorized JSON schema, then dominion_tool_call with the exact \
             discovered name and arguments. The provider function declaration \
             list stays fixed; discovery never increases host permissions."
        }else{
            "Every currently permitted host tool is directly available."
        };
        let system=format!(
            "You are DOMINION's model planner. The Rust kernel owns action dispatch, \
             limits, permission checks and verification. Respect the owner's instructions \
             and never claim an unobserved task is done. {} \
             Owner-required checks are executed by the Rust kernel on proposed completion. \
             You may run diagnostic tests while working, but do not waste a model turn \
             rerunning an unchanged acceptance command. After edits, propose completion; \
             Rust will execute outstanding exact tests. If a test fails, inspect the recorded \
             diagnostic, modify the source and propose completion again. A requested source \
             edit requires a workspace tool receipt with written=true. \
             Previous tool outputs remain in the durable journal. Use dominion_receipt_search \
             and dominion_receipt_read for prior details; never replay external effects merely \
             to retrieve an observation. {} Strategy: {}. Owner instructions: {}.",
            response_contract,discovery_hint,strategy,owner_instructions
        );
        system
    }
    fn classify(&self,instruction:&str)->Intent{
        Intent{requires_file_mutation:owner_requires_file_mutation(instruction),
               requires_effect:owner_requires_effect(instruction)}
    }
}

/// Compact reasoning policy, selectable independently for a single job.
/// It does not change the runtime's budgets or the host's tool approvals.
#[derive(Clone,Copy,Debug,Default)]
pub struct FocusedAlgorithm;
impl AgentAlgorithm for FocusedAlgorithm {
    fn id(&self)->&'static str {"focused-v1"}
    fn planner_instructions(&self,native_wire:bool,needs_discovery:bool,
        strategy:&str,owner_instructions:&str)->String {
        let mut text=StandardAlgorithm.planner_instructions(
            native_wire,needs_discovery,strategy,owner_instructions);
        text.push_str("\nFocused mode: keep the plan concise, inspect relevant state, perform the minimum authorized actions, and finish only after observed completion.");
        text
    }
    fn classify(&self,instruction:&str)->Intent {
        StandardAlgorithm.classify(instruction)
    }
    fn project_context(&self,receipts:&[Receipt],strategy:Strategy,max_chars:usize)
        ->context::Projection {
        if matches!(strategy,Strategy::VerifyFirst) {
            context::project(receipts,strategy,max_chars)
        } else {
            context::project(receipts,Strategy::LowCost,max_chars.min(2400))
        }
    }
    fn validate_plan(&self,plan:&[String])->Result<(),String> {
        if plan.len()>10 {
            Err("Focused policy allows at most 10 plan steps".into())
        } else {Ok(())}
    }
}

fn owner_requires_file_mutation(text: &str) -> bool {
    let text=text.to_lowercase();
    let edit=["수정","편집","작성","만들","생성","삭제","제거","고쳐","구현","개발",
        "리팩토링","패치","write","edit","delete","remove","modify",
        "fix","implement","refactor","patch","create"];
    let artifact=["파일","코드","소스","저장소","프로젝트","버그","구현","개발",
        "리팩토링","readme","source","repository","code","file","script",
        "unit test",".rs",".py",".ts",".js",".md",".txt","artifact"];
    edit.iter().any(|word|text.contains(word))
        && artifact.iter().any(|word|text.contains(word))
}

fn owner_requires_effect(text: &str) -> bool {
    let t = text.to_lowercase();
    ["만들","생성","작성","수정","편집","삭제","제거","고쳐","구현",
     "개발","리팩토링","배포","설치","실행","저장","커밋","복사","이동",
     "create","write","edit","delete","remove","modify","fix","implement",
     "build","deploy","install","execute","save","commit","refactor"]
        .iter().any(|word| t.contains(word))
}


#[cfg(test)] mod tests {
    use super::*;
    #[test] fn policy_owner_intent_is_preserved(){
        let a=StandardAlgorithm;
        assert!(a.classify("README 수정해라").requires_file_mutation);
        assert!(a.classify("deploy the service").requires_effect);
        assert!(!a.classify("요약해줘").requires_effect);
        assert!(!a.classify("기존 파일 읽어줘").requires_file_mutation);
    }
    #[test] fn additional_algorithm_cannot_relax_baseline_evidence(){
        let base=StandardAlgorithm.classify("파일 수정");
        let combined=base.union(Intent::default());
        assert!(combined.requires_file_mutation && combined.requires_effect);
    }
    #[test] fn focused_profile_has_distinct_policy_and_preserves_effect_intent(){
        let focused=FocusedAlgorithm;
        assert_eq!(focused.id(),"focused-v1");
        assert!(focused.planner_instructions(false,false,"Balanced","owner").contains("Focused mode"));
        assert!(focused.classify("README 수정").requires_file_mutation);
        assert!(focused.validate_plan(&vec!["step".into();10]).is_ok());
        assert!(focused.validate_plan(&vec!["step".into();11]).is_err());
    }
    #[test] fn native_and_text_model_instructions_are_distinct(){
        let p=StandardAlgorithm;
        assert!(p.planner_instructions(true,false,"Balanced","owner").contains("provider-native"));
        assert!(p.planner_instructions(false,true,"LowCost","owner").contains("JSON object"));
    }
}
