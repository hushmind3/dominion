//! Download a private Node LTS runtime, verify its official SHA-256, then install
//! the pinned upstream SDK. No global npm packages or user profiles are changed.
use std::{path::{Path,PathBuf},process::Stdio,time::Duration};
use serde_json::Value;
use sha2::{Digest,Sha256};
use tokio::process::Command;
pub fn node_path(root:&Path)->PathBuf{
    if cfg!(windows){root.join("node/node.exe")}else{root.join("node/bin/node")}
}
async fn download(url:&str,max:usize)->Result<Vec<u8>,String>{
    let client=reqwest::Client::builder().timeout(Duration::from_secs(180)).redirect(reqwest::redirect::Policy::none()).build().map_err(|e|e.to_string())?;
    let mut response=client.get(url).send().await.map_err(|e|e.to_string())?.error_for_status().map_err(|e|e.to_string())?;
    let mut bytes=Vec::new();
    while let Some(chunk)=response.chunk().await.map_err(|e|e.to_string())?{
        if bytes.len()+chunk.len()>max{return Err("실행 환경 다운로드 크기를 초과했습니다.".into())}bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}
pub async fn node(root:&Path)->Result<(),String>{
    if node_path(root).is_file(){return Ok(())}
    tokio::fs::create_dir_all(root).await.map_err(|e|e.to_string())?;
    let versions:Value=serde_json::from_slice(&download("https://nodejs.org/dist/index.json",4*1024*1024).await?).map_err(|e|e.to_string())?;
    let version=versions.as_array().into_iter().flatten().filter_map(|v|v["version"].as_str()).find(|v|v.starts_with("v24.")).ok_or("지원되는 Node LTS 버전을 찾지 못했습니다.")?;
    if !version.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'.'){return Err("Invalid Node release".into())}
    let os=match std::env::consts::OS{"macos"=>"darwin","linux"=>"linux","windows"=>"win",_=>return Err("지원되지 않는 실행 환경입니다.".into())};
    let arch=match std::env::consts::ARCH{"aarch64"=>"arm64","x86_64"=>"x64",_=>return Err("지원되지 않는 CPU입니다.".into())};
    let folder=format!("node-{version}-{os}-{arch}");let extension=if cfg!(windows){"zip"}else{"tar.gz"};
    let name=format!("{folder}.{extension}");let base=format!("https://nodejs.org/dist/{version}");
    let checksums=String::from_utf8(download(&format!("{base}/SHASUMS256.txt"),1024*1024).await?).map_err(|e|e.to_string())?;
    let expected=checksums.lines().find_map(|line|{let mut fields=line.split_whitespace();let sha=fields.next()?;let file=fields.next()?;(file==name).then_some(sha)}).ok_or("실행 환경의 공식 체크섬을 찾지 못했습니다.")?;
    let bytes=download(&format!("{base}/{name}"),128*1024*1024).await?;
    if format!("{:x}",Sha256::digest(&bytes))!=expected{return Err("Node 실행 환경의 SHA-256 검증에 실패했습니다.".into())}
    let staging=root.join(format!("node-stage-{}",uuid::Uuid::new_v4().simple()));tokio::fs::create_dir_all(&staging).await.map_err(|e|e.to_string())?;
    let archive=staging.join(&name);tokio::fs::write(&archive,bytes).await.map_err(|e|e.to_string())?;
    let mut extract=if cfg!(windows){let mut cmd=Command::new("powershell.exe");cmd.args(["-NoProfile","-NonInteractive","-Command","Expand-Archive -LiteralPath $env:DOMINION_NODE_ARCHIVE -DestinationPath $env:DOMINION_NODE_STAGE -Force"]);cmd.env("DOMINION_NODE_ARCHIVE",&archive).env("DOMINION_NODE_STAGE",&staging);cmd}else{let mut cmd=Command::new("tar");cmd.arg("-xzf").arg(&archive).arg("-C").arg(&staging);cmd};
    let output=tokio::time::timeout(Duration::from_secs(120),extract.stdin(Stdio::null()).kill_on_drop(true).output()).await.map_err(|_|"실행 환경 압축 해제 시간이 초과됐습니다.")?.map_err(|e|e.to_string())?;
    if !output.status.success(){let _=tokio::fs::remove_dir_all(&staging).await;return Err("실행 환경의 압축을 해제하지 못했습니다.".into())}
    tokio::fs::rename(staging.join(folder),root.join("node")).await.map_err(|e|e.to_string())?;
    let _=tokio::fs::remove_dir_all(&staging).await;Ok(())
}
pub async fn sdk(root:&Path,node:&Path,version:&str)->Result<(),String>{
    let prefix=root.join("runtime");tokio::fs::create_dir_all(&prefix).await.map_err(|e|e.to_string())?;
    let canonical=std::fs::canonicalize(node).unwrap_or_else(|_|node.to_path_buf());
    let prefix=if cfg!(windows){canonical.parent()}else{canonical.parent().and_then(Path::parent)}.ok_or("Node installation has no prefix")?;
    let npm=if cfg!(windows){prefix.join("node_modules/npm/bin/npm-cli.js")}else{prefix.join("lib/node_modules/npm/bin/npm-cli.js")};
    if !npm.is_file(){return Err("플러그인 설치에 필요한 npm을 찾지 못했습니다.".into())}
    let user_config=root.join("npmrc");let global_config=root.join("npm-globalrc");tokio::fs::write(&global_config,b"registry=https://registry.npmjs.org/\n").await.map_err(|e|e.to_string())?;tokio::fs::write(&user_config,b"registry=https://registry.npmjs.org/\n").await.map_err(|e|e.to_string())?;
    let mut command=Command::new(node);
    command.arg(npm).args(["install","--omit=dev","--no-audit","--no-fund","--prefix"]).arg(&prefix).arg(format!("openclaw@{version}"))
        .env_clear().env("NPM_CONFIG_USERCONFIG",&user_config).env("NPM_CONFIG_GLOBALCONFIG",&global_config)
        .env("NPM_CONFIG_CACHE",root.join("npm-cache")).env("NPM_CONFIG_REGISTRY","https://registry.npmjs.org/")
        .current_dir(&prefix).stdin(Stdio::null()).kill_on_drop(true);
    for key in ["PATH","HOME","USERPROFILE","SystemRoot","SYSTEMROOT","TEMP","TMP","TMPDIR"]{if let Some(value)=std::env::var_os(key){command.env(key,value);}}
    let output=tokio::time::timeout(Duration::from_secs(600),command.output()).await.map_err(|_|"플러그인 실행 환경 설치 시간이 초과됐습니다.")?.map_err(|e|e.to_string())?;
    if !output.status.success(){return Err(format!("플러그인 실행 환경 설치 실패: {}",String::from_utf8_lossy(&output.stderr).chars().rev().take(2000).collect::<String>().chars().rev().collect::<String>()))}
    Ok(())
}
