//! End-to-end Rust SDK acceptance against actual filesystem and shell effects.
//! This is a self-contained test host, not a mock tool-result interpreter.
//! The scripted planner is deterministic so tool/recovery behavior is tested
//! without paid provider credentials.
use dominion_agent_sdk::{
    Action, BoxFuture, Criterion, CriterionKind, Evidence, Journal, Kernel,
    Limits, Model, ModelReply, Receipt, RunRequest, Snapshot, Status, Strategy,
    ToolSpec, Tools,
};
use serde_json::{json, Value};
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    process::Command,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

const VERIFY: &str =
    "grep -q '^READY$' artifact.txt || { echo 'artifact missing READY' >&2; exit 1; }";

struct Planner {
    scripted: Mutex<VecDeque<String>>,
    prompts: Mutex<Vec<String>>,
}
impl Planner {
    fn new(turns: Vec<Value>) -> Self {
        Self {
            scripted: Mutex::new(turns.into_iter().map(|v|v.to_string()).collect()),
            prompts: Mutex::new(Vec::new()),
        }
    }
}
impl Model for Planner {
    fn complete<'a>(&'a self, _system:&'a str, prompt:&'a str)
        -> BoxFuture<'a,Result<ModelReply,String>> {
        Box::pin(async move {
            self.prompts.lock().unwrap().push(prompt.to_string());
            let content=self.scripted.lock().unwrap().pop_front()
                .ok_or("unexpected model call")?;
            Ok(ModelReply{content,input_tokens:Some(90),output_tokens:Some(30)})
        })
    }
}

struct LocalTools {
    root: PathBuf,
    calls: Mutex<Vec<String>>,
}
impl Tools for LocalTools {
    fn definitions(&self)->Vec<ToolSpec>{
        vec![
            ToolSpec{name:"workspace_write".into(),description:"Edit a file".into(),
                schema:json!({}),read_only:false},
            ToolSpec{name:"shell_exec".into(),description:"Run a test".into(),
                schema:json!({}),read_only:false},
        ]
    }
    fn execute<'a>(&'a self,action:&'a Action)->BoxFuture<'a,Result<Value,String>>{
        Box::pin(async move {
            self.calls.lock().unwrap().push(action.id.clone());
            match action.tool.as_str() {
                "workspace_write" => {
                    let relative=action.arguments["path"].as_str().ok_or("path missing")?;
                    if relative!="artifact.txt" {return Err("test host blocks unknown paths".into());}
                    let text=action.arguments["content"].as_str().ok_or("content missing")?;
                    std::fs::write(self.root.join(relative),text).map_err(|e|e.to_string())?;
                    Ok(json!({"written":true,"path":relative,"bytes":text.len()}))
                }
                "shell_exec" => {
                    let command=action.arguments["command"].as_str().ok_or("command missing")?;
                    if command!=VERIFY {return Err("test host blocks unknown commands".into());}
                    let output=Command::new("sh").arg("-c").arg(command)
                        .current_dir(&self.root).output().map_err(|e|e.to_string())?;
                    let stderr=String::from_utf8_lossy(&output.stderr).to_string();
                    let code=output.status.code().unwrap_or(-1);
                    if code!=0 {
                        Err(format!("exitCode={code}; stderr_tail={stderr}"))
                    } else {
                        Ok(json!({"exitCode":0,"stderr":stderr,
                            "stdout":String::from_utf8_lossy(&output.stdout)}))
                    }
                }
                other=>Err(format!("unauthorized: {other}")),
            }
        })
    }
}
struct LocalEvidence {root:PathBuf}
impl Evidence for LocalEvidence {
    fn check<'a>(&'a self,criterion:&'a Criterion)->BoxFuture<'a,Result<bool,String>>{
        Box::pin(async move {
            if criterion.path.as_deref()!=Some("artifact.txt") {
                return Err("unregistered evidence path".into());
            }
            let file=self.root.join("artifact.txt");
            match criterion.kind {
                CriterionKind::FileExists=>Ok(file.exists()),
                CriterionKind::FileContains=>Ok(
                    std::fs::read_to_string(file).map_err(|e|e.to_string())?
                    .contains(criterion.contains.as_deref().unwrap_or(""))),
                _=>Err("unsupported by local evidence host".into()),
            }
        })
    }
}
struct DiskJournal {
    file:PathBuf,
    saved:Mutex<Vec<Snapshot>>,
}
impl Journal for DiskJournal {
    fn save(&self,run:&Snapshot)->Result<(),String>{
        let bytes=serde_json::to_vec(run).map_err(|e|e.to_string())?;
        let pending=self.file.with_extension("pending");
        let mut writer=std::fs::File::create(&pending).map_err(|e|e.to_string())?;
        use std::io::Write;
        writer.write_all(&bytes).map_err(|e|e.to_string())?;
        writer.sync_all().map_err(|e|e.to_string())?;
        std::fs::rename(&pending,&self.file).map_err(|e|e.to_string())?;
        self.saved.lock().unwrap().push(run.clone());
        Ok(())
    }
}
fn temp_workspace(label:&str)->PathBuf {
    let nano=SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let path=std::env::temp_dir()
        .join(format!("dominion-native-{label}-{}-{nano}",std::process::id()));
    std::fs::create_dir_all(&path).unwrap();
    path
}
fn request(id:&str)->RunRequest {
    RunRequest{id:id.into(),
        instruction:"코드 파일 수정 후 실제 테스트 통과".into(),
        owner_instructions:"Only confirmed file edits count".into(),
        strategy:Strategy::Balanced,
        limits:Limits{max_steps:8,max_model_calls:8,max_tool_calls:96,max_estimated_tokens:100_000,
            max_parallel_reads:2,max_result_chars:2000},
        allowed_tools:vec!["workspace_write".into(),"shell_exec".into()],
        checks:vec![Criterion{kind:CriterionKind::ToolSucceeded,
            tool:Some("shell_exec".into()),contains:Some(VERIFY.into()),path:None}],
        require_evidence:true}
}
fn edit(id:&str,content:&str)->Value {
    json!({"plan":["edit then verify"],"actions":[{
        "id":id,"tool":"workspace_write",
        "arguments":{"path":"artifact.txt","content":content},
        "dependsOn":[]
    }],"answer":null})
}
fn done()->Value {
    json!({"plan":["verify completion"],"actions":[],"answer":"Source repaired and checked"})
}

