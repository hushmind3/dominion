use crate::state::AppState;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const MAX_SKILL_METADATA_BYTES: usize = 64 * 1024;
const MAX_CATALOG_ENTRIES: usize = 20_000;
const MAX_CATALOG_SKILLS: usize = 512;
const MAX_SKILL_TREE_ENTRIES: usize = 4_096;
const MAX_SKILL_TREE_DEPTH: usize = 12;
const MAX_SKILL_TREE_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Default)]
struct CatalogBudget {
    entries: usize,
    skills: usize,
    exhausted: bool,
}

#[derive(Default)]
struct CopyBudget {
    entries: usize,
    bytes: u64,
}

fn read_skill_prefix(path: &Path) -> Option<String> {
    use std::io::Read;

    let file = std::fs::File::open(path).ok()?;
    let metadata = file.metadata().ok()?;
    if !metadata.is_file() {
        return None;
    }
    let mut bytes = Vec::with_capacity(MAX_SKILL_METADATA_BYTES + 1);
    file.take((MAX_SKILL_METADATA_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .ok()?;
    let truncated = bytes.len() > MAX_SKILL_METADATA_BYTES;
    bytes.truncate(MAX_SKILL_METADATA_BYTES);
    match String::from_utf8(bytes) {
        Ok(text) => Some(text),
        Err(error) if truncated && error.utf8_error().error_len().is_none() => {
            let valid = error.utf8_error().valid_up_to();
            String::from_utf8(error.into_bytes()[..valid].to_vec()).ok()
        }
        Err(_) => None,
    }
}

/// Skill catalog: scans SKILL.md trees and installs a copy into the data dir.
/// No Node process is involved; this is plain Rust filesystem work.
pub fn roots(state: &AppState) -> Vec<(String, PathBuf)> {
    let home = std::env::var("HOME").unwrap_or_default();
    vec![
        ("DOMINION".to_string(), state.config.data_dir.join("skills")),
        (
            "워크스페이스".to_string(),
            state.config.workspace.join("skills"),
        ),
        (
            "DOMINION 사용자".to_string(),
            PathBuf::from(&home).join(".dominion/skills"),
        ),
        (
            "플러그인".to_string(),
            state.config.data_dir.join("plugins"),
        ),
    ]
}

fn id_for(path: &Path) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in path.to_string_lossy().as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn walk(dir: &Path, depth: usize, budget: &mut CatalogBudget, found: &mut Vec<PathBuf>) {
    if depth > 5 || budget.exhausted {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if budget.entries >= MAX_CATALOG_ENTRIES || budget.skills >= MAX_CATALOG_SKILLS {
            budget.exhausted = true;
            return;
        }
        budget.entries += 1;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name == "node_modules" || name == ".git" {
            continue;
        }
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            walk(&path, depth + 1, budget, found);
        } else if name == "SKILL.md" {
            found.push(path);
            budget.skills += 1;
        }
    }
}

fn frontmatter(text: &str) -> (String, String) {
    let mut name = String::new();
    let mut description = String::new();
    let mut lines = text.lines();
    if let Some(first) = lines.next() {
        if first.trim() == "---" {
            for line in lines {
                let line = line.trim();
                if line == "---" {
                    break;
                }
                if let Some(value) = line.strip_prefix("name:") {
                    name = value.trim().trim_matches('"').to_string();
                } else if let Some(value) = line.strip_prefix("description:") {
                    description = value.trim().trim_matches('"').to_string();
                }
            }
        }
    }
    (name, description)
}

fn item(path: &Path, source: &str) -> Option<Value> {
    let text = read_skill_prefix(path)?;
    let (mut name, description) = frontmatter(&text);
    if name.is_empty() {
        name = path
            .parent()
            .and_then(|parent| parent.file_name())
            .map(|value| value.to_string_lossy().to_string())
            .unwrap_or_else(|| "Skill".into());
    }
    Some(json!({
        "id": id_for(path),
        "name": name,
        "description": description,
        "source": source,
        "path": path.to_string_lossy(),
        "dir": path.parent().map(|parent| parent.to_string_lossy().to_string()),
    }))
}

pub fn catalog(state: &AppState) -> Value {
    let mut items = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut budget = CatalogBudget::default();
    for (source, root) in roots(state) {
        if !root.exists() || budget.exhausted {
            continue;
        }
        let mut found = Vec::new();
        walk(&root, 0, &mut budget, &mut found);
        for path in found {
            if let Some(item) = item(&path, &source) {
                if let Some(id) = item.get("id").and_then(|value| value.as_str()) {
                    if seen.insert(id.to_string()) {
                        items.push(item);
                    }
                }
            }
        }
    }
    json!({
        "items": items,
        "total": items.len(),
        "truncated": budget.exhausted,
        "updatedAt": chrono::Utc::now().to_rfc3339(),
        "roots": roots(state).into_iter().map(|(source, path)| json!({
            "source": source,
            "path": path.to_string_lossy(),
        })).collect::<Vec<_>>(),
    })
}

fn copy_dir(from: &Path, to: &Path, depth: usize, budget: &mut CopyBudget) -> std::io::Result<()> {
    if depth > MAX_SKILL_TREE_DEPTH {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "skill asset directory nesting exceeds the limit",
        ));
    }
    let original=std::fs::canonicalize(from)?;
    if let Ok(destination)=std::fs::canonicalize(to) {
        if destination==original {return Ok(());}
        if destination.starts_with(&original) {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput,
                "Cannot recursively install a skill inside itself"));
        }
    } else if to.parent().and_then(|parent|parent.canonicalize().ok())
        .is_some_and(|parent|parent.starts_with(&original)) {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput,
            "Cannot install a skill into its own descendant"));
    }
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        budget.entries = budget.entries.saturating_add(1);
        if budget.entries > MAX_SKILL_TREE_ENTRIES {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "skill asset tree exceeds the entry limit",
            ));
        }
        let target = to.join(entry.file_name());
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            copy_dir(&entry.path(), &target, depth + 1, budget)?;
        } else if file_type.is_file() {
            use std::io::Read;
            let remaining = MAX_SKILL_TREE_BYTES.saturating_sub(budget.bytes);
            let mut source = std::fs::File::open(entry.path())?;
            if source.metadata()?.len() > remaining {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "skill asset tree exceeds the 64 MiB limit",
                ));
            }
            let mut destination = std::fs::File::create(target)?;
            let copied = std::io::copy(
                &mut source.by_ref().take(remaining.saturating_add(1)),
                &mut destination,
            )?;
            if copied > remaining {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "skill asset tree exceeds the 64 MiB limit",
                ));
            }
            budget.bytes = budget.bytes.saturating_add(copied);
        } else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "skill assets cannot contain symbolic links or special files",
            ));
        }
    }
    Ok(())
}

