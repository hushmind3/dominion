//! Account-bound, task-oriented GitHub operations.
//! Reads are independent of approval; mutations go through the standard
//! owner-approved tool gateway and verify the created remote object.
use crate::state::AppState;
use serde_json::{json,Value};
use reqwest::{Client,Method,Url};

const ROOT:&str="https://api.github.com/";

fn arg<'a>(v:&'a Value,name:&str)->Result<&'a str,String>{
    v[name].as_str().filter(|s|!s.trim().is_empty())
        .ok_or_else(||format!("GitHub {name} is required"))
}
fn component(s:&str)->Result<&str,String>{
    if s.len()>120||s.is_empty()||s=="."||s==".."
      ||!s.bytes().all(|b|b.is_ascii_alphanumeric()||matches!(b,b'.'|b'_'|b'-')){
        return Err("Invalid GitHub owner/repository/identifier".into());
    }
    Ok(s)
}
fn branch(s:&str)->Result<&str,String>{
    if s.len()>180||s.is_empty()||s.starts_with('-')||s.contains("..")
        ||s.split('/').any(|part|part.is_empty()||part=="."||part=="..")
        ||s.chars().any(|c|c.is_control()||c.is_whitespace()||c=='\\'){
        return Err("Invalid GitHub branch".into());
    }
    Ok(s)
}
fn repo(v:&Value)->Result<(String,String),String>{
    Ok((component(arg(v,"owner")?)?.to_string(),component(arg(v,"repo")?)?.to_string()))
}
fn uri(origin:&str,parts:&[&str],queries:&[(&str,String)])->Result<Url,String>{
    let mut url=Url::parse(origin).map_err(|e|e.to_string())?;
    {let mut path=url.path_segments_mut().map_err(|_|"Invalid API origin")?;
     path.clear();
     for part in parts {path.push(part);}
    }
    if !queries.is_empty(){
        let mut q=url.query_pairs_mut();
        for (name,value) in queries {q.append_pair(name,value);}
    }
    Ok(url)
}
async fn request(client:&Client,token:&str,method:Method,url:Url,body:Option<Value>)
    ->Result<Value,String>{
    let mut req=client.request(method,url).bearer_auth(token)
        .header("Accept","application/vnd.github+json")
        .header("X-GitHub-Api-Version","2022-11-28")
        .header("User-Agent","DOMINION");
    if let Some(body)=body{req=req.json(&body);}
    let mut response=req.send().await.map_err(|e|format!("GitHub unavailable: {e}"))?;
    let status=response.status();
    let mut bytes=Vec::new();
    while let Some(chunk)=response.chunk().await.map_err(|e|e.to_string())?{
        if bytes.len()+chunk.len()>160_000{return Err("GitHub response too large".into());}
        bytes.extend_from_slice(&chunk);
    }
    if !status.is_success(){
        let message=String::from_utf8_lossy(&bytes);
        return Err(format!("GitHub HTTP {}: {} (check account permission and branch)",status,
            message.chars().take(400).collect::<String>()));
    }
    serde_json::from_slice(&bytes).map_err(|_|"GitHub returned invalid JSON".into())
}
fn bounded(value:&Value)->Value{
    // Never materialize an unlimited listing in the model context.
    if let Some(rows)=value.as_array(){return json!(rows.iter().take(40).collect::<Vec<_>>());}
    value.clone()
}
async fn execute_with(client:&Client,token:&str,origin:&str,args:&Value,write:bool)
    ->Result<Value,String>{
    let operation=arg(args,"operation")?;
    if write && !matches!(operation,"create_branch"|"open_pr"|"create_issue"){
        return Err("Unsupported GitHub write operation".into());
    }
    if !write && !matches!(operation,"repositories"|"branches"|"pull_requests"|"issues"|"ci_runs"){
        return Err("Unsupported GitHub read operation".into());
    }
    if operation=="repositories" {
        let url=uri(origin,&["user","repos"],&[
            ("affiliation","owner,collaborator,organization_member".into()),
            ("per_page","30".into()),("sort","updated".into())])?;
        return Ok(bounded(&request(client,token,Method::GET,url,None).await?));
    }
    let (owner,name)=repo(args)?;
    let base=["repos",owner.as_str(),name.as_str()];
    match operation {
        "branches"|"pull_requests"|"issues"|"ci_runs"=>{
            let (path,queries):(Vec<&str>,Vec<(&str,String)>)=match operation {
                "branches"=>(vec!["branches"],vec![("per_page","30".into())]),
                "pull_requests"=>(vec!["pulls"],vec![("state","open".into()),("per_page","30".into())]),
                "issues"=>(vec!["issues"],vec![("state","open".into()),("per_page","30".into())]),
                _=>(vec!["actions","runs"],vec![
                    ("per_page","10".into()),
                    ("branch",branch(arg(args,"branch")?)?.into())]),
            };
            let path=[base.as_slice(),path.as_slice()].concat();
            let items=request(client,token,Method::GET,uri(origin,&path,&queries)?,None).await?;
            if operation=="ci_runs" {
                return Ok(json!({"total_count":items["total_count"],
                    "workflow_runs":items["workflow_runs"].as_array()
                        .map(|a|a.iter().take(10).map(|x|json!({
                            "id":x["id"],"name":x["name"],"status":x["status"],
                            "conclusion":x["conclusion"],"html_url":x["html_url"],
                            "head_sha":x["head_sha"],"run_number":x["run_number"]}))
                            .collect::<Vec<_>>()).unwrap_or_default()}));
            }
            Ok(bounded(&items))
        }
        "create_branch"=>{
            let next=branch(arg(args,"branch")?)?;
            let from=branch(arg(args,"from")?)?;
            if from==next{return Err("New branch must differ from source".into());}
            let mut ref_path=vec!["git","ref","heads"];
            ref_path.extend(from.split('/'));
            let ref_path=[base.as_slice(),ref_path.as_slice()].concat();
            let current=request(client,token,Method::GET,uri(origin,&ref_path,&[])?,None).await?;
            let sha=current["object"]["sha"].as_str()
                .filter(|x|x.len()==40&&x.bytes().all(|b|b.is_ascii_hexdigit()))
                .ok_or("GitHub source branch lacks a valid commit SHA")?;
            let created=request(client,token,Method::POST,
                uri(origin,&[base.as_slice(),&["git","refs"]].concat(),&[])?,
                Some(json!({"ref":format!("refs/heads/{next}"),"sha":sha}))).await?;
            let mut new_path=vec!["git","ref","heads"];
            new_path.extend(next.split('/'));
            let verified=request(client,token,Method::GET,
                uri(origin,&[base.as_slice(),new_path.as_slice()].concat(),&[])?,None).await?;
            if verified["object"]["sha"]!=sha {
                return Err("GitHub branch was created, but independent readback differs; do not retry automatically".into());
            }
            Ok(json!({"operation":operation,"repository":format!("{owner}/{name}"),
                "branch":next,"sha":sha,"verified":true,"ref":created["ref"]}))
        }
        "open_pr"=>{
            let head=branch(arg(args,"head")?)?;
            let target=branch(arg(args,"base")?)?;
            if head==target{return Err("PR head and base branches must differ".into());}
            let title=arg(args,"title")?;
            if title.len()>300{return Err("PR title too long".into());}
            let body=args["body"].as_str().unwrap_or("");
            if body.len()>20_000{return Err("PR body too long".into());}
            let created=request(client,token,Method::POST,
                uri(origin,&[base.as_slice(),&["pulls"]].concat(),&[])?,
                Some(json!({"head":head,"base":target,"title":title,"body":body}))).await?;
            let number=created["number"].as_u64().ok_or("GitHub PR number missing")?;
            let number_str=number.to_string();
            let verified=request(client,token,Method::GET,
                uri(origin,&[base.as_slice(),&["pulls",&number_str]].concat(),&[])?,None).await?;
            if verified["number"]!=number || verified["title"]!=title {
                return Err("GitHub PR accepted but remote verification disagrees; do not retry automatically".into());
            }
            Ok(json!({"operation":operation,"number":number,"url":verified["html_url"],
                "head":verified["head"]["ref"],"base":verified["base"]["ref"],"verified":true}))
        }
        "create_issue"=>{
            let title=arg(args,"title")?;
            if title.len()>300{return Err("Issue title too long".into());}
            let body=args["body"].as_str().unwrap_or("");
            if body.len()>20_000{return Err("Issue body too long".into());}
            let created=request(client,token,Method::POST,
                uri(origin,&[base.as_slice(),&["issues"]].concat(),&[])?,
                Some(json!({"title":title,"body":body}))).await?;
            let number=created["number"].as_u64().ok_or("GitHub issue number missing")?;
            let number_str=number.to_string();
            let verified=request(client,token,Method::GET,
                uri(origin,&[base.as_slice(),&["issues",&number_str]].concat(),&[])?,None).await?;
            if verified["number"]!=number||verified["title"]!=title{
                return Err("Issue accepted but remote verification disagrees; do not retry automatically".into());
            }
            Ok(json!({"operation":operation,"number":number,
                "url":verified["html_url"],"verified":true}))
        }
        _=>Err("Unsupported GitHub operation".into()),
    }
}
pub async fn execute(state:&AppState,args:&Value,write:bool)->Result<Value,String>{
    let token=crate::engine::oauth::fresh_access_token(state,"github").await
        .map_err(|e|format!("Connect GitHub or refresh its permissions: {e}"))?;
    let client=state.provider_http_client(30)?;
    execute_with(&client,&token,ROOT,args,write).await
}

