//! Model-independent decision core for ordinary owner-directed tasks.
//! The async runtime owns providers, tools, checkpoints and persistent state;
//! this module owns bounded repair/review, effect replay safety, goal steering
//! and usage deltas. No mutation or autonomous campaign can start here.
mod budget;
mod cycle;
pub mod verification;
pub mod failure;
pub mod scheduling;
pub use budget::BudgetLedger;
pub use cycle::{Cycle, Observation, Assessment};

use serde_json::{json,Value};

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Strategy { Focused, Conservative }

#[derive(Clone,Copy,Debug)]
pub struct CycleConfig {
    pub attempts: usize,
    pub strategy: Strategy,
}

impl CycleConfig {
    /// User parameters are bounded by the engine; invalid values never turn
    /// into unbounded model calls.
    pub fn from_input(input:&Value)->Self {
        let attempts=input["maxAutonomousAttempts"].as_u64().unwrap_or(3).clamp(1,6) as usize;
        let strategy=if input["executionStrategy"]=="conservative" {
            Strategy::Conservative
        }else{Strategy::Focused};
        Self{attempts,strategy}
    }
}

#[derive(Debug,PartialEq,Eq)]
pub enum Decision { Accept, Repair, Review }

/// A shell command may be replayed only when its subcommand is observational.
/// Global git options must not hide a mutating action from the replay barrier.
pub fn read_only_shell_command(command: &str) -> bool {
    if command.chars().any(|ch| matches!(ch,
        ';'|'&'|'|'|'>'|'<'|'\n'|'\r'|'$') || (ch as u32)==96) {
        return false;
    }
    let words=command.split_whitespace().collect::<Vec<_>>();
    let Some(exe)=words.first().copied() else {return false};
    match exe {
        "pwd"|"ls"|"cat"|"grep"=>true,
        "rg"=>!words.iter().any(|word|word.starts_with("--pre")),
        "pytest"=>true,
        "cargo"=>matches!(words.get(1).copied(),
            Some("check"|"test"|"metadata"|"clippy")),
        "npm"=>match words.get(1).copied() {
            Some("test")=>true,
            Some("run")=>matches!(words.get(2).copied(),
                Some("build"|"typecheck"|"lint"|"test")),
            _=>false,
        },
        "git"=>{
            let mut at=1;
            while let Some(flag)=words.get(at).copied() {
                if matches!(flag,"-C"|"-c"|"--git-dir"|"--work-tree") {
                    at+=2;
                }else{break}
            }
            matches!(words.get(at).copied(),
                Some("status"|"diff"|"show"|"log"|"rev-parse"|
                    "ls-files"|"cat-file"|"merge-base"))
        },
        _=>false,
    }
}

/// `terminal_exec` sends an executable and argv separately. Evaluate the
/// actual native command for replay admission, not just the executable name:
/// `cargo` alone is not a test, while `cargo + ["test"]` is. Never interpolate
/// argv through a shell; this is classification of an already executed call.
fn observed_read_only_command(item:&Value)->bool{
    let tool=item["tool"].as_str().unwrap_or("");
    let args=&item["arguments"];
    let Some(executable)=args["command"].as_str() else{return false};
    if tool=="shell_exec"{return read_only_shell_command(executable);}
    if tool!="terminal_exec"{return false;}
    if executable.is_empty() || executable.chars().any(char::is_whitespace){
        return false;
    }
    let Some(argv)=args["args"].as_array() else{
        return read_only_shell_command(executable);
    };
    let mut command=executable.to_owned();
    for part in argv{
        let Some(part)=part.as_str() else{return false};
        if part.is_empty() || part.chars().any(char::is_whitespace){
            // We cannot prove a shell-like string is just one benign flag.
            return false;
        }
        command.push(' ');
        command.push_str(part);
    }
    read_only_shell_command(&command)
}

/// A tool result explicitly labelled uncertain must never be silently
/// promoted by the final file-oracle. This differs from a *known successful*
/// non-idempotent write, which should not be replayed but can be verified.
pub fn observed_unknown_outcome(evidence:&[Value])->bool{
    evidence.iter().any(|item|
        item["outcomeUnknown"]==true || item["result"]["outcomeUnknown"]==true
        || item["effectUnknown"]==true || item["result"]["effectUnknown"]==true)
}

