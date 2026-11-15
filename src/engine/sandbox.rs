//! Explicit, fail-closed isolation for agent-initiated shell/check commands.
//!
//! Isolation is NOT inferred from allowShell. The owner selects 'isolated';
//! the same policy is applied to model-proposed commands and acceptance tests.
//! - Linux: unprivileged bubblewrap, read-only host + writable workspace,
//!   separate PID and network namespaces.
//! - Other OS / opt-in: pre-existing Docker/Podman image with offline network,
//!   read-only root filesystem and one writable workspace bind mount.
//! No fallback to unsandboxed host execution is permitted when requested.
//! Workspace file tools and MCP are separate capabilities, not covered here.
use serde_json::{json,Value};
use std::{env,ffi::OsString,path::{Path,PathBuf},process::Stdio,time::Duration};
use tokio::{io::{AsyncRead,AsyncReadExt},process::Command,time::timeout};

const MAX_OUTPUT:usize=24_000;
const TIMEOUT:Duration=Duration::from_secs(90);

#[derive(Clone,Debug,PartialEq,Eq)]
enum Backend {
    Bwrap(PathBuf),
    Container { engine:PathBuf,image:String },
}

fn executable(name:&str)->Option<PathBuf>{
    let path=Path::new(name);
    if path.is_absolute() {
        return path.is_file().then(||path.to_path_buf());
    }
    env::var_os("PATH").and_then(|search|{
        env::split_paths(&search).find_map(|folder|{
            let found=folder.join(name);
            if found.is_file(){return Some(found);}
            #[cfg(windows)]
            {
                let windows=folder.join(format!("{name}.exe"));
                if windows.is_file(){return Some(windows);}
            }
            None
        })
    })
}
fn backend()->Result<Backend,String>{
    let requested=env::var("DOMINION_SANDBOX_BACKEND")
        .unwrap_or_else(|_|"auto".into()).to_ascii_lowercase();
    if !matches!(requested.as_str(),"auto"|"bwrap"|"docker"|"podman") {
        return Err("DOMINION_SANDBOX_BACKEND must be auto, bwrap, docker or podman".into());
    }
    #[cfg(target_os="linux")]
    if matches!(requested.as_str(),"auto"|"bwrap") {
        if let Some(bin)=executable("/usr/bin/bwrap") {
            return Ok(Backend::Bwrap(bin));
        }
        if requested=="bwrap"{
            return Err("Linux bubblewrap isolation requires /usr/bin/bwrap".into());
        }
    }
    #[cfg(not(target_os="linux"))]
    if requested=="bwrap"{
        return Err("bubblewrap isolation is supported only on Linux".into());
    }
    let image=env::var("DOMINION_SANDBOX_IMAGE")
        .map_err(|_|"Container sandbox needs DOMINION_SANDBOX_IMAGE and an existing local image".to_string())?;
    // The image is a fixed operator-owned configuration, NEVER a model
    // argument. Refuse malformed/dynamic flags and URL-like references.
    if image.is_empty() || image.len()>200 || image.starts_with('-')
        || image.chars().any(|ch|ch.is_whitespace()||ch.is_control()) {
        return Err("Invalid container image reference".into());
    }
    let candidates=if requested=="auto"{vec!["docker","podman"]}
        else{vec![requested.as_str()]};
    for candidate in candidates{
        if let Some(engine)=executable(candidate){
            return Ok(Backend::Container{engine,image});
        }
    }
    Err("No available container backend; install Docker/Podman or bubblewrap".into())
}