#[tokio::test]
async fn actual_shell_failure_triggers_real_file_repair_then_green_test() {
    let root=temp_workspace("repair");
    let model=Planner::new(vec![edit("wrong","WRONG\n"),done(),
        edit("repair","READY\n"),done()]);
    let host=LocalTools{root:root.clone(),calls:Mutex::new(Vec::new())};
    let evidence=LocalEvidence{root:root.clone()};
    let journal=DiskJournal{file:root.join("checkpoint.json"),saved:Mutex::new(Vec::new())};
    let kernel=Kernel{model:&model,tools:&host,evidence:&evidence,journal:&journal};
    let result=kernel.run(request("local-repair")).await.unwrap();
    assert!(matches!(result.status,Status::Completed),"{result:?}");
    assert_eq!(std::fs::read_to_string(root.join("artifact.txt")).unwrap(),"READY\n");
    assert_eq!(result.model_calls,4);
    assert_eq!(host.calls.lock().unwrap().as_slice(),
        ["wrong","kernel-verify-2-0","repair","kernel-verify-4-0"]);
    assert!(result.receipts.iter().any(|r|r.id=="kernel-verify-2-0"&&!r.success));
    assert!(result.receipts.iter().any(|r|r.id=="kernel-verify-4-0"&&r.success));
    assert!(model.prompts.lock().unwrap()[2].contains("artifact missing READY"),
        "The planner must receive the actual compiler/test diagnostic");
    let from_disk:Snapshot=serde_json::from_slice(
        &std::fs::read(&journal.file).unwrap()).unwrap();
    assert!(matches!(from_disk.status,Status::Completed));
    assert!(from_disk.in_flight.is_empty());
    let _=std::fs::remove_dir_all(&root);
}

#[tokio::test]
async fn restored_verified_receipt_does_not_replay_a_shell_command() {
    let root=temp_workspace("resume");
    let req=request("local-resume");
    let model=Planner::new(vec![done()]);
    let host=LocalTools{root:root.clone(),calls:Mutex::new(Vec::new())};
    let evidence=LocalEvidence{root:root.clone()};
    std::fs::write(root.join("artifact.txt"),"READY\n").unwrap();
    let journal=DiskJournal{file:root.join("checkpoint.json"),saved:Mutex::new(Vec::new())};
    let mut saved=Snapshot::from_request(&req);
    saved.step=2;
    saved.model_calls=2;
    saved.receipts.push(Receipt{id:"written".into(),tool:"workspace_write".into(),
        arguments:json!({"path":"artifact.txt","content":"READY\n"}),
        success:true,result:json!({"written":true}),step:1});
    saved.receipts.push(Receipt{id:"verified".into(),tool:"shell_exec".into(),
        arguments:json!({"command":VERIFY}),
        success:true,result:json!({"exitCode":0}),step:2});
    journal.save(&saved).unwrap();
    let persisted:Snapshot=serde_json::from_slice(&std::fs::read(&journal.file).unwrap()).unwrap();
    let kernel=Kernel{model:&model,tools:&host,evidence:&evidence,journal:&journal};
    let outcome=kernel.resume(req,persisted).await.unwrap();
    assert!(matches!(outcome.status,Status::Completed),"{outcome:?}");
    assert_eq!(host.calls.lock().unwrap().len(),0);
    let _=std::fs::remove_dir_all(root);
}
