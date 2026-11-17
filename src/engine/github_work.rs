//! Bound GitHub file workflow: connected account -> repository -> source SHA ->
//! approved commit -> remote re-read. Never sends OAuth tokens to an MCP URL.
use crate::state::AppState;
use base64::{engine::general_purpose::STANDARD,Engine as _};
use serde_json::{json,Value};
use std::time::Duration;

#[derive(Debug)]
struct Target<'a>{owner:&'a str,repo:&'a str,path:&'a str,branch:Option<&'a str>}
impl<'a> Target<'a>{
    fn parse(args:&'a Value)->Result<Self,String>{
        let part=|k:&str|args[k].as_str().filter(|v|!v.trim().is_empty())
            .ok_or_else(||format!("GitHub {k} 값이 필요합니다."));
        let owner=part("owner")?;
        let repo=part("repo")?;
        let path=part("path")?;
        for component in [owner,repo]{
            if component.len()>100 || !component.bytes().all(|b|
                b.is_ascii_alphanumeric() || matches!(b,b'-'|b'_'|b'.')){
                return Err("GitHub owner/repo 이름이 올바르지 않습니다.".into());
            }
        }
        if path.len()>1024 || path.starts_with('/') ||
            path.split('/').any(|segment|segment.is_empty() || matches!(segment,"."|"..") ||
                segment.chars().any(char::is_control)){
            return Err("저장소 안의 상대 파일 경로를 지정하세요.".into());
        }
        let branch=args["branch"].as_str().filter(|v|!v.is_empty());
        if branch.is_some_and(|b|b.len()>256 || b.chars().any(char::is_control)){
            return Err("GitHub 브랜치 이름이 올바르지 않습니다.".into());
        }
        Ok(Self{owner,repo,path,branch})
    }

    fn url(&self)->Result<reqwest::Url,String>{
        let mut url=reqwest::Url::parse("https://api.github.com/repos/")
            .map_err(|e|e.to_string())?;
        url.path_segments_mut().map_err(|_|"GitHub URL 조립 실패")?
            .push(self.owner).push(self.repo).push("contents")
            .extend(self.path.split('/'));
        if let Some(branch)=self.branch {
            url.query_pairs_mut().append_pair("ref",branch);
        }
        Ok(url)
    }
    fn repo_url(&self)->Result<reqwest::Url,String>{
        let mut url=reqwest::Url::parse("https://api.github.com/repos/")
            .map_err(|e|e.to_string())?;
        url.path_segments_mut().map_err(|_|"GitHub URL 조립 실패")?
            .push(self.owner).push(self.repo);
        Ok(url)
    }
}

fn client()->Result<reqwest::Client,String>{
    // A cloned reqwest Client shares its connection pool: repeated repository
    // discovery / read / verification does not re-create TCP/TLS handshakes.
    static CLIENT:std::sync::OnceLock<Result<reqwest::Client,String>>=
        std::sync::OnceLock::new();
    CLIENT.get_or_init(||
        reqwest::Client::builder().timeout(Duration::from_secs(30))
            .build().map_err(|e|e.to_string())
    ).clone()
}
fn request(client:&reqwest::Client,url:reqwest::Url,token:&str)
    ->reqwest::RequestBuilder {
    client.get(url).bearer_auth(token)
        .header("Accept","application/vnd.github+json")
        .header("User-Agent","DOMINION")
}
async fn get_json(client:&reqwest::Client,url:reqwest::Url,token:&str)
    ->Result<(u16,Value),String>{
    let response=request(client,url,token).send().await
        .map_err(|error|format!("GitHub 조회 실패: {error}"))?;
    let status=response.status().as_u16();
    let body=response.json::<Value>().await
        .map_err(|error|format!("GitHub 응답 해석 실패: {error}"))?;
    Ok((status,body))
}
fn http_error(status:u16)->String{
    match status{
        401=>"GitHub 계정 인증이 만료되었습니다. 계정을 다시 연결하세요.".into(),
        403=>"GitHub 저장소 권한이 부족합니다. Contents 권한과 접근 가능한 저장소를 확인해 재연결하세요.".into(),
        404=>"GitHub 저장소 또는 파일에 접근할 수 없습니다. 연결 계정과 경로를 확인하세요.".into(),
        409=>"원격 브랜치가 변경되었습니다. 파일을 다시 읽고 수정하세요.".into(),
        _=>format!("GitHub 요청 실패 (HTTP {status})"),
    }
}
fn decoded_file(body:&Value)->Result<String,String>{
    if body["type"]!="file" || body["encoding"]!="base64"{
        return Err("GitHub 파일 응답이 아니거나 지원하지 않는 인코딩입니다.".into());
    }
    let encoded=body["content"].as_str().ok_or("GitHub 파일 내용이 없습니다.")?;
    if encoded.len()>1_500_000{return Err("파일이 너무 큽니다. 작은 파일을 선택하세요.".into());}
    let stripped=encoded.chars().filter(|c|!c.is_ascii_whitespace()).collect::<String>();
    let bytes=STANDARD.decode(stripped).map_err(|_|"GitHub 파일 base64 오류")?;
    String::from_utf8(bytes).map_err(|_|"UTF-8 텍스트 파일만 지원합니다.".into())
}