#[cfg(test)]
mod tests{
    use super::*;
    #[derive(Default)]
    struct Remote {
        branches:std::collections::HashMap<String,String>,
        pr_title:Option<String>,issue_title:Option<String>,
        branch_writes:usize,pr_writes:usize,issue_writes:usize,
    }
    type RemoteState=std::sync::Arc<std::sync::Mutex<Remote>>;
    async fn fake_branch(
        axum::extract::State(remote):axum::extract::State<RemoteState>,
        axum::extract::Path(branch):axum::extract::Path<String>,
        headers:axum::http::HeaderMap,
    )->(axum::http::StatusCode,axum::Json<Value>){
        assert_eq!(headers["authorization"],"Bearer connected-gh-token");
        let state=remote.lock().unwrap();
        match state.branches.get(&branch){
            Some(sha)=>(axum::http::StatusCode::OK,
                axum::Json(json!({"ref":format!("refs/heads/{branch}"),"object":{"sha":sha}}))),
            None=>(axum::http::StatusCode::NOT_FOUND,
                axum::Json(json!({"message":"Not Found"})))
        }
    }
    async fn fake_create_branch(
        axum::extract::State(remote):axum::extract::State<RemoteState>,
        axum::Json(payload):axum::Json<Value>,
    )->(axum::http::StatusCode,axum::Json<Value>){
        let name=payload["ref"].as_str().unwrap().trim_start_matches("refs/heads/");
        let sha=payload["sha"].as_str().unwrap();
        let mut data=remote.lock().unwrap();
        if data.branches.contains_key(name){
            return (axum::http::StatusCode::UNPROCESSABLE_ENTITY,
                axum::Json(json!({"message":"ref already exists"})));
        }
        data.branches.insert(name.to_string(),sha.to_string());
        data.branch_writes+=1;
        (axum::http::StatusCode::CREATED,
            axum::Json(json!({"ref":format!("refs/heads/{name}"),"object":{"sha":sha}})))
    }
    async fn fake_create_pr(
        axum::extract::State(remote):axum::extract::State<RemoteState>,
        axum::Json(payload):axum::Json<Value>,
    )->axum::Json<Value>{
        let mut remote=remote.lock().unwrap();
        remote.pr_title=payload["title"].as_str().map(str::to_string);
        remote.pr_writes+=1;
        axum::Json(json!({"number":1}))
    }
    async fn fake_get_pr(
        axum::extract::State(remote):axum::extract::State<RemoteState>,
    )->axum::Json<Value>{
        let remote=remote.lock().unwrap();
        axum::Json(json!({"number":1,"title":remote.pr_title,
            "head":{"ref":"feature/new"},"base":{"ref":"main"},
            "html_url":"https://github.com/octo/work/pull/1"}))
    }
    async fn fake_create_issue(
        axum::extract::State(remote):axum::extract::State<RemoteState>,
        axum::Json(payload):axum::Json<Value>,
    )->axum::Json<Value>{
        let mut remote=remote.lock().unwrap();
        remote.issue_title=payload["title"].as_str().map(str::to_string);
        remote.issue_writes+=1;
        axum::Json(json!({"number":2}))
    }
    async fn fake_get_issue(
        axum::extract::State(remote):axum::extract::State<RemoteState>,
    )->axum::Json<Value>{
        let remote=remote.lock().unwrap();
        axum::Json(json!({"number":2,"title":remote.issue_title,
            "html_url":"https://github.com/octo/work/issues/2"}))
    }
    #[tokio::test]
    async fn connected_account_creates_real_http_branch_pr_issue_and_reads_ci(){
        let state:RemoteState=Default::default();
        state.lock().unwrap().branches.insert("main".into(),"a".repeat(40));
        let router=axum::Router::new()
            .route("/user/repos",axum::routing::get(||
                async{axum::Json(json!([{"full_name":"octo/work"}]))}))
            .route("/repos/octo/work/git/ref/heads/*branch",
                axum::routing::get(fake_branch))
            .route("/repos/octo/work/git/refs",axum::routing::post(fake_create_branch))
            .route("/repos/octo/work/pulls",axum::routing::post(fake_create_pr))
            .route("/repos/octo/work/pulls/1",axum::routing::get(fake_get_pr))
            .route("/repos/octo/work/issues",axum::routing::post(fake_create_issue))
            .route("/repos/octo/work/issues/2",axum::routing::get(fake_get_issue))
            .route("/repos/octo/work/actions/runs",axum::routing::get(||
                async{axum::Json(json!({"total_count":1,"workflow_runs":[{
                    "id":7,"name":"CI","status":"completed","conclusion":"success",
                    "head_sha":"x","run_number":1}]}))}))
            .with_state(state.clone());
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin=format!("http://{}/",listener.local_addr().unwrap());
        let server=tokio::spawn(async move{axum::serve(listener,router).await.unwrap();});
        let client=Client::builder().timeout(std::time::Duration::from_secs(5)).build().unwrap();
        let token="connected-gh-token";
        let repos=execute_with(&client,token,&origin,
            &json!({"operation":"repositories"}),false).await.unwrap();
        assert_eq!(repos[0]["full_name"],"octo/work");

        let created=execute_with(&client,token,&origin,
            &json!({"operation":"create_branch","owner":"octo","repo":"work",
                "from":"main","branch":"feature/new"}),true).await.unwrap();
        assert_eq!(created["verified"],true);
        assert_eq!(state.lock().unwrap().branch_writes,1);
        let duplicate=execute_with(&client,token,&origin,
            &json!({"operation":"create_branch","owner":"octo","repo":"work",
                "from":"main","branch":"feature/new"}),true).await;
        assert!(duplicate.is_err());
        assert_eq!(state.lock().unwrap().branch_writes,1);

        let pr=execute_with(&client,token,&origin,
            &json!({"operation":"open_pr","owner":"octo","repo":"work",
                "head":"feature/new","base":"main","title":"Checked code","body":"Implemented"}),true)
            .await.unwrap();
        assert_eq!(pr["verified"],true);
        assert_eq!(pr["number"],1);
        let issue=execute_with(&client,token,&origin,
            &json!({"operation":"create_issue","owner":"octo","repo":"work",
                "title":"Task","body":"Observed"}),true).await.unwrap();
        assert_eq!(issue["verified"],true);
        let ci=execute_with(&client,token,&origin,
            &json!({"operation":"ci_runs","owner":"octo","repo":"work","branch":"feature/new"}),false)
            .await.unwrap();
        assert_eq!(ci["workflow_runs"][0]["conclusion"],"success");
        let record=state.lock().unwrap();
        assert_eq!((record.branch_writes,record.pr_writes,record.issue_writes),(1,1,1));
        server.abort();
    }

    #[test]
    fn github_workflow_rejects_path_escape_and_unapproved_operation(){
        assert!(component("..").is_err());
        assert!(branch("main/../admin").is_err());
        assert!(branch("feature/new").is_ok());
        let args=json!({"operation":"create_issue","owner":"test","repo":"repo","title":"ok"});
        let runtime=tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let client=Client::new();
            assert!(execute_with(&client,"x","http://127.0.0.1:9/",&args,false).await.is_err());
            assert!(execute_with(&client,"x","http://127.0.0.1:9/",
                &json!({"operation":"issues","owner":"bad/path","repo":"repo"}),false).await.is_err());
        });
    }
    #[test]
    fn url_never_interpolates_user_input_into_host_or_query(){
        let url=uri("https://api.github.com/",&["repos","one","two","pulls"],&[
            ("branch","feature/new".into())]).unwrap();
        assert_eq!(url.host_str(),Some("api.github.com"));
        assert!(url.as_str().contains("feature%2Fnew"));
    }
}
