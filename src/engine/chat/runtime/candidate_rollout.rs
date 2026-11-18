//! Executable, candidate-local repair rollouts (2026).
//!
//! The real ProgramIR interpreter and native tool dispatcher are reused: no
//! second agent runner, fake tool returns, model-invented success, or shared
//! workspace write. Each candidate gets a distinct SQLite database and a
//! bounded copy of its declared local file dependencies. Non-local effects
//! (shell, MCP, network, delegation, arbitrary paths) are ineligible.
//! Verified rollouts are evidence for the real gateway, never permission.
use super::*;
use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};
use std::time::Instant;
use sha2::{Digest, Sha256};

const MAX_FILES: usize=32;
const MAX_TOTAL_BYTES: usize=8*1024*1024;
const MAX_NODES: usize=16;
const MAX_CANDIDATES: usize=3;

/// Expensive counterfactual work is an owner-selected capability, not the
/// default execution loop. Reuses the existing algorithm/session policies;
/// no new background worker or runtime state is created at startup.
pub(super) fn enabled(state:&AppState,session:&Value)->bool{
    if std::env::var("DOMINION_BENCHMARK").as_deref()==Ok("1")
        && crate::engine::policy::active(state,"execution").as_ref()
            .is_some_and(|policy|policy["parameters"]["benchmarkRepairArm"]=="treatment"){
        return true;
    }
    session["sdkUiPolicy"]["candidateRollout"]==true
        || crate::engine::algorithms::active(state,"program.repair")
            .as_ref().is_some_and(|algorithm|
                algorithm["parameters"]["candidateRollout"]==true)
}

struct TrialArea {
    root:PathBuf,
}
impl TrialArea {
    fn create()->Result<Self,String>{
        let root=std::env::temp_dir().join(format!(
            "dominion-candidate-{}",uuid::Uuid::new_v4()
        ));
        std::fs::create_dir(&root).map_err(|error|error.to_string())?;
        std::fs::create_dir_all(root.join("workspace"))
            .and_then(|()|std::fs::create_dir_all(root.join("data")))
            .map_err(|error|error.to_string())?;
        Ok(Self{root})
    }
    fn workspace(&self)->PathBuf{self.root.join("workspace")}
}
impl Drop for TrialArea {
    fn drop(&mut self){let _=std::fs::remove_dir_all(&self.root);}
}