pub fn side_effect_unknown(evidence:&[Value])->bool {
    // Workspace edits and read-only operations are retryable after a fresh
    // verifier pass; arbitrary shell/MCP/HTTP writes are not.
    evidence.iter().any(|item|{
        // A multi-file patch has no remote provider, but a failed move can
        // leave a destination committed while source deletion is uncertain.
        // Do not treat that partial result as a safe replan/replay.
        if item["outcomeUnknown"]==true || item["result"]["outcomeUnknown"]==true
            || item["effectUnknown"]==true || item["result"]["effectUnknown"]==true {
            return true;
        }
        // Rejected proposals and pre-dispatch failures never started an effect.
        if item["dispatched"]==false {return false;}
        let tool=item["tool"].as_str().unwrap_or("");
        // The previous barrier treated even ordinary web/file lookups as
        // unresolved writes. That prevented a search→edit task from repairing
        // a failed acceptance check. Only explicit native observational tools
        // and GET requests are replayable; unknown MCP tools remain gated.
        let known_read=matches!(tool,
            "web_search"|"http_fetch"|"memory_search"|"skill_read"|"room_message_read"|"code_search"|
            "workspace_read"|"workspace_read_range"|"workspace_list"|
            "workspace_search"|"file_read"|"file_list"|
            "task_status"|"task_result"|"account_status"|
            "google_drive_search"|"google_docs_read"|"google_sheets_read"|
            "github_repository_activity"|"github_file_read"
        ) || (tool=="github_api" && item["arguments"]["method"]
            .as_str().is_some_and(|method|method.eq_ignore_ascii_case("GET")));
        let local_replay=matches!(tool,
            "workspace_write"|"workspace_replace"|"workspace_replace_range"|
            "workspace_apply_patch"|"file_write"
        );
        !(known_read || local_replay || observed_read_only_command(item))
    })
}
pub fn decide(
    strategy:Strategy,
    has_verifiable_checks:bool,
    gaps:&[String],
    error:Option<&str>,
    evidence:&[Value],
    attempt:usize,
    max_attempts:usize,
) -> Decision {
    if observed_unknown_outcome(evidence){return Decision::Review;}
    if gaps.is_empty() && error.is_none(){return Decision::Accept;}
    // Even if a provider fails after the last successful workspace write,
    // a frozen independent verifier can demonstrate the artifact exists.
    if gaps.is_empty() && has_verifiable_checks {return Decision::Accept;}
    if side_effect_unknown(evidence) {return Decision::Review;}
    if strategy==Strategy::Conservative && !evidence.is_empty() && error.is_some(){
        return Decision::Review;
    }
    if attempt+1>=max_attempts {return Decision::Review;}
    Decision::Repair
}
pub fn narrow(text:&str,max:usize)->String {
    let mut limited=text.chars().take(max).collect::<String>();
    if text.chars().count()>max {limited.push('…');}
    limited
}
/// Stop paying for the same unsuccessful model turn without fresh evidence.
pub fn usage_delta(previous:&Value,current:&Value)->Value {
    let mut out=json!({});
    for key in ["calls","promptTokens","completionTokens","totalTokens","estimatedCalls"] {
        let before=previous[key].as_u64().unwrap_or(0);
        let after=current[key].as_u64().unwrap_or(0);
        out[key]=json!(after.saturating_sub(before));
    }
    out
}
pub fn stalled(previous:&[String],current:&[String],new_evidence:usize,
    previous_error:Option<&str>,error:Option<&str>)->bool{
    !current.is_empty() && previous==current
        && new_evidence==0 && previous_error==error
}
pub fn next_instruction(goal:&str,gaps:&[String],error:Option<&str>,round:usize)->String {
    let evidence=gaps.iter().take(6).map(|gap|format!("- {}",narrow(gap,300)))
        .collect::<Vec<_>>().join("\n");
    let error=error.map(|text|narrow(text,400)).unwrap_or_default();
    format!(
        "원래 사용자 작업:\n{goal}\n\n\
         Rust 실행 엔진이 {round}번째 시도 이후 확인한 미완료 항목:\n{evidence}\n\
         이전 실행 오류: {error}\n\n\
         이미 수행한 변경을 되돌리거나 중복 실행하지 마세요. \
         현재 파일/테스트 상태를 필요한 범위만 확인한 뒤 실패한 항목만 수정하고 검증하세요. \
         설명만으로 완료했다고 주장하지 말고 검증 가능한 산출물을 만드세요."
    )
}


