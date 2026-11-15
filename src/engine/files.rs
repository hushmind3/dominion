//! Bounded codebase operations for the native agent loop.
//! No model or SDK is permitted to bypass the workspace boundary.
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::VecDeque,
    path::{Component, Path, PathBuf},
};

const MAX_RESULTS: usize = 200;
const MAX_FILES: usize = 350;
const MAX_DEPTH: usize = 8;
const MAX_READ: u64 = 131_072;

fn directory(root: &Path, raw: &str) -> Result<PathBuf, String> {
    if raw.len() > 512 || raw.contains('\0') {
        return Err("invalid directory path".into());
    }
    let base = root.canonicalize().map_err(|error| error.to_string())?;
    if raw.is_empty() || raw == "." { return Ok(base); }
    let rel = Path::new(raw);
    if rel.is_absolute() || !rel.components()
        .all(|part| matches!(part, Component::Normal(_))) {
        return Err("directory must be relative to the workspace".into());
    }
    let candidate = base.join(rel).canonicalize().map_err(|error| error.to_string())?;
    if !candidate.starts_with(&base) || !candidate.is_dir() {
        return Err("directory escapes workspace or is not a folder".into());
    }
    Ok(candidate)
}

fn collect(root: &Path, start: &str) -> Result<(Vec<PathBuf>, bool), String> {
    let base = root.canonicalize().map_err(|error| error.to_string())?;
    let start = directory(root, start)?;
    let mut queue = VecDeque::from([(start, 0usize)]);
    let mut files = Vec::new();
    let mut truncated = false;
    while let Some((dir, depth)) = queue.pop_front() {
        if depth > MAX_DEPTH { truncated = true; continue; }
        let mut entries = std::fs::read_dir(&dir).map_err(|error| error.to_string())?
            .filter_map(Result::ok).collect::<Vec<_>>();
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let filename = entry.file_name();
            let name = filename.to_string_lossy();
            if matches!(name.as_ref(), ".git" | "node_modules" | "target" | ".dominion" | "dist")
                || name.starts_with(".dominion-stage-") || name.starts_with(".dominion-backup-") {
                continue;
            }
            let kind = entry.file_type().map_err(|error| error.to_string())?;
            if kind.is_symlink() { continue; }
            let path = entry.path();
            if !path.starts_with(&base) { continue; }
            if kind.is_dir() {
                if depth < MAX_DEPTH { queue.push_back((path, depth + 1)); }
                else { truncated = true; }
            } else if kind.is_file() {
                files.push(path);
                if files.len() >= MAX_FILES { truncated = true; break; }
            }
        }
        if files.len() >= MAX_FILES { break; }
    }
    Ok((files, truncated))
}

pub fn list(root: &Path, directory_path: &str) -> Result<Value, String> {
    let base = root.canonicalize().map_err(|error| error.to_string())?;
    let (files, truncated) = collect(root, directory_path)?;
    let entries = files.into_iter().take(MAX_RESULTS).filter_map(|path| {
        path.strip_prefix(&base).ok()
            .map(|part| part.to_string_lossy().to_string())
    }).collect::<Vec<_>>();
    Ok(json!({"files":entries,"truncated":truncated || entries.len()==MAX_RESULTS}))
}

pub fn search(root: &Path, query: &str, directory_path: &str) -> Result<Value, String> {
    if query.is_empty() || query.len() > 256 {
        return Err("search text must contain 1 to 256 bytes".into());
    }
    let base = root.canonicalize().map_err(|error| error.to_string())?;
    let (files, truncated) = collect(root, directory_path)?;
    let mut hits = Vec::new();
    let mut scanned = 0usize;
    for path in files {
        let meta = match std::fs::metadata(&path) { Ok(meta) => meta, Err(_) => continue };
        if meta.len() > MAX_READ { continue; }
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        scanned += 1;
        let relative = path.strip_prefix(&base).unwrap_or(&path).to_string_lossy().to_string();
        for (index, line) in text.lines().enumerate() {
            if line.contains(query) {
                hits.push(json!({
                    "path": relative,
                    "line": index + 1,
                    "preview": line.chars().take(240).collect::<String>()
                }));
                if hits.len() >= 60 { break; }
            }
        }
        if hits.len() >= 60 { break; }
    }
    Ok(json!({"hits":hits,"filesScanned":scanned,"truncated":truncated || hits.len()>=60}))
}

