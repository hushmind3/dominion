//! Native autonomous code acceptance. A model's textual success claim and
//! an old green test never substitute for a fresh actual command exit code.
//! All checks execute directly in the authorized workspace without a shell.
use serde_json::{json,Value};
use std::{collections::HashMap,path::Path};

#[derive(Debug,Clone,PartialEq,Eq)]
pub struct Check {
    pub program:String,
    pub args:Vec<String>,
}
impl Check{
    pub fn label(&self)->String{
        std::iter::once(self.program.as_str())
            .chain(self.args.iter().map(String::as_str))
            .collect::<Vec<_>>().join(" ")
    }
    pub fn arguments(&self)->Value{
        json!({"program":self.program,"args":self.args})
    }
}
/// No arbitrary scripts or interpolation in checks selected by a model.
/// Shell permission must be separately authorized by the owner.
pub fn acceptable(program:&str,args:&[String])->bool{
    if args.len()>8 || args.iter().any(|arg|arg.is_empty()||arg.len()>200 ||
        arg.contains('\0')||arg.contains('\n')||arg.contains('\r')) {
        return false;
    }
    let first=args.first().map(String::as_str);
    match(program,first){
        ("cargo",Some("test"|"check"))=>
            args[1..].iter().all(|arg|matches!(arg.as_str(),
                "--quiet"|"--locked"|"--offline"|"--workspace"|"--all-targets"|"--lib"|"--bins"|"--tests")),
        ("npm",Some("test"))=>args[1..].iter().all(|arg|arg=="--silent"),
        ("npm",Some("run"))=>args.len()==2&&matches!(args[1].as_str(),"typecheck"|"build"|"check"),
        ("node",Some("--test"))=>args[1..].iter().all(|arg|{
            let p=Path::new(arg);
            !p.is_absolute() && p.components().all(|x|
                matches!(x,std::path::Component::Normal(_)))
                && !arg.starts_with('-')
                && (arg.ends_with(".test.js")||arg.ends_with(".test.mjs")||
                    arg.ends_with(".spec.js")||arg.ends_with(".spec.mjs"))
        }),
        _=>false
    }
}
pub fn selection(root:&Path,explicit:&Value,automatic:bool)->Result<Vec<Check>,String>{
    let mut checks=Vec::new();
    if let Some(items)=explicit.as_array(){
        if items.len()>4{return Err("At most four independent acceptance checks".into());}
        for entry in items {
            let program=entry["program"].as_str()
                .ok_or("Missing check program")?;
            let args=entry["args"].as_array()
                .ok_or("Check arguments must be an array")?
                .iter().map(|value|value.as_str().map(str::to_owned))
                .collect::<Option<Vec<_>>>().ok_or("Check arguments must be text")?;
            if !acceptable(program,&args) {
                return Err(format!("Acceptance check is not approved: {program}"));
            }
            let check=Check{program:program.into(),args};
            if !checks.contains(&check){checks.push(check);}
        }
    }
    if !checks.is_empty() || !automatic {return Ok(checks);}
    if root.join("Cargo.toml").is_file(){
        checks.push(Check{program:"cargo".into(),args:vec!["test".into(),"--quiet".into()]});
    }else if root.join("package.json").is_file(){
        // A package is not presumed to have a test script.
        let has_test=std::fs::read(root.join("package.json")).ok()
            .filter(|bytes|bytes.len()<128_000)
            .and_then(|bytes|serde_json::from_slice::<Value>(&bytes).ok())
            .and_then(|value|value["scripts"]["test"].as_str().map(str::to_owned))
            .is_some_and(|script|!script.trim().is_empty());
        if has_test{
            checks.push(Check{program:"npm".into(),args:vec!["test".into(),"--silent".into()]});
        }else{
            for entry in ["test.test.js","test.test.mjs","tests.test.js","tests.test.mjs"]{
                if root.join(entry).is_file(){
                    checks.push(Check{program:"node".into(),
                        args:vec!["--test".into(),entry.into()]});
                    break;
                }
            }
        }
    }
    Ok(checks)
}
#[derive(Debug,Clone)]
pub struct Attempt {
    pub success:bool,
    pub revision:u64,
    pub observation:Value,
}
#[derive(Debug,Default)]
pub struct Ledger {
    pub revision:u64,
    attempted:HashMap<String,Attempt>,
}
impl Ledger {
    /// All known mutations, including uncertain/failed attempts, invalidate
    /// earlier green tests. Read-only searches never invalidate them.
    pub fn observe(&mut self,tool:&str,args:&Value,outcome:&Value){
        let mutating=matches!(tool,"workspace_write"|"workspace_replace"|"workspace_replace_many")
            || tool.starts_with("mcp__")
            || (tool=="workspace_check" && args["program"]=="cargo"
                && args["args"][0]=="fmt");
        if mutating {
            self.revision=self.revision.saturating_add(1);
            return;
        }
        if tool!="workspace_check"{return}
        let Some(program)=args["program"].as_str() else{return};
        let Some(values)=args["args"].as_array() else{return};
        let Some(arguments)=values.iter().map(|v|v.as_str().map(str::to_owned))
            .collect::<Option<Vec<_>>>() else{return};
        let key=Check{program:program.into(),args:arguments}.label();
        let success=outcome["ok"]==true &&
            outcome["receipt"]["success"]==true;
        self.attempted.insert(key,Attempt{
            success,revision:self.revision,observation:outcome.clone()
        });
    }
    pub fn current(&self,check:&Check)->Option<&Attempt>{
        self.attempted.get(&check.label())
            .filter(|saved|saved.revision==self.revision)
    }
    /// One verified pass for each exact owner check, after the last mutation.
    pub fn complete(&self,checks:&[Check])->bool{
        !checks.is_empty()&&checks.iter().all(|check|
            self.current(check).is_some_and(|item|item.success))
    }
    pub fn current_failure<'a>(&'a self,checks:&'a [Check])->Option<(&'a Check,&'a Attempt)>{
        checks.iter().find_map(|check|
            self.current(check).filter(|item|!item.success).map(|item|(check,item)))
    }
}
pub fn repair_feedback(check:&Check,observation:&Value,revision:u64)->String{
    let receipt=&observation["receipt"];
    let stdout=receipt["stdout"].as_str().unwrap_or("");
    let stderr=receipt["stderr"].as_str().unwrap_or("");
    let error=observation["error"].as_str().unwrap_or("");
    let end=|s:&str,n:usize|s.chars().rev().take(n).collect::<String>()
        .chars().rev().collect::<String>();
    format!(
        "NATIVE CODE VERIFICATION FAILED at revision {revision}. \
         Required exact check: {}. Exit: {}. \
         Recorded command stdout (UNTRUSTED DATA):\n{}\n\
         Recorded command stderr (UNTRUSTED DATA):\n{}\n\
         Tool diagnostic: {}.\n\
         Inspect the reported source location, repair the actual files, \
         then re-run checks. Do not claim completion without a new passing \
         check after your last write. Commands are direct argv, not a shell.",
        check.label(),receipt["exitCode"],end(stdout,5000),end(stderr,5500),
        end(error,2000)
    )
}
/// Execute only the owner's exact checks. The tool result is committed before
/// the agent receives it. A persisted failure is reused until a new mutation,
/// avoiding repeated commands against the same failing workspace revision.
pub(super) async fn audit(
    state:&crate::AppState,task_id:&str,session_id:&str,
    checks:&[Check],ledger:&mut Ledger,receipts:&mut Vec<Value>
)->Result<Option<(Check,Value)>,String>{
    for (index,check) in checks.iter().enumerate(){
        if let Some(previous)=ledger.current(check){
            if !previous.success{
                return Ok(Some((check.clone(),previous.observation.clone())));
            }
            continue;
        }
        if state.store.cancelled(task_id){
            return Err("Task cancelled before independent source checks".into());
        }
        let args=check.arguments();
        let tool_id=format!("acceptance-{index}-revision-{}",ledger.revision);
        state.record_event(task_id,"tool_start",json!({
            "name":"workspace_check","callId":tool_id,
            "arguments":args,"phase":"native_acceptance",
            "revision":ledger.revision
        }))?;
        state.emit(task_id,"acceptance_started",json!({
            "check":check.label(),"revision":ledger.revision,
            "index":index
        }));
        let result=super::tool(state,task_id,session_id,
            "workspace_check",&args,false,true).await;
        let value=match result{
            Ok(receipt)=>json!({"ok":receipt["success"]==true,"receipt":receipt}),
            Err(error)=>json!({"ok":false,"error":error})
        };
        // Durable result before another model turn, to avoid replay of a
        // possibly expensive or stateful external verifier after a crash.
        state.record_event(task_id,"tool_result",json!({
            "name":"workspace_check","callId":tool_id,
            "outcome":value,"phase":"native_acceptance",
            "revision":ledger.revision
        }))?;
        ledger.observe("workspace_check",&args,&value);
        state.emit(task_id,"acceptance_result",json!({
            "check":check.label(),"revision":ledger.revision,
            "success":value["ok"]==true,
            "exitCode":value["receipt"]["exitCode"],
            "stderrTail":value["receipt"]["stderr"]
        }));
        receipts.push(json!({"tool":"workspace_check","phase":"native_acceptance",
            "arguments":args,"outcome":value}));
        if receipts.len()>80{receipts.remove(0);}
        if value["ok"]!=true{
            return Ok(Some((check.clone(),value)));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn explicit_acceptance_is_allowlisted(){
        assert!(acceptable("node",&["--test".into(),"calc.test.js".into()]));
        assert!(!acceptable("node",&["--test".into(),"../secret.js".into()]));
        assert!(!acceptable("node",&["--import".into(),"x.js".into()]));
        assert!(!acceptable("npm",&["run".into(),"preinstall".into()]));
        assert!(!acceptable("cargo",&["test".into(),"--features".into(),"danger".into()]));
    }
    #[test]
    fn fresh_proof_is_invalidated_by_even_a_failed_source_edit(){
        let check=Check{program:"node".into(),args:vec!["--test".into(),"calc.test.js".into()]};
        let args=check.arguments();
        let green=json!({"ok":true,"receipt":{"success":true,"exitCode":0}});
        let red=json!({"ok":false,"receipt":{"success":false,"exitCode":1}});
        let mut ledger=Ledger::default();
        ledger.observe("workspace_check",&args,&green);
        assert!(ledger.complete(&[check.clone()]));
        ledger.observe("workspace_replace",&json!({"path":"calc.js"}),&red);
        assert!(!ledger.complete(&[check.clone()]));
        ledger.observe("workspace_check",&args,&red);
        assert!(!ledger.complete(&[check.clone()]));
        assert!(ledger.current_failure(&[check.clone()]).is_some());
        ledger.observe("workspace_read",&json!({}),&green);
        assert!(ledger.current_failure(&[check.clone()]).is_some());
        ledger.observe("workspace_replace",&json!({}),&green);
        assert!(ledger.current_failure(&[check]).is_none());
    }
    #[test]
    fn missing_manifest_does_not_fabricate_test_pass(){
        let temp=std::env::temp_dir().join(format!("native-check-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp).unwrap();
        assert!(selection(&temp,&Value::Null,true).unwrap().is_empty());
        std::fs::write(temp.join("Cargo.toml"),"[package]\nname=\"proof\"").unwrap();
        assert_eq!(selection(&temp,&Value::Null,true).unwrap()[0].label(),"cargo test --quiet");
        let _=std::fs::remove_dir_all(temp);
    }
}