fn permitted_path(raw:&str)->Result<PathBuf,String>{
    if raw.is_empty() || raw.len()>280 {
        return Err("candidate path is empty or too large".into());
    }
    let path=Path::new(raw);
    if path.is_absolute() {return Err("absolute candidate path denied".into());}
    for part in path.components(){
        match part {
            Component::Normal(segment) => {
                let value=segment.to_string_lossy();
                if matches!(value.as_ref(),".git"|"node_modules"|"target"|
                    "runtime"|".env"|".secrets"|"secrets") {
                    return Err("candidate path enters a protected directory".into());
                }
            },
            _=>return Err("candidate path contains traversal or normalization".into())
        }
    }
    Ok(path.to_path_buf())
}
fn path_of(node:&Value)->Result<PathBuf,String>{
    let path=node["inputs"]["path"].as_str()
        .ok_or("candidate tool must use a literal relative path")?;
    permitted_path(path)
}
fn count_and_collect(nodes:&[Value],paths:&mut BTreeSet<PathBuf>,
                     visits:&mut usize,calls:&mut usize)->Result<(),String>{
    for node in nodes{
        *visits+=1;
        if *visits>MAX_NODES{return Err("candidate execution plan is too large".into());}
        match node["type"].as_str().unwrap_or("") {
            "call_tool"=>{
                if !matches!(node["tool"].as_str(),
                    Some("workspace_read"|"workspace_read_range"|
                    "workspace_write"|"workspace_replace"|"workspace_replace_range"))
                {return Err("candidate requires an unisolated or effectful external tool".into());}
                paths.insert(path_of(node)?);
                *calls+=1;
            },
            "observe"|"verify"=>{},
            "sequence"=>{
                let children=node["nodes"].as_array()
                    .ok_or("candidate sequence has no child nodes")?;
                count_and_collect(children,paths,visits,calls)?;
            },
            _=>return Err("candidate requires a nondeterministic or external program node".into()),
        }
    }
    Ok(())
}
/// A candidate may fall back to preflight-only selection ONLY if no
/// preflight-qualified candidate has a local-only, bounded execution plan.
/// A failed local rollout must never silently bypass independent testing.
fn sandboxable(variant:&Value)->bool{
    let Some(nodes)=variant["revision"]["nodes"].as_array() else{return false};
    let Some(postflights)=variant["postflights"].as_array() else{return false};
    if postflights.is_empty() || postflights.len()>3{return false;}
    let mut paths=BTreeSet::new();let mut visits=0usize;let mut calls=0usize;
    if count_and_collect(nodes,&mut paths,&mut visits,&mut calls).is_err(){return false;}
    for post in postflights{
        if !matches!(post["tool"].as_str(),Some("workspace_read"|"workspace_read_range")){return false;}
        let Some(path)=post["inputs"]["path"].as_str() else{return false};
        let Ok(path)=permitted_path(path) else{return false};
        paths.insert(path);
    }
    paths.len()<=MAX_FILES
}
fn annotate_native_cas(revision:&mut Value, baseline:&Value)->Result<(),String>{
    let nodes=revision["nodes"].as_array_mut().ok_or("revision nodes absent")?;
    for node in nodes {
        if node["type"]!="call_tool"{continue;}
        let tool=node["tool"].as_str().unwrap_or("");
        if !matches!(tool,"workspace_write"|"workspace_replace"|"workspace_replace_range"){continue;}
        let path=node["inputs"]["path"].as_str().ok_or("write lacks path")?;
        let hash=&baseline[path];
        if let Some(digest)=hash.as_str(){
            node["inputs"]["expectedSha256"]=json!(digest);
            node["inputs"].as_object_mut().map(|o|o.remove("requireAbsent"));
        }else if hash.is_null() && tool=="workspace_write" {
            node["inputs"]["requireAbsent"]=json!(true);
            node["inputs"].as_object_mut().map(|o|o.remove("expectedSha256"));
        }else{
            return Err("cannot promote replace on a non-existent snapshot path".into());
        }
    }
    Ok(())
}
fn file_digest(path:&Path)->Result<Option<String>,String>{
    match std::fs::symlink_metadata(path){
        Ok(meta)=>{
            if !meta.file_type().is_file(){
                return Err("candidate dependency is a symlink or non-file".into());
            }
            let bytes=std::fs::read(path).map_err(|e|e.to_string())?;
            Ok(Some(format!("{:x}",Sha256::digest(bytes))))
        }
        Err(error) if error.kind()==std::io::ErrorKind::NotFound=>Ok(None),
        Err(error)=>Err(error.to_string()),
    }
}
fn clone_dependencies(source:&Path,target:&Path,paths:&BTreeSet<PathBuf>)
    ->Result<Value,String>{
    if paths.len()>MAX_FILES{return Err("candidate touches too many files".into());}
    let source=source.canonicalize().map_err(|e|e.to_string())?;
    let mut total=0usize;
    let mut baseline=json!({});
    for relative in paths {
        let raw=relative.to_string_lossy().to_string();
        permitted_path(&raw)?;
        let from=source.join(relative);
        // Symlinks at *any* ancestor cannot escape the snapshot through a
        // path that looks relative. A missing target is copied as missing.
        let mut parent=from.parent();
        while let Some(folder)=parent {
            if folder==source {break;}
            if folder.is_symlink(){return Err("candidate parent is a symlink".into());}
            parent=folder.parent();
        }
        let digest=file_digest(&from)?;
        baseline[raw.as_str()]=match &digest{Some(s)=>json!(s),None=>Value::Null};
        let into=target.join(relative);
        if let Some(parent)=into.parent(){
            std::fs::create_dir_all(parent).map_err(|e|e.to_string())?;
        }
        if digest.is_some(){
            let bytes=std::fs::read(&from).map_err(|e|e.to_string())?;
            total=total.saturating_add(bytes.len());
            if total>MAX_TOTAL_BYTES{
                return Err("candidate file snapshot exceeds 8 MiB".into());
            }
            std::fs::write(into,bytes).map_err(|e|e.to_string())?;
        }
    }
    Ok(baseline)
}
/// Recheck the parent workspace immediately before live promotion.
/// This *cannot* replace the normal per-file gateway lock/approval; it
/// prevents stale trials from being intentionally accepted as fresh.
pub(super) fn baseline_matches(workspace:&Path,baseline:&Value)->Result<bool,String>{
    let Some(entries)=baseline.as_object() else{return Err("invalid rollout baseline".into());};
    for (path,digest) in entries {
        let relative=permitted_path(path)?;
        let joined=workspace.join(&relative);
        let mut ancestor=joined.parent();
        while let Some(folder)=ancestor {
            if folder==workspace {break;}
            if folder.is_symlink(){return Err("snapshot parent became a symlink".into());}
            ancestor=folder.parent();
        }
        let actual=file_digest(&joined)?;
        if actual.as_deref()!=digest.as_str(){return Ok(false);}
    }
    Ok(true)
}
fn eligible(variant:&Value,observations:&Value)->bool{
    if variant["legacy"]==true || variant["repairsFailedNode"]!=true{return false;}
    let Some(probes)=variant["probes"].as_array() else{return false};
    if probes.is_empty(){return false;}
    probes.iter().all(|probe|{
        let Some(id)=probe["id"].as_str() else {return false};
        let Some(observed)=observations.get(id) else{return false};
        if observed["status"]!="completed"{return false;}
        let scope=json!({"result":observed["result"]});
        crate::engine::program::predicate_verdict(&probe["predicate"],&scope)
            .ok().is_some_and(|verdict|verdict.is_true())
    })
}
/// Trial a bounded ProgramIR candidate using the *same* native interpreter
/// and dispatcher as a real task, in an independent state/store/workspace.
/// Tool calls are charged against the parent request's existing budget;
/// sandbox auto-approval applies only to five fixed local file tools.
#[allow(clippy::too_many_arguments)]
async fn run_one(
    ctx:&ProgramCtx<'_>,
    variant:&Value,
    checkpoint:&Value,
    budget:&mut super::budget::BudgetLedger,
)->Result<Value,String>{
    let started=Instant::now();
    let candidate_id=variant["id"].as_str().ok_or("candidate has no id")?;
    let nodes=variant["revision"]["nodes"].as_array()
        .cloned().ok_or("candidate lacks executable ProgramIR")?;
    let mut paths=BTreeSet::new();
    let mut visits=0usize;let mut tools=0usize;
    count_and_collect(&nodes,&mut paths,&mut visits,&mut tools)?;
    let Some(posts)=variant["postflights"].as_array() else{
        return Err("candidate has no independent postflight".into());
    };
    if posts.is_empty() || posts.len()>3 {return Err("postflight count outside 1..3".into());}
    for post in posts {
        let tool=post["tool"].as_str().ok_or("invalid postflight tool")?;
        if !matches!(tool,"workspace_read"|"workspace_read_range"){
            return Err("candidate postflight must read the isolated workspace".into());
        }
        paths.insert(permitted_path(post["inputs"]["path"].as_str()
            .ok_or("postflight must have a static path")?)?);
    }
    // Preserve the full request budget. Reserve all trial tool operations
    // before dispatch; failure cannot make speculative calls free.
    let before=budget.clone();
    for _ in 0..tools+posts.len(){
        if !budget.charge_tool(){return Err("candidate rollout tool budget exhausted".into());}
    }
    let area=TrialArea::create()?;
    let workspace=area.workspace();
    let baseline=clone_dependencies(&ctx.state.config.workspace,&workspace,&paths)?;
    let store=crate::db::Store::open(&area.root.join("data/dominion.sqlite"))
        .map_err(|error|error.to_string())?;
    let mut sandbox=ctx.state.clone();
    sandbox.config.workspace=workspace.clone();
    sandbox.config.data_dir=area.root.join("data");
    sandbox.store=store.clone();
    sandbox.bus=crate::events::Bus::new(store.clone());
    sandbox.instance_id=format!("repair-trial-{}",uuid::Uuid::new_v4());
    let mut session=ctx.session.clone();
    session["id"]=json!(ctx.session_id);
    store.put("session",&session).ok_or("sandbox session persistence failed")?;
    store.put("session_cancel",&json!({
        "id":ctx.session_id,"cancelled":false,"generation":ctx.generation
    })).ok_or("sandbox cancellation epoch persistence failed")?;
    store.put("policy",&json!({
        "id":"default","autoApproveAll":false,
        "autoApprove":["workspace_write","workspace_replace","workspace_replace_range"]
    })).ok_or("sandbox scoped tool approval persistence failed")?;
    let run_id=format!("candidate-{candidate_id}-{}",uuid::Uuid::new_v4());
    store.put("execution",&json!({
        "id":run_id,"sessionId":ctx.session_id,
        "status":"running","programCheckpoint":checkpoint
    })).ok_or("sandbox execution persistence failed")?;
    let (tx,_rx)=tokio::sync::mpsc::unbounded_channel::<Value>();
    let sandbox_ctx=ProgramCtx {
        state:&sandbox,tx:&tx,session:&session,
        request:ctx.request,session_id:ctx.session_id,
        execution_id:&run_id,metrics:ctx.metrics,
        generation:ctx.generation,
    };
    let mut base=checkpoint.clone();
    base["programFailure"]=Value::Null;
    base["pendingOperationIds"]=json!([]);
    let frame=execute_nodes(&sandbox_ctx,nodes,
        base["resolvedInputs"].clone(),base,before,0,0,false,true).await?;
    let mut post_results=Vec::new();
    let mut all_pass=!frame.paused && frame.failure.is_none()
        && frame.resolved["lastVerify"]["ok"]!=false;
    let mut op_seq=frame.op_seq;
    let mut scratch=frame.budget;
    if all_pass {
        for post in posts {
            if ctx.state.is_cancelled(ctx.session_id)
                || ctx.state.cancellation_generation(ctx.session_id)!=ctx.generation {
                return Err("candidate rollout was cancelled".into());
            }
            let name=post["tool"].as_str().unwrap_or("");
            let actual=dispatch_observation(
                &sandbox_ctx,name,post["inputs"].clone(),
                "candidate_postflight",0,&mut scratch,&mut op_seq,
            ).await?;
            let Some((result,ok,receipt))=actual else{
                all_pass=false;
                post_results.push(json!({"status":"budget_exhausted"}));
                break;
            };
            let verdict=crate::engine::adaptation::repair_tournament::postflight_verdict(
                post,&result,ok,
            );
            post_results.push(json!({
                "id":post["id"],"receipt":receipt,"verdict":verdict,
            }));
            if verdict!="true"{all_pass=false;break;}
        }
    }
    let ops=store.execution_operations(&run_id)?;
    let failed=ops.iter().any(|op|op["status"]!="completed");
    let changed=paths.iter().filter(|relative|{
        let key=relative.to_string_lossy().to_string();
        file_digest(&workspace.join(relative)).ok().flatten().as_deref()
            != baseline[key.as_str()].as_str()
    }).count();
    // "Success" requires independent observed read-back, at least one
    // changed file, and no hidden native operation failure.
    let verified=all_pass && !failed && changed>0 && post_results.len()==posts.len();
    let receipts=ops.iter().map(|op|json!({
        "tool":op["tool"],"status":op["status"],
        "receiptDigest":crate::engine::memory::stable_id(&[
            op["id"].as_str().unwrap_or(""),
            op["status"].as_str().unwrap_or(""),
            op["toolReceipt"]["requestDigest"].as_str().unwrap_or(""),
        ]),
    })).collect::<Vec<_>>();
    Ok(json!({
        "id":candidate_id,"status":if verified{"verified"}else{"rejected"},
        "verified":verified,"toolCalls":ops.len(),"changedPaths":changed,
        "elapsedMs":started.elapsed().as_millis() as u64,
        "postflights":post_results,"receipts":receipts,"baseline":baseline,
    }))
}
/// Return None only if no preflight-qualified candidate is safe to sandbox.
/// If any are sandboxable, the final choice must be supported by actual
/// verified isolated execution; no guessed success fallback is permitted.
pub(super) async fn evaluate(
    ctx:&ProgramCtx<'_>,
    variants:&[Value],
    observations:&Value,
    checkpoint:&Value,
    budget:&mut super::budget::BudgetLedger,
)->Result<Option<(Value,Value)>,String>{
    let eligible_variants=variants.iter()
        .filter(|v|eligible(v,observations) && sandboxable(v))
        .take(MAX_CANDIDATES).collect::<Vec<_>>();
    if eligible_variants.is_empty(){return Ok(None);}
    let mut results=Vec::new();
    for variant in eligible_variants{
        if ctx.state.is_cancelled(ctx.session_id)
            || ctx.state.cancellation_generation(ctx.session_id)!=ctx.generation {
            return Err("candidate comparison was cancelled".into());
        }
        match run_one(ctx,variant,checkpoint,budget).await {
            Ok(row)=>{results.push(row);}
            Err(error)=>{
                // Ineligible local tools are not equivalent to a tested
                // failure; record the distinction without running them.
                results.push(json!({"id":variant["id"],
                    "verified":false,"status":"ineligible_or_failed",
                    "reason":error.chars().take(180).collect::<String>()}));
            }
        }
    }
    let mut ranked=results.iter().filter(|row|row["verified"]==true)
        .collect::<Vec<_>>();
    ranked.sort_by(|a,b|{
        a["toolCalls"].as_u64().unwrap_or(u64::MAX)
            .cmp(&b["toolCalls"].as_u64().unwrap_or(u64::MAX))
            .then_with(||a["elapsedMs"].as_u64().unwrap_or(u64::MAX)
                .cmp(&b["elapsedMs"].as_u64().unwrap_or(u64::MAX)))
            .then_with(||a["id"].as_str().cmp(&b["id"].as_str()))
    });
    let Some(winner)=ranked.first() else{
        return Err("all isolated candidate executions failed postconditions; live mutation withheld".into());
    };
    let id=winner["id"].as_str().ok_or("winner lacks id")?;
    let selected=variants.iter().find(|v|v["id"]==id)
        .ok_or("verified candidate is missing")?;
    let mut revised=selected["revision"].clone();
    revised["repairPostflights"]=selected["postflights"].clone();
    revised["repairPromotionBaseline"]=winner["baseline"].clone();
    // Source-byte compare-and-write is performed again inside the native
    // workspace gateway under its existing per-file lock, not just here.
    annotate_native_cas(&mut revised,&winner["baseline"])?;
    let audit=json!({
        "schema":"dominion.real-candidate-rollout.2026.v1",
        "selected":id,"selectedVerified":true,
        "comparison":results.iter().map(|r|json!({
            "id":r["id"],"status":r["status"],"verified":r["verified"],
            "toolCalls":r["toolCalls"],"changedPaths":r["changedPaths"],
            "elapsedMs":r["elapsedMs"],"postflights":r["postflights"],
            "receipts":r["receipts"],"reason":r["reason"],
        })).collect::<Vec<_>>(),
        "selectionBasis":"native isolated ProgramIR execution and postcondition verification",
        "isolation":"separate SQLite and bounded workspace; no shell/external effects",
    });
    revised["repairSandboxEvidence"]=audit.clone();
    Ok(Some((revised,audit)))
}

