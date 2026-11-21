//! External account tool registration and execution are NOT engine responsibilities.
//! The engine calls this host adapter after its own authorization/approval gate.
use crate::state::AppState;
use serde_json::{json,Value};
use std::time::Duration;

const MAX_GITHUB_REQUEST_BYTES:usize=160_000;
const MAX_GITHUB_RESPONSE_BYTES:usize=1024*1024;
use super::{oauth,github_files,github_workflow,google_workspace};

fn tool(name:&str,description:&str,parameters:Value)->Value{
    json!({"type":"function","function":{"name":name,"description":description,"parameters":parameters}})
}

pub fn definitions()->Vec<Value>{
    let object=|properties:Value,required:Vec<&str>|json!({
        "type":"object","properties":properties,"required":required,"additionalProperties":false
    });
    let text=|description:&str|json!({"type":"string","description":description});
    vec![
        tool("account_status", "List OAuth accounts connected to DOMINION", object(json!({}), vec![])),
        tool("github_file_read", "Read a UTF-8 file on a specific GitHub repository branch. Returns a blob SHA for safe, conflict-checked updates.", object(json!({
            "owner":text("GitHub repository owner"),"repo":text("Repository name"),
            "branch":text("Exact branch to inspect"),"path":text("Relative repository file path")
        }),vec!["owner","repo","branch","path"])),
        tool("github_file_commit", "Commit a UTF-8 file to the exact selected GitHub branch after checking its observed blob SHA; remotely re-read and verify (approval required).", object(json!({
            "owner":text("GitHub repository owner"),"repo":text("Repository name"),
            "branch":text("Exact branch to update"),"path":text("Relative repository file path"),
            "content":text("Complete replacement UTF-8 content"),"message":text("Commit message"),
            "expectedSha":text("Exact blobSha from github_file_read; mandatory for existing files"),
            "createNew":json!({"type":"boolean","description":"Explicitly create a missing file; cannot overwrite existing source"})
        }),vec!["owner","repo","branch","path","content","message"])),
        tool("github_api", "Call the GitHub REST API as the connected account. Write methods require approval (approval required)", object(json!({"method": json!({"type": "string", "enum": ["GET", "POST", "PATCH", "PUT", "DELETE"]}), "path": text("GitHub API path beginning with /"), "body": json!({"type": "object"})}), vec!["method", "path"])),

        tool("github_work_read", "Use the connected GitHub account to list repositories, branches, PRs, issues and CI runs.", object(json!({
            "operation":{"type":"string","enum":["repositories","search_repositories","branches","pull_requests","issues","ci_runs"]},
            "owner":text("Repository owner"),"repo":text("Repository name"),
            "query":text("Repository name/description or GitHub search query"),
            "branch":text("Branch for CI runs")
        }),vec!["operation"])),
        tool("github_work_write", "Create a GitHub branch, PR or issue and verify it on the connected account. Owner approval required.", object(json!({
            "operation":{"type":"string","enum":["create_branch","open_pr","create_issue"]},
            "owner":text("Repository owner"),"repo":text("Repository"),
            "from":text("Existing branch"),"branch":text("New branch"),
            "head":text("PR head"),"base":text("PR destination"),
            "title":text("PR/issue title"),"body":text("PR/issue details")
        }),vec!["operation","owner","repo"])),
        tool("google_workspace_read", "Search/read connected Google Gmail, Drive, Docs and Calendar. Specify service and operation.", object(json!({
            "service":{"type":"string","enum":["gmail","drive","docs","calendar"]},
            "operation":{"type":"string","enum":["search","get","list"]},
            "query":text("Gmail search or Drive name fragment"),"id":text("Resource ID"),
            "calendarId":text("Calendar (default primary)"),"timeMin":text("RFC3339 start"),
            "maxResults":{"type":"integer","minimum":1,"maximum":50},
            "pageToken":text("Next page token")
        }),vec!["service","operation"])),
        tool("google_workspace_write", "Owner-approved Gmail send, Drive text create/update, Docs append or Calendar create/update/delete.", object(json!({
            "service":{"type":"string","enum":["gmail","drive","docs","calendar"]},
            "operation":{"type":"string","enum":["send","create_text","update_text","append","create","update","delete"]},
            "to":text("Gmail recipient"),"subject":text("Subject"),"body":text("Plain-text email"),
            "id":text("Resource ID"),"name":text("Drive file name"),
            "content":text("File or appended text"),"expectedVersion":text("Prior Drive version"),
            "calendarId":text("Calendar ID (default primary)"),
            "summary":text("Event title"),"description":text("Event description"),
            "start":text("RFC3339 start"),"end":text("RFC3339 end")
        }),vec!["service","operation"])),    ]
}