/// The GitHub OAuth grant is the only credential used here. A repo result
/// is descriptive, never a write authorization by itself: the commit path
/// rechecks permissions, branch state and content SHA immediately before PUT.
fn repo_summaries(body:&Value,query:&str,writable_only:bool)->Result<Vec<Value>,String>{
    let rows=body.as_array().ok_or("GitHub 저장소 목록 형식이 올바르지 않습니다.")?;
    let search=query.to_lowercase();
    Ok(rows.iter().filter_map(|item|{
        let owner=item["owner"]["login"].as_str()?;
        let repo=item["name"].as_str()?;
        let fullname=item["full_name"].as_str()?;
        let can_write=item["permissions"]["push"].as_bool()==Some(true);
        let can_read=item["permissions"]["pull"].as_bool()==Some(true)
            || can_write || item["permissions"]["admin"].as_bool()==Some(true);
        if !can_read || writable_only && !can_write ||
            !fullname.to_lowercase().contains(&search){return None;}
        Some(json!({
            "owner":owner,"repo":repo,"fullName":fullname,
            "defaultBranch":item["default_branch"],"private":item["private"],
            "canRead":can_read,"canWrite":can_write,
            "archived":item["archived"],"disabled":item["disabled"],
            "url":item["html_url"],
        }))
    }).collect())
}

/// List only repositories visible to the *connected user*. The model chooses
/// an explicit owner/repo after this read, then calls github_file_read before
/// any github_file_commit (the latter still requires owner approval).
pub async fn repositories(state:&AppState,args:&Value)->Result<Value,String>{
    let page=match args.get("page"){
        None=>1,
        Some(value)=>value.as_u64().filter(|page|(1..=10).contains(page))
            .ok_or("GitHub 저장소 페이지는 1~10 사이 정수여야 합니다.")?,
    };
    let query=match args.get("query"){
        None=>"",
        Some(value)=>value.as_str().ok_or("GitHub 저장소 검색어는 문자열이어야 합니다.")?,
    };
    if query.chars().count()>100{
        return Err("저장소 검색어는 최대 100자입니다.".into());
    }
    let writable_only=match args.get("writableOnly"){
        None=>false,
        Some(value)=>value.as_bool().ok_or("writableOnly는 참/거짓이어야 합니다.")?,
    };
    let token=crate::engine::oauth::fresh_access_token(state,"github").await
        .map_err(|error|format!("{error} GitHub 계정을 연결하거나 권한을 확인하세요."))?;
    let mut url=reqwest::Url::parse("https://api.github.com/user/repos")
        .map_err(|e|e.to_string())?;
    url.query_pairs_mut().append_pair("sort","updated")
        .append_pair("direction","desc").append_pair("per_page","50")
        .append_pair("page",&page.to_string());
    let (status,body)=get_json(&client()?,url,&token).await?;
    if status>=400{return Err(http_error(status));}
    let received=body.as_array().map(Vec::len)
        .ok_or("GitHub 저장소 목록 형식이 올바르지 않습니다.")?;
    let repos=repo_summaries(&body,query,writable_only)?;
    Ok(json!({
        "repositories":repos,"page":page,"hasMore":received==50,
        "filteredOnPage":true,
        "nextStep":"Choose owner/repo and defaultBranch. Call github_file_read before any existing-file commit.",
    }))
}

