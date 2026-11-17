//! Connected-account GitHub repository discovery for the native tool route.
//! Returned metadata is never a write grant: commits still require approval and observed SHA.
use super::ToolContext;
use serde_json::{json, Value};

fn summaries(payload:&Value,query:&str,writable_only:bool)->Result<Vec<Value>,String>{
    let rows=payload.as_array().ok_or("GitHub 저장소 목록이 배열이 아닙니다.")?;
    let needle=query.to_lowercase();
    Ok(rows.iter().filter_map(|item|{
        let owner=item["owner"]["login"].as_str()?;
        let repo=item["name"].as_str()?;
        let full_name=item["full_name"].as_str()?;
        let permissions=&item["permissions"];
        let can_write=permissions["push"].as_bool()==Some(true)
            ||permissions["admin"].as_bool()==Some(true);
        let can_read=permissions["pull"].as_bool()==Some(true)||can_write;
        if !can_read || (writable_only&&!can_write)
            || !full_name.to_lowercase().contains(&needle){return None;}
        Some(json!({
            "owner":owner,"repo":repo,"fullName":full_name,
            "defaultBranch":item["default_branch"],"private":item["private"],
            "canRead":can_read,"canWrite":can_write,"archived":item["archived"],
            "disabled":item["disabled"],"url":item["html_url"]
        }))
    }).collect())
}

pub(super) async fn list(ctx:&ToolContext<'_>,args:&Value)->Result<Value,String>{
    let page=match args.get("page"){
        None=>1,
        Some(value)=>value.as_u64().filter(|page|(1..=10).contains(page))
            .ok_or("GitHub 저장소 페이지는 1~10 사이 정수여야 합니다.")?,
    };
    let query=match args.get("query"){
        None=>"",
        Some(value)=>value.as_str().ok_or("저장소 검색어는 문자열이어야 합니다.")?,
    };
    if query.chars().count()>100{return Err("저장소 검색어는 최대 100자입니다.".into());}
    let writable_only=match args.get("writableOnly"){
        None=>false,
        Some(value)=>value.as_bool().ok_or("writableOnly는 참/거짓이어야 합니다.")?,
    };
    let token=crate::engine::oauth::fresh_access_token(ctx.state,"github").await
        .map_err(|error|format!("GitHub 계정을 다시 연결하거나 권한을 확인하세요: {error}"))?;
    let mut url=reqwest::Url::parse("https://api.github.com/user/repos")
        .map_err(|e|e.to_string())?;
    url.query_pairs_mut().append_pair("sort","updated")
        .append_pair("direction","desc").append_pair("per_page","50")
        .append_pair("page",&page.to_string());
    let response=ctx.state.provider_http_client(30)?
        .get(url).bearer_auth(token)
        .header("Accept","application/vnd.github+json")
        .header("User-Agent","DOMINION").send().await
        .map_err(|error|error.to_string())?;
    let status=response.status();
    let body:Value=response.json().await.map_err(|error|error.to_string())?;
    if !status.is_success(){
        let message=body["message"].as_str().unwrap_or("계정과 저장소 권한을 확인하세요.");
        return Err(format!("GitHub 저장소 조회 실패 ({}): {message}",status.as_u16()));
    }
    let received=body.as_array().ok_or("GitHub 저장소 목록이 배열이 아닙니다.")?.len();
    let repositories=summaries(&body,query,writable_only)?;
    Ok(json!({
        "repositories":repositories,"page":page,"hasMore":received==50,
        "filteredOnPage":true,
        "nextStep":"Select owner, repo and defaultBranch. Use github_file_read before github_file_update."
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn discovery_filters_owner_name_and_push_permission(){
        let data=json!([
            {"name":"one","full_name":"team/one","owner":{"login":"team"},
             "permissions":{"pull":true,"push":true},"default_branch":"main",
             "private":true,"html_url":"https://github.com/team/one"},
            {"name":"other","full_name":"else/other","owner":{"login":"else"},
             "permissions":{"pull":true,"push":false},"default_branch":"stable"},
            {"name":"hidden","full_name":"hidden/repo","owner":{"login":"hidden"},
             "permissions":{"pull":false,"push":false}}
        ]);
        assert_eq!(summaries(&data,"TEAM",false).unwrap().len(),1);
        assert_eq!(summaries(&data,"",true).unwrap().len(),1);
        assert_eq!(summaries(&data,"",false).unwrap().len(),2);
        assert_eq!(summaries(&data,"",false).unwrap()[0]["canWrite"],true);
        assert!(summaries(&json!({}),"",false).is_err());
    }
}
