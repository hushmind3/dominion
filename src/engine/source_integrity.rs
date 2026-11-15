//! Byte-exact source inventory around owner-approved executable checks.
//! Detects false-green tests that replace the source they purported to test.
use sha2::{Digest,Sha256};
use std::{fs,io::Read,path::{Path,PathBuf}};
const MAX_FILES:usize=25_000;
const MAX_SOURCE_BYTES:u64=256*1024*1024;
const EXCLUDED:&[&str]=&[
    ".git",".dominion","node_modules","target","dist","build",
    "coverage",".next",".nuxt",".venv","venv","__pycache__",
    ".cache",".turbo",".yarn",".pnpm-store","test-results","tmp","temp","logs"
];
const EXTENSIONS:&[&str]=&[
    "rs","ts","tsx","js","jsx","mjs","cjs","py","go","java",
    "c","cc","cpp","h","hpp","cs","swift","kt","sh","bash",
    "toml","json","jsonc","yaml","yml","xml","html","css",
    "scss","sql","md","mdx","vue","svelte","proto","snap","txt",
    "lock","graphql","gql","rb","php","ex","exs","erl"
];
fn is_source(path:&Path)->bool{
    let name=path.file_name().and_then(|v|v.to_str()).unwrap_or("");
    matches!(name,"Dockerfile"|"Makefile"|"Justfile"|"CMakeLists.txt"
        |"Gemfile"|"Rakefile"|"Procfile"|".gitignore"|".gitattributes"
        |".npmrc"|".editorconfig"|".env"|"LICENSE"|"Cargo.lock")
        || path.extension().and_then(|v|v.to_str())
            .is_some_and(|ext|EXTENSIONS.contains(&ext))
}
/// Bounded source/configuration inventory. Skip build output, never follow
/// symlinks. This is an integrity check, not an OS sandbox.
pub(super) fn snapshot(root:&Path)->Result<String,String>{
    let base=root.canonicalize().map_err(|e|format!("Source audit root: {e}"))?;
    let mut pending=vec![(base.clone(),0usize)];
    let mut paths=Vec::<PathBuf>::new();
    while let Some((directory,depth))=pending.pop(){
        if depth>24{return Err("Source audit depth exceeded".into());}
        let entries=fs::read_dir(&directory).map_err(|e|
            format!("Source audit cannot read {}: {e}",directory.display()))?;
        for entry in entries{
            let entry=entry.map_err(|e|format!("Source audit entry: {e}"))?;
            let name=entry.file_name();
            let name=name.to_string_lossy();
            let path=entry.path();
            let meta=fs::symlink_metadata(&path).map_err(|e|
                format!("Source audit cannot inspect {}: {e}",path.display()))?;
            if meta.file_type().is_dir(){
                if !EXCLUDED.contains(&name.as_ref()) {pending.push((path,depth+1));}
            }else if (meta.file_type().is_file()||meta.file_type().is_symlink())
                && is_source(&path){
                paths.push(path);
                if paths.len()>MAX_FILES{
                    return Err("Source audit file cap exceeded".into());
                }
            }
        }
    }
    paths.sort();
    let mut digest=Sha256::new();
    let mut total=0u64;
    let mut buffer=[0u8;64*1024];
    for path in paths{
        let relative=path.strip_prefix(&base).map_err(|e|e.to_string())?;
        let encoded=relative.to_string_lossy();
        digest.update((encoded.len() as u64).to_be_bytes());
        digest.update(encoded.as_bytes());
        let meta=fs::symlink_metadata(&path)
            .map_err(|e|format!("Source audit changed during inspection: {e}"))?;
        if meta.file_type().is_symlink(){
            digest.update(b"SYMLINK");
            digest.update(fs::read_link(&path).map_err(|e|e.to_string())?
                .to_string_lossy().as_bytes());
            continue;
        }
        total=total.checked_add(meta.len()).ok_or("Source audit size overflow")?;
        if total>MAX_SOURCE_BYTES{
            return Err("Source audit byte cap exceeded".into());
        }
        digest.update(b"FILE");
        digest.update(meta.len().to_be_bytes());
        let mut file=fs::File::open(&path).map_err(|e|
            format!("Source audit cannot open {}: {e}",path.display()))?;
        loop{
            let n=file.read(&mut buffer).map_err(|e|e.to_string())?;
            if n==0{break;}
            digest.update(&buffer[..n]);
        }
    }
    Ok(format!("{:x}",digest.finalize()))
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn detects_source_changes_but_ignores_generated_dependencies(){
        let dir=std::env::temp_dir().join(format!("dominion-audit-{}",uuid::Uuid::new_v4()));
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::create_dir_all(dir.join("target")).unwrap();
        fs::write(dir.join("src/main.rs"),"fn main(){}").unwrap();
        let baseline=snapshot(&dir).unwrap();
        fs::write(dir.join("target/generated.rs"),"generated").unwrap();
        assert_eq!(snapshot(&dir).unwrap(),baseline);
        fs::write(dir.join("src/main.rs"),"fn main(){println!(\"hi\");}").unwrap();
        assert_ne!(snapshot(&dir).unwrap(),baseline);
        fs::remove_dir_all(&dir).unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn never_follows_symlinks_outside_workspace(){
        use std::os::unix::fs::symlink;
        let dir=std::env::temp_dir().join(format!("dominion-audit-{}",uuid::Uuid::new_v4()));
        let outside=std::env::temp_dir().join(format!("outside-{}.rs",uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(&outside,"before").unwrap();
        symlink(&outside,dir.join("external.rs")).unwrap();
        let baseline=snapshot(&dir).unwrap();
        fs::write(&outside,"after").unwrap();
        assert_eq!(snapshot(&dir).unwrap(),baseline);
        fs::remove_dir_all(&dir).unwrap();
        fs::remove_file(outside).unwrap();
    }
}