//! A reproducible software-engineering job: two real Rust files, a failing
//! compiler/test command, repair and crash-safe continuation. No paid provider.
//! The host executes rustc, not a mocked test-success response.
use dominion_agent_sdk::{
    Action, BoxFuture, Criterion, CriterionKind, Evidence, Journal, Kernel,
    Limits, Model, ModelReply, RunRequest, Snapshot, Status, Strategy, ToolSpec, Tools,
};
use serde_json::{json, Value};
use std::{
    collections::VecDeque, fs, path::{Path, PathBuf}, process::Command,
    sync::{atomic::{AtomicBool, Ordering}, Mutex},
};

const VERIFY: &str = "rustc --test src/main.rs";
const BAD_MATH: &str = "pub fn sum(a:i32,b:i32)->i32{a-b}\n";
const GOOD_MATH: &str = "pub fn sum(a:i32,b:i32)->i32{a+b}\n";
const TEST_FILE: &str = "#[path=\"math.rs\"] mod math;\n#[test] fn sum_works(){assert_eq!(math::sum(2,3),5);}\n";

struct Scripted {
    turns: Mutex<VecDeque<Value>>,
    prompts: Mutex<Vec<String>>,
}
impl Scripted {
    fn new(turns:Vec<Value>)->Self {
        Self{turns:Mutex::new(turns.into()),prompts:Mutex::new(Vec::new())}
    }
}
impl Model for Scripted {
    fn complete<'a>(&'a self,_system:&'a str,prompt:&'a str)
        -> BoxFuture<'a,Result<ModelReply,String>> {
        Box::pin(async move {
            self.prompts.lock().unwrap().push(prompt.to_string());
            let value=self.turns.lock().unwrap().pop_front()
                .ok_or("unexpected additional model call")?;
            Ok(ModelReply{content:value.to_string(),
                input_tokens:Some(120),output_tokens:Some(40)})
        })
    }
}
fn decision(actions:Vec<Value>,answer:Option<&str>)->Value {
    json!({"plan":["edit","run actual tests","repair failures"],"actions":actions,"answer":answer})
}
fn write(id:&str,path:&str,content:&str,deps:&[&str])->Value {
    json!({"id":id,"tool":"workspace_write",
        "arguments":{"path":path,"content":content},"dependsOn":deps})
}
struct RealRustHost {
    root:PathBuf,
    invoked:Mutex<Vec<String>>,
}
impl Tools for RealRustHost {
    fn definitions(&self)->Vec<ToolSpec>{
        vec![
            ToolSpec{name:"workspace_write".into(),description:"Write a scoped source file".into(),
                schema:json!({"type":"object","required":["path","content"],
                    "properties":{"path":{"type":"string"},"content":{"type":"string"}}}),
                read_only:false},
            ToolSpec{name:"shell_exec".into(),description:"Run real rustc tests".into(),
                schema:json!({"type":"object","required":["command"],
                    "properties":{"command":{"type":"string"}}}),read_only:false},
        ]
    }
    fn execute<'a>(&'a self,action:&'a Action)
        -> BoxFuture<'a,Result<Value,String>>{
        Box::pin(async move {
            self.invoked.lock().unwrap().push(action.id.clone());
            match action.tool.as_str() {
                "workspace_write"=>{
                    let path=action.arguments["path"].as_str().ok_or("missing path")?;
                    if !matches!(path,"src/math.rs"|"src/main.rs") {
                        return Err("outside test sandbox".into());
                    }
                    let content=action.arguments["content"].as_str().ok_or("missing content")?;
                    fs::write(self.root.join(path),content).map_err(|e|e.to_string())?;
                    Ok(json!({"written":true,"path":path,"bytes":content.len()}))
                }
                "shell_exec"=>{
                    if action.arguments["command"].as_str()!=Some(VERIFY) {
                        return Err("unapproved verification command".into());
                    }
                    let binary=self.root.join(format!("dominion_checks{}",
                        std::env::consts::EXE_SUFFIX));
                    let build=Command::new("rustc").arg("--edition=2021")
                        .arg("--test").arg("src/main.rs").arg("-o").arg(&binary)
                        .current_dir(&self.root).output().map_err(|e|e.to_string())?;
                    if !build.status.success() {
                        return Err(format!("rustc failed: {}",
                            String::from_utf8_lossy(&build.stderr)));
                    }
                    let tested=Command::new(&binary).current_dir(&self.root)
                        .output().map_err(|e|e.to_string())?;
                    let output=format!("{}{}",String::from_utf8_lossy(&tested.stdout),
                        String::from_utf8_lossy(&tested.stderr));
                    if !tested.status.success(){return Err(format!("tests failed: {output}"));}
                    Ok(json!({"exitCode":0,"output":output}))
                }
                _=>Err("unapproved host tool".into())
            }
        })
    }
}
struct RealEvidence{root:PathBuf}
impl Evidence for RealEvidence {
    fn check<'a>(&'a self,criterion:&'a Criterion)
        -> BoxFuture<'a,Result<bool,String>>{
        Box::pin(async move {
            let path=criterion.path.as_deref().unwrap_or("");
            if !matches!(path,"src/math.rs"|"src/main.rs") {
                return Err("unknown verification path".into());
            }
            let filename=self.root.join(path);
            match criterion.kind {
                CriterionKind::FileExists=>Ok(filename.is_file()),
                CriterionKind::FileContains=>Ok(fs::read_to_string(filename)
                    .map(|content|content.contains(criterion.contains.as_deref().unwrap_or("")))
                    .unwrap_or(false)),
                _=>Err("unhandled evidence type".into()),
            }
        })
    }
}
struct FaultJournal {
    file:PathBuf,
    interrupt_after_first_write:AtomicBool,
}
impl FaultJournal {
    fn new(file:PathBuf,interrupt:bool)->Self {
        Self{file,interrupt_after_first_write:AtomicBool::new(interrupt)}
    }
    fn load(&self)->Snapshot {
        serde_json::from_slice(&fs::read(&self.file).unwrap()).unwrap()
    }
}
impl Journal for FaultJournal {
    fn save(&self,run:&Snapshot)->Result<(),String>{
        use std::io::Write;
        let pending=self.file.with_extension("pending");
        let mut out=fs::File::create(&pending).map_err(|e|e.to_string())?;
        out.write_all(&serde_json::to_vec(run).map_err(|e|e.to_string())?)
            .map_err(|e|e.to_string())?;
        out.sync_all().map_err(|e|e.to_string())?;
        fs::rename(&pending,&self.file).map_err(|e|e.to_string())?;
        // Inject a process failure AFTER persisting the confirmed first write,
        // but BEFORE running the second source write or any acceptance test.
        if run.in_flight.is_empty() && run.receipts.iter().any(|receipt|
            receipt.id=="initial-math" && receipt.success)
            && self.interrupt_after_first_write.swap(false,Ordering::SeqCst) {
            return Err("injected crash after durable write receipt".into());
        }
        Ok(())
    }
}
fn workspace()->PathBuf {
    let nonce=std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let root=std::env::temp_dir().join(format!(
        "dominion-fault-multifile-{}-{nonce}",std::process::id()));
    // Isolated across CI processes and repeated local invocations.
    fs::create_dir_all(root.join("src")).unwrap();
    root
}
fn request()->RunRequest {
    RunRequest{
        id:"multifile-fault-recovery".into(),
        instruction:"수정: 두 Rust 파일의 코드를 고치고 실제 테스트를 통과하세요".into(),
        owner_instructions:"Do not report success without an actual compiler run".into(),
        strategy:Strategy::Balanced,
        limits:Limits{max_steps:10,max_model_calls:10,max_tool_calls:20,
            max_estimated_tokens:60_000,max_parallel_reads:2,max_result_chars:2000},
        allowed_tools:vec!["workspace_write".into(),"shell_exec".into()],
        checks:vec![
            Criterion{kind:CriterionKind::FileContains,
                path:Some("src/main.rs".into()),
                contains:Some("sum_works".into()),tool:None},
            Criterion{kind:CriterionKind::ToolSucceeded,
                path:None,contains:Some(VERIFY.into()),tool:Some("shell_exec".into())},
        ],
        require_evidence:true,
    }
}