#[derive(Debug,PartialEq,Eq)]
pub struct OwnerIntent{
    pub goal:String,pub additions:Vec<String>,pub replacement_revision:u64,
}
impl OwnerIntent{
    pub fn effective_goal(&self)->String{
        if self.additions.is_empty(){return self.goal.clone();}
        format!("{}\n\n사용자가 추가한 작업 조건:\n{}",self.goal,
            self.additions.iter().map(|s|format!("- {s}")).collect::<Vec<_>>().join("\n"))
    }
}
/// Replay durable owner instructions: replacement invalidates the old task and
/// prior additions; subsequent additions remain binding.
pub fn owner_intent(original:&str,messages:&[Value])->OwnerIntent{
    let mut active=OwnerIntent{goal:original.into(),additions:Vec::new(),replacement_revision:0};
    for message in messages{
        let Some(instruction)=message["instruction"].as_str()
            .map(str::trim).filter(|s|!s.is_empty()) else{continue;};
        if message["mode"]=="replace"{
            active.goal=instruction.into();
            active.additions.clear();
            active.replacement_revision=message["revision"].as_u64().unwrap_or(0);
        }else{active.additions.push(instruction.into());}
    }
    active
}


#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scoped_room_message_read_never_requires_non_idempotent_replay(){
        let evidence=vec![json!({"tool":"room_message_read",
            "arguments":{"messageId":"immutable-row","offset":0},
            "result":{"source":"committed-room-transcript","readOnly":true}})];
        assert!(!side_effect_unknown(&evidence));
        assert!(!observed_unknown_outcome(&evidence));
    }
    #[test]
    fn scoped_skill_read_is_not_an_unknown_remote_effect(){
        let observation=json!({"tool":"skill_read",
            "arguments":{"skillId":"owner-approved"},
            "result":{"id":"owner-approved","readOnly":true}});
        assert!(!side_effect_unknown(&[observation]));
    }
    #[test]
    fn structured_terminal_read_only_argv_allows_safe_verifier_retries(){
        let command=|exe:&str,args:&[&str]|json!({
            "tool":"terminal_exec","arguments":{"command":exe,"args":args},
            "result":{"exitCode":0}
        });
        for (exe,args) in [
            ("cargo",vec!["test","--lib"]),
            ("npm",vec!["run","typecheck"]),
            ("git",vec!["-C","project","diff","--stat"]),
            ("rg",vec!["needles","src"]),
        ]{
            let evidence=vec![command(exe,&args)];
            assert!(!side_effect_unknown(&evidence),
                "successful read-only terminal argv should permit repair: {evidence:?}");
            assert_eq!(decide(Strategy::Focused,true,
                &["missing criterion".into()],None,&evidence,0,3),Decision::Repair);
        }
        for (exe,args) in [
            ("git",vec!["commit","-am","msg"]),
            ("npm",vec!["install"]),
            ("python",vec!["-c","print(1)"]),
        ]{
            assert!(side_effect_unknown(&[command(exe,&args)]),
                "{exe} {args:?} must remain replay gated");
        }
        assert!(side_effect_unknown(&[command("cargo",&["test;rm","-rf"])]));
        assert!(side_effect_unknown(&[json!({"tool":"terminal_exec",
            "arguments":{"command":"cargo","args":["test",{"invalid":true}]},
            "result":{"exitCode":0}})]));
    }

    #[test]
    fn known_successful_remote_effect_and_unknown_outcome_are_distinct(){
        let completed=vec![json!({"tool":"github_api",
            "arguments":{"method":"POST"},"result":{"ok":true}})];
        assert!(side_effect_unknown(&completed),
            "do not automatically replay an already applied remote mutation");
        assert!(!observed_unknown_outcome(&completed),
            "settled tool success is not an unknown outcome");
        assert_eq!(decide(Strategy::Focused,true,&[],None,&completed,0,3),
            Decision::Accept,
            "independent verifier success may settle a known remote effect");
        assert_eq!(decide(Strategy::Focused,true,
            &["missing evidence".into()],None,&completed,0,3),
            Decision::Review);
        let unresolved=vec![json!({"tool":"github_api",
            "arguments":{"method":"POST"},"result":{"outcomeUnknown":true}})];
        assert!(observed_unknown_outcome(&unresolved));
        assert_eq!(decide(Strategy::Focused,true,&[],None,&unresolved,0,3),
            Decision::Review);
    }

    #[test]
    fn unknown_external_effect_never_grants_automatic_repair() {
        let observations=vec![json!({"tool":"mcp__billing__charge",
            "arguments":{"amount":1},"result":{"outcomeUnknown":true}})];
        assert!(side_effect_unknown(&observations));
        assert_eq!(decide(Strategy::Focused,true,&[],None,&observations,0,6),
            Decision::Review);
    }
    #[test]
    fn failed_tool_effect_uncertainty_is_not_lost_before_retry() {
        let blocked=vec![json!({"tool":"github_api","status":"failed",
            "result":{"effectUnknown":true,"error":"unsettled prior write"}})];
        assert!(side_effect_unknown(&blocked));
        assert_eq!(decide(Strategy::Focused,false,&["remote result unknown".into()],
            Some("tool failed"),&blocked,0,6),Decision::Review);
        let denied=vec![json!({"tool":"github_api","status":"failed",
            "dispatched":false,"result":{"error":"permission denied"}})];
        assert!(!side_effect_unknown(&denied),
            "a rejected proposal must not be mistaken for an executed remote write");
    }
    #[test]
    fn read_only_discovery_can_precede_a_safe_repair(){
        for tool in ["web_search","http_fetch","memory_search",
            "google_drive_search","google_docs_read","google_sheets_read",
            "github_repository_activity","file_list","task_result"] {
            let evidence=vec![json!({"tool":tool,"arguments":{"query":"source"},
                "result":{"ok":true}})];
            assert!(!side_effect_unknown(&evidence),"{tool} must not block repair");
            assert_eq!(decide(Strategy::Focused,true,&["output missing".into()],
                None,&evidence,0,3),Decision::Repair);
        }
        let get=vec![json!({"tool":"github_api",
            "arguments":{"method":"get"},"result":{"status":200}})];
        assert!(!side_effect_unknown(&get));
        let post=vec![json!({"tool":"github_api",
            "arguments":{"method":"POST"},"result":{"status":200}})];
        assert!(side_effect_unknown(&post));
        let unknown_mcp=vec![json!({"tool":"mcp__payments__lookup",
            "result":{"ok":true}})];
        assert!(side_effect_unknown(&unknown_mcp));
    }
    #[test]
    fn owner_goal_revisions_do_not_reapply_superseded_additions() {
        let notes=vec![
            json!({"revision":1,"mode":"append","instruction":"old extra"}),
            json!({"revision":2,"mode":"replace","instruction":"new target"}),
            json!({"revision":3,"mode":"append","instruction":"new extra"}),
        ];
        let intent=owner_intent("old target",&notes);
        assert_eq!(intent.effective_goal(),"new target\n\n사용자가 추가한 작업 조건:\n- new extra");
        assert_eq!(intent.replacement_revision,2);
    }
    #[test]
    fn observed_calls_cannot_be_refunded_by_stale_counters() {
        let delta=usage_delta(&json!({"calls":10,"totalTokens":1000}),
            &json!({"calls":9,"totalTokens":800}));
        assert_eq!(delta["calls"],0);
        assert_eq!(delta["totalTokens"],0);
    }
}