pub async fn read(state:&AppState,args:&Value)->Result<Value,String>{
    let target=Target::parse(args)?;
    let token=crate::engine::oauth::fresh_access_token(state,"github").await?;
    let (status,body)=get_json(&client()?,target.url()?,&token).await?;
    if status>=400{return Err(http_error(status));}
    let content=decoded_file(&body)?;
    Ok(json!({
        "repository":format!("{}/{}",target.owner,target.repo),
        "branch":target.branch,"path":target.path,
        "sha":body["sha"],"content":content,"size":content.len(),
        "verified":true
    }))
}

pub async fn commit(state:&AppState,args:&Value)->Result<Value,String>{
    let target=Target::parse(args)?;
    let content=args["content"].as_str().ok_or("content가 필요합니다.")?;
    let message=args["message"].as_str().map(str::trim).filter(|s|!s.is_empty())
        .ok_or("커밋 메시지가 필요합니다.")?;
    if content.len()>256_000{return Err("파일 변경은 256KB 이내로 제한합니다.".into());}
    if message.len()>500{return Err("커밋 메시지는 500자 이내로 입력하세요.".into());}
    let expected=args["expectedSha"].as_str().filter(|s|!s.is_empty());
    if expected.is_some_and(|s|s.len()!=40 || !s.bytes().all(|b|b.is_ascii_hexdigit())){
        return Err("expectedSha는 이전 파일 조회에서 받은 40자리 SHA여야 합니다.".into());
    }
    let token=crate::engine::oauth::fresh_access_token(state,"github").await?;
    let client=client()?;
    // Check actual repository write rights before any mutation. The Contents
    // endpoint remains authoritative for branch-specific protection rules.
    let (repo_status,repository)=get_json(&client,target.repo_url()?,&token).await?;
    if repo_status>=400{return Err(http_error(repo_status));}
    if repository["permissions"]["push"]==false {
        return Err("연결 계정에 해당 저장소의 쓰기 권한이 없습니다.".into());
    }
    let url=target.url()?;
    let (status,previous)=get_json(&client,url.clone(),&token).await?;
    let old_sha=match status {
        200=>{
            if previous["type"]!="file"{return Err("이 경로는 파일이 아닙니다.".into());}
            previous["sha"].as_str().ok_or("기존 파일 SHA를 조회하지 못했습니다.")?
        },
        404=>"",
        _=>return Err(http_error(status))
    };
    // Explicit optimistic concurrency: editing a previously unseen file
    // requires a read first, never silently overwrites another author.
    match (old_sha.is_empty(),expected) {
        (true,None)=>{},
        (true,Some(_))=>return Err("파일이 삭제되었습니다. 저장소를 다시 읽으세요.".into()),
        (false,None)=>return Err("기존 파일을 github_file_read로 먼저 읽고 expectedSha를 전달하세요.".into()),
        (false,Some(expected)) if old_sha!=expected=>
            return Err("원격 파일 SHA가 변경되었습니다. 파일을 다시 읽으세요.".into()),
        _=>{},
    }
    if !old_sha.is_empty() && decoded_file(&previous)?==content {
        return Ok(json!({"success":true,"verified":true,"unchanged":true,
            "sha":old_sha,"repository":format!("{}/{}",target.owner,target.repo),
            "branch":target.branch,"path":target.path}));
    }
    let mut body=json!({"message":message,"content":STANDARD.encode(content.as_bytes())});
    if !old_sha.is_empty(){body["sha"]=json!(old_sha);}
    if let Some(branch)=target.branch{body["branch"]=json!(branch);}
    let outcome=client.put({
        let mut uri=url.clone();
        uri.set_query(None);
        uri
    }).bearer_auth(&token)
        .header("Accept","application/vnd.github+json")
        .header("User-Agent","DOMINION")
        .json(&body).send().await;
    let response=match outcome{
        Ok(response)=>response,
        Err(error)=>return Ok(json!({
            "success":false,"outcomeUnknown":true,"requiresInspection":true,
            "error":format!("GitHub 커밋 응답을 받지 못했습니다: {error}"),
            "repository":format!("{}/{}",target.owner,target.repo),
            "branch":target.branch,"path":target.path,
        }))
    };
    let put_status=response.status().as_u16();
    if put_status>=500 {
        return Ok(json!({"success":false,"outcomeUnknown":true,
            "requiresInspection":true,"status":put_status,
            "error":"서버 오류로 커밋 적용 여부가 불확실합니다. 파일을 다시 조회하세요."}));
    }
    if put_status>=400{return Err(http_error(put_status));}
    let receipt=match response.json::<Value>().await{
        Ok(receipt)=>receipt,
        Err(_)=>return Ok(json!({"success":false,"outcomeUnknown":true,
            "requiresInspection":true,
            "error":"GitHub 파일 커밋 후 응답 해석 실패. 원격 파일을 확인하세요."})),
    };
    let commit_sha=receipt["commit"]["sha"].clone();
    // Re-read what GitHub actually stores rather than trusting an HTTP 201
    // or a model's success text. If this final request fails, never retry PUT.
    let observed=get_json(&client,url,&token).await;
    let verified=match observed{
        Ok((200,body))=>body["sha"]==receipt["content"]["sha"]
            && decoded_file(&body).is_ok_and(|saved|saved==content),
        _=>false
    };
    if !verified {
        return Ok(json!({"success":false,"outcomeUnknown":true,
            "requiresInspection":true,"commitSha":commit_sha,
            "error":"커밋 요청 후 원격 파일 재조회가 일치하지 않습니다. 재실행하지 말고 확인하세요.",
            "repository":format!("{}/{}",target.owner,target.repo),
            "branch":target.branch,"path":target.path}));
    }
    Ok(json!({"success":true,"verified":true,"commitSha":commit_sha,
        "sha":receipt["content"]["sha"],
        "repository":format!("{}/{}",target.owner,target.repo),
        "branch":target.branch,"path":target.path}))
}