/// Route a single exact replacement through the same staged, permission-
/// preserving and rollback-aware transaction used for multi-file edits.
pub async fn replace(
    root: &Path, path: &str, old: &str, new: &str, expected_sha: Option<&str>
) -> Result<Value, String> {
    if old.is_empty() || old.len() > 65_536 || new.len() > 131_072 {
        return Err("replacement size is invalid".into());
    }
    let batch = replace_many(root, &[json!({
        "path": path, "old": old, "new": new, "sha256": expected_sha
    })]).await?;
    let change = &batch["changes"][0];
    Ok(json!({
        "path": path, "oldSha256": change["oldSha256"],
        "sha256": change["sha256"], "bytes": change["bytes"],
        "verified": true, "change": "exact-replace"
    }))
}

/// A new/overwritten source file is never truncated in place. Prepare and
/// sync a same-directory staging file, verify that the inspected predecessor
/// is unchanged, and restore its original bytes if adoption or readback fails.
/// This is crash-recoverable best-effort single-file adoption, not a
/// cross-process transaction or a sandbox.
pub async fn write(root: &Path, path: &str, content: &str) -> Result<Value, String> {
    use std::io::ErrorKind;
    use tokio::io::AsyncWriteExt;
    if content.len() > 131_072 {
        return Err("file write exceeds 128 KiB".into());
    }
    let file = super::scoped_path(root, path, true)?;
    let prior = match tokio::fs::symlink_metadata(&file).await {
        Ok(meta) => {
            if !meta.file_type().is_file() || meta.file_type().is_symlink() {
                return Err("Source must be a regular file, not a symlink".into());
            }
            Some((tokio::fs::read(&file).await.map_err(|e| e.to_string())?,
                meta.permissions()))
        }
        Err(error) if error.kind() == ErrorKind::NotFound => None,
        Err(error) => return Err(error.to_string())
    };
    let parent = file.parent().ok_or("Missing source parent directory")?;
    let nonce = uuid::Uuid::new_v4().simple().to_string();
    let stage = parent.join(format!(".dominion-stage-{nonce}"));
    let backup = parent.join(format!(".dominion-backup-{nonce}"));
    let staged = async {
        let mut output = tokio::fs::OpenOptions::new()
            .create_new(true).write(true).open(&stage).await
            .map_err(|e| e.to_string())?;
        output.write_all(content.as_bytes()).await.map_err(|e| e.to_string())?;
        output.sync_all().await.map_err(|e| e.to_string())?;
        if let Some((_, permissions)) = &prior {
            tokio::fs::set_permissions(&stage, permissions.clone()).await
                .map_err(|e| e.to_string())?;
        }
        Ok::<(), String>(())
    }.await;
    if let Err(error) = staged {
        let _ = tokio::fs::remove_file(&stage).await;
        return Err(format!("Cannot stage workspace file: {error}"));
    }

    let current = match tokio::fs::read(&file).await {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == ErrorKind::NotFound => None,
        Err(error) => {
            let _ = tokio::fs::remove_file(&stage).await;
            return Err(error.to_string());
        }
    };
    if current.as_deref() != prior.as_ref().map(|(data,_)| data.as_slice()) {
        let _ = tokio::fs::remove_file(&stage).await;
        return Err("Source changed after staging; refusing stale overwrite".into());
    }
    if prior.is_some() {
        if let Err(error) = tokio::fs::rename(&file, &backup).await {
            let _ = tokio::fs::remove_file(&stage).await;
            return Err(format!("Cannot back up existing source: {error}"));
        }
    }
    if let Err(error) = tokio::fs::rename(&stage, &file).await {
        let restore = if prior.is_some() {
            tokio::fs::rename(&backup, &file).await
                .err().map(|e| format!("; original retained in backup: {e}"))
                .unwrap_or_default()
        } else { String::new() };
        let _ = tokio::fs::remove_file(&stage).await;
        return Err(format!("Cannot adopt staged source: {error}{restore}"));
    }
    let observed = tokio::fs::read(&file).await;
    if !matches!(&observed, Ok(bytes) if bytes == content.as_bytes()) {
        let removal_error = tokio::fs::remove_file(&file).await.err()
            .map(|e| format!("; failed to remove invalid source: {e}"))
            .unwrap_or_default();
        let restore_error = if prior.is_some() {
            tokio::fs::rename(&backup, &file).await.err()
                .map(|e| format!("; original retained in backup: {e}"))
                .unwrap_or_default()
        } else { String::new() };
        return Err(format!("Source write readback failed{removal_error}{restore_error}"));
    }
    if prior.is_some() {
        let _ = tokio::fs::remove_file(&backup).await;
    }
    let bytes = observed.map_err(|e| e.to_string())?;
    Ok(json!({
        "path": path, "bytes": bytes.len(),
        "sha256": format!("{:x}", Sha256::digest(&bytes)),
        "previousSha256": prior.as_ref().map(|(data,_)|
            format!("{:x}", Sha256::digest(data))),
        "verified": true, "change": "staged-write"
    }))
}

