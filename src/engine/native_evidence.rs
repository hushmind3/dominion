//! Shared host-owned artifact evidence for native execution and forensic replay.
//! No model, DB, tool dispatch or additional agent loop lives here.
use dominion_sdk::harness::{Action,Criterion,CriterionKind};
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::collections::BTreeMap;

/// Single canonical path policy for SDK acceptance and forensic witnesses.
/// The caller supplies a canonical workspace root, which allows a multi-file
/// witness to reuse one root resolution instead of canonicalizing per file.
pub(crate) fn resolve_canonical_workspace_path(
    root: &std::path::Path,
    raw: &str,
) -> Result<std::path::PathBuf, String> {
    use std::path::{Component, Path};
    if raw.is_empty() || raw.len() > 4096 {
        return Err("Invalid verification path".into());
    }
    let relative = Path::new(raw);
    if !relative.components().all(|part| matches!(part, Component::Normal(_))) {
        return Err("Verification cannot escape the workspace".into());
    }
    let target = root.join(relative);
    let mut ancestor = target.as_path();
    while std::fs::symlink_metadata(ancestor).is_err() {
        ancestor = ancestor.parent().ok_or("No existing verification parent")?;
    }
    let canonical = std::fs::canonicalize(ancestor).map_err(|e| e.to_string())?;
    if !canonical.starts_with(root) {
        return Err("Verification path escapes the workspace through a symlink".into());
    }
    Ok(target)
}

/// Host-sealed, bounded file-state witnesses for the exact acceptance paths.
/// 2026 VeriHarness-style evidence lineage: this records observed bytes, not
/// the model's claim that it edited a file. Never follow an escaping symlink,
/// and never persist the file contents in the job record.
pub(crate) fn artifact_witness(workspace:&std::path::Path,checks:&[Criterion])->Value{
 let root=match std::fs::canonicalize(workspace){
  Ok(root)=>root,
  Err(_)=>return json!({"complete":false,"reason":"workspace_unavailable","entries":{}})
 };
 let mut entries=BTreeMap::<String,Value>::new();
 let mut complete=true;
 let mut remaining_bytes=32usize*1024*1024;
 for c in checks {
  if c.kind==CriterionKind::ToolSucceeded {continue;}
  let Some(name)=c.path.as_deref() else {complete=false;continue;};
  if entries.contains_key(name){continue;}
  let target = match resolve_canonical_workspace_path(&root, name) {
   Ok(path) => path,
   Err(_) => {
    complete = false;
    entries.insert(name.to_string(), json!({"state":"unsafe_path"}));
    continue;
   }
  };
  match std::fs::symlink_metadata(&target){
   Err(e) if e.kind()==std::io::ErrorKind::NotFound=>{
    entries.insert(name.to_string(),json!({"state":"missing"}));
   },
   Err(_)=>{
    complete=false;
    entries.insert(name.to_string(),json!({"state":"unavailable"}));
   },
   Ok(metadata) if !metadata.is_file() || metadata.len()>16*1024*1024 || metadata.len()>remaining_bytes as u64=>{
    complete=false;
    entries.insert(name.to_string(),json!({"state":"unsupported_artifact"}));
   },
   Ok(metadata)=>{
    match std::fs::read(&target){
     Ok(data) if data.len()<=16*1024*1024=>{
      remaining_bytes=remaining_bytes.saturating_sub(data.len());
      entries.insert(name.to_string(),json!({"state":"file",
       "bytes":data.len(),"sha256":format!("{:x}",Sha256::digest(&data))}));
     },
     _=>{
      complete=false;
      entries.insert(name.to_string(),json!({"state":"unavailable"}));
     }
    }
   }
  }
 }
 let verdicts=checks.iter().map(|check|{
  let verdict=match check.kind {
   CriterionKind::ToolSucceeded=>Value::Null,
   CriterionKind::FileExists|CriterionKind::FileAbsent|
   CriterionKind::FileContains|CriterionKind::FileSha256=>{
    let entry=check.path.as_deref().and_then(|p|entries.get(p));
    match entry.and_then(|v|v["state"].as_str()){
     Some("missing")=>json!(check.kind==CriterionKind::FileAbsent),
     Some("file")=>{
      let yes=match check.kind{
       CriterionKind::FileExists=>Some(true),
       CriterionKind::FileAbsent=>Some(false),
       CriterionKind::FileSha256=>check.contains.as_deref().map(|v|
        entry.is_some_and(|e|e["sha256"].as_str()
         .is_some_and(|actual|actual.eq_ignore_ascii_case(v)))),
       // Content is not persisted. The kernel's independent FileContains
       // acceptance check is still required; don't invent its verdict.
       CriterionKind::FileContains=>None,
       CriterionKind::ToolSucceeded=>None
      };
      yes.map(Value::Bool).unwrap_or(Value::Null)
     },
     _=>Value::Null,
    }
   }
  };
  verdict
 }).collect::<Vec<_>>();
 json!({"complete":complete,"scope":"acceptance_artifact_paths_only",
  "workspaceRootDigest":digest(&json!(root.to_string_lossy().to_string())),
  "criteriaDigest":digest(&json!(checks)),
  "entries":entries,"directVerdicts":verdicts})
}

/// Capture one first-party workspace effect's pre/post image without
/// reading raw content into the journal. Opaque shell/MCP/browser effects
/// intentionally return null: their affected paths cannot be fabricated.
pub(crate) fn action_witness(
 workspace:&std::path::Path,action:&Action,
)->Value {
 if !matches!(action.tool.as_str(),
  "workspace_write"|"workspace_replace"|"workspace_replace_range") {
  return Value::Null
 }
 let Some(path)=action.arguments["path"].as_str() else{return Value::Null};
 let witness=artifact_witness(workspace,&[Criterion{
  kind:CriterionKind::FileExists,
  path:Some(path.to_owned()),contains:None,tool:None,
 }]);
 json!({"path":path,"complete":witness["complete"],
  "state":witness["entries"][path],
  "hostObservation":true,
  "exclusiveWriter":false})
}

pub(crate) fn digest(v:&Value)->String{
 format!("{:x}",Sha256::digest(v.to_string().as_bytes()))
}
