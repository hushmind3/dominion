//! Native multi-file patch executor. The model supplies Codex-style Add,
//! Update, Delete and Move sections; Dominion owns parsing, preflight,
//! workspace isolation, staged writes and durable per-file receipts.
//!
//! All operations are preflighted before a mutation. A commit I/O failure may
//! leave a *known partial* outcome; it is reported, never called atomic or
//! automatically replayed. Existing tools, approval and effect journal remain
//! the execution authority.

use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::{
    collections::HashSet,
    fs::{self,OpenOptions},
    io::Write,
    path::{Component,Path,PathBuf},
};

const MAX_PATCH_BYTES:usize=512*1024;
const MAX_OPERATIONS:usize=32;
const MAX_FILE_BYTES:usize=1_500_000;
const MAX_TOTAL_BYTES:usize=8*1024*1024;

#[derive(Debug,Clone,PartialEq,Eq)]
enum Kind{Add,Update,Delete,Move(String)}

#[derive(Debug,Clone)]
struct Hunk{old:Vec<String>,new:Vec<String>,anchor:String,at_end:bool}

#[derive(Debug,Clone)]
struct Operation{
    path:String,
    kind:Kind,
    additions:Vec<String>,
    hunks:Vec<Hunk>,
}

struct Prepared{
    operation:Operation,
    source:PathBuf,
    destination:PathBuf,
    before:Option<Vec<u8>>,
    after:Option<Vec<u8>>,
    stage:Option<PathBuf>,
}

struct StageCleanup(Vec<PathBuf>);
impl Drop for StageCleanup{
    fn drop(&mut self){
        for file in &self.0{let _=fs::remove_file(file);}
    }
}

fn fingerprint(content:&[u8])->String{
    format!("{:x}",Sha256::digest(content))
}

fn valid_relative(path:&str)->Result<(),String>{
    if path.is_empty() || path.len()>260 || path.contains('\0'){
        return Err("Patch path must have 1-260 valid characters".into());
    }
    let path=Path::new(path);
    if path.is_absolute() || path.components().any(|c|
        !matches!(c,Component::Normal(_)|Component::CurDir)){
        return Err("Patch paths must be relative and cannot contain '..'".into());
    }
    if path.components().any(|c|matches!(c,Component::Normal(value)
        if value.to_string_lossy().eq_ignore_ascii_case(".git"))){
        return Err("Git internal metadata is not a patch target".into());
    }
    Ok(())
}