/// Range-based source reading handles repository files larger than the
/// legacy 64 KiB all-or-nothing read limit. Each response is bounded and
/// identifies the exact source revision by its SHA-256 digest.
pub fn read_lines(root:&Path,raw:&str,start:u64,count:u64)->Result<Value,String>{
    let path=super::scoped_path(root,raw,false)?;
    let metadata=std::fs::metadata(&path).map_err(|e|e.to_string())?;
    if metadata.len()>12*1024*1024{
        return Err("Source file exceeds 12 MiB indexed-read limit".into());
    }
    if !(1..=1_000_000).contains(&start)||!(1..=160).contains(&count){
        return Err("Read range must have startLine 1..1000000, maxLines 1..160".into());
    }
    let bytes=std::fs::read(&path).map_err(|e|e.to_string())?;
    let revision=format!("{:x}",Sha256::digest(&bytes));
    let content=String::from_utf8(bytes).map_err(|_|"Source must be UTF-8")?;
    let start_index=start.saturating_sub(1) as usize;
    let total_lines=content.lines().count();
    let mut output=String::new();
    let mut emitted=0usize;
    for (index,line) in content.lines().enumerate().skip(start_index)
        .take(count as usize){
        // Huge/minified lines must still advance the page cursor. Preserve
        // UTF-8 boundaries and report truncation rather than getting stuck
        // forever at the same startLine.
        let mut boundary=12_000.min(line.len());
        while !line.is_char_boundary(boundary){boundary-=1;}
        let snippet=&line[..boundary];
        let suffix=if boundary<line.len(){" [… line clipped …]"}else{""};
        let next=format!("{}: {}{}\n",index+1,snippet,suffix);
        if output.len().saturating_add(next.len())>20_000{break;}
        output.push_str(&next);
        emitted+=1;
    }
    let next=(start_index+emitted<total_lines).then_some(start+emitted as u64);
    Ok(json!({"path":raw,"startLine":start,"returnedLines":emitted,
        "totalLines":total_lines,"nextLine":next,
        "revisionSha256":revision,"content":output,
        "truncated":next.is_some()}))
}

