//! Task-oriented GitHub file operations over the user's own OAuth grant.
//! One scoped read/commit tool avoids asking the LLM to construct raw REST
//! payloads. A commit is a compare-and-swap on the observed blob SHA; the
//! host re-reads the remote branch before claiming success.
//!
//! The origin is a fixed GitHub API URL, never model-controlled. Other MCP
//! servers receive no GitHub credential.
use crate::state::AppState;
use base64::Engine as _;
use serde_json::{json,Value};
use sha2::{Digest,Sha256};

const API_ORIGIN:&str="https://api.github.com/";
const MAX_SOURCE_BYTES:usize=160_000;
const MAX_VISIBLE_CHARS:usize=16_000;

#[derive(Debug)]
struct Target<'a>{
    owner:&'a str,
    repo:&'a str,
    branch:&'a str,
    path:&'a str,
}
impl<'a> Target<'a>{
    fn parse(args:&'a Value)->Result<Self,String>{
        let get=|key|args[key].as_str().filter(|v|!v.is_empty())
            .ok_or_else(||format!("GitHub {key} is required"));
        let owner=get("owner")?;
        let repo=get("repo")?;
        let branch=get("branch")?;
        let path=get("path")?;
        let segment=|s:&str|!s.is_empty() && s.len()<=100
            && s.bytes().all(|ch|ch.is_ascii_alphanumeric() || matches!(ch,b'-'|b'_'|b'.'));
        if !segment(owner)||!segment(repo)||owner=="."||repo=="."||owner==".."||repo==".." {
            return Err("Invalid GitHub repository owner/name".into());
        }
        if branch.len()>200||branch.starts_with('-')||branch.contains("..")
            ||branch.chars().any(|c|c.is_control() || c.is_whitespace())
            ||path.len()>1024||path.contains('\\')
            ||path.split('/').any(|part|part.is_empty()||part=="."||part=="..") {
            return Err("Invalid GitHub branch or relative file path".into());
        }
        Ok(Self{owner,repo,branch,path})
    }
    fn url(&self,origin:&str)->Result<reqwest::Url,String>{
        let mut url=reqwest::Url::parse(origin).map_err(|e|e.to_string())?;
        if url.cannot_be_a_base(){return Err("GitHub API origin is invalid".into())}
        {
            let mut segments=url.path_segments_mut().map_err(|_|"Invalid API URL")?;
            segments.clear();
            segments.extend(["repos",self.owner,self.repo,"contents"]);
            for part in self.path.split('/'){segments.push(part);}
        }
        url.query_pairs_mut().append_pair("ref",self.branch);
        Ok(url)
    }
}
fn sha256(bytes:&[u8])->String{format!("{:x}",Sha256::digest(bytes))}
fn body_text(value:&Value)->Result<(String,String),String>{
    if value["type"]!="file" || value["encoding"]!="base64"{
        return Err("GitHub path is not a base64 text file".into());
    }
    let sha=value["sha"].as_str().filter(|v|v.len()==40
        &&v.bytes().all(|c|c.is_ascii_hexdigit()))
        .ok_or("GitHub did not provide a valid blob SHA")?;
    let encoded=value["content"].as_str().ok_or("Missing remote file content")?;
    if encoded.len()>MAX_SOURCE_BYTES.saturating_mul(2){
        return Err("Remote source exceeds the task-oriented file limit".into());
    }
    let clean=encoded.chars().filter(|c|!c.is_ascii_whitespace()).collect::<String>();
    let bytes=base64::engine::general_purpose::STANDARD.decode(clean)
        .map_err(|_|"Invalid GitHub source encoding")?;
    if bytes.len()>MAX_SOURCE_BYTES {return Err("Source exceeds the 160 KB edit limit".into());}
    let content=String::from_utf8(bytes).map_err(|_|"GitHub file is not UTF-8 text")?;
    Ok((sha.to_string(),content))
}
fn api_error(status:reqwest::StatusCode,body:&str)->String {
    match status.as_u16(){
        401|403=>format!("GitHub permission expired or insufficient ({status}). Reconnect with repository Contents write permission; workflow edits also need workflow permission."),
        404=>"GitHub repository, branch or file is inaccessible; verify selected repository and account".into(),
        409|422=>format!("GitHub rejected conflicting/stale change ({status}); re-read the branch and source SHA"),
        _=>format!("GitHub API error ({status}): {}",body.chars().take(400).collect::<String>()),
    }
}
async fn remote_file(client:&reqwest::Client,token:&str,url:&reqwest::Url)
    ->Result<Option<(String,String)>,String>{
    let reply=client.get(url.clone()).bearer_auth(token)
        .header("Accept","application/vnd.github+json")
        .header("User-Agent","DOMINION-Agent")
        .send().await.map_err(|e|format!("GitHub read unavailable: {e}"))?;
    if reply.status()==reqwest::StatusCode::NOT_FOUND{return Ok(None)}
    if !reply.status().is_success(){
        let status=reply.status();
        let body=reply.text().await.unwrap_or_default();
        return Err(api_error(status,&body));
    }
    let value:Value=reply.json().await.map_err(|_|"Invalid GitHub file response")?;
    Ok(Some(body_text(&value)?))
}

