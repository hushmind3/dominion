//! Optional GitHub HTTP implementation.
use super::*;

pub(super) async fn github_api(ctx: &ToolContext<'_>, args: &Value) -> Result<Value, String> {
    // Use the same renewable grant as the connected-account UI instead of
    // bypassing expiry checks. The access token never leaves api.github.com.
    let token = crate::engine::oauth::fresh_access_token(ctx.state, "github").await
        .map_err(|message|format!("GitHub 계정 연결을 확인하세요: {message}"))?;
    let method = string_arg(args, "method").unwrap_or("GET").to_uppercase();
    if !matches!(method.as_str(), "GET" | "POST" | "PATCH" | "PUT" | "DELETE") {
        return Err("지원하지 않는 HTTP 메서드입니다.".into());
    }
    let path = required_arg(args, "path")?;
    if !path.starts_with('/') || path.contains("..") {
        return Err("GitHub API 경로는 / 로 시작해야 합니다.".into());
    }
    let method =
        reqwest::Method::from_bytes(method.as_bytes()).map_err(|error| error.to_string())?;
    let mut request = reqwest::Client::new()
        .request(method, format!("https://api.github.com{path}"))
        .header("Authorization", format!("Bearer {token}"))
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "DOMINION")
        .timeout(Duration::from_secs(30));
    if let Some(body) = args.get("body").filter(|value| !value.is_null()) {
        request = request.json(body);
    }
    let response = request.send().await.map_err(|error| error.to_string())?;
    let status = response.status().as_u16();
    let text = response.text().await.unwrap_or_default();
    let parsed: Value = serde_json::from_str(&text).unwrap_or_else(|_| json!(text));
    Ok(json!({
        "status": status,
        "body": parsed.to_string().chars().take(20_000).collect::<String>(),
    }))
}


pub(super) fn github_file_url(args:&Value)->Result<reqwest::Url,String>{
    let owner=required_arg(args,"owner")?;
    let repo=required_arg(args,"repo")?;
    if [owner.as_str(),repo.as_str()].iter().any(|s|s.is_empty()||s.len()>100||
        !s.bytes().all(|b|b.is_ascii_alphanumeric()||matches!(b,b'-'|b'_'|b'.'))){
        return Err("GitHub 저장소 소유자/이름이 올바르지 않습니다.".into());
    }
    let path=required_arg(args,"path")?;
    if path.len()>1024||path.starts_with('/')||path.contains('\\')||
        path.split('/').any(|s|s.is_empty()||s=="."||s==".."||s.contains('%')||s.chars().any(char::is_control)){
        return Err("GitHub 파일 경로는 저장소 내부의 상대경로여야 합니다.".into());
    }
    let mut url=reqwest::Url::parse("https://api.github.com").map_err(|e|e.to_string())?;
    {
        let mut parts=url.path_segments_mut().map_err(|_|"GitHub API URL 오류")?;
        parts.extend(["repos",owner.as_str(),repo.as_str(),"contents"]);
        for segment in path.split('/') {parts.push(segment);}
    }
    if let Some(branch)=args["branch"].as_str(){
        if branch.is_empty()||branch.len()>255||branch.chars().any(char::is_control)||
            branch.starts_with('-')||branch.contains("..")||branch.contains('@')||
            branch.contains('\\')||branch.contains(' '){
            return Err("GitHub 브랜치 이름이 올바르지 않습니다.".into());
        }
        url.query_pairs_mut().append_pair("ref",branch);
    }
    Ok(url)
}