#[derive(Debug)]
struct StagedEdit {
    name:String,
    file:PathBuf,
    stage:PathBuf,
    backup:PathBuf,
    original:Vec<u8>,
    updated:Vec<u8>,
    old_sha:String,
    new_sha:String,
}
/// Verify all source revisions and exact hunks BEFORE changing any file;
/// stage every new body beside its original, then atomically replace the
/// individual files. On a mid-commit failure, restore previous backup files.
/// The entire batch is one tool intent/receipt in the persistent event log.
pub async fn replace_many(root:&Path,edits:&[Value])->Result<Value,String>{
    use std::collections::HashSet;
    if edits.is_empty()||edits.len()>8{
        return Err("Multi-file edit requires 1..8 separate exact replacements".into());
    }
    let mut prepared=Vec::<StagedEdit>::new();
    let mut known=HashSet::new();
    let batch=uuid::Uuid::new_v4().simple().to_string();
    for (index,entry) in edits.iter().enumerate(){
        let name=entry["path"].as_str().ok_or("Each edit requires a source path")?;
        let old=entry["old"].as_str().filter(|v|!v.is_empty())
            .ok_or("Each edit requires non-empty prior source text")?;
        let new=entry["new"].as_str().ok_or("Each edit requires replacement text")?;
        if !known.insert(name.to_string()){
            return Err("One file may appear only once per atomic edit".into());
        }
        let file=super::scoped_path(root,name,false)?;
        if std::fs::symlink_metadata(&file).map_err(|e|e.to_string())?
            .file_type().is_symlink(){
            return Err("Atomic edits refuse symlink source files".into());
        }
        let original=tokio::fs::read(&file).await.map_err(|e|e.to_string())?;
        if original.len()>262_144{return Err("Atomic edit file too large".into());}
        let old_sha=format!("{:x}",Sha256::digest(&original));
        if entry["sha256"].as_str().is_some_and(|sha|sha!=old_sha){
            return Err(format!("Stale source digest for {name}; re-read before editing"));
        }
        let text=std::str::from_utf8(&original)
            .map_err(|_|"Atomic edit requires UTF-8 source")?;
        if text.matches(old).count()!=1{
            return Err(format!("Atomic edit for {name} needs exactly one matching original span"));
        }
        let updated=text.replacen(old,new,1).into_bytes();
        if updated.len()>524_288{
            return Err("Atomic edit output exceeds 512 KiB".into());
        }
        let new_sha=format!("{:x}",Sha256::digest(&updated));
        let parent=file.parent().ok_or("Missing file directory")?;
        let stage=parent.join(format!(".dominion-stage-{batch}-{index}"));
        let backup=parent.join(format!(".dominion-backup-{batch}-{index}"));
        prepared.push(StagedEdit{
            name:name.into(),file,stage,backup,original,updated,old_sha,new_sha
        });
    }
    // No source has changed. Stage all outputs using create_new to avoid
    // clobbering unrelated files, and ensure bytes reach storage before swap.
    let mut staged=Vec::<usize>::new();
    for (index,entry) in prepared.iter().enumerate(){
        let result=async{
            use tokio::io::AsyncWriteExt;
            let mut output=tokio::fs::OpenOptions::new()
                .create_new(true).write(true).open(&entry.stage).await
                .map_err(|e|e.to_string())?;
            output.write_all(&entry.updated).await.map_err(|e|e.to_string())?;
            output.sync_all().await.map_err(|e|e.to_string())?;
            // Preserve executable bits and other source permissions.
            let original_permissions=std::fs::metadata(&entry.file)
                .map_err(|e|e.to_string())?.permissions();
            tokio::fs::set_permissions(&entry.stage,original_permissions)
                .await.map_err(|e|e.to_string())
        }.await;
        if let Err(error)=result{
            for i in staged {
                let _=tokio::fs::remove_file(&prepared[i].stage).await;
            }
            let _=tokio::fs::remove_file(&entry.stage).await;
            return Err(format!("Could not stage atomic source changes: {error}"));
        }
        staged.push(index);
    }
    for entry in &prepared {
        let current=match tokio::fs::read(&entry.file).await{
            Ok(bytes)=>bytes,
            Err(error)=>{
                for stage in &prepared{let _=tokio::fs::remove_file(&stage.stage).await;}
                return Err(format!("Cannot recheck prior file before commit: {error}"));
            }
        };
        if current!=entry.original {
            for stage in &prepared{let _=tokio::fs::remove_file(&stage.stage).await;}
            return Err("Source was concurrently modified; no staged edit was adopted".into());
        }
    }
    let mut committed=Vec::<usize>::new();
    let mut failed=None;
    for (index,entry) in prepared.iter().enumerate(){
        if let Err(error)=tokio::fs::rename(&entry.file,&entry.backup).await{
            failed=Some(format!("Cannot preserve original: {error}"));break;
        }
        if let Err(error)=tokio::fs::rename(&entry.stage,&entry.file).await{
            let _=tokio::fs::rename(&entry.backup,&entry.file).await;
            failed=Some(format!("Could not adopt staged file: {error}"));break;
        }
        committed.push(index);
        match tokio::fs::read(&entry.file).await{
            Ok(bytes) if bytes==entry.updated=>{},
            _=>{failed=Some(format!("Atomic edit readback failed for {}",entry.name));break;}
        }
    }
    if let Some(error)=failed{
        let mut rollback_errors=Vec::new();
        for index in committed.into_iter().rev(){
            let item=&prepared[index];
            if let Err(err)=tokio::fs::remove_file(&item.file).await{
                rollback_errors.push(format!("{}: {err}",item.name));continue;
            }
            if let Err(err)=tokio::fs::rename(&item.backup,&item.file).await{
                rollback_errors.push(format!("{}: {err}",item.name));
            }
        }
        for item in &prepared{let _=tokio::fs::remove_file(&item.stage).await;}
        return Err(format!(
            "{error}. Rollback {}. Inspect remaining .dominion-backup files before retry.",
            if rollback_errors.is_empty(){"attempted".into()}
            else{format!("incomplete: {}",rollback_errors.join("; "))}
        ));
    }
    for entry in &prepared{
        let _=tokio::fs::remove_file(&entry.backup).await;
    }
    let changes=prepared.iter().map(|entry|json!({
        "path":entry.name,"oldSha256":entry.old_sha,"sha256":entry.new_sha,
        "bytes":entry.updated.len(),"verified":true
    })).collect::<Vec<_>>();
    Ok(json!({"verified":true,"changes":changes,"fileCount":prepared.len(),
        "change":"staged-exact-multi-file"}))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn search_and_list_keep_file_paths_under_workspace() {
        let temp = std::env::temp_dir().join(format!("dominion-files-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(temp.join("src")).unwrap();
        std::fs::write(temp.join("src").join("main.rs"), "fn main() { println!(\"hello\"); }").unwrap();
        let listing = list(&temp, ".").unwrap();
        assert_eq!(listing["files"][0], "src/main.rs");
        let hits = search(&temp, "println!", ".").unwrap();
        assert_eq!(hits["hits"][0]["path"], "src/main.rs");
        assert!(list(&temp, "../").is_err());
        let _ = std::fs::remove_dir_all(temp);
    }
    #[test]
    fn ranged_source_read_returns_stable_revision_for_large_files(){
        let root=std::env::temp_dir().join(format!("dominion-range-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let text=(1..=9000).map(|line|format!("line-{line}: function implementation\n"))
            .collect::<String>();
        std::fs::write(root.join("big.rs"),text).unwrap();
        let source=read_lines(&root,"big.rs",8900,24).unwrap();
        assert_eq!(source["startLine"],8900);
        assert_eq!(source["returnedLines"],24);
        assert!(source["content"].as_str().unwrap().contains("line-8900"));
        assert_eq!(source["nextLine"],8924);
        assert_eq!(source["revisionSha256"].as_str().unwrap().len(),64);
        assert!(read_lines(&root,"../outside",1,10).is_err());
        std::fs::write(root.join("minified.js"),
            format!("{}\nsecond-line\n","언어🔍".repeat(12000))).unwrap();
        let first=read_lines(&root,"minified.js",1,4).unwrap();
        assert_eq!(first["returnedLines"],2);
        assert!(first["content"].as_str().unwrap().contains("line clipped"));
        assert!(first["content"].as_str().unwrap().contains("second-line"));
        std::fs::remove_dir_all(root).unwrap();
    }
    #[tokio::test]
    async fn multi_file_edits_are_prevalidated_and_readback_verified(){
        let root=std::env::temp_dir().join(format!("dominion-multi-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("a.rs"),"alpha 1\n").unwrap();
        std::fs::write(root.join("b.rs"),"beta 2\n").unwrap();
        let invalid=vec![
            json!({"path":"a.rs","old":"alpha 1","new":"alpha 3"}),
            json!({"path":"b.rs","old":"unknown","new":"beta 4"})
        ];
        assert!(replace_many(&root,&invalid).await.is_err());
        assert_eq!(std::fs::read_to_string(root.join("a.rs")).unwrap(),"alpha 1\n");
        assert_eq!(std::fs::read_to_string(root.join("b.rs")).unwrap(),"beta 2\n");
        let valid=vec![
            json!({"path":"a.rs","old":"alpha 1","new":"alpha 3"}),
            json!({"path":"b.rs","old":"beta 2","new":"beta 4"})
        ];
        let receipt=replace_many(&root,&valid).await.unwrap();
        assert_eq!(receipt["fileCount"],2);
        assert_eq!(receipt["changes"][0]["verified"],true);
        assert_eq!(std::fs::read_to_string(root.join("a.rs")).unwrap(),"alpha 3\n");
        assert_eq!(std::fs::read_to_string(root.join("b.rs")).unwrap(),"beta 4\n");
        assert!(replace_many(&root,&valid).await.is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn standalone_writes_and_replacements_preserve_original_permissions_and_receipts() {
        let root=std::env::temp_dir().join(format!("dominion-safe-write-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let file=root.join("source.sh");
        std::fs::write(&file,"original\n").unwrap();
        #[cfg(unix)] {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&file,std::fs::Permissions::from_mode(0o750)).unwrap();
        }
        let created=write(&root,"source.sh","updated\n").await.unwrap();
        assert_eq!(created["verified"],true);
        assert_eq!(created["change"],"staged-write");
        assert_eq!(std::fs::read_to_string(&file).unwrap(),"updated\n");
        let digest=created["sha256"].as_str().unwrap();
        assert!(replace(&root,"source.sh","updated","broken",Some("stale")).await.is_err());
        assert_eq!(std::fs::read_to_string(&file).unwrap(),"updated\n");
        let fixed=replace(&root,"source.sh","updated","fixed",Some(digest)).await.unwrap();
        assert_eq!(fixed["verified"],true);
        assert_eq!(std::fs::read_to_string(&file).unwrap(),"fixed\n");
        #[cfg(unix)] {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,0o750);
        }
        let created_new=write(&root,"new.rs","fn new_file() {}\n").await.unwrap();
        assert!(created_new["previousSha256"].is_null());
        assert_eq!(std::fs::read_to_string(root.join("new.rs")).unwrap(),
            "fn new_file() {}\n");
        let debris=std::fs::read_dir(&root).unwrap().filter_map(Result::ok)
            .any(|e|e.file_name().to_string_lossy().starts_with(".dominion-"));
        assert!(!debris,"successful edits leave no staging or backup files");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn bounded_replace_refuses_ambiguous_and_stale_updates() {
        let temp = std::env::temp_dir().join(format!("dominion-edit-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp).unwrap();
        std::fs::write(temp.join("example.rs"), "old\nold\n").unwrap();
        assert!(replace(&temp, "example.rs", "old", "new", None).await.is_err());
        std::fs::write(temp.join("example.rs"), "old\n").unwrap();
        assert!(replace(&temp, "example.rs", "old", "new", Some("stale")).await.is_err());
        let receipt = replace(&temp, "example.rs", "old", "new", None).await.unwrap();
        assert_eq!(receipt["verified"], true);
        assert_eq!(std::fs::read_to_string(temp.join("example.rs")).unwrap(), "new\n");
        let _ = std::fs::remove_dir_all(temp);
    }
}