async fn read_with(client:&reqwest::Client,token:&str,origin:&str,
    args:&Value)->Result<Value,String>{
    let target=Target::parse(args)?;
    let url=target.url(origin)?;
    let (sha,content)=remote_file(client,token,&url).await?
        .ok_or("File does not exist on selected GitHub branch")?;
    let truncated=content.chars().count()>MAX_VISIBLE_CHARS;
    Ok(json!({"repository":format!("{}/{}",target.owner,target.repo),
        "branch":target.branch,"path":target.path,"blobSha":sha,
        "contentSha256":sha256(content.as_bytes()),"bytes":content.len(),
        "content":content.chars().take(MAX_VISIBLE_CHARS).collect::<String>(),
        "truncated":truncated,"verified":true}))
}

async fn commit_with(client:&reqwest::Client,token:&str,origin:&str,
    args:&Value)->Result<Value,String>{
    let target=Target::parse(args)?;
    let new_text=args["content"].as_str().ok_or("GitHub UTF-8 file content is required")?;
    let message=args["message"].as_str().filter(|value|!value.trim().is_empty()
        &&value.len()<=500).ok_or("Commit message must contain 1..500 characters")?;
    if new_text.len()>MAX_SOURCE_BYTES{return Err("File exceeds 160 KB commit limit".into())}
    let expected=args["expectedSha"].as_str().filter(|v|!v.is_empty());
    let create_new=args["createNew"]==true;
    if expected.is_some()==create_new {
        return Err("Supply exactly one of expectedSha (existing file) or createNew=true".into());
    }
    if let Some(sha)=expected{
        if sha.len()!=40||!sha.bytes().all(|b|b.is_ascii_hexdigit()){
            return Err("expectedSha must be a 40-character Git blob SHA".into());
        }
    }
    let url=target.url(origin)?;
    let existing=remote_file(client,token,&url).await?;
    let prior_sha=match existing {
        Some((sha,old_content))=>{
            if create_new{return Err("Refusing to overwrite an existing GitHub file".into())}
            if expected!=Some(sha.as_str()){
                return Err("Remote file changed since read; fetch the latest blob SHA".into());
            }
            if old_content==new_text{return Err("Remote file already has identical content; no commit needed".into());}
            Some(sha)
        }
        None=>{
            if !create_new{return Err("Remote file missing; explicit createNew=true is required".into())}
            None
        }
    };
    let mut body=json!({
        "message":message,"content":base64::engine::general_purpose::STANDARD.encode(new_text),
        "branch":target.branch,
    });
    if let Some(sha)=prior_sha {body["sha"]=json!(sha);}
    let mut put_url=url.clone();
    put_url.set_query(None);
    let response=client.put(put_url).bearer_auth(token)
        .header("Accept","application/vnd.github+json")
        .header("User-Agent","DOMINION-Agent")
        .json(&body).send().await.map_err(|e|format!(
            "GitHub write result uncertain; do not retry without re-reading the remote SHA: {e}"))?;
    let status=response.status();
    if !status.is_success(){
        let detail=response.text().await.unwrap_or_default();
        return Err(api_error(status,&detail));
    }
    let changed:Value=response.json().await
        .map_err(|_|"GitHub write response unknown; re-read before retry")?;
    let expected_new_sha=changed["content"]["sha"].as_str().ok_or(
        "GitHub commit result missing blob SHA; re-read before retry")?;
    let commit_sha=changed["commit"]["sha"].as_str().ok_or(
        "GitHub commit result missing commit SHA; re-read before retry")?;
    // An actual independent GET, not the PUT response, proves that the branch
    // contains the newly submitted bytes.
    let verified=remote_file(client,token,&url).await?
        .ok_or("Committed file not visible on remote branch; verify before retry")?;
    if verified.0!=expected_new_sha || verified.1!=new_text {
        return Err("Remote verification disagrees with committed source; manual review required".into());
    }
    Ok(json!({"repository":format!("{}/{}",target.owner,target.repo),
        "branch":target.branch,"path":target.path,
        "commitSha":commit_sha,"blobSha":verified.0,
        "contentSha256":sha256(new_text.as_bytes()),
        "written":true,"verified":true,"bytes":new_text.len()}))
}
pub async fn read(state:&AppState,args:&Value)->Result<Value,String>{
    let token=crate::engine::oauth::fresh_access_token(state,"github").await
        .map_err(|e|format!("Connect or reconnect GitHub first: {e}"))?;
    let client=state.provider_http_client(30)?;
    read_with(&client,&token,API_ORIGIN,args).await
}
pub async fn commit(state:&AppState,args:&Value)->Result<Value,String>{
    let token=crate::engine::oauth::fresh_access_token(state,"github").await
        .map_err(|e|format!("Connect or reconnect GitHub first: {e}"))?;
    let client=state.provider_http_client(30)?;
    commit_with(&client,&token,API_ORIGIN,args).await
}