#[cfg(test)]
mod tests{
    use super::*;
    #[tokio::test]
    async fn real_native_interpreter_executes_two_isolated_candidates_and_selects_actual_winner(){
        let root=std::env::temp_dir().join(format!(
            "dominion-real-trial-{}",uuid::Uuid::new_v4()));
        let config=crate::config::Config{
            port:0,data_dir:root.join("data"),web_dir:root.join("web"),
            workspace:root.join("workspace"),
        };
        std::fs::create_dir_all(config.workspace.join("bench")).unwrap();
        std::fs::create_dir_all(&config.data_dir).unwrap();
        let target=config.workspace.join("bench/config.txt");
        std::fs::write(&target,"mode=draft").unwrap();
        let store=crate::db::Store::open(&config.db_path()).unwrap();
        let state=AppState::new(config.clone(),store,
            crate::vault::Vault::open(&config.key_path()));
        let session=json!({"id":"sandbox-test-session","title":"isolated test"});
        state.store.put("session",&session).unwrap();
        let request=TurnRequest::from_value(&json!({
            "input":"turn draft configuration into final","toolsEnabled":true,
            "toolsExecutionEnabled":true,
        })).unwrap();
        let original=json!({"revision":"original-v1","nodes":[
            {"id":"repair_target","type":"call_tool","tool":"workspace_read",
             "inputs":{"path":"bench/config.txt"},"onFailure":"revise"}
        ]});
        let checkpoint=crate::engine::program::new_checkpoint(&original);
        let variant=|id:&str,content:&str|json!({
            "id":id,"repairsFailedNode":true,
            "revision":{"revision":format!("revision-{id}"),"nodes":[
                {"id":"inspect","type":"call_tool","tool":"workspace_read",
                 "inputs":{"path":"bench/config.txt"}},
                {"id":"repair_target","type":"call_tool","tool":"workspace_write",
                 "inputs":{"path":"bench/config.txt","content":content}}
            ]},
            "probes":[{"id":id,
                "predicate":{"type":"equals","from":"result",
                    "path":"/content","value":"mode=draft"}}],
            "postflights":[{"id":format!("post-{id}"),
                "tool":"workspace_read","inputs":{"path":"bench/config.txt"},
                "predicate":{"type":"equals","from":"result",
                    "path":"/content","value":"mode=final"}}],
        });
        let variants=vec![variant("p0","mode=incorrect"),variant("p1","mode=final")];
        let observed=json!({
            "p0":{"status":"completed","result":{"content":"mode=draft"}},
            "p1":{"status":"completed","result":{"content":"mode=draft"}},
        });
        let (tx,_rx)=tokio::sync::mpsc::unbounded_channel::<Value>();
        let metrics=std::sync::Mutex::new(Metrics::default());
        let ctx=ProgramCtx{
            state:&state,tx:&tx,session:&session,request:&request,
            session_id:"sandbox-test-session",execution_id:"real-parent-run",
            metrics:&metrics,generation:0,
        };
        let mut budget=super::budget::BudgetLedger::new(4,24,
            std::time::Duration::from_secs(40));
        let result=evaluate(&ctx,&variants,&observed,&checkpoint,&mut budget)
            .await.unwrap().expect("native local candidates must be executed");
        let (chosen,audit)=result;
        assert_eq!(audit["selected"],"p1","only the actual successful write should win");
        assert_eq!(audit["comparison"][0]["verified"],false);
        assert_eq!(audit["comparison"][1]["verified"],true);
        assert_eq!(chosen["nodes"][1]["inputs"]["expectedSha256"],
            format!("{:x}",Sha256::digest(b"mode=draft")));
        assert_eq!(std::fs::read_to_string(&target).unwrap(),"mode=draft",
            "no speculative candidate may change the owner workspace");
        assert!(state.store.all("execution_operation").is_empty(),
            "speculative receipts must stay in independent trial databases");
        assert!(!baseline_matches(&config.workspace,&json!({
            "bench/config.txt":"intentionally-stale"
        })).unwrap());
        assert!(budget.snapshot()["tools"].as_u64().unwrap()>=6);
        drop(state);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[tokio::test]
    async fn no_successful_local_candidate_prevents_live_mutation(){
        let root=std::env::temp_dir().join(format!(
            "dominion-failed-trial-{}",uuid::Uuid::new_v4()));
        let config=crate::config::Config{
            port:0,data_dir:root.join("data"),web_dir:root.join("web"),
            workspace:root.join("workspace"),
        };
        std::fs::create_dir_all(config.workspace.join("bench")).unwrap();
        std::fs::create_dir_all(&config.data_dir).unwrap();
        std::fs::write(config.workspace.join("bench/config.txt"),"mode=draft").unwrap();
        let store=crate::db::Store::open(&config.db_path()).unwrap();
        let state=AppState::new(config.clone(),store,
            crate::vault::Vault::open(&config.key_path()));
        let session=json!({"id":"failed-trial-session"});
        state.store.put("session",&session).unwrap();
        let request=TurnRequest::from_value(&json!({"input":"repair config"})).unwrap();
        let source=json!({"revision":"initial","nodes":[]});
        let checkpoint=crate::engine::program::new_checkpoint(&source);
        let candidates=vec![json!({
            "id":"p0","repairsFailedNode":true,
            "revision":{"revision":"bad","nodes":[
                {"id":"read","type":"call_tool","tool":"workspace_read",
                 "inputs":{"path":"bench/config.txt"}},
                {"id":"fix","type":"call_tool","tool":"workspace_write",
                 "inputs":{"path":"bench/config.txt","content":"broken"}}
            ]},
            "probes":[{"id":"p0","predicate":{"type":"equals",
                "from":"result","path":"/content","value":"mode=draft"}}],
            "postflights":[{"id":"post","tool":"workspace_read",
                "inputs":{"path":"bench/config.txt"},
                "predicate":{"type":"equals","from":"result",
                    "path":"/content","value":"mode=final"}}]
        })];
        let (tx,_rx)=tokio::sync::mpsc::unbounded_channel::<Value>();
        let metrics=std::sync::Mutex::new(Metrics::default());
        let ctx=ProgramCtx{state:&state,tx:&tx,session:&session,request:&request,
            session_id:"failed-trial-session",execution_id:"real-parent-run",
            metrics:&metrics,generation:0};
        let mut budget=super::budget::BudgetLedger::new(4,12,
            std::time::Duration::from_secs(40));
        let err=evaluate(&ctx,&candidates,&json!({
            "p0":{"status":"completed","result":{"content":"mode=draft"}}
        }),&checkpoint,&mut budget).await.unwrap_err();
        assert!(err.contains("all isolated candidate executions failed"));
        assert_eq!(std::fs::read_to_string(
            config.workspace.join("bench/config.txt")).unwrap(),"mode=draft");
        drop(state);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn candidate_execution_is_opt_in_and_default_runner_is_unchanged(){
        let dir=std::env::temp_dir().join(format!("rollout-toggle-{}",uuid::Uuid::new_v4()));
        let config=crate::config::Config{
            port:0,data_dir:dir.join("data"),web_dir:dir.join("web"),
            workspace:dir.join("workspace"),
        };
        std::fs::create_dir_all(&config.workspace).unwrap();
        let state=AppState::new(config.clone(),
            crate::db::Store::open(&config.db_path()).unwrap(),
            crate::vault::Vault::open(&config.key_path()));
        assert!(!enabled(&state,&json!({"id":"session-1"})));
        assert!(enabled(&state,&json!({"id":"session-2",
            "sdkUiPolicy":{"candidateRollout":true}})));
        assert!(!enabled(&state,&json!({"id":"session-3",
            "sdkUiPolicy":{"candidateRollout":false}})));
        drop(state);
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn rejects_absolute_traversal_and_nonlocal_candidate_paths(){
        assert!(permitted_path("src/main.rs").is_ok());
        assert!(permitted_path("../secret.txt").is_err());
        assert!(permitted_path("/private/foo").is_err());
        assert!(permitted_path(".git/config").is_err());
        assert!(permitted_path("src/../secret").is_err());
        let mut paths=BTreeSet::new();let mut visits=0;let mut calls=0;
        assert!(count_and_collect(&[json!({"type":"call_tool","tool":"shell_exec",
            "inputs":{"command":"echo hello"}})],&mut paths,&mut visits,&mut calls).is_err());
    }
    #[test]
    fn promoted_candidate_carries_atomic_original_version_constraints(){
        let mut revised=json!({"nodes":[
            {"id":"read","type":"call_tool","tool":"workspace_read",
                "inputs":{"path":"src/a.rs"}},
            {"id":"fix","type":"call_tool","tool":"workspace_write",
                "inputs":{"path":"src/a.rs","content":"new"}},
            {"id":"create","type":"call_tool","tool":"workspace_write",
                "inputs":{"path":"src/new.rs","content":"new"}}
        ]});
        let baseline=json!({"src/a.rs":"trusted-sha","src/new.rs":null});
        annotate_native_cas(&mut revised,&baseline).unwrap();
        assert_eq!(revised["nodes"][1]["inputs"]["expectedSha256"],"trusted-sha");
        assert_eq!(revised["nodes"][2]["inputs"]["requireAbsent"],true);
        assert!(!revised["nodes"][2]["inputs"]["expectedSha256"].is_string());
    }
    #[test]
    fn unsupported_external_tool_cannot_be_reported_as_isolated_trial(){
        let candidate=json!({"revision":{"nodes":[
            {"id":"cmd","type":"call_tool","tool":"shell_exec",
             "inputs":{"command":"cargo test"}}
        ]},"postflights":[{"tool":"workspace_read","inputs":{"path":"src/lib.rs"}}]});
        assert!(!sandboxable(&candidate));
    }
    #[test]
    fn independent_file_snapshots_and_stale_parent_detection(){
        let root=std::env::temp_dir().join(format!("candidate-unit-{}",uuid::Uuid::new_v4()));
        let source=root.join("source");let isolated=root.join("trial");
        std::fs::create_dir_all(source.join("src")).unwrap();
        std::fs::create_dir_all(&isolated).unwrap();
        std::fs::write(source.join("src/a.txt"),"original").unwrap();
        let paths=BTreeSet::from([PathBuf::from("src/a.txt"),PathBuf::from("src/new.txt")]);
        let snapshot=clone_dependencies(&source,&isolated,&paths).unwrap();
        assert!(baseline_matches(&source,&snapshot).unwrap());
        std::fs::write(isolated.join("src/a.txt"),"changed in trial").unwrap();
        assert_eq!(std::fs::read_to_string(source.join("src/a.txt")).unwrap(),"original");
        std::fs::write(source.join("src/a.txt"),"concurrently changed").unwrap();
        assert!(!baseline_matches(&source,&snapshot).unwrap());
        std::fs::remove_dir_all(root).unwrap();
    }
}
