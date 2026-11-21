//! Approved cross-device file receipt merge. The local Workspace Patch
//! executor remains the only writer: remote agents cannot directly mutate
//! coordinator files, even when their native Job reports success.
use super::{ToolContext,patch};
use serde_json::{json,Value};
use std::path::{Path,Component};

fn validated_target(workspace:&Path,relative:&str)->Result<std::path::PathBuf,String>{
    if !crate::remote::valid_artifact_path(relative) ||
        Path::new(relative).components().any(|c|!matches!(c,Component::Normal(_))){
        return Err("Remote artifact path is not a safe workspace-relative file".into());
    }
    let root=workspace.canonicalize().map_err(|e|e.to_string())?;
    let target=root.join(relative);
    if target.exists(){
        let meta=std::fs::symlink_metadata(&target).map_err(|e|e.to_string())?;
        if !meta.is_file() ||meta.file_type().is_symlink(){
            return Err("Remote destination must be a regular non-symlink file".into());
        }
        if !target.canonicalize().map_err(|e|e.to_string())?.starts_with(&root){
            return Err("Remote artifact cannot write outside the workspace".into());
        }
    }else{
        let mut parent=target.parent().ok_or("Artifact destination missing parent")?;
        while !parent.exists(){parent=parent.parent().ok_or("Ancestor does not exist")?;}
        if !parent.canonicalize().map_err(|e|e.to_string())?.starts_with(&root){
            return Err("Remote artifact parent escapes workspace".into());
        }
    }
    Ok(target)
}
fn file_lines(source:&str)->Vec<&str>{
    if source.is_empty(){return vec![];}
    let mut parts=source.split('\n').collect::<Vec<_>>();
    if source.ends_with('\n'){parts.pop();}
    parts
}
fn build_patch(workspace:&Path,record:&Value)->Result<String,String>{
    let artifacts=record["artifacts"].as_array().ok_or("Remote receipt has no artifact manifest")?;
    let baselines=record["artifactBases"].as_array().ok_or("Remote task has no frozen baselines")?;
    if artifacts.is_empty() || artifacts.len()>8 || artifacts.len()!=baselines.len(){
        return Err("Remote artifact receipt or source baseline is incomplete".into());
    }
    let mut patch=String::from("*** Begin Patch\n");
    for (index,item) in artifacts.iter().enumerate(){
        let path=item["path"].as_str().ok_or("Artifact has no source path")?;
        if baselines[index]["path"]!=path {
            return Err("Artifact order differs from the approved frozen plan".into());
        }
        let data=item["content"].as_str().ok_or("Artifact is not UTF-8")?;
        if data.len()>crate::remote::MAX_ARTIFACT_BYTES ||
           item["sha256"]!=crate::remote::digest(data.as_bytes()){
            return Err("Remote artifact hash or size mismatch".into());
        }
        let target=validated_target(workspace,path)?;
        let old=if target.exists(){
            let data=std::fs::read(&target).map_err(|e|e.to_string())?;
            if data.len()>1_500_000{return Err("Workspace source exceeds patch engine limits".into());}
            Some(String::from_utf8(data).map_err(|_|"Coordinator source is not UTF-8")?)
        }else{None};
        let previous_sha=old.as_ref().map(|s|crate::remote::digest(s.as_bytes()));
        match (baselines[index]["expectedSha256"].as_str(),previous_sha.as_deref()){
            (None,None)=>{},
            (Some(expected),Some(found)) if expected==found=>{},
            _=>return Err(format!("Workspace file changed since remote job admission: {path}")),
        }
        if old.as_deref()==Some(data){continue;}
        // The reusable Codex-style Patch engine works with text lines;
        // decline newline changes it cannot represent losslessly.
        if data.contains('\r') ||
            old.as_deref().is_some_and(|s|s.contains('\r')) ||
            (!data.is_empty()&&!data.ends_with('\n')) ||
            old.as_deref().is_some_and(|s|!s.is_empty()&&!s.ends_with('\n')){
            return Err(format!("Remote artifact requires stable UTF-8 LF line endings: {path}"));
        }
        if let Some(old)=old{
            patch.push_str(&format!("*** Update File: {path}\n@@\n"));
            for line in file_lines(&old){patch.push('-');patch.push_str(line);patch.push('\n');}
            for line in file_lines(data){patch.push('+');patch.push_str(line);patch.push('\n');}
        }else{
            patch.push_str(&format!("*** Add File: {path}\n"));
            for line in file_lines(data){patch.push('+');patch.push_str(line);patch.push('\n');}
        }
    }
    patch.push_str("*** End Patch");
    if patch.len()>512*1024 {return Err("Remote changes exceed native patch limits".into());}
    Ok(patch)
}
pub(super) fn preview(ctx:&ToolContext<'_>,args:&Value)->Result<Value,String>{
    let team_id=args["teamId"].as_str().ok_or("teamId required")?;
    let task_id=args["remoteTaskId"].as_str().ok_or("remoteTaskId required")?;
    let team=ctx.store.get("managed_team",team_id).ok_or("Team not found")?;
    if team["leadSessionId"]!=ctx.session_id{return Err("Lead session mismatch".into());}
    let source=ctx.store.get("remote_task",task_id).ok_or("Remote task not found")?;
    if source["teamId"]!=team_id || source["status"]!="completed"
        || source["readOnly"]!=false || source["artifactApplyStatus"]!="pending"{
        return Err("Remote Writer result is not awaiting approval".into());
    }
    let patch=build_patch(ctx.workspace,&source)?;
    let files=source["artifacts"].as_array().ok_or("Missing remote files")?
        .iter().map(|item|json!({"path":item["path"],"sha256":item["sha256"]}))
        .collect::<Vec<_>>();
    Ok(json!({"teamId":team_id,"remoteTaskId":task_id,"files":files,
        "reviewDigest":crate::remote::digest(patch.as_bytes()),
        "patch":patch,"status":"pending"}))
}
pub(super) fn reject(ctx:&ToolContext<'_>,args:&Value)->Result<Value,String>{
    let team_id=args["teamId"].as_str().ok_or("teamId required")?;
    let task_id=args["remoteTaskId"].as_str().ok_or("remoteTaskId required")?;
    let team=ctx.store.get("managed_team",team_id).ok_or("Team not found")?;
    if team["leadSessionId"]!=ctx.session_id{return Err("Lead session mismatch".into());}
    let task=ctx.store.get("remote_task",task_id).ok_or("Remote task not found")?;
    if task["teamId"]!=team_id || task["status"]!="completed" || task["readOnly"]!=false{
        return Err("Remote Writer receipt missing or ineligible".into());
    }
    let saved=ctx.store.patch_if("remote_task",task_id,"artifactApplyStatus",
        &json!("pending"),&json!({"artifactApplyStatus":"rejected",
            "artifactRejectedAt":chrono::Utc::now().to_rfc3339()}))
        .ok_or("Remote changes were already processed; inspect current status")?;
    crate::remote::publish_task(ctx.state,&saved);
    Ok(json!({"remoteTaskId":task_id,"teamId":team_id,"status":"rejected"}))
}
pub(super) async fn apply(ctx:&ToolContext<'_>,args:&Value,run_id:&str)->Result<Value,String>{
    apply_with_digest(ctx,args,run_id,None).await
}
pub(super) async fn apply_reviewed(
    ctx:&ToolContext<'_>,args:&Value,run_id:&str,review_digest:&str,
)->Result<Value,String>{
    apply_with_digest(ctx,args,run_id,Some(review_digest)).await
}
async fn apply_with_digest(
    ctx:&ToolContext<'_>,args:&Value,run_id:&str,expected_digest:Option<&str>,
)->Result<Value,String>{
    let team_id=args["teamId"].as_str().filter(|s|!s.is_empty()).ok_or("teamId required")?;
    let task_id=args["remoteTaskId"].as_str().filter(|s|!s.is_empty())
        .ok_or("remoteTaskId required")?;
    let team=ctx.store.get("managed_team",team_id).ok_or("Managed team missing")?;
    if team["leadSessionId"]!=ctx.session_id{
        return Err("Only the owning Lead session can approve a remote artifact merge".into());
    }
    let source=ctx.store.get("remote_task",task_id).ok_or("Remote receipt missing")?;
    if source["teamId"]!=team_id ||source["status"]!="completed" ||source["readOnly"]!=false{
        return Err("A completed remote Writer receipt is required".into());
    }
    if source["artifactApplyStatus"]=="applied"{
        return Ok(json!({"remoteTaskId":task_id,"status":"applied",
            "alreadyApplied":true,"receipt":source["artifactApplyReceipt"]}));
    }
    if source["artifactApplyStatus"]!="pending"{
        return Err("Artifact application is uncertain or was already claimed; review is required".into());
    }
    let changes=build_patch(ctx.workspace,&source)?;
    if expected_digest.is_some_and(|digest|crate::remote::digest(changes.as_bytes())!=digest){
        return Err("Reviewed file diff changed since approval; inspect again".into());
    }
    // Freeze one coordinator workspace ownership transition before the
    // first possible native file effect. A crashed process will not replay.
    let claimed=ctx.store.patch_if("remote_task",task_id,
        "artifactApplyStatus",&json!("pending"),&json!({
            "artifactApplyStatus":"applying","artifactApplyClaimedAt":chrono::Utc::now().to_rfc3339(),
            "artifactApplyRunId":run_id,
        })).ok_or("Remote artifact was claimed concurrently")?;
    crate::remote::publish_task(ctx.state,&claimed);
    let result=patch::apply(ctx.workspace,&changes).await;
    match result{
        Ok(receipt)=>{
            let valid=source["artifacts"].as_array().is_some_and(|items|items.iter().all(|entry|{
                let Some(path)=entry["path"].as_str()else{return false;};
                let Ok(target)=validated_target(ctx.workspace,path)else{return false;};
                std::fs::read(&target).ok().is_some_and(|bytes|
                    crate::remote::digest(&bytes)==entry["sha256"])
            }));
            if !valid {
                let failed=ctx.store.patch_if("remote_task",task_id,"artifactApplyStatus",
                    &json!("applying"),&json!({"artifactApplyStatus":"needs_review",
                        "artifactApplyReceipt":receipt}));
                if let Some(failed)=failed{crate::remote::publish_task(ctx.state,&failed);}
                return Err("Remote patch effect differs from requested SHA-256; inspect workspace".into());
            }
            let saved=ctx.store.patch_if("remote_task",task_id,"artifactApplyStatus",
                &json!("applying"),&json!({
                    "artifactApplyStatus":"applied","artifactApplyReceipt":receipt,
                    "artifactAppliedAt":chrono::Utc::now().to_rfc3339(),
                })).ok_or("Artifact patch applied but receipt CAS failed; inspect workspace")?;
            crate::remote::publish_task(ctx.state,&saved);
            if receipt["changed"]==true{
                if let Ok(workspace)=ctx.workspace.canonicalize(){
                    let key=crate::engine::code_index::index_id(&workspace);
                    if let Some(mut index)=ctx.store.get("code_index",&key){
                        index["dirty"]=json!(true);ctx.store.put("code_index",&index);
                    }
                }
            }
            Ok(json!({"remoteTaskId":task_id,"teamId":team_id,"status":"applied",
                "receipt":receipt,"paths":source["artifactPaths"]}))
        }
        Err(err)=>{
            let failed=ctx.store.patch_if("remote_task",task_id,"artifactApplyStatus",
                &json!("applying"),&json!({"artifactApplyStatus":"needs_review",
                    "artifactApplyError":err.chars().take(500).collect::<String>()}));
            if let Some(failed)=failed{crate::remote::publish_task(ctx.state,&failed);}
            Err(format!("Native patch merge outcome requires inspection: {err}"))
        }
    }
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn artifact_patch_enforces_frozen_base_and_exact_hash(){
        let root=std::env::temp_dir().join(format!("artifact-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("existing.rs"),"old\n").unwrap();
        let data="new\n";
        let task=json!({
            "artifactBases":[
                {"path":"existing.rs","expectedSha256":crate::remote::digest(b"old\n")},
                {"path":"src/new.rs","expectedSha256":null}
            ],
            "artifacts":[
                {"path":"existing.rs","content":data,"sha256":crate::remote::digest(data.as_bytes())},
                {"path":"src/new.rs","content":"fn main() {}\n","sha256":crate::remote::digest(b"fn main() {}\n")}
            ]
        });
        let patch=build_patch(&root,&task).unwrap();
        assert!(patch.contains("*** Update File: existing.rs"));
        assert!(patch.contains("*** Add File: src/new.rs"));
        std::fs::write(root.join("existing.rs"),"competing writer\n").unwrap();
        assert!(build_patch(&root,&task).unwrap_err().contains("changed"));
        let _=std::fs::remove_dir_all(&root);
    }
    #[tokio::test]
    async fn approved_remote_source_applies_once_through_native_patch_engine(){
        use crate::{config::Config,db::Store,vault::Vault,state::AppState};
        let root=std::env::temp_dir().join(format!("remote-merge-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("existing.rs"),"previous\n").unwrap();
        let config=Config{port:0,data_dir:root.clone(),web_dir:root.join("web"),
            workspace:root.clone()};
        let state=AppState::new(config.clone(),Store::open(&config.db_path()).unwrap(),
            Vault::open(&config.key_path()));
        state.store.put("managed_team",&json!({
            "id":"lead-team","leadSessionId":"lead-session"
        })).unwrap();
        let content="updated\n";
        state.store.put("remote_task",&json!({
            "id":"remote-job","teamId":"lead-team",
            "status":"completed","readOnly":false,
            "artifactApplyStatus":"pending",
            "artifactPaths":["existing.rs"],
            "artifactBases":[{"path":"existing.rs",
                "expectedSha256":crate::remote::digest(b"previous\n")}],
            "artifacts":[{"path":"existing.rs","content":content,
                "sha256":crate::remote::digest(content.as_bytes())}]
        })).unwrap();
        let ctx=ToolContext{workspace:&root,store:&state.store,state:&state,
            session_id:"lead-session",agent_id:Some("lead"),
            tools_enabled:true,tools_execution_enabled:true};
        let args=json!({"teamId":"lead-team","remoteTaskId":"remote-job"});
        let result=apply(&ctx,&args,"run-owner-1").await.unwrap();
        assert_eq!(result["status"],"applied");
        assert_eq!(std::fs::read_to_string(root.join("existing.rs")).unwrap(),content);
        assert_eq!(state.store.get("remote_task","remote-job").unwrap()
            ["artifactApplyRunId"],"run-owner-1");
        let replay=apply(&ctx,&args,"run-owner-2").await.unwrap();
        assert_eq!(replay["alreadyApplied"],true);
        drop(ctx);drop(state);let _=std::fs::remove_dir_all(root);
    }
    #[test]
    fn rejects_artifact_path_escape_and_symlink_destinations(){
        assert!(!crate::remote::valid_artifact_path("../outside"));
        assert!(!crate::remote::valid_artifact_path(".git/config"));
        assert!(!crate::remote::valid_artifact_path(".env"));
        assert!(!crate::remote::valid_artifact_path("C:\\\\private\\\\secret"));
    }
}