fn parse(input:&str)->Result<Vec<Operation>,String>{
    if input.len()>MAX_PATCH_BYTES{return Err("Patch input exceeds 512 KiB".into());}
    let normalized=input.replace("\r\n","\n");
    let mut lines=normalized.split('\n').collect::<Vec<_>>();
    if lines.last()==Some(&""){lines.pop();}
    if lines.first()!=Some(&"*** Begin Patch")
        || lines.last()!=Some(&"*** End Patch"){
        return Err("Patch must start with *** Begin Patch and end with *** End Patch".into());
    }
    let mut index=1usize;
    let mut result=Vec::<Operation>::new();
    while index+1<lines.len(){
        if result.len()>=MAX_OPERATIONS{return Err("A patch can modify at most 32 files".into());}
        let header=lines[index];
        let (kind,path)=if let Some(path)=header.strip_prefix("*** Add File: "){
            (Kind::Add,path)
        }else if let Some(path)=header.strip_prefix("*** Update File: "){
            (Kind::Update,path)
        }else if let Some(path)=header.strip_prefix("*** Delete File: "){
            (Kind::Delete,path)
        }else{
            return Err(format!("Expected Add/Update/Delete File header at patch line {}",index+1));
        };
        valid_relative(path)?;
        index+=1;
        let mut op=Operation{
            path:path.to_string(),kind,additions:Vec::new(),hunks:Vec::new(),
        };
        if op.kind==Kind::Update && index<lines.len()-1 {
            if let Some(move_to)=lines[index].strip_prefix("*** Move to: "){
                valid_relative(move_to)?;
                if move_to==path{return Err("A move destination must differ from its source".into());}
                op.kind=Kind::Move(move_to.to_string());
                index+=1;
            }
        }
        match &op.kind{
            Kind::Add=>{
                while index<lines.len()-1 && !lines[index].starts_with("*** "){
                    let text=lines[index].strip_prefix('+')
                        .ok_or_else(||format!("Added file line {} must start with +",index+1))?;
                    op.additions.push(text.to_string());index+=1;
                }
            }
            Kind::Delete=>{
                if index<lines.len()-1 && !lines[index].starts_with("*** "){
                    return Err("Delete File cannot have hunk content".into());
                }
            }
            Kind::Update|Kind::Move(_)=>{
                let mut current=None::<Hunk>;
                while index<lines.len()-1 && !lines[index].starts_with("*** "){
                    let line=lines[index];
                    if let Some(anchor)=line.strip_prefix("@@"){
                        if let Some(hunk)=current.take(){op.hunks.push(hunk);}
                        current=Some(Hunk{old:Vec::new(),new:Vec::new(),
                            anchor:anchor.trim().to_string(),at_end:false});
                    }else{
                        let hunk=current.as_mut().ok_or_else(||
                            format!("Update line {} needs an @@ hunk header",index+1))?;
                        let (old,new,text)=match line.chars().next(){
                            Some(' ')=>(true,true,&line[1..]),
                            Some('-')=>(true,false,&line[1..]),
                            Some('+')=>(false,true,&line[1..]),
                            _=>return Err(format!("Invalid patch hunk line {}",index+1)),
                        };
                        if old{hunk.old.push(text.to_string());}
                        if new{hunk.new.push(text.to_string());}
                    }
                    index+=1;
                }
                if index<lines.len()-1 && lines[index]=="*** End of File"{
                    let hunk=current.as_mut().ok_or("End of File requires an active hunk")?;
                    hunk.at_end=true;
                    index+=1;
                }
                if let Some(hunk)=current.take(){op.hunks.push(hunk);}
                if op.hunks.is_empty() && op.kind==Kind::Update{
                    return Err("Update File needs at least one @@ hunk".into());
                }
            }
        }
        result.push(op);
    }
    Ok(result)
}

fn read_existing(path:&Path)->Result<Option<Vec<u8>>,String>{
    match fs::symlink_metadata(path){
        Ok(meta)=>{
            if !meta.file_type().is_file(){
                return Err(format!("Patch target is not a regular file: {}",path.display()));
            }
            if meta.len()>MAX_FILE_BYTES as u64{
                return Err(format!("File exceeds 1.5 MB patch limit: {}",path.display()));
            }
            let bytes=fs::read(path).map_err(|e|e.to_string())?;
            if bytes.len()>MAX_FILE_BYTES{return Err("Source grew beyond patch limit".into());}
            std::str::from_utf8(&bytes).map_err(|_|format!(
                "Binary or non-UTF8 file cannot be patched: {}",path.display()))?;
            Ok(Some(bytes))
        }
        Err(e) if e.kind()==std::io::ErrorKind::NotFound=>Ok(None),
        Err(e)=>Err(e.to_string()),
    }
}

fn apply_hunks(source:&str,hunks:&[Hunk])->Result<String,String>{
    let crlf=source.contains("\r\n");
    let normalized=source.replace("\r\n","\n");
    let ended=normalized.ends_with('\n');
    let mut lines=normalized.split('\n').map(str::to_string).collect::<Vec<_>>();
    if ended{lines.pop();}
    if source.is_empty(){lines.clear();}
    let mut cursor=0usize;
    for (index,hunk) in hunks.iter().enumerate(){
        if hunk.old.is_empty() && !hunk.at_end{
            return Err(format!("Patch hunk {} has no matching context",index+1));
        }
        let mut from=cursor;
        if !hunk.anchor.is_empty(){
            let Some(position)=(cursor..lines.len()).find(|&i|
                lines[i].contains(hunk.anchor.as_str()))
            else{return Err(format!("Patch hunk {} anchor not found",index+1));};
            from=position;
        }
        let candidates=if hunk.old.is_empty(){
            vec![lines.len()]
        }else if hunk.old.len()>lines.len(){
            Vec::new()
        }else{
            (from..=lines.len()-hunk.old.len())
                .filter(|&i|lines[i..i+hunk.old.len()]==hunk.old[..])
                .filter(|&i|!hunk.at_end || i+hunk.old.len()==lines.len())
                .collect::<Vec<_>>()
        };
        if candidates.len()!=1{
            return Err(format!("Patch hunk {} matches {} positions (expected exactly 1)",
                index+1,candidates.len()));
        }
        let at=candidates[0];
        let old_len=hunk.old.len();
        let new_len=hunk.new.len();
        lines.splice(at..at+old_len,hunk.new.clone());
        cursor=at+new_len;
    }
    let mut out=lines.join("\n");
    if ended || (source.is_empty() && !lines.is_empty()){
        out.push('\n');
    }
    Ok(if crlf{out.replace('\n',"\r\n")}else{out})
}