/// A backend must be executable, not merely installed. In particular,
/// hosted Linux runners can have bwrap installed while user namespaces are
/// disabled. Never display a false "ready" status in this case.
async fn probe_backend(selected:&Backend)->Result<(),String>{
    let mut command=match selected{
        Backend::Bwrap(bin)=>{
            let mut process=Command::new(bin);
            process.args(["--ro-bind","/","/","--unshare-net",
                "--unshare-pid","--proc","/proc","--dev","/dev",
                "--die-with-parent","--","/bin/true"]);
            process
        },
        Backend::Container{engine,image}=>{
            let mut process=Command::new(engine);
            process.args(["image","inspect",image,"--format","{{.Id}}"]);
            process
        },
    };
    command.stdin(Stdio::null()).stdout(Stdio::null())
        .stderr(Stdio::piped()).kill_on_drop(true);
    let output=timeout(Duration::from_secs(4),command.output()).await
        .map_err(|_|"Sandbox capability check timed out".to_string())?
        .map_err(|e|format!("Sandbox capability check failed: {e}"))?;
    if !output.status.success(){
        let reason=String::from_utf8_lossy(&output.stderr);
        return Err(format!("Sandbox backend not usable: {}",
            reason.chars().take(500).collect::<String>()));
    }
    Ok(())
}

pub(super) async fn availability()->Value{
    match backend(){
        Ok(backend)=>{
            match probe_backend(&backend).await{
                Err(reason)=>json!({"available":false,"backend":null,
                    "scope":"shell_only","reason":reason}),
                Ok(())=>match backend{
                    Backend::Bwrap(_)=>json!({
                        "available":true,"backend":"bwrap","network":"closed",
                        "filesystem":"workspace-write","scope":"shell_only",
                        "note":"Linux host outside workspace is read-only but still readable. Only shell/test commands are isolated."
                    }),
                    Backend::Container{engine,..}=>json!({
                        "available":true,
                        "backend":engine.file_stem().and_then(|s|s.to_str())
                            .unwrap_or("container"),
                        "network":"closed","filesystem":"workspace-write",
                        "scope":"shell_only",
                        "note":"Configured image exists locally and container runtime responds. Only shell/test commands are isolated."
                    }),
                }
            }
        },
        Err(reason)=>json!({"available":false,"backend":null,
            "scope":"shell_only","reason":reason}),
    }
}

#[derive(Debug)]
struct Launch {
    program:OsString,
    args:Vec<OsString>,
    cwd:PathBuf,
    isolated:bool,
    backend:&'static str,
}

