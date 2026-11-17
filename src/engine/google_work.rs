//! Task-level Google Workspace adapter with a fixed host allowlist.
//! Only Google's actually granted OAuth scopes unlock these tools.
//! Mutations require the normal Rust tool approval and are never blindly retried.
use crate::state::AppState;
use reqwest::{Client,Method,Url};
use serde_json::{json,Value};
use std::{sync::OnceLock,time::Duration};

fn client()->Result<Client,String>{
    static INSTANCE:OnceLock<Result<Client,String>>=OnceLock::new();
    INSTANCE.get_or_init(||Client::builder().timeout(Duration::from_secs(25))
        .build().map_err(|e|e.to_string())).clone()
}
fn required<'a>(args:&'a Value,key:&str,max:usize)->Result<&'a str,String>{
    args[key].as_str().filter(|v|!v.trim().is_empty()&&v.len()<=max)
        .ok_or_else(||format!("{key} 값이 없거나 너무 깁니다."))
}
fn id(raw:&str)->Result<&str,String>{
    if raw.len()<4||raw.len()>180||!raw.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'-'||b==b'_'){
        return Err("Google 문서 ID가 올바르지 않습니다.".into());
    }
    Ok(raw)
}
fn url(host:&str,segments:&[&str])->Result<Url,String>{
    if !matches!(host,"www.googleapis.com"|"docs.googleapis.com"|"sheets.googleapis.com"){
        return Err("Google API 호스트가 허용되지 않습니다.".into());
    }
    let mut url=Url::parse(&format!("https://{host}/")).map_err(|e|e.to_string())?;
    url.path_segments_mut().map_err(|_|"Google API 경로 오류")?
        .extend(segments.iter().copied());
    Ok(url)
}
fn sheet_url(spreadsheet:&str,range:&str)->Result<Url,String>{
    if range.is_empty()||range.len()>160||range.chars().any(char::is_control)||
        !range.chars().any(|ch|ch.is_ascii_alphabetic()){
        return Err("Google Sheets A1 범위가 올바르지 않습니다.".into());
    }
    url("sheets.googleapis.com",&["v4","spreadsheets",id(spreadsheet)?,"values",range])
}
async fn request(state:&AppState,scope:&str,method:Method,url:Url,body:Option<Value>)
    ->Result<Value,String>{
    if !crate::engine::oauth::has_granted_scope(state,"google",scope){
        return Err(format!("Google {scope} 권한이 없습니다. 연결 메뉴에서 Google을 재연결해 승인하세요."));
    }
    let token=crate::engine::oauth::fresh_access_token(state,"google").await?;
    let mut req=client()?.request(method,url).bearer_auth(token)
        .header("User-Agent","DOMINION");
    if let Some(body)=body{req=req.json(&body);}
    let response=req.send().await.map_err(|e|format!("Google API 연결 오류: {e}"))?;
    let status=response.status();
    if response.content_length().is_some_and(|len|len>1_000_000){
        return Err("Google 응답이 1MB를 넘습니다. 조회 범위를 줄이세요.".into());
    }
    let bytes=response.bytes().await.map_err(|e|format!("Google 응답 수신 실패: {e}"))?;
    if bytes.len()>1_000_000{return Err("Google 응답이 1MB를 넘습니다.".into());}
    if !status.is_success(){
        return Err(match status.as_u16(){
            401=>"Google 연결이 만료됐습니다. 다시 로그인하세요.".into(),
            403=>"Google 작업 권한 또는 Workspace API 활성화를 확인하세요.".into(),
            404=>"요청한 Google 문서에 접근할 수 없습니다.".into(),
            429=>"Google API 요청 한도에 도달했습니다.".into(),
            other=>format!("Google API 오류 (HTTP {other})")
        });
    }
    serde_json::from_slice(&bytes).map_err(|_|"Google JSON 응답 해석 실패".into())
}
fn document_text(node:&Value,out:&mut String){
    if out.len()>24_000{return;}
    if let Some(text)=node.pointer("/textRun/content").and_then(Value::as_str){
        out.push_str(text);return;
    }
    match node{
        Value::Array(items)=>for item in items{document_text(item,out);if out.len()>24_000{break;}},
        Value::Object(fields)=>for (key,item) in fields{
            if matches!(key.as_str(),"body"|"content"|"paragraph"|"elements"|
                "table"|"tableRows"|"tableCells"|"tabs"|"documentTab"|"childTabs"){
                document_text(item,out);
            }
        },
        _=>{}
    }
}
fn doc_view(document:&Value)->Value{
    let mut text=String::new();
    document_text(document,&mut text);
    json!({"documentId":document["documentId"],"title":document["title"],
        "revisionId":document["revisionId"],
        "text":text.chars().take(12_000).collect::<String>(),
        "truncated":text.chars().count()>12_000})
}
pub async fn drive_search(state:&AppState,args:&Value)->Result<Value,String>{
    let q=args["query"].as_str().unwrap_or("").trim();
    if q.len()>120{return Err("Drive 검색어는 최대 120자입니다.".into());}
    let mut endpoint=url("www.googleapis.com",&["drive","v3","files"])?;
    let mut filter="trashed = false".to_owned();
    if !q.is_empty(){
        let escaped=q.replace('\\',"\\\\").replace('\'',"\\'");
        filter.push_str(&format!(" and name contains '{escaped}'"));
    }
    endpoint.query_pairs_mut()
        .append_pair("q",&filter).append_pair("pageSize","50")
        .append_pair("orderBy","modifiedTime desc")
        .append_pair("fields","nextPageToken,files(id,name,mimeType,modifiedTime,webViewLink,capabilities(canEdit))");
    if let Some(cursor)=args["pageToken"].as_str().filter(|v|!v.is_empty()){
        if cursor.len()>1024||cursor.chars().any(char::is_control){
            return Err("Drive 페이지 토큰 형식 오류".into());
        }
        endpoint.query_pairs_mut().append_pair("pageToken",cursor);
    }
    request(state,"drive.readonly",Method::GET,endpoint,None).await
}
pub async fn docs_read(state:&AppState,args:&Value)->Result<Value,String>{
    let doc=id(required(args,"documentId",180)?)?;
    let mut endpoint=url("docs.googleapis.com",&["v1","documents",doc])?;
    endpoint.query_pairs_mut().append_pair("includeTabsContent","true");
    let data=request(state,"documents",Method::GET,endpoint,None).await?;
    Ok(doc_view(&data))
}
pub async fn docs_replace(state:&AppState,args:&Value)->Result<Value,String>{
    let doc=id(required(args,"documentId",180)?)?;
    let needle=required(args,"find",5000)?;
    let replacement=args["replace"].as_str().filter(|v|v.len()<=30_000)
        .ok_or("replace는 최대 30000자 문자열이어야 합니다.")?;
    let expected=required(args,"expectedRevisionId",240)?;
    let mut read_url=url("docs.googleapis.com",&["v1","documents",doc])?;
    read_url.query_pairs_mut().append_pair("includeTabsContent","true");
    let before=request(state,"documents",Method::GET,read_url.clone(),None).await?;
    if before["revisionId"].as_str()!=Some(expected){
        return Err("문서 버전이 변경됐습니다. 먼저 다시 읽으세요.".into());
    }
    let write=url("docs.googleapis.com",&["v1","documents",&format!("{doc}:batchUpdate")])?;
    let payload=json!({"requests":[{
        "replaceAllText":{"containsText":{"text":needle,"matchCase":true},
            "replaceText":replacement}
    }],"writeControl":{"requiredRevisionId":expected}});
    let sent=request(state,"documents",Method::POST,write,Some(payload)).await;
    let reply=match sent{
        Ok(v)=>v,
        Err(e)=>return Ok(json!({"documentId":doc,"outcomeUnknown":true,
            "requiresReview":true,"verified":false,"message":e}))
    };
    let changed=reply["replies"].as_array().map(|r|r.iter()
        .filter_map(|item|item["replaceAllText"]["occurrencesChanged"].as_u64()).sum::<u64>())
        .unwrap_or(0);
    match request(state,"documents",Method::GET,read_url,None).await{
        Ok(after)=>Ok(json!({"documentId":doc,"replacements":changed,
            "revisionId":after["revisionId"],
            "verified":changed>0&&after["revisionId"]!=before["revisionId"],
            "outcomeUnknown":false})),
        Err(_)=>Ok(json!({"documentId":doc,"replacements":changed,
            "verified":false,"outcomeUnknown":true,"requiresReview":true,
            "message":"수정 후 확인 실패. 작업을 반복하지 말고 문서를 다시 읽으세요."}))
    }
}
pub async fn sheets_read(state:&AppState,args:&Value)->Result<Value,String>{
    let spreadsheet=id(required(args,"spreadsheetId",180)?)?;
    let range=required(args,"range",160)?;
    let mut endpoint=sheet_url(spreadsheet,range)?;
    endpoint.query_pairs_mut().append_pair("valueRenderOption","UNFORMATTED_VALUE");
    let data=request(state,"spreadsheets",Method::GET,endpoint,None).await?;
    Ok(json!({"spreadsheetId":spreadsheet,"range":data["range"],
        "values":data.get("values").cloned().unwrap_or_else(||json!([]))}))
}
pub async fn sheets_update(state:&AppState,args:&Value)->Result<Value,String>{
    let spreadsheet=id(required(args,"spreadsheetId",180)?)?;
    let range=required(args,"range",160)?;
    let values=args["values"].as_array().filter(|v|!v.is_empty()&&v.len()<=200)
        .ok_or("시트 업데이트는 최대 200개 행이어야 합니다.")?;
    if values.iter().any(|r|r.as_array().is_none_or(|cells|cells.len()>50||
        cells.iter().any(|v|!v.is_string()&&!v.is_number()&&!v.is_boolean()))){
        return Err("시트 행에는 최대 50개의 숫자·문자·참거짓 값만 허용됩니다.".into());
    }
    let expected=args["expectedValues"].as_array()
        .ok_or("먼저 시트를 읽고 expectedValues를 전달하세요.")?;
    let mut read=sheet_url(spreadsheet,range)?;
    read.query_pairs_mut().append_pair("valueRenderOption","UNFORMATTED_VALUE");
    let before=request(state,"spreadsheets",Method::GET,read.clone(),None).await?;
    if before.get("values").cloned().unwrap_or_else(||json!([]))!=json!(expected){
        return Err("시트 값이 바뀌었습니다. 다시 읽고 수정하세요.".into());
    }
    let mut write=sheet_url(spreadsheet,range)?;
    write.query_pairs_mut().append_pair("valueInputOption","RAW");
    let sent=request(state,"spreadsheets",Method::PUT,write,Some(json!({
        "range":range,"majorDimension":"ROWS","values":values
    }))).await;
    let updated=match sent{
        Ok(data)=>data,
        Err(e)=>return Ok(json!({"outcomeUnknown":true,"requiresReview":true,
            "verified":false,"message":e}))
    };
    match request(state,"spreadsheets",Method::GET,read,None).await{
        Ok(after)=>{
            let observed=after.get("values").cloned().unwrap_or_else(||json!([]));
            Ok(json!({"spreadsheetId":spreadsheet,"range":range,
                "updatedCells":updated["updatedCells"],"observedValues":observed,
                "verified":observed==json!(values),"outcomeUnknown":false}))
        },
        Err(_)=>Ok(json!({"spreadsheetId":spreadsheet,"range":range,
            "verified":false,"outcomeUnknown":true,"requiresReview":true,
            "message":"Sheets 수정 후 원격 확인 실패. 먼저 현재 시트 값을 다시 읽으세요."}))
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn workspace_api_uses_only_fixed_google_hosts(){
        assert!(url("evil.example",&["drive"]).is_err());
        assert!(id("../secret").is_err());
        assert_eq!(sheet_url("abc_123","Sheet1!A1:B2").unwrap().host_str(),Some("sheets.googleapis.com"));
    }
    #[test]
    fn document_preview_includes_real_revision_and_text(){
        let doc=doc_view(&json!({"documentId":"doc","revisionId":"5","title":"Title",
            "body":{"content":[{"paragraph":{"elements":[{"textRun":{"content":"Hello"}}]}}]}}));
        assert_eq!(doc["revisionId"],"5");
        assert_eq!(doc["text"],"Hello");
    }
}