fn prepare(root:&Path,ops:Vec<Operation>)->Result<Vec<Prepared>,String>{
    let mut claimed=HashSet::<PathBuf>::new();
    let mut prepared=Vec::new();
    let mut total=0usize;
    for op in ops {
        let source=super::resolve_workspace(root,&op.path)?;
        let destination=if let Kind::Move(to)=&op.kind{
            super::resolve_workspace(root,to)?
        }else{source.clone()};
        if !claimed.insert(source.clone()) {
            return Err(format!("Patch path is targeted twice: {}",op.path));
        }
        if source!=destination && !claimed.insert(destination.clone()){
            return Err(format!("Patch move destination is targeted twice: {}",destination.display()));
        }
        let before=read_existing(&source)?;
        let destination_before=if source==destination{None}else{read_existing(&destination)?};
        let after=match &op.kind{
            Kind::Add=>{
                if before.is_some(){return Err(format!(
                    "Add File would overwrite an existing file: {}",op.path));}
                Some(format!("{}\n",op.additions.join("\n")).into_bytes())
            }
            Kind::Delete=>{
                if before.is_none(){return Err(format!(
                    "Delete File source does not exist: {}",op.path));}
                None
            }
            Kind::Update|Kind::Move(_)=>{
                let bytes=before.as_ref().ok_or_else(||format!(
                    "Update File source does not exist: {}",op.path))?;
                if destination_before.is_some(){
                    return Err(format!("Move destination already exists: {}",destination.display()));
                }
                let text=std::str::from_utf8(bytes).map_err(|e|e.to_string())?;
                Some(apply_hunks(text,&op.hunks)?.into_bytes())
            }
        };
        total=total.saturating_add(before.as_ref().map_or(0,Vec::len))
            .saturating_add(after.as_ref().map_or(0,Vec::len));
        if total>MAX_TOTAL_BYTES{
            return Err("Total source and destination bytes exceed 8 MB".into());
        }
        if after.as_ref().is_some_and(|a|a.len()>MAX_FILE_BYTES){
            return Err("A generated file exceeds 1.5 MB".into());
        }
        prepared.push(Prepared{operation:op,source,destination,before,after,stage:None});
    }
    Ok(prepared)
}

fn stage_all(prepared:&mut [Prepared])->Result<StageCleanup,String>{
    let mut cleanup=StageCleanup(Vec::new());
    for item in prepared {
        let Some(bytes)=&item.after else{continue;};
        let parent=item.destination.parent().ok_or("Patch file has no parent")?;
        fs::create_dir_all(parent).map_err(|e|e.to_string())?;
        let name=format!(".dominion-patch-{}.tmp",uuid::Uuid::new_v4());
        let stage=parent.join(name);
        let mut file=OpenOptions::new().write(true).create_new(true).open(&stage)
            .map_err(|e|e.to_string())?;
        cleanup.0.push(stage.clone());
        if item.before.is_some(){
            if let Ok(meta)=fs::metadata(&item.source){
                fs::set_permissions(&stage,meta.permissions()).map_err(|e|e.to_string())?;
            }
        }
        file.write_all(bytes).map_err(|e|e.to_string())?;
        file.sync_all().map_err(|e|e.to_string())?;
        item.stage=Some(stage);
    }
    Ok(cleanup)
}

fn still_current(item:&Prepared)->Result<(),String>{
    let current=read_existing(&item.source)?;
    if current.as_deref()!=item.before.as_deref(){
        return Err(format!("Source changed after patch preflight: {}",item.operation.path));
    }
    if item.source!=item.destination && read_existing(&item.destination)?.is_some(){
        return Err(format!("Move destination appeared: {}",item.destination.display()));
    }
    Ok(())
}