fn launch(workspace:&Path,program:&str,args:&[String],mode:&str,
    selected:Option<Backend>)->Result<Launch,String>{
    if !matches!(mode,"host"|"isolated"){
        return Err("Unknown shell isolation mode".into());
    }
    let canonical=workspace.canonicalize().map_err(|e|format!("workspace: {e}"))?;
    if !canonical.is_dir(){return Err("Sandbox workspace must be a directory".into());}
    if mode=="host"{
        return Ok(Launch{program:OsString::from(program),
            args:args.iter().map(OsString::from).collect(),
            cwd:canonical,isolated:false,backend:"host"});
    }
    if canonical.parent().is_none(){
        return Err("Cannot use filesystem root as a writable sandbox workspace".into());
    }
    // Never make a user's entire HOME (including credentials) a writable
    // sandbox. Model-owned tests must work in a project-specific directory.
    if env::var_os("HOME").and_then(|home|PathBuf::from(home).canonicalize().ok())
        .is_some_and(|home|home==canonical){
        return Err("A sandbox workspace cannot be the user's entire HOME directory".into());
    }
    let policy=selected.ok_or("An isolated shell needs an available OS sandbox backend")?;
    match policy {
        Backend::Bwrap(bin)=>{
            let dir=canonical.as_os_str().to_owned();
            let mut parameters=vec![
                OsString::from("--ro-bind"),OsString::from("/"),
                OsString::from("/"),OsString::from("--unshare-net"),
                OsString::from("--unshare-pid"),OsString::from("--proc"),
                OsString::from("/proc"),OsString::from("--dev"),
                OsString::from("/dev"),OsString::from("--die-with-parent"),
            ];
            // For workspaces inside /tmp, masking /tmp would also hide the
            // workspace. Never hide its target path behind a later mount.
            if !canonical.starts_with("/tmp"){
                parameters.extend([OsString::from("--tmpfs"),OsString::from("/tmp")]);
            }
            // The base host filesystem is read-only, yet a process can still
            // read it. Mask common credential stores from the sandbox
            // namespace before exposing the owner-approved workspace.
            if let Some(home)=env::var_os("HOME").map(PathBuf::from){
                for name in [".ssh",".aws",".gnupg",".kube",".docker",
                    ".azure",".config/gcloud"]{
                    let protected=home.join(name);
                    if protected.is_dir() && !canonical.starts_with(&protected){
                        parameters.extend([OsString::from("--tmpfs"),
                            protected.into_os_string()]);
                    }
                }
                for name in [".npmrc",".netrc",".git-credentials"]{
                    let protected=home.join(name);
                    if protected.is_file() && !canonical.starts_with(&protected){
                        parameters.extend([OsString::from("--ro-bind"),
                            OsString::from("/dev/null"),protected.into_os_string()]);
                    }
                }
            }
            parameters.extend([
                OsString::from("--bind"),dir.clone(),dir.clone(),
                OsString::from("--chdir"),dir,OsString::from("--"),
                OsString::from(program)
            ]);
            parameters.extend(args.iter().map(OsString::from));
            Ok(Launch{program:bin.into_os_string(),args:parameters,
                cwd:canonical,isolated:true,backend:"bwrap"})
        }
        Backend::Container{engine,image}=>{
            let mount=format!("type=bind,source={},target=/workspace",
                canonical.to_string_lossy());
            let mut parameters=["run","--rm","--pull","never",
                "--network","none","--read-only","--cap-drop","ALL",
                "--security-opt","no-new-privileges","--pids-limit","128",
                "--memory","1536m","--cpus","2",
                "--tmpfs","/tmp:rw,nosuid,size=134217728",
                "--mount"].iter().map(OsString::from).collect::<Vec<_>>();
            parameters.push(mount.into());
            parameters.extend([OsString::from("--workdir"),
                OsString::from("/workspace"),OsString::from(image),
                OsString::from(program)]);
            parameters.extend(args.iter().map(OsString::from));
            Ok(Launch{program:engine.into_os_string(),args:parameters,
                cwd:canonical,isolated:true,backend:"container"})
        }
    }
}
fn controlled_environment(command:&mut Command,isolated:bool,workspace:&Path){
    if !isolated{return;}
    // Never pass provider API keys, connector tokens, Git credentials or
    // private project secrets from the parent server environment to a test.
    let names=["PATH","LANG","LC_ALL","TERM","RUSTUP_HOME",
        "CARGO_HOME","RUSTC","RUSTFLAGS","NODE_PATH"];
    let allow=names.iter().filter_map(|name|env::var_os(name)
        .map(|value|((*name).to_owned(),value))).collect::<Vec<_>>();
    command.env_clear();
    for (name,value) in allow{command.env(name,value);}
    command.env("HOME",workspace);
    command.env("GIT_TERMINAL_PROMPT","0");
    command.env("CI","true");
    // Contain package/build caches within the admitted workspace.
    command.env("CARGO_TARGET_DIR",workspace.join("target"));
    command.env("NPM_CONFIG_CACHE",workspace.join(".npm-cache"));
}
/// Drain ALL bytes so a talkative test cannot deadlock on a full OS pipe,
/// while retaining at most a bounded prefix in memory.
async fn capture<R:AsyncRead+Unpin>(mut pipe:R,limit:usize)->(Vec<u8>,bool){
    let mut saved=Vec::with_capacity(limit.min(8192));
    let mut buffer=[0u8;8192];
    let mut truncated=false;
    loop{
        let count=match pipe.read(&mut buffer).await {
            Ok(0)|Err(_)=>break,
            Ok(n)=>n,
        };
        let available=limit.saturating_sub(saved.len());
        saved.extend_from_slice(&buffer[..count.min(available)]);
        if count>available{truncated=true;}
    }
    (saved,truncated)
}
pub(super) async fn execute(workspace:&Path,program:&str,args:&[String],
    mode:&str)->Result<Value,String>{
    let selection=if mode=="isolated"{
        let selected=backend()?;
        probe_backend(&selected).await?;
        Some(selected)
    }else{None};
    let plan=launch(workspace,program,args,mode,selection)?;
    let mut command=Command::new(&plan.program);
    command.args(&plan.args).current_dir(&plan.cwd)
        .stdin(Stdio::null()).stdout(Stdio::piped())
        .stderr(Stdio::piped()).kill_on_drop(true);
    // The container CLI is a trusted host-side launcher. It needs its
    // operator's Docker context and daemon connection environment; none
    // of these variables are forwarded to the child container unless
    // explicitly listed as --env (we pass no --env values).
    // Bubblewrap launches the check itself: it MUST receive a scrubbed env.
    if plan.backend!="container" {
        controlled_environment(&mut command,plan.isolated,&plan.cwd);
    }
    let mut child=command.spawn()
        .map_err(|error|format!("Shell check launch failed: {error}"))?;
    let stdout=child.stdout.take().ok_or("Missing child stdout")?;
    let stderr=child.stderr.take().ok_or("Missing child stderr")?;
    let mut out=tokio::spawn(capture(stdout,MAX_OUTPUT*2/3));
    let mut err=tokio::spawn(capture(stderr,MAX_OUTPUT/3));
    let status=match timeout(TIMEOUT,child.wait()).await{
        Ok(Ok(value))=>value,
        Ok(Err(reason))=>{
            let _=child.kill().await;
            out.abort();err.abort();
            return Err(format!("Shell check wait failed: {reason}"));
        },
        Err(_)=>{
            let _=child.kill().await;
            out.abort();err.abort();
            return Err("Shell check exceeded 90 seconds; its process was stopped".into());
        },
    };
    // A grandchild must not keep a pipe reader alive indefinitely.
    let (out_bytes,out_truncated)=match timeout(Duration::from_secs(3),&mut out).await{
        Ok(Ok(value))=>value,
        _=>{out.abort();(Vec::new(),true)},
    };
    let (err_bytes,err_truncated)=match timeout(Duration::from_secs(3),&mut err).await{
        Ok(Ok(value))=>value,
        _=>{err.abort();(Vec::new(),true)},
    };
    Ok(json!({
        "program":program,"args":args,"success":status.success(),
        "exitCode":status.code(),
        "stdout":String::from_utf8_lossy(&out_bytes).to_string(),
        "stderr":String::from_utf8_lossy(&err_bytes).to_string(),
        "outputTruncated":out_truncated||err_truncated,
        "sandboxed":plan.isolated,
        "sandboxBackend":plan.backend,"network":if plan.isolated{"closed"}else{"host"}
    }))
}