// Typed GitHub actions keep repository selection, permissions, and API
// receipts in Rust instead of making the model invent REST request bodies.
fn repo_arg<'a>(args:&'a Value,key:&str)->Result<&'a str,String>{
    let value=args[key].as_str().filter(|s|!s.is_empty()&&s.len()<=100)
        .ok_or_else(||format!("GitHub {key} 값이 필요합니다."))?;
    if !value.bytes().all(|c|c.is_ascii_alphanumeric()||matches!(c,b'_'|b'-'|b'.')){
        return Err(format!("GitHub {key} 형식이 올바르지 않습니다."));
    }
    Ok(value)
}
fn branch_arg<'a>(args:&'a Value,key:&str)->Result<&'a str,String>{
    let value=args[key].as_str().filter(|s|!s.is_empty()&&s.len()<=160)
        .ok_or_else(||format!("GitHub {key} 브랜치가 필요합니다."))?;
    if value.starts_with('/') || value.ends_with('/') || value.contains("..") ||
        value.split('/').any(|p|p.is_empty() || p.starts_with('.') || p.ends_with('.')) ||
        !value.bytes().all(|c|c.is_ascii_alphanumeric()||matches!(c,b'-'|b'_'|b'.'|b'/')){
        return Err("GitHub 브랜치 이름이 올바르지 않습니다.".into());
    }
    Ok(value)
}
fn repo_endpoint(owner:&str,repo:&str,more:&[&str])->Result<reqwest::Url,String>{
    let mut uri=reqwest::Url::parse("https://api.github.com/repos/")
        .map_err(|e|e.to_string())?;
    uri.path_segments_mut().map_err(|_|"GitHub 경로 오류")?
        .push(owner).push(repo).extend(more.iter().copied());
    Ok(uri)
}
async fn writable_repo(client:&reqwest::Client,token:&str,owner:&str,repo:&str)
    ->Result<Value,String>{
    let (code,data)=get_json(client,repo_endpoint(owner,repo,&[])?,token).await?;
    if code>=400{return Err(http_error(code));}
    if data["permissions"]["push"]!=true{
        return Err("연결한 GitHub 계정에 이 저장소의 쓰기 권한이 없습니다.".into());
    }
    if data["archived"]==true || data["disabled"]==true{
        return Err("보관 또는 비활성화된 저장소는 변경하지 않습니다.".into());
    }
    Ok(data)
}
fn list_url(owner:&str,repo:&str,resource:&str)->Result<reqwest::Url,String>{
    let mut url=repo_endpoint(owner,repo,&[resource])?;
    url.query_pairs_mut().append_pair("per_page","30").append_pair("state","open");
    Ok(url)
}
pub async fn activity(state:&AppState,args:&Value)->Result<Value,String>{
    let owner=repo_arg(args,"owner")?;
    let repo=repo_arg(args,"repo")?;
    let token=crate::engine::oauth::fresh_access_token(state,"github").await?;
    let client=client()?;
    let (issue_status,issue_data)=get_json(&client,list_url(owner,repo,"issues")?,&token).await?;
    if issue_status>=400{return Err(http_error(issue_status));}
    let (pr_status,pr_data)=get_json(&client,list_url(owner,repo,"pulls")?,&token).await?;
    if pr_status>=400{return Err(http_error(pr_status));}
    let issues=issue_data.as_array().ok_or("GitHub 이슈 응답 형식 오류")?
        .iter().filter(|v|v.get("pull_request").is_none())
        .map(|v|json!({"number":v["number"],"title":v["title"],
            "state":v["state"],"url":v["html_url"]})).collect::<Vec<_>>();
    let prs=pr_data.as_array().ok_or("GitHub PR 응답 형식 오류")?
        .iter().map(|v|json!({"number":v["number"],"title":v["title"],
            "head":v["head"]["ref"],"base":v["base"]["ref"],
            "url":v["html_url"]})).collect::<Vec<_>>();
    Ok(json!({"repository":format!("{owner}/{repo}"),
        "issues":issues,"pullRequests":prs,"pageSize":30}))
}
async fn write_json(client:&reqwest::Client,token:&str,url:reqwest::Url,body:Value)
    ->Result<Value,String>{
    let response=client.post(url).bearer_auth(token)
        .header("Accept","application/vnd.github+json")
        .header("User-Agent","DOMINION")
        .json(&body).send().await;
    let response=match response{
        Ok(r)=>r,
        Err(e)=>return Ok(json!({"outcomeUnknown":true,"requiresReview":true,
            "error":format!("원격 GitHub 쓰기 응답이 없습니다: {e}")}))
    };
    let code=response.status().as_u16();
    if code>=500{
        return Ok(json!({"outcomeUnknown":true,"requiresReview":true,
            "status":code,"error":"GitHub 서버 오류. 원격 상태를 직접 다시 조회하세요."}));
    }
    if code>=400{return Err(http_error(code));}
    match response.json::<Value>().await {
        Ok(body)=>Ok(body),
        Err(_)=>Ok(json!({"outcomeUnknown":true,"requiresReview":true,
            "error":"GitHub 쓰기 후 성공 응답 해석 실패. 원격 결과를 확인하고 중복 생성하지 마세요."}))
    }
}
pub async fn issue_create(state:&AppState,args:&Value)->Result<Value,String>{
    let owner=repo_arg(args,"owner")?;
    let repo=repo_arg(args,"repo")?;
    let title=args["title"].as_str().filter(|s|!s.trim().is_empty()&&s.len()<=240)
        .ok_or("GitHub 이슈 제목은 240자 이내여야 합니다.")?;
    let body=args["body"].as_str().unwrap_or("");
    if body.len()>30_000{return Err("이슈 본문은 30000자 이내여야 합니다.".into());}
    let token=crate::engine::oauth::fresh_access_token(state,"github").await?;
    let client=client()?;
    writable_repo(&client,&token,owner,repo).await?;
    let result=write_json(&client,&token,repo_endpoint(owner,repo,&["issues"])?,
        json!({"title":title,"body":body})).await?;
    if result["outcomeUnknown"]==true{return Ok(result);}
    let number=result["number"].as_u64().ok_or("이슈 번호가 없는 응답입니다.")?;
    let observed=get_json(&client,
        repo_endpoint(owner,repo,&["issues",&number.to_string()])?,&token).await;
    let verified=observed.is_ok_and(|(code,seen)|
        code==200 && seen["title"]==title && seen["body"]==body);
    Ok(json!({"repository":format!("{owner}/{repo}"),"number":number,
        "url":result["html_url"],"verified":verified,
        "outcomeUnknown":!verified,"requiresReview":!verified}))
}
pub async fn branch_create(state:&AppState,args:&Value)->Result<Value,String>{
    let owner=repo_arg(args,"owner")?;
    let repo=repo_arg(args,"repo")?;
    let name=branch_arg(args,"newBranch")?;
    let token=crate::engine::oauth::fresh_access_token(state,"github").await?;
    let client=client()?;
    let repo_info=writable_repo(&client,&token,owner,repo).await?;
    let source=match args["baseBranch"].as_str(){
        Some(_) => branch_arg(args,"baseBranch")?,
        None=>repo_info["default_branch"].as_str().ok_or("저장소 기본 브랜치를 찾을 수 없습니다.")?
    };
    let mut base_ref=vec!["git","ref","heads"];
    base_ref.extend(source.split('/'));
    let (code,base)=get_json(&client,repo_endpoint(owner,repo,&base_ref)?,&token).await?;
    if code>=400{return Err(http_error(code));}
    let sha=base["object"]["sha"].as_str().ok_or("기준 브랜치 SHA가 없습니다.")?;
    let payload=json!({"ref":format!("refs/heads/{name}"),"sha":sha});
    let response=write_json(&client,&token,repo_endpoint(owner,repo,&["git","refs"])?,payload).await?;
    if response["outcomeUnknown"]==true{return Ok(response);}
    let mut branch_path=vec!["git","ref","heads"];
    branch_path.extend(name.split('/'));
    let observed=get_json(&client,repo_endpoint(owner,repo,&branch_path)?,&token).await;
    let verified=observed.is_ok_and(|(code,data)|code==200 && data["object"]["sha"]==sha);
    Ok(json!({"repository":format!("{owner}/{repo}"),"branch":name,
        "baseBranch":source,"sha":sha,"verified":verified,
        "outcomeUnknown":!verified,"requiresReview":!verified}))
}
pub async fn pr_create(state:&AppState,args:&Value)->Result<Value,String>{
    let owner=repo_arg(args,"owner")?;
    let repo=repo_arg(args,"repo")?;
    let head=branch_arg(args,"head")?;
    let title=args["title"].as_str().filter(|s|!s.trim().is_empty()&&s.len()<=240)
        .ok_or("PR 제목은 240자 이내여야 합니다.")?;
    let body=args["body"].as_str().unwrap_or("");
    if body.len()>30_000{return Err("PR 본문은 30000자 이내여야 합니다.".into());}
    let token=crate::engine::oauth::fresh_access_token(state,"github").await?;
    let client=client()?;
    let info=writable_repo(&client,&token,owner,repo).await?;
    let base=match args["base"].as_str(){
        Some(_)=>branch_arg(args,"base")?,
        None=>info["default_branch"].as_str().ok_or("기본 브랜치를 찾지 못했습니다.")?
    };
    if head==base{return Err("같은 브랜치끼리 PR을 만들 수 없습니다.".into());}
    let result=write_json(&client,&token,repo_endpoint(owner,repo,&["pulls"])?,
        json!({"title":title,"body":body,"head":head,"base":base})).await?;
    if result["outcomeUnknown"]==true{return Ok(result);}
    let number=result["number"].as_u64().ok_or("PR 번호가 없는 응답입니다.")?;
    let observed=get_json(&client,
        repo_endpoint(owner,repo,&["pulls",&number.to_string()])?,&token).await;
    let verified=observed.is_ok_and(|(code,seen)|
        code==200 && seen["title"]==title &&
        seen["head"]["ref"]==head && seen["base"]["ref"]==base);
    Ok(json!({"repository":format!("{owner}/{repo}"),"number":number,
        "url":result["html_url"],"verified":verified,
        "outcomeUnknown":!verified,"requiresReview":!verified}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn typed_branch_and_repository_routing_rejects_host_or_path_injection(){
        let params=json!({"owner":"team","repo":"project","head":"feature/test","newBranch":"topic/changes"});
        assert_eq!(repo_arg(&params,"owner").unwrap(),"team");
        assert_eq!(branch_arg(&params,"newBranch").unwrap(),"topic/changes");
        let uri=repo_endpoint("team","project",&["pulls"]).unwrap();
        assert_eq!(uri.host_str(),Some("api.github.com"));
        assert!(branch_arg(&json!({"newBranch":"../main"}),"newBranch").is_err());
        assert!(repo_arg(&json!({"owner":"bad/path"}),"owner").is_err());
    }
    #[test]
    fn repository_discovery_uses_only_explicit_git_permissions_and_no_secrets(){
        let items=json!([
            {"owner":{"login":"dev"},"name":"private-project",
              "full_name":"dev/private-project","default_branch":"dev",
              "private":true,"permissions":{"pull":true,"push":false}},
            {"owner":{"login":"org"},"name":"write-project",
              "full_name":"org/write-project","default_branch":"main",
              "private":false,"permissions":{"pull":true,"push":true},
              "html_url":"https://github.com/org/write-project"},
            {"owner":{"login":"org"},"name":"hidden-project",
              "full_name":"org/hidden-project",
              "permissions":{"pull":false,"push":false}},
        ]);
        let list=repo_summaries(&items,"project",false).unwrap();
        assert_eq!(list.len(),2);
        assert_eq!(list[0]["canWrite"],false);
        assert_eq!(list[1]["defaultBranch"],"main");
        assert!(list[0].get("accessToken").is_none());
        assert_eq!(repo_summaries(&items,"",true).unwrap().len(),1);
        assert!(repo_summaries(&json!({"error":"invalid"}),"",false).is_err());
    }
    #[test]
    fn target_rejects_traversal_and_keeps_branch_as_query_not_path(){
        for p in ["../secrets","/abs.txt","src/../x","src//x","./x"]{
            assert!(Target::parse(&json!({"owner":"acme","repo":"core","path":p})).is_err());
        }
        let args=json!({"owner":"acme","repo":"code","path":"src/my file.rs",
            "branch":"feature/nested"});
        let t=Target::parse(&args).unwrap();
        let url=t.url().unwrap();
        assert_eq!(url.host_str(),Some("api.github.com"));
        assert!(url.as_str().contains("src/my%20file.rs"));
        assert_eq!(url.query_pairs().find(|(k,_)|k=="ref").unwrap().1,"feature/nested");
    }
    #[test]
    fn only_base64_text_files_are_admitted(){
        let encoded=STANDARD.encode("안녕하세요".as_bytes());
        assert_eq!(decoded_file(&json!({"type":"file","encoding":"base64",
            "content":encoded})).unwrap(),"안녕하세요");
        assert!(decoded_file(&json!({"type":"dir","encoding":"base64",
            "content":"QQ=="})).is_err());
    }
}