#[cfg(test)]
mod tests{
    use super::*;
    #[derive(Clone)]
    struct MockRemote(std::sync::Arc<std::sync::Mutex<RemoteFile>>);
    struct RemoteFile {
        content:String,
        sha:String,
        reads:usize,
        writes:usize,
        mismatch_after_second_write:bool,
    }
    async fn mock_read(
        axum::extract::State(state):axum::extract::State<MockRemote>,
        axum::extract::Query(query):axum::extract::Query<std::collections::HashMap<String,String>>,
    )->(axum::http::StatusCode,axum::Json<Value>){
        use base64::Engine as _;
        if query.get("ref").map(String::as_str)!=Some("feature/test"){
            return (axum::http::StatusCode::BAD_REQUEST,
                axum::Json(json!({"error":"branch must be explicit"})));
        }
        let mut remote=state.0.lock().unwrap();
        remote.reads+=1;
        let (sha,content)=if remote.mismatch_after_second_write && remote.writes>=2 {
            ("z".repeat(40),"unexpected bytes".to_string())
        }else{(remote.sha.clone(),remote.content.clone())};
        (axum::http::StatusCode::OK,axum::Json(json!({
            "type":"file","encoding":"base64","sha":sha,
            "content":base64::engine::general_purpose::STANDARD.encode(content)
        })))
    }
    async fn mock_put(
        axum::extract::State(state):axum::extract::State<MockRemote>,
        axum::Json(input):axum::Json<Value>,
    )->(axum::http::StatusCode,axum::Json<Value>){
        use base64::Engine as _;
        let mut remote=state.0.lock().unwrap();
        if input["branch"]!="feature/test"
            || input["sha"].as_str()!=Some(remote.sha.as_str()){
            return (axum::http::StatusCode::CONFLICT,
                axum::Json(json!({"message":"stale blob SHA or branch"})));
        }
        let Some(encoded)=input["content"].as_str() else {
            return (axum::http::StatusCode::BAD_REQUEST,
                axum::Json(json!({"message":"content required"})));
        };
        let Ok(decoded)=base64::engine::general_purpose::STANDARD.decode(encoded) else{
            return (axum::http::StatusCode::BAD_REQUEST,
                axum::Json(json!({"message":"invalid base64"})));
        };
        remote.content=String::from_utf8(decoded).unwrap();
        remote.writes+=1;
        remote.sha=if remote.writes==1 {"b".repeat(40)}else{"c".repeat(40)};
        (axum::http::StatusCode::OK,
            axum::Json(json!({"content":{"sha":remote.sha},
                "commit":{"sha":"d".repeat(40)}})))
    }