fn receipt(path:&Path,action:&str,before:Option<&[u8]>,after:Option<&[u8]>)->Value{
    json!({
        "path":path.to_string_lossy(),
        "action":action,
        "written":true,
        "changed":before!=after,
        "beforeSha256":before.map(fingerprint),
        "postSha256":after.map(fingerprint),
    })
}

fn apply_blocking(root:&Path,input:&str)->Result<Value,String>{
    let operations=parse(input)?;
    if operations.is_empty(){return Ok(json!({
        "status":"completed","written":false,"changed":false,
        "applied":0,"files":[],"noOp":true
    }));}
    let mut prepared=match prepare(root,operations){
        Ok(value)=>value,
        Err(error)=>return Ok(json!({
            "error":error,"preflightBlocked":true,"written":false,
            "applied":0,"files":[],"changed":false,
        })),
    };
    let _cleanup=stage_all(&mut prepared)?;
    // Revalidate ALL targets after staging and before applying the first
    // mutation, so stale preflights have zero committed operations.
    for item in &prepared{
        if let Err(error)=still_current(item){
            return Ok(json!({
                "error":error,"staleVersion":true,"preflightBlocked":true,
                "written":false,"applied":0,"files":[],
            }));
        }
        let source_now=super::resolve_workspace(root,&item.operation.path)?;
        if source_now!=item.source{
            return Ok(json!({"error":"Patch source path changed during preflight",
                "preflightBlocked":true,"applied":0,"files":[]}));
        }
        if let Kind::Move(to)=&item.operation.kind{
            if super::resolve_workspace(root,to)?!=item.destination{
                return Ok(json!({"error":"Patch destination path changed during preflight",
                    "preflightBlocked":true,"applied":0,"files":[]}));
            }
        }
    }
    let mut committed=Vec::<Value>::new();
    let mut completed=0usize;
    let mut failure=None::<String>;
    let mut unknown=false;
    for item in &prepared{
        if let Err(error)=still_current(item){
            failure=Some(error);break;
        }
        let outcome=match &item.operation.kind{
            Kind::Delete=>{
                fs::remove_file(&item.source).map(|_|{
                    committed.push(receipt(&item.source,"delete",
                        item.before.as_deref(),None));
                })
            }
            Kind::Add|Kind::Update=>{
                fs::rename(item.stage.as_ref().expect("staged target"),&item.source).map(|_|{
                    committed.push(receipt(&item.source,if item.operation.kind==Kind::Add{
                        "add"}else{"update"},item.before.as_deref(),item.after.as_deref()));
                })
            }
            Kind::Move(_)=>{
                match fs::rename(item.stage.as_ref().expect("staged move"),&item.destination){
                    Ok(())=>{
                        committed.push(receipt(&item.destination,"move-to",None,item.after.as_deref()));
                        match fs::remove_file(&item.source){
                            Ok(())=>{
                                committed.push(receipt(&item.source,"move-from",
                                    item.before.as_deref(),None));
                                Ok(())
                            }
                            Err(error)=>{
                                unknown=true;
                                Err(error)
                            }
                        }
                    }
                    Err(error)=>Err(error),
                }
            }
        };
        if let Err(error)=outcome{
            failure=Some(format!("Patch commit failed for {}: {error}",item.operation.path));
            // An external failure may occur after a write completed. The
            // receipt list is factual; unknown outcomes require inspection.
            break;
        }
        completed+=1;
    }
    if let Some(error)=failure{
        return Ok(json!({
            "error":error,
            "written":!committed.is_empty(),
            "changed":committed.iter().any(|r|r["changed"]==true),
            "partialApplied":!committed.is_empty(),
            "outcomeUnknown":unknown,
            "applied":completed,
            "notExecuted":prepared.len().saturating_sub(completed),
            "files":committed,
            "status":"partial",
            "remedy":"Re-read affected files before retrying; do not replay a partial patch blindly.",
        }));
    }
    Ok(json!({
        "status":"completed","written":!committed.is_empty(),
        "changed":committed.iter().any(|r|r["changed"]==true),
        "applied":completed,"files":committed,
    }))
}

