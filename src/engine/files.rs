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
            if matches!(name.as_ref(), ".git" | "node_modules" | "target" | ".dominion" | "dist") {
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

pub async fn replace(
    root: &Path, path: &str, old: &str, new: &str, expected_sha: Option<&str>
) -> Result<Value, String> {
    if old.is_empty() || old.len() > 65_536 || new.len() > 131_072 {
        return Err("replacement size is invalid".into());
    }
    let file = super::scoped_path(root, path, false)?;
    let bytes = tokio::fs::read(&file).await.map_err(|error| error.to_string())?;
    if bytes.len() > 262_144 { return Err("file is too large for bounded edit".into()); }
    let before = Sha256::digest(&bytes);
    let before_hex = format!("{before:x}");
    if expected_sha.is_some_and(|expected| expected != before_hex) {
        return Err("file changed since inspection; inspect again before editing".into());
    }
    let content = String::from_utf8(bytes).map_err(|_| "replacement needs UTF-8 file")?;
    let occurrences = content.matches(old).count();
    if occurrences != 1 {
        return Err(format!("exact replacement requires one match, found {occurrences}"));
    }
    let changed = content.replacen(old, new, 1);
    if changed.len() > 524_288 { return Err("result exceeds file edit limit".into()); }
    // Recheck the original before writing: stale edits must not silently win.
    let current = tokio::fs::read(&file).await.map_err(|error| error.to_string())?;
    if Sha256::digest(&current) != before {
        return Err("file changed concurrently; refusing to overwrite".into());
    }
    tokio::fs::write(&file, changed.as_bytes()).await.map_err(|error| error.to_string())?;
    let observed = tokio::fs::read(&file).await.map_err(|error| error.to_string())?;
    let after = Sha256::digest(&observed);
    if observed != changed.as_bytes() {
        return Err("file write verification failed".into());
    }
    Ok(json!({
        "path":path,
        "oldSha256":before_hex,
        "sha256":format!("{after:x}"),
        "bytes":observed.len(),
        "verified":true,
        "change":"exact-replace"
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn search_and_list_keep_file_paths_under_workspace() {
        let temp = std::env::temp_dir().join(format!("dominion-files-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(temp.join("src")).unwrap();
        std::fs::write(temp.join("src").join("main.rs"), "fn main() { println!(\"hello\"); }").unwrap();
        let list = list(&temp, ".").unwrap();
        assert_eq!(list["files"][0], "src/main.rs");
        let hits = search(&temp, "println!", ".").unwrap();
        assert_eq!(hits["hits"][0]["path"], "src/main.rs");
        assert!(list(&temp, "../").is_err());
        let _ = std::fs::remove_dir_all(temp);
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
