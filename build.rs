//! Embed the identity of the Rust binary, not the current checkout at request time.
//! In a Git worktree, the branch tip lives in the shared Git directory. Cargo
//! must watch both the worktree HEAD and the pointed branch ref.
use std::{path::PathBuf,process::Command};

fn manifest_dir()->PathBuf{
    std::env::var_os("CARGO_MANIFEST_DIR").map(PathBuf::from)
        .unwrap_or_else(||std::env::current_dir().unwrap_or_default())
}
fn git(args:&[&str])->Option<String>{
    let output=Command::new("git").current_dir(manifest_dir())
        .args(args).output().ok()?;
    if !output.status.success(){return None;}
    let value=String::from_utf8(output.stdout).ok()?.trim().to_owned();
    (!value.is_empty()).then_some(value)
}
fn safe(value:&str)->bool{
    !value.is_empty()&&!value.contains('\r')&&!value.contains('\n')
}
fn watch_git_path(name:&str){
    let Some(value)=git(&["rev-parse","--git-path",name]) else{return};
    let path=PathBuf::from(value);
    let absolute=if path.is_absolute(){path}else{
        manifest_dir().join(path)
    };
    if absolute.exists(){
        println!("cargo:rerun-if-changed={}",absolute.display());
    }
}

fn main(){
    println!("cargo:rerun-if-changed=build.rs");
    for name in ["DOMINION_RELEASE_COMMIT","DOMINION_RELEASE_BRANCH",
        "DOMINION_RELEASE_DIRTY"]{
        println!("cargo:rerun-if-env-changed={name}");
    }
    // Preserve the explicit release SHA contract used by the official build.
    if let Ok(release)=std::env::var("DOMINION_RELEASE_COMMIT"){
        if safe(&release){
            println!("cargo:rustc-env=DOMINION_BUILD_COMMIT={release}");
            for (source,target) in [
                ("DOMINION_RELEASE_BRANCH","DOMINION_BUILD_BRANCH"),
                ("DOMINION_RELEASE_DIRTY","DOMINION_BUILD_DIRTY")
            ]{
                if let Ok(value)=std::env::var(source){
                    if safe(&value){println!("cargo:rustc-env={target}={value}");}
                }
            }
            println!("cargo:rustc-env=DOMINION_BUILD_IDENTITY_SOURCE=embedded-release");
            return;
        }
    }

    // Ordinary local/watch builds now embed their observed Git revision.
    // Without a Git checkout the commit is unknown, not today's unrelated
    // checkout SHA read by an older running binary.
    watch_git_path("HEAD");
    watch_git_path("packed-refs");
    if let Some(reference)=git(&["symbolic-ref","-q","HEAD"]){
        watch_git_path(&reference);
    }
    let Some(commit)=git(&["rev-parse","--verify","HEAD"]) else{return};
    if commit.len()!=40||!commit.bytes().all(|b|b.is_ascii_hexdigit()){
        return;
    }
    println!("cargo:rustc-env=DOMINION_BUILD_COMMIT={commit}");
    if let Some(branch)=git(&["symbolic-ref","--short","-q","HEAD"]){
        if safe(&branch){println!("cargo:rustc-env=DOMINION_BUILD_BRANCH={branch}");}
    }
    if let Ok(output)=Command::new("git").current_dir(manifest_dir())
        .args(["status","--porcelain","--untracked-files=no"]).output(){
        if output.status.success() {
            println!("cargo:rustc-env=DOMINION_BUILD_DIRTY={}",!output.stdout.is_empty());
        }
    }
    println!("cargo:rustc-env=DOMINION_BUILD_IDENTITY_SOURCE=embedded-build");
}