fn install(state: &AppState, source_dir: &Path) -> Result<Value, String> {
    let skill_file = source_dir.join("SKILL.md");
    let text = read_skill_prefix(&skill_file)
        .ok_or_else(|| "SKILL.md를 읽을 수 없습니다.".to_string())?;
    let (mut name, description) = frontmatter(&text);
    if name.is_empty() {
        name = source_dir
            .file_name()
            .map(|value| value.to_string_lossy().to_string())
            .unwrap_or_else(|| "Skill".into());
    }
    let slug: String = name
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect();
    let target = state.config.data_dir.join("skills").join(&slug);
    copy_dir(source_dir, &target, 0, &mut CopyBudget::default())
        .map_err(|error| format!("스킬 복사 실패: {error}"))?;

    let record = state.store.put(
        "skill",
        &json!({
            "name": name,
            "description": description,
            "instructions": text,
            "enabled": false,
            "status": "draft",
            "source": "catalog",
            "path": target.to_string_lossy(),
        }),
    );
    if let Some(record) = &record {
        state.bus.record("skill", record);
    }
    record.ok_or_else(|| "스킬을 저장할 수 없습니다.".into())
}

pub fn import(state: &AppState, id: &str) -> Result<Value, String> {
    let catalog = catalog(state);
    let item = catalog
        .get("items")
        .and_then(|value| value.as_array())
        .and_then(|items| {
            items
                .iter()
                .find(|item| item.get("id").and_then(|value| value.as_str()) == Some(id))
        })
        .ok_or_else(|| "카탈로그에서 스킬을 찾을 수 없습니다.".to_string())?;
    let dir = item
        .get("dir")
        .and_then(|value| value.as_str())
        .ok_or("스킬 경로가 없습니다.")?;
    install(state, Path::new(dir))
}