pub(super) async fn apply(workspace:&Path,input:&str)->Result<Value,String>{
    let workspace=workspace.to_path_buf();
    let patch=input.to_owned();
    tokio::task::spawn_blocking(move||apply_blocking(&workspace,&patch))
        .await.map_err(|error|format!("Patch worker interrupted: {error}"))?
}

#[cfg(test)]
mod tests{
    use super::*;
    fn root()->PathBuf{
        let path=std::env::temp_dir().join(format!("dominion-multipatch-{}",uuid::Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();path
    }
    #[test]
    fn multi_file_add_update_delete_and_move_use_one_preflight(){
        let root=root();
        fs::write(root.join("one.rs"),"fn first() {\n    old();\n}\n").unwrap();
        fs::write(root.join("two.rs"),"obsolete\n").unwrap();
        fs::write(root.join("move.rs"),"pub fn old() {}\n").unwrap();
        let patch="*** Begin Patch\n*** Update File: one.rs\n@@\n fn first() {\n-    old();\n+    new();\n }\n*** Delete File: two.rs\n*** Add File: nested/three.rs\n+pub fn three() {}\n*** Update File: move.rs\n*** Move to: renamed.rs\n@@\n-pub fn old() {}\n+pub fn renamed() {}\n*** End Patch";
        let result=apply_blocking(&root,patch).unwrap();
        assert!(result.get("error").is_none(),"{result}");
        assert_eq!(result["applied"],4);
        assert_eq!(fs::read_to_string(root.join("one.rs")).unwrap(),
            "fn first() {\n    new();\n}\n");
        assert!(!root.join("two.rs").exists());
        assert_eq!(fs::read_to_string(root.join("nested/three.rs")).unwrap(),
            "pub fn three() {}\n");
        assert!(!root.join("move.rs").exists());
        assert_eq!(fs::read_to_string(root.join("renamed.rs")).unwrap(),
            "pub fn renamed() {}\n");
        assert_eq!(result["files"].as_array().unwrap().len(),5);
        let _=fs::remove_dir_all(root);
    }
    #[test]
    fn preflight_failure_leaves_every_file_unchanged(){
        let root=root();
        fs::write(root.join("a.rs"),"fn a() {}\n").unwrap();
        fs::write(root.join("b.rs"),"fn b() {}\n").unwrap();
        let patch="*** Begin Patch\n*** Update File: a.rs\n@@\n-fn a() {}\n+fn changed() {}\n*** Update File: b.rs\n@@\n-not-in-file\n+oops\n*** End Patch";
        let result=apply_blocking(&root,patch).unwrap();
        assert_eq!(result["preflightBlocked"],true);
        assert_eq!(result["applied"],0);
        assert_eq!(fs::read_to_string(root.join("a.rs")).unwrap(),"fn a() {}\n");
        assert_eq!(fs::read_to_string(root.join("b.rs")).unwrap(),"fn b() {}\n");
        let _=fs::remove_dir_all(root);
    }
    #[test]
    fn duplicate_targets_and_path_traversal_are_refused(){
        let root=root();
        let dup="*** Begin Patch\n*** Add File: x.rs\n+first\n*** Add File: x.rs\n+second\n*** End Patch";
        assert!(apply_blocking(&root,dup).unwrap()["preflightBlocked"]==true);
        assert!(!root.join("x.rs").exists());
        let escape="*** Begin Patch\n*** Add File: ../escape.rs\n+bad\n*** End Patch";
        assert!(apply_blocking(&root,escape).is_err());
        let _=fs::remove_dir_all(root);
    }
    #[test]
    fn repeated_context_is_refused_without_any_mutation(){
        let root=root();
        fs::write(root.join("file.rs"),"same\nsame\n").unwrap();
        let patch="*** Begin Patch\n*** Update File: file.rs\n@@\n-same\n+new\n*** End Patch";
        assert_eq!(apply_blocking(&root,patch).unwrap()["preflightBlocked"],true);
        assert_eq!(fs::read_to_string(root.join("file.rs")).unwrap(),"same\nsame\n");
        let _=fs::remove_dir_all(root);
    }
}