#[tokio::test]
async fn two_file_crash_restore_failed_test_repair_and_verified_completion(){
    let root=workspace();
    let host=RealRustHost{root:root.clone(),invoked:Mutex::new(Vec::new())};
    let evidence=RealEvidence{root:root.clone()};
    let checkpoint=root.join("agent-checkpoint.json");
    let journal=FaultJournal::new(checkpoint.clone(),true);
    let first=Scripted::new(vec![decision(vec![
        write("initial-math","src/math.rs",BAD_MATH,&[]),
        write("original-test","src/main.rs",TEST_FILE,&["initial-math"]),
    ],None)]);
    let kernel=Kernel{model:&first,tools:&host,evidence:&evidence,journal:&journal};
    let err=kernel.run(request()).await.err().expect("crash fixture must interrupt");
    assert!(err.contains("injected crash"),"{err}");
    assert_eq!(fs::read_to_string(root.join("src/math.rs")).unwrap(),BAD_MATH);
    assert!(!root.join("src/main.rs").exists(),"uncommitted action cannot leak");
    let persisted=journal.load();
    assert!(matches!(persisted.status,Status::Running));
    assert!(!persisted.model_in_flight && persisted.in_flight.is_empty());
    assert_eq!(persisted.receipts.len(),1);
    assert_eq!(persisted.model_calls,1);
    // A new worker recovers only the committed source edit, then independently
    // executes rustc, receives a real failing test, repairs and re-verifies.
    let resumed=Scripted::new(vec![
        decision(vec![write("test-file","src/main.rs",TEST_FILE,&[])],None),
        decision(vec![],Some("Tests passed")), // first claim must fail
        decision(vec![write("repair-math","src/math.rs",GOOD_MATH,&[])],None),
        decision(vec![],Some("Verified source and tests")),
    ]);
    let recovered_journal=FaultJournal::new(checkpoint,false);
    let resumed_kernel=Kernel{model:&resumed,tools:&host,
        evidence:&evidence,journal:&recovered_journal};
    let result=resumed_kernel.resume(request(),persisted).await.unwrap();
    assert!(matches!(result.status,Status::Completed),"{result:?}");
    assert_eq!(result.model_calls,5);
    assert_eq!(fs::read_to_string(root.join("src/math.rs")).unwrap(),GOOD_MATH);
    assert_eq!(fs::read_to_string(root.join("src/main.rs")).unwrap(),TEST_FILE);
    let tool_ids=host.invoked.lock().unwrap().clone();
    assert_eq!(tool_ids.iter().filter(|id|id.as_str()=="initial-math").count(),1);
    assert!(result.receipts.iter().any(|r|
        r.tool=="shell_exec" && !r.success && r.result.to_string().contains("tests failed")));
    assert!(result.receipts.iter().any(|r|r.tool=="shell_exec" && r.success));
    assert!(resumed.prompts.lock().unwrap()[2].contains("tests failed"),
        "planner must see the real failure before rewriting source");
    assert!(matches!(recovered_journal.load().status,Status::Completed));
    assert!(result.charged_tokens>0);
    let _=fs::remove_dir_all(&root);
}