struct CloneTemp(PathBuf);

impl Drop for CloneTemp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub async fn import_repo(state: &AppState, repo: &str) -> Result<Value, String> {
    let repo = repo.trim();
    if !(repo.starts_with("https://") || repo.starts_with("git@")) {
        return Err("https:// 또는 git@ 저장소 URL만 지원합니다.".into());
    }
    let temp = std::env::temp_dir().join(format!("dominion-skills-{}", uuid::Uuid::new_v4()));
    let _cleanup = CloneTemp(temp.clone());
    let mut command = tokio::process::Command::new("git");
    command.args(["clone", "--depth", "1", repo]).arg(&temp);
    let output = crate::integrations::local_tools::run_command(
        &mut command, std::time::Duration::from_secs(180)).await
        .map_err(|error| format!("저장소 복제 실패: {error}"))?;
    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr);
        return Err(message.chars().take(800).collect::<String>());
    }

    let mut found = Vec::new();
    let mut budget = CatalogBudget::default();
    walk(&temp, 0, &mut budget, &mut found);
    let mut added = 0;
    let mut names = Vec::new();
    for file in found {
        if let Some(dir) = file.parent() {
            if install(state, dir).is_ok() {
                added += 1;
                names.push(
                    dir.file_name()
                        .map(|value| value.to_string_lossy().to_string())
                        .unwrap_or_default(),
                );
            }
        }
    }
    if added == 0 {
        return Err("저장소에서 SKILL.md를 찾지 못했습니다.".into());
    }
    Ok(json!({ "repo": repo, "added": added, "skills": names }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::db::Store;
    use crate::vault::Vault;

    fn temp_state() -> (AppState, PathBuf) {
        let dir = std::env::temp_dir().join(format!("dominion-skills-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let store = Store::open(&config.db_path()).unwrap();
        let vault = Vault::open(&config.key_path());
        (AppState::new(config, store, vault), dir)
    }

    #[test]
    fn copying_into_the_same_physical_skill_folder_never_overwrites_itself() {
        let dir=std::env::temp_dir().join(format!("dominion-skill-self-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("SKILL.md"),"original skill").unwrap();
        copy_dir(&dir,&dir,0,&mut CopyBudget::default()).expect("same-directory install is a no-op");
        assert_eq!(std::fs::read_to_string(dir.join("SKILL.md")).unwrap(),"original skill");
        assert!(copy_dir(&dir,&dir.join("nested"),0,&mut CopyBudget::default()).is_err(),
            "recursive destination must not create unbounded directories");
        let _=std::fs::remove_dir_all(dir);
    }

    #[test]
    fn scans_and_imports_a_skill() {
        let (state, dir) = temp_state();
        let skill_dir = dir.join("skills").join("reviewer");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: Reviewer\ndescription: Reviews diffs\n---\nBe strict.\n",
        )
        .unwrap();

        let catalog = catalog(&state);
        let items = catalog["items"].as_array().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["name"], "Reviewer");
        let id = items[0]["id"].as_str().unwrap().to_string();

        let installed = import(&state, &id).expect("import");
        assert_eq!(installed["name"], "Reviewer");
        assert!(dir.join("skills/Reviewer/SKILL.md").exists());

        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn rejects_non_http_repo() {
        let (state, dir) = temp_state();
        assert!(import_repo(&state, "file:///etc/passwd").await.is_err());
        let _ = std::fs::remove_dir_all(dir);
    }
}