pub(super) async fn github_file_read(ctx:&ToolContext<'_>,args:&Value)->Result<Value,String>{
    let url=github_file_url(args)?;
    let token=crate::engine::oauth::fresh_access_token(ctx.state,"github").await
        .map_err(|e|format!("GitHub 계정 연결을 확인하세요: {e}"))?;
    let response=ctx.state.provider_http_client(30)?
        .get(url).bearer_auth(token)
        .header("Accept","application/vnd.github+json")
        .header("User-Agent","DOMINION").send().await
        .map_err(|e|e.to_string())?;
    let status=response.status();
    let value:Value=response.json().await.map_err(|e|e.to_string())?;
    if !status.is_success(){
        return Err(format!("GitHub 파일 조회 실패 ({}): {}",
            status.as_u16(),value["message"].as_str().unwrap_or("권한 또는 경로를 확인하세요.")));
    }
    let content=value["content"].as_str().ok_or("GitHub 파일 본문이 없습니다.")?;
    use base64::Engine;
    let bytes=base64::engine::general_purpose::STANDARD
        .decode(content.chars().filter(|c|!c.is_whitespace()).collect::<String>())
        .map_err(|_|"GitHub 파일 인코딩이 올바르지 않습니다.")?;
    if bytes.len()>256*1024 {return Err("GitHub 파일이 256KiB를 초과해 분할 조회가 필요합니다.".into());}
    let text=String::from_utf8(bytes).map_err(|_|"바이너리 파일은 텍스트 도구로 수정할 수 없습니다.")?;
    Ok(json!({"path":value["path"],"sha":value["sha"],
        "content":text,"size":value["size"],"ref":args["branch"],
        "verified":true}))
}

pub(super) async fn github_file_update(ctx:&ToolContext<'_>,args:&Value)->Result<Value,String>{
    let expected=required_arg(args,"expectedSha")?;
    if expected.len()!=40||!expected.bytes().all(|b|b.is_ascii_hexdigit()){
        return Err("파일 수정 전 github_file_read에서 받은 40자리 SHA가 필요합니다.".into());
    }
    let content=required_arg(args,"content")?;
    if content.len()>256*1024{return Err("수정할 파일은 256KiB 이하여야 합니다.".into());}
    let message=required_arg(args,"message")?;
    if message.trim().is_empty()||message.len()>1024{return Err("커밋 메시지는 1~1024자여야 합니다.".into());}
    if args["branch"].as_str().is_none(){return Err("수정할 브랜치를 지정해야 합니다.".into());}
    let mut url=github_file_url(args)?;
    // GitHub Contents write endpoint uses the branch in the JSON body.
    url.set_query(None);
    let token=crate::engine::oauth::fresh_access_token(ctx.state,"github").await
        .map_err(|e|format!("GitHub 계정 연결을 확인하세요: {e}"))?;
    use base64::Engine;
    let body=json!({"message":message,"sha":expected,"branch":args["branch"],
        "content":base64::engine::general_purpose::STANDARD.encode(content.as_bytes())});
    let client=ctx.state.provider_http_client(30)?;
    let result=client.put(url.clone()).bearer_auth(&token)
        .header("Accept","application/vnd.github+json")
        .header("User-Agent","DOMINION").json(&body)
        .send().await.map_err(|e|e.to_string())?;
    let status=result.status();
    let changed:Value=result.json().await.map_err(|e|e.to_string())?;
    if !status.is_success(){
        return Err(format!("GitHub 커밋 실패 ({}): {}",
            status.as_u16(),changed["message"].as_str().unwrap_or("권한, SHA 또는 브랜치를 확인하세요.")));
    }
    let commit_sha=changed["commit"]["sha"].as_str().unwrap_or("");
    let file_sha=changed["content"]["sha"].as_str().unwrap_or("");
    if commit_sha.is_empty()||file_sha.is_empty(){
        return Err("GitHub 응답에 커밋 또는 파일 SHA가 없습니다. 원격에서 확인하세요.".into());
    }
    url.query_pairs_mut().append_pair("ref",&required_arg(args,"branch")?);
    let confirmed=client.get(url).bearer_auth(&token)
        .header("Accept","application/vnd.github+json")
        .header("User-Agent","DOMINION").send().await.map_err(|e|e.to_string())?;
    if !confirmed.status().is_success(){
        return Err(format!("커밋은 생성됐지만 원격 파일 재조회가 실패했습니다: {commit_sha}"));
    }
    let check:Value=confirmed.json().await.map_err(|e|e.to_string())?;
    if check["sha"]!=file_sha{
        return Err(format!("커밋은 생성됐지만 원격 파일 SHA 검증이 일치하지 않습니다: {commit_sha}"));
    }
    Ok(json!({"commitSha":commit_sha,"fileSha":file_sha,
        "path":args["path"],"branch":args["branch"],"verified":true,
        "remoteReadBack":true}))
}

