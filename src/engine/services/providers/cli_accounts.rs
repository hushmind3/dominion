//! Key-free local account bridge. Reuses the OS credential store owned by
//! GitHub CLI (gh) / Google Workspace CLI (gws). Never parses browser cookies,
//! persists CLI tokens, or calls a shell for remote tool execution.
//! When a packaged OAuth app is configured the native OAuth flow wins.
use reqwest::{Method,Url};
use serde_json::{json,Value};
use std::io::Read;
use std::process::{Command,Output,Stdio};
use std::thread;
use std::time::{Duration,Instant};
use tokio::io::{AsyncRead,AsyncReadExt};

const MAX_CLI_OUTPUT_BYTES:usize=64*1024;
const CLI_STATUS_TIMEOUT:Duration=Duration::from_secs(5);

#[derive(Debug)]
pub(crate) enum CliOutputError { Spawn, Timeout, OutputLimit, Io }

async fn capture_cli_pipe<R:AsyncRead+Unpin>(mut pipe:R)->Result<(Vec<u8>,bool),CliOutputError>{
    let mut bytes=Vec::new();
    let mut buffer=[0u8;4096];
    let mut exceeded=false;
    loop{
        let count=pipe.read(&mut buffer).await.map_err(|_|CliOutputError::Io)?;
        if count==0{break;}
        let retained=count.min(MAX_CLI_OUTPUT_BYTES.saturating_sub(bytes.len()));
        bytes.extend_from_slice(&buffer[..retained]);
        exceeded|=retained<count;
    }
    Ok((bytes,exceeded))
}

pub(crate) async fn run_cli_bounded(
    executable:std::path::PathBuf,
    args:&[&str],
    limit:Duration,
)->Result<Output,CliOutputError>{
    let mut child=tokio::process::Command::new(executable)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_|CliOutputError::Spawn)?;
    let stdout=child.stdout.take().ok_or(CliOutputError::Io)?;
    let stderr=child.stderr.take().ok_or(CliOutputError::Io)?;
    let output=tokio::time::timeout(limit,async{
        let (stdout,stderr,status)=tokio::join!(
            capture_cli_pipe(stdout),
            capture_cli_pipe(stderr),
            child.wait(),
        );
        let (stdout,stdout_exceeded)=stdout?;
        let (stderr,stderr_exceeded)=stderr?;
        let status=status.map_err(|_|CliOutputError::Io)?;
        if stdout_exceeded||stderr_exceeded{return Err(CliOutputError::OutputLimit);}
        Ok(Output{status,stdout,stderr})
    }).await.map_err(|_|CliOutputError::Timeout)??;
    Ok(output)
}

#[derive(Clone,Debug)]
pub struct LocalStatus {
    pub installed:bool,
    pub connected:bool,
    pub account:Option<String>,
    pub scopes:Vec<String>,
}
fn program(provider:&str)->Option<&'static str>{
    match provider{"github"=>Some("gh"),"google"=>Some("gws"),_=>None}
}
fn executable(provider:&str)->Option<std::path::PathBuf>{
    let binary=program(provider)?;
    // Finder-launched macOS apps do not necessarily inherit Homebrew PATH.
    // Search the current PATH, then standard desktop CLI installation roots.
    if let Some(path)=std::env::var_os("PATH"){
        for root in std::env::split_paths(&path){
            let candidate=root.join(binary);
            if candidate.is_file(){return Some(candidate);}
            #[cfg(windows)]
            {
                let exe=root.join(format!("{binary}.exe"));
                if exe.is_file(){return Some(exe);}
            }
        }
    }
    for root in ["/opt/homebrew/bin","/usr/local/bin","/usr/bin"]{
        let candidate=std::path::Path::new(root).join(binary);
        if candidate.is_file(){return Some(candidate);}
    }
    #[cfg(windows)]
    {
        // Never invoke a .cmd shim with model-supplied --json: shell
        // metacharacters in sheet cells would become command injection.
        if provider=="github"{
            let candidate=std::path::Path::new("C:\\Program Files\\GitHub CLI\\gh.exe");
            if candidate.is_file(){return Some(candidate.to_path_buf());}
        }
    }
    None
}
fn capture_std_pipe<R:Read>(mut pipe:R)->std::io::Result<(Vec<u8>,bool)>{
    let mut bytes=Vec::new();
    let mut buffer=[0u8;4096];
    let mut exceeded=false;
    loop{
        let count=pipe.read(&mut buffer)?;
        if count==0{break;}
        let retained=count.min(MAX_CLI_OUTPUT_BYTES.saturating_sub(bytes.len()));
        bytes.extend_from_slice(&buffer[..retained]);
        exceeded|=retained<count;
    }
    Ok((bytes,exceeded))
}