pub fn filter_available(state:&AppState, definitions:Vec<Value>)->Vec<Value>{
    let providers=oauth::status(state);
    let active=|id:&str,workspace:bool|providers.iter().any(|provider|
        provider["id"]==id && provider["connected"]==true &&
        (!workspace || provider["workspaceReady"]==true));
    let github=active("github",false);
    let google=active("google",true);
    definitions.into_iter().filter(|tool|{
        match tool["function"]["name"].as_str().unwrap_or("") {
            "github_work_read"|"github_work_write"=>github,
            "google_workspace_read"|"google_workspace_write"=>google,
            _=>true
        }
    }).collect()
}


pub fn requires_approval(name:&str)->bool{
    matches!(name,"github_api"|"github_file_commit"|"github_work_write"|"google_workspace_write")
}

pub async fn execute(state:&AppState,name:&str,args:&Value)->Option<Result<Value,String>>{
    match name {
        "account_status"=>Some(Ok(json!({"providers":oauth::status(state)}))),
        "github_file_read"=>Some(github_files::read(state,args).await),
        "github_file_commit"=>Some(github_files::commit(state,args).await),
        "github_api"=>Some(github_api(state,args).await),
        "github_work_read"=>Some(github_workflow::execute(state,args,false).await),
        "github_work_write"=>Some(github_workflow::execute(state,args,true).await),
        "google_workspace_read"=>Some(google_workspace::execute(state,args,false).await),
        "google_workspace_write"=>Some(google_workspace::execute(state,args,true).await),
        _=>None,
    }
}

async fn github_api(state: &AppState, args: &Value) -> Result<Value, String> {
    // Respect the account connector's expiry/refresh lifecycle. A stale
    // cached bearer must not repeatedly fail GitHub requests until restart.
    let token=oauth::fresh_access_token(state,"github").await
        .map_err(|error|format!("GitHub 계정 재연결 또는 권한 확인이 필요합니다: {error}"))?;
    let method = args["method"].as_str().unwrap_or("GET").to_uppercase();
    if !matches!(method.as_str(), "GET" | "POST" | "PATCH" | "PUT" | "DELETE") {
        return Err("지원하지 않는 HTTP 메서드입니다.".into());
    }
    let path = args["path"].as_str().filter(|s|!s.is_empty())
        .ok_or("GitHub API path is required")?.to_string();
    if !path.starts_with('/') || path.contains("..") {
        return Err("GitHub API 경로는 / 로 시작해야 합니다.".into());
    }
    let method =
        reqwest::Method::from_bytes(method.as_bytes()).map_err(|error| error.to_string())?;
    let mut request = state.account_http_client(30)?
        .request(method, format!("https://api.github.com{path}"))
        .header("Authorization", format!("Bearer {token}"))
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "DOMINION")
        .timeout(Duration::from_secs(30));
    if let Some(body) = args.get("body").filter(|value| !value.is_null()) {
        let encoded=serde_json::to_vec(body).map_err(|error|error.to_string())?;
        if encoded.len()>MAX_GITHUB_REQUEST_BYTES{
            return Err("GitHub request body exceeds 160 KB".into());
        }
        request = request.json(body);
    }
    let response = request.send().await.map_err(|error| error.to_string())?;
    let status = response.status().as_u16();
    if response.content_length().is_some_and(|size|size>MAX_GITHUB_RESPONSE_BYTES as u64){
        return Err("GitHub response exceeds 1 MiB".into());
    }
    let mut response=response;
    let mut bytes=Vec::new();
    while let Some(chunk)=response.chunk().await.map_err(|error|error.to_string())?{
        if bytes.len().saturating_add(chunk.len())>MAX_GITHUB_RESPONSE_BYTES{
            return Err("GitHub response exceeds 1 MiB".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    let text=String::from_utf8_lossy(&bytes);
    let parsed: Value = serde_json::from_str(&text).unwrap_or_else(|_| json!(text.as_ref()));
    Ok(json!({
        "status": status,
        "body": parsed.to_string().chars().take(20_000).collect::<String>(),
    }))
}


#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn account_catalog_and_approval_policy_share_the_same_adapter(){
        let entries=definitions();
        let names=entries.iter().map(|item|item["function"]["name"].as_str().unwrap()).collect::<Vec<_>>();
        assert_eq!(names.len(),8);
        assert!(names.contains(&"account_status"));
        assert!(names.contains(&"github_file_commit"));
        assert!(names.contains(&"google_workspace_write"));
        assert!(names.iter().all(|name|execute_tool_declared(name)));
        assert!(requires_approval("google_workspace_write"));
        assert!(!requires_approval("google_workspace_read"));
    }
    fn execute_tool_declared(name:&str)->bool{
        matches!(name,"account_status"|"github_file_read"|"github_file_commit"|"github_api"|
            "github_work_read"|"github_work_write"|"google_workspace_read"|"google_workspace_write")
    }
}