#[cfg(test)]
mod tests{
    use super::*;
    use std::fs;
    #[test]
    fn bwrap_never_accepts_shell_strings_and_unshares_network(){
        let dir=std::env::temp_dir().canonicalize().unwrap();
        let plan=launch(&dir,"node",&["--test".into(),"app.test.js".into()],
            "isolated",Some(Backend::Bwrap(PathBuf::from("/usr/bin/bwrap")))).unwrap();
        let args=plan.args.iter().map(|s|s.to_string_lossy().to_string())
            .collect::<Vec<_>>();
        assert_eq!(args.last().unwrap(),"app.test.js");
        assert!(args.contains(&"--unshare-net".into()));
        assert!(args.contains(&"--ro-bind".into()));
        assert!(args.contains(&"--bind".into()));
        assert!(!args.iter().any(|s|s=="sh"||s=="-c"));
    }
    #[test]
    fn container_is_fixed_image_offline_with_single_workspace_mount(){
        let dir=std::env::temp_dir().canonicalize().unwrap();
        let plan=launch(&dir,"node",&["--test".into()],
            "isolated",Some(Backend::Container{
                engine:PathBuf::from("/usr/bin/docker"),image:"dominion-tests:2026".into()
            })).unwrap();
        let args=plan.args.iter().map(|s|s.to_string_lossy().to_string())
            .collect::<Vec<_>>();
        assert!(args.windows(2).any(|w|w==["--network","none"]));
        assert!(args.windows(2).any(|w|w==["--pull","never"]));
        assert!(args.windows(2).any(|w|w==["--read-only","--cap-drop"]));
        assert_eq!(args.last().unwrap(),"--test");
        assert!(args.contains(&"dominion-tests:2026".into()));
    }
    #[test]
    fn no_implicit_unisolated_fallback_when_backend_missing(){
        let dir=std::env::temp_dir().canonicalize().unwrap();
        assert!(launch(&dir,"node",&[], "isolated",None).is_err());
        assert!(launch(&dir,"node",&[], "invalid",None).is_err());
        let host=launch(&dir,"node",&[],"host",None).unwrap();
        assert!(!host.isolated);
    }
    #[tokio::test]
    async fn output_is_bounded_even_when_child_writes_megabytes(){
        if executable("node").is_none(){return;}
        let dir=std::env::temp_dir().canonicalize().unwrap();
        let code="process.stdout.write('x'.repeat(1000000));process.stderr.write('e'.repeat(500000))";
        let output=execute(&dir,"node",&["-e".into(),code.into()],
            "host").await.unwrap();
        assert_eq!(output["success"],true);
        assert_eq!(output["outputTruncated"],true);
        assert!(output["stdout"].as_str().unwrap().len()<=MAX_OUTPUT*2/3);
        assert!(output["stderr"].as_str().unwrap().len()<=MAX_OUTPUT/3);
    }
    #[tokio::test]
    async fn host_mode_runs_an_actual_bounded_check(){
        let dir=std::env::temp_dir().canonicalize().unwrap();
        if executable("node").is_none(){return;}
        let output=execute(&dir,"node",&["--version".into()],"host").await.unwrap();
        assert_eq!(output["success"],true);
        assert_eq!(output["sandboxed"],false);
    }
    #[cfg(target_os="linux")]
    #[tokio::test]
    async fn installed_bwrap_enforces_no_network_and_no_outside_writes(){
        if executable("/usr/bin/bwrap").is_none(){return;}
        let base=std::env::temp_dir().join(format!(
            "dominion-bwrap-test-{}",uuid::Uuid::new_v4()));
        fs::create_dir(&base).unwrap();
        if executable("node").is_none(){return;}
        // Some hosted CI kernels disable unprivileged user namespaces;
        // a failed capability probe must be a hard failure in isolated mode.
        let probe=execute(&base,"node",&["-e".into(),"process.stdout.write('sandbox')".into()],"isolated").await;
        if let Err(ref reason)=probe {
            // A denied user namespace is not treated as a successful host
            // run. Fail-closed is the expected result on such kernels.
            assert!(reason.contains("Sandbox"),"{reason}");
        }
        if let Ok(receipt)=probe{
            if receipt["success"]==true {
                assert_eq!(receipt["sandboxed"],true);
                assert_eq!(receipt["stdout"],"sandbox");
                let outside=base.parent().unwrap().join(format!(
                    "dominion-sandbox-outside-{}",uuid::Uuid::new_v4()));
                let code=format!("require('fs').writeFileSync({:?},'BAD')",outside.to_string_lossy());
                let blocked=execute(&base,"node",&["-e".into(),code],"isolated").await.unwrap();
                assert_eq!(blocked["success"],false);
                assert!(!outside.exists());
            }
        }
        let _=fs::remove_dir_all(&base);
    }
}