fn cli_output(provider:&str,args:&[&str])->Option<Output>{
    let mut child=Command::new(executable(provider)?)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn().ok()?;
    let stdout=child.stdout.take()?;
    let stderr=child.stderr.take()?;
    let stdout_reader=thread::Builder::new().name("cli-status-stdout".into())
        .spawn(move||capture_std_pipe(stdout));
    let stdout_reader=match stdout_reader{
        Ok(reader)=>reader,
        Err(_)=>{let _=child.kill();let _=child.wait();return None;}
    };
    let stderr_reader=thread::Builder::new().name("cli-status-stderr".into())
        .spawn(move||capture_std_pipe(stderr));
    let stderr_reader=match stderr_reader{
        Ok(reader)=>reader,
        Err(_)=>{
            let _=child.kill();
            let _=child.wait();
            let _=stdout_reader.join();
            return None;
        }
    };
    let deadline=Instant::now()+CLI_STATUS_TIMEOUT;
    let status=loop{
        match child.try_wait(){
            Ok(Some(status))=>break status,
            Ok(None) if Instant::now()<deadline=>thread::sleep(Duration::from_millis(20)),
            _=>{
                let _=child.kill();
                let _=child.wait();
                let _=stdout_reader.join();
                let _=stderr_reader.join();
                return None;
            }
        }
    };
    let (stdout,stdout_exceeded)=stdout_reader.join().ok()?.ok()?;
    let (stderr,stderr_exceeded)=stderr_reader.join().ok()?.ok()?;
    if stdout_exceeded||stderr_exceeded{return None;}
    Some(Output{status,stdout,stderr})
}
fn status_from_google(data:&Value)->LocalStatus{
    let scopes=match data.get("scopes"){
        Some(Value::Array(v))=>v.iter().filter_map(Value::as_str).map(str::to_owned).collect(),
        Some(Value::String(s))=>s.split_whitespace().map(str::to_owned).collect(),
        _=>Vec::new(),
    };
    let connected=data["token_valid"].as_bool()==Some(true)
        || data["authenticated"].as_bool()==Some(true);
    let account=data["account"].as_str().or_else(||data["email"].as_str())
        .or_else(||data["default_account"].as_str()).map(str::to_owned);
    LocalStatus{installed:true,connected,account,scopes}
}
pub fn status(provider:&str)->LocalStatus{
    if executable(provider).is_none() || cli_output(provider,&["--version"]).is_none(){
        return LocalStatus{installed:false,connected:false,account:None,scopes:vec![]};
    }
    if provider=="github"{
        let connected=cli_output("github",&["auth","status","--hostname","github.com"])
            .is_some_and(|o|o.status.success());
        return LocalStatus{installed:true,connected,account:None,scopes:vec![]};
    }
    let out=cli_output("google",&["auth","status","--json"]);
    out.filter(|o|o.status.success())
        .and_then(|o|serde_json::from_slice::<Value>(&o.stdout).ok())
        .map(|data|status_from_google(&data))
        .unwrap_or(LocalStatus{installed:true,connected:false,account:None,scopes:vec![]})
}
pub async fn github_token()->Result<String,String>{
    let executable=executable("github").ok_or("GitHub CLI가 설치되지 않았습니다.")?;
    let result=run_cli_bounded(executable,&["auth","token","--hostname","github.com"],Duration::from_secs(12))
        .await.map_err(|error|match error{
            CliOutputError::Timeout=>"GitHub CLI 인증 조회가 지연됩니다.",
            CliOutputError::OutputLimit=>"GitHub CLI 인증 응답이 허용 크기를 초과했습니다.",
            CliOutputError::Spawn|CliOutputError::Io=>"GitHub CLI 실행에 실패했습니다. 설치와 계정 연결을 확인하세요.",
        })?;
    if !result.status.success(){
        return Err("GitHub CLI 계정이 연결되지 않았습니다. 연결 메뉴에서 GitHub 로그인을 진행하세요.".into());
    }
    let token=String::from_utf8(result.stdout)
        .map_err(|_|"GitHub CLI 토큰 인코딩 오류")?;
    let token=token.trim();
    if token.is_empty()||token.len()>2000||token.chars().any(char::is_whitespace){
        return Err("GitHub CLI 인증 정보가 올바르지 않습니다.".into());
    }
    Ok(token.to_owned())
}
/// Launch the trusted CLI's interactive consent in a real terminal.
pub fn login(provider:&str)->Result<Value,String>{
    program(provider).ok_or("지원하지 않는 로컬 계정입니다.")?;
    if !status(provider).installed{
        return Err(match provider{
            "github"=>"GitHub CLI(gh)를 먼저 설치하세요: https://cli.github.com/",
            _=>"Google Workspace CLI(gws)를 먼저 설치하세요: https://github.com/googleworkspace/cli",
        }.into());
    }
    let command=match provider{
        "github"=>"gh auth login --hostname github.com --web --scopes repo,read:user,user:email",
        // Try existing gws OAuth configuration first. If it is unconfigured,
        // the setup wizard can create a Cloud project via gcloud. The final
        // explicit login requests only the three capabilities this UI uses.
        _=>"gws auth login --scopes https://www.googleapis.com/auth/drive.readonly,https://www.googleapis.com/auth/documents,https://www.googleapis.com/auth/spreadsheets || (gws auth setup && gws auth login --scopes https://www.googleapis.com/auth/drive.readonly,https://www.googleapis.com/auth/documents,https://www.googleapis.com/auth/spreadsheets)",
    };
    #[cfg(target_os="macos")]
    let launched={
        let escaped=command.replace('\\',"\\\\").replace('"',"\\\"");
        Command::new("/usr/bin/osascript")
            .args(["-e",&format!("tell application \"Terminal\" to do script \"{escaped}\"")])
            .spawn()
    };
    #[cfg(target_os="windows")]
    let launched=Command::new("cmd.exe")
        .args(["/C","start","","cmd.exe","/K",command]).spawn();
    #[cfg(all(unix,not(target_os="macos")))]
    let launched={
        Command::new("x-terminal-emulator").args(["-e","sh","-c",command]).spawn()
            .or_else(|_|Command::new("gnome-terminal").args(["--","sh","-c",command]).spawn())
    };
    launched.map_err(|_|"로그인 터미널을 열지 못했습니다. 지원 CLI를 설치하고 다시 시도하세요.".to_string())?;
    Ok(json!({"opened":true,"mode":"cli","provider":provider,
        "message":"시스템 로그인 창에서 계정을 승인한 후 이 화면으로 돌아오세요."}))
}
fn percent_decode(input:&str)->Result<String,String>{
    let bytes=input.as_bytes();
    let mut decoded=Vec::with_capacity(bytes.len());
    let mut i=0;
    while i<bytes.len(){
        if bytes[i]==b'%'{
            if i+2>=bytes.len(){return Err("잘못된 Google API 인코딩".into());}
            let hex=|v:u8|->Option<u8>{
                match v{b'0'..=b'9'=>Some(v-b'0'),
                    b'a'..=b'f'=>Some(v-b'a'+10),
                    b'A'..=b'F'=>Some(v-b'A'+10),_=>None}
            };
            let hi=hex(bytes[i+1]).ok_or("잘못된 Google API 인코딩")?;
            let lo=hex(bytes[i+2]).ok_or("잘못된 Google API 인코딩")?;
            decoded.push(hi*16+lo);
            i+=3;
        }else{decoded.push(bytes[i]);i+=1;}
    }
    String::from_utf8(decoded).map_err(|_|"Google API 경로 UTF-8 오류".into())
}
fn google_parts(url:&Url,method:&Method)->Result<(Vec<String>,Value),String>{
    let path=url.path_segments().ok_or("Google API 경로 오류")?
        .map(percent_decode).collect::<Result<Vec<_>,_>>()?;
    let seg=path.iter().map(String::as_str).collect::<Vec<_>>();
    let mut pairs=serde_json::Map::new();
    for (k,v) in url.query_pairs(){
        // Google CLI validates Discovery schemas: numeric and boolean
        // params must not be sent as strings from the URL query.
        let typed=match k.as_ref(){
            "pageSize"=>v.parse::<u32>().map(|n|json!(n))
                .map_err(|_|"잘못된 Drive pageSize")?,
            "includeTabsContent"=>match v.as_ref(){
                "true"=>json!(true),"false"=>json!(false),
                _=>return Err("잘못된 Docs includeTabsContent".into()),
            },
            _=>json!(v.as_ref())
        };
        pairs.insert(k.to_string(),typed);
    }
    let (command,params):(Vec<String>,Vec<(&str,String)>)=match (url.host_str(),seg.as_slice(),method.as_str()){
        (Some("www.googleapis.com"),["drive","v3","files"],"GET")=>
            (vec!["drive","files","list"].into_iter().map(str::to_owned).collect(),vec![]),
        (Some("docs.googleapis.com"),["v1","documents",id],"GET")=>
            (vec!["docs","documents","get"].into_iter().map(str::to_owned).collect(),
             vec![("documentId",(*id).to_owned())]),
        (Some("docs.googleapis.com"),["v1","documents",id],"POST") if id.ends_with(":batchUpdate")=>
            (vec!["docs","documents","batchUpdate"].into_iter().map(str::to_owned).collect(),
             vec![("documentId",id.trim_end_matches(":batchUpdate").to_owned())]),
        (Some("sheets.googleapis.com"),["v4","spreadsheets",id,"values",range],"GET")=>
            (vec!["sheets","spreadsheets","values","get"].into_iter().map(str::to_owned).collect(),
             vec![("spreadsheetId",(*id).to_owned()),("range",(*range).to_owned())]),
        (Some("sheets.googleapis.com"),["v4","spreadsheets",id,"values",range],"PUT")=>
            (vec!["sheets","spreadsheets","values","update"].into_iter().map(str::to_owned).collect(),
             vec![("spreadsheetId",(*id).to_owned()),("range",(*range).to_owned())]),
        _=>return Err("지원하지 않는 Google Workspace CLI 작업입니다.".into()),
    };
    for (k,v) in params{pairs.insert(k.to_string(),json!(v));}
    Ok((command,Value::Object(pairs)))
}
pub async fn google_request(method:Method,url:Url,body:Option<Value>)->Result<Value,String>{
    let (command,params)=google_parts(&url,&method)?;
    let executable=executable("google").ok_or("Google Workspace CLI(gws)가 설치되지 않았습니다.")?;
    let mut invocation=tokio::process::Command::new(executable);
    invocation.args(command).args(["--params",&params.to_string()]);
    if let Some(payload)=body{invocation.args(["--json",&payload.to_string()]);}
    invocation.stdin(Stdio::null()).kill_on_drop(true);
    let out=tokio::time::timeout(Duration::from_secs(35),invocation.output()).await
        .map_err(|_|"Google Workspace CLI 작업이 제한 시간을 초과했습니다.")?
        .map_err(|_|"Google Workspace CLI(gws)가 설치되지 않았습니다.")?;
    if out.stdout.len()>1_000_000 || out.stderr.len()>20_000{
        return Err("Google Workspace CLI 출력이 너무 큽니다.".into());
    }
    if !out.status.success(){
        let error=String::from_utf8_lossy(&out.stderr);
        return Err(format!("Google Workspace CLI 작업 실패: {}",
            error.chars().take(600).collect::<String>()));
    }
    serde_json::from_slice(&out.stdout).map_err(|_|"Google Workspace CLI JSON 응답 오류".into())
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn status_does_not_expose_secrets_and_reports_granted_scopes(){
        let s=status_from_google(&json!({"token_valid":true,"account":"user@example.com",
            "scopes":["https://www.googleapis.com/auth/drive.readonly"]}));
        assert!(s.connected);
        assert_eq!(s.scopes.len(),1);
    }
    #[test]
    fn gws_route_only_maps_fixed_services_and_operations(){
        let u=Url::parse("https://docs.googleapis.com/v1/documents/abc_123:batchUpdate").unwrap();
        let (cmd,params)=google_parts(&u,&Method::POST).unwrap();
        assert_eq!(cmd,vec!["docs","documents","batchUpdate"]);
        assert_eq!(params["documentId"],"abc_123");
        assert!(google_parts(&Url::parse("https://evil.example/drive/v3/files").unwrap(),&Method::GET).is_err());
        let s=Url::parse("https://sheets.googleapis.com/v4/spreadsheets/abc_123/values/Sheet1%21A1%3AB2").unwrap();
        let (cmd,params)=google_parts(&s,&Method::PUT).unwrap();
        assert_eq!(cmd,vec!["sheets","spreadsheets","values","update"]);
        assert_eq!(params["range"],"Sheet1!A1:B2");
        let drive=Url::parse("https://www.googleapis.com/drive/v3/files?pageSize=50").unwrap();
        let (_,params)=google_parts(&drive,&Method::GET).unwrap();
        assert_eq!(params["pageSize"],50);
        let docs=Url::parse("https://docs.googleapis.com/v1/documents/abc_123?includeTabsContent=true").unwrap();
        let (_,params)=google_parts(&docs,&Method::GET).unwrap();
        assert_eq!(params["includeTabsContent"],true);
    }
}
