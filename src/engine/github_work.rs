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
    reqwest::Client::builder().timeout(Duration::from_secs(30))
        .build().map_err(|e|e.to_string())
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
    let receipt=response.json::<Value>().await.map_err(|_|"커밋 성공 응답을 파싱하지 못했습니다. 원격 파일을 확인하세요.")?;
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

#[cfg(test)]
mod tests {
    use super::*;
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