    #[tokio::test]
    async fn github_file_read_commit_and_remote_readback_prove_effect_once(){
        // Real HTTP exchanges against a GitHub Contents-shaped local server:
        // no external credentials, paid API or real repository is touched.
        let remote=MockRemote(std::sync::Arc::new(std::sync::Mutex::new(
            RemoteFile{content:"before\n".into(),sha:"a".repeat(40),
                reads:0,writes:0,mismatch_after_second_write:false}
        )));
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port=listener.local_addr().unwrap().port();
        let router=axum::Router::new()
            .route("/repos/:owner/:repo/contents/*path",
                axum::routing::get(mock_read).put(mock_put))
            .with_state(remote.clone());
        let server=tokio::spawn(async move {
            let _=axum::serve(listener,router).await;
        });
        let api=format!("http://127.0.0.1:{port}/");
        let client=reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(6)).build().unwrap();
        let target=json!({"owner":"example","repo":"project",
            "branch":"feature/test","path":"src/lib.rs"});
        let original=read_with(&client,"fixture-token",&api,&target).await.unwrap();
        assert_eq!(original["content"],"before\n");
        assert_eq!(original["blobSha"],"a".repeat(40));

        let mut edit=target.clone();
        edit["expectedSha"]=original["blobSha"].clone();
        edit["content"]=json!("after\n");
        edit["message"]=json!("test: update source");
        let committed=commit_with(&client,"fixture-token",&api,&edit).await.unwrap();
        assert_eq!(committed["verified"],true);
        assert_eq!(committed["written"],true);
        assert_eq!(committed["blobSha"],"b".repeat(40));
        assert_eq!(committed["contentSha256"],sha256(b"after\n"));
        {
            let remote=remote.0.lock().unwrap();
            assert_eq!(remote.content,"after\n");
            assert_eq!(remote.writes,1,"exactly one accepted GitHub PUT");
            assert_eq!(remote.reads,3,"initial read, SHA check and remote readback");
        }
        let stale=commit_with(&client,"fixture-token",&api,&edit).await
            .err().expect("stale blob SHA must refuse an unsafe second PUT");
        assert!(stale.contains("changed since read"),"{stale}");
        assert_eq!(remote.0.lock().unwrap().writes,1);

        // A successful PUT is not completion proof when the independent GET
        // reports different remote bytes. The tool must return an uncertainty
        // error, not claim a verified commit or retry the effect.
        remote.0.lock().unwrap().mismatch_after_second_write=true;
        let mut second=target.clone();
        second["expectedSha"]=json!("b".repeat(40));
        second["content"]=json!("third\n");
        second["message"]=json!("test: second update");
        let mismatch=commit_with(&client,"fixture-token",&api,&second)
            .await.err().expect("incorrect remote readback must fail");
        assert!(mismatch.contains("verification disagrees"),"{mismatch}");
        {
            let remote=remote.0.lock().unwrap();
            assert_eq!(remote.writes,2,"uncertain results must not cause duplicate PUT");
            assert_eq!(remote.reads,6);
        }
        server.abort();
    }

    #[test]
    fn branch_scoped_path_prevents_default_branch_edits_or_url_injection(){
        let input=json!({"owner":"sample","repo":"project","branch":"feature/one",
            "path":"src/lib.rs"});
        let target=Target::parse(&input).unwrap();
        let url=target.url(API_ORIGIN).unwrap();
        assert_eq!(url.host_str(),Some("api.github.com"));
        assert!(url.path().ends_with("/repos/sample/project/contents/src/lib.rs"),"{url}");
        assert!(url.query().unwrap_or("").contains("feature%2Fone"));
        for path in ["../secret","/abs","src/../secret","src\\secret","src//main.rs"]{
            let mut invalid=input.clone();invalid["path"]=json!(path);
            assert!(Target::parse(&invalid).is_err(),"accepted {path}");
        }
        for branch in ["","../main","bad branch"]{
            let mut invalid=input.clone();invalid["branch"]=json!(branch);
            assert!(Target::parse(&invalid).is_err(),"accepted {branch}");
        }
    }
    #[test]
    fn github_file_payload_decodes_utf8_and_refuses_nontext(){
        let content="안녕하세요, Rust!\n";
        let encoded=base64::engine::general_purpose::STANDARD.encode(content);
        let result=json!({"type":"file","encoding":"base64","content":encoded,
            "sha":"a".repeat(40)});
        let (sha,source)=body_text(&result).unwrap();
        assert_eq!(sha,"a".repeat(40));
        assert_eq!(source,content);
        assert!(body_text(&json!({"type":"dir","encoding":"base64",
            "sha":"b".repeat(40),"content":encoded})).is_err());
    }
}
