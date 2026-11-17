//! Google Workspace account tools: Gmail, Drive, Docs and Calendar.
//! Operations are typed, bounded, host-pinned and use the existing encrypted
//! Google OAuth grant. No bearer token enters the tool result or model prompt.
use crate::state::AppState;
use base64::{engine::general_purpose::{STANDARD,URL_SAFE,URL_SAFE_NO_PAD},Engine as _};
use chrono::DateTime;
use reqwest::{Client,Method,Url};
use serde_json::{json,Value};

const GMAIL:&str="https://gmail.googleapis.com/";
const DRIVE:&str="https://www.googleapis.com/";
const UPLOAD:&str="https://www.googleapis.com/";
const CALENDAR:&str="https://www.googleapis.com/";
const DOCS:&str="https://docs.googleapis.com/";
const MAX_BYTES:usize=160_000;
const MAX_TEXT:usize=30_000;

fn arg<'a>(v:&'a Value,key:&str)->Result<&'a str,String>{
    v[key].as_str().filter(|s|!s.trim().is_empty())
        .ok_or_else(||format!("Google {key} is required"))
}
fn identifier(value:&str)->Result<&str,String>{
    if value.is_empty()||value.len()>256
        ||!value.bytes().all(|b|b.is_ascii_alphanumeric()||matches!(b,b'_'|b'-'|b'.'|b'@')){
        return Err("Invalid Google resource ID".into());
    }
    Ok(value)
}
fn query(v:&Value,key:&str,max:usize)->Result<Option<String>,String>{
    match v[key].as_str(){
        None=>Ok(None),
        Some(value) if value.len()<=max=>Ok(Some(value.to_string())),
        _=>Err(format!("Google {key} is too long")),
    }
}
fn url(origin:&str,parts:&[&str],params:&[(&str,String)])->Result<Url,String>{
    let mut url=Url::parse(origin).map_err(|e|e.to_string())?;
    {
        let mut segments=url.path_segments_mut().map_err(|_|"Invalid Google origin")?;
        segments.clear();
        for part in parts{segments.push(part);}
    }
    if !params.is_empty(){
        let mut pairs=url.query_pairs_mut();
        for (key,value) in params{pairs.append_pair(key,value);}
    }
    Ok(url)
}
fn text(value:&Value,key:&str,max:usize)->Result<String,String>{
    let s=arg(value,key)?;
    if s.len()>max{return Err(format!("{key} exceeds {max} bytes"));}
    Ok(s.to_string())
}
fn bounded(value:&str,max:usize)->String{value.chars().take(max).collect()}
fn scope_for(service:&str,operation:&str,write:bool)->Result<&'static str,String>{
    match (service,operation,write){
        ("gmail","search"|"get",false)=>Ok("https://www.googleapis.com/auth/gmail.readonly"),
        ("gmail","send",true)=>Ok("https://www.googleapis.com/auth/gmail.send"),
        ("drive","search"|"get",false)=>Ok("https://www.googleapis.com/auth/drive"),
        ("drive","create_text"|"update_text",true)=>Ok("https://www.googleapis.com/auth/drive"),
        ("docs","get",false)|("docs","append",true)=>
            Ok("https://www.googleapis.com/auth/documents"),
        ("calendar","list"|"get",false)
        |("calendar","create"|"update"|"delete",true)=>
            Ok("https://www.googleapis.com/auth/calendar.events"),
        _=>Err("Unsupported Google service operation or approval mode".into()),
    }
}
fn ensure_granted(state:&AppState,scope:&str)->Result<(),String>{
    let grant=state.store.get("oauth_config","google")
        .ok_or("Connect your Google account first")?;
    let granted=grant["scopes"].as_str().unwrap_or("");
    if !granted.split(|c:char|c.is_whitespace()||c==',').any(|s|s==scope){
        return Err("Google account needs upgraded Gmail/Drive/Docs/Calendar permissions; reconnect Google and approve the requested scopes".into());
    }
    Ok(())
}
async fn response_json(mut res:reqwest::Response)->Result<Value,String>{
    let status=res.status();
    let mut bytes=Vec::new();
    while let Some(chunk)=res.chunk().await.map_err(|e|e.to_string())? {
        if bytes.len()+chunk.len()>MAX_BYTES{return Err("Google response exceeds 160 KB limit".into());}
        bytes.extend_from_slice(&chunk);
    }
    if !status.is_success(){
        return Err(format!("Google API HTTP {status}: {}",
            bounded(&String::from_utf8_lossy(&bytes),380)));
    }
    if bytes.is_empty(){return Ok(json!({"ok":true}));}
    serde_json::from_slice(&bytes).map_err(|_|"Invalid Google API JSON".into())
}
async fn response_text(mut res:reqwest::Response)->Result<String,String>{
    let status=res.status();
    let mut bytes=Vec::new();
    while let Some(chunk)=res.chunk().await.map_err(|e|e.to_string())?{
        if bytes.len()+chunk.len()>MAX_BYTES{return Err("Google document exceeds 160 KB limit".into());}
        bytes.extend_from_slice(&chunk);
    }
    if !status.is_success(){
        return Err(format!("Google API HTTP {status}: {}",bounded(&String::from_utf8_lossy(&bytes),380)));
    }
    String::from_utf8(bytes).map_err(|_|"Google file is not UTF-8 text".into())
}
async fn call(client:&Client,token:&str,method:Method,target:Url,
    body:Option<Value>)->Result<Value,String>{
    let mut req=client.request(method,target).bearer_auth(token);
    if let Some(body)=body{req=req.json(&body);}
    response_json(req.send().await.map_err(|e|format!("Google request failed: {e}"))?).await
}
fn mime_subject(value:&str)->Result<String,String>{
    if value.len()>600||value.chars().any(|c|c=='\r'||c=='\n'||c=='\0'){
        return Err("Invalid Gmail subject".into());
    }
    Ok(format!("=?UTF-8?B?{}?=",STANDARD.encode(value.as_bytes())))
}
fn email(value:&str)->Result<&str,String>{
    if value.len()>254||value.contains(['\r','\n','\0',',',';',' '])
        ||!value.contains('@') {return Err("Invalid email recipient".into());}
    Ok(value)
}
fn decode_part(value:&Value,depth:usize)->Option<String>{
    if depth>6{return None;}
    if let Some(data)=value["body"]["data"].as_str(){
        let bytes=URL_SAFE_NO_PAD.decode(data).or_else(|_|URL_SAFE.decode(data)).ok()?;
        if let Ok(content)=String::from_utf8(bytes){
            return Some(bounded(&content,MAX_TEXT));
        }
    }
    value["parts"].as_array().and_then(|parts|parts.iter()
        .filter(|part|part["mimeType"]=="text/plain")
        .chain(parts.iter().filter(|part|part["mimeType"]=="text/html"))
        .find_map(|part|decode_part(part,depth+1)))
}
fn document_text(value:&Value,output:&mut String,depth:usize){
    if depth>18||output.len()>=MAX_TEXT{return;}
    if let Some(content)=value.pointer("/textRun/content").and_then(Value::as_str){
        if output.len()+content.len()>MAX_TEXT{
            let remaining=MAX_TEXT.saturating_sub(output.len());
            output.push_str(&bounded(content,remaining));
        }else {output.push_str(content);}
    }
    if let Some(map)=value.as_object(){
        for (key,child) in map {
            if key!="textRun"{document_text(child,output,depth+1);}
        }
    }else if let Some(items)=value.as_array(){
        for child in items {document_text(child,output,depth+1);}
    }
}
fn gmail_message(value:&Value)->Value{
    let headers=value["payload"]["headers"].as_array().map(|items|items.iter()
        .filter_map(|x|{
            let key=x["name"].as_str()?.to_ascii_lowercase();
            matches!(key.as_str(),"from"|"to"|"subject"|"date").then(||(key,x["value"].clone()))
        }).collect::<serde_json::Map<String,Value>>()).unwrap_or_default();
    json!({"id":value["id"],"threadId":value["threadId"],
        "snippet":value["snippet"],"labels":value["labelIds"],
        "headers":headers,
        "bodyText":decode_part(&value["payload"],0)})
}
fn event_date(v:&Value,key:&str)->Result<String,String>{
    let value=text(v,key,128)?;
    DateTime::parse_from_rfc3339(&value)
        .map_err(|_|format!("{key} must be RFC3339 with timezone"))?;
    Ok(value)
}
async fn execute_with(client:&Client,token:&str,bases:&[&str;5],
    args:&Value,write:bool)->Result<Value,String>{
    let service=arg(args,"service")?;
    let operation=arg(args,"operation")?;
    scope_for(service,operation,write)?;
    let max=args["maxResults"].as_u64().unwrap_or(20).clamp(1,50);
    match (service,operation,write){
        ("gmail","search",false)=>{
            let mut params=vec![("maxResults",max.to_string())];
            if let Some(q)=query(args,"query",1000)? {params.push(("q",q));}
            if let Some(p)=query(args,"pageToken",1024)? {params.push(("pageToken",p));}
            call(client,token,Method::GET,url(bases[0],&["gmail","v1","users","me","messages"],&params)?,None).await
        }
        ("gmail","get",false)=>{
            let id=identifier(arg(args,"id")?)?;
            let data=call(client,token,Method::GET,
                url(bases[0],&["gmail","v1","users","me","messages",id],
                    &[("format","full".into())])?,None).await?;
            Ok(gmail_message(&data))
        }
        ("gmail","send",true)=>{
            let recipient=email(arg(args,"to")?)?;
            let subject=mime_subject(arg(args,"subject")?)?;
            let body=text(args,"body",50_000)?;
            // Body may contain newlines, but headers never accept arbitrary
            // line breaks or extra recipients supplied by the model.
            let raw=format!("To: {recipient}\r\nSubject: {subject}\r\nMIME-Version: 1.0\r\nContent-Type: text/plain; charset=UTF-8\r\nContent-Transfer-Encoding: 8bit\r\n\r\n{body}");
            let data=call(client,token,Method::POST,
                url(bases[0],&["gmail","v1","users","me","messages","send"],&[])?,
                Some(json!({"raw":URL_SAFE_NO_PAD.encode(raw.as_bytes())}))).await?;
            Ok(json!({"sent":true,"id":data["id"],"threadId":data["threadId"]}))
        }
        ("drive","search",false)=>{
            let mut params=vec![("fields","nextPageToken,files(id,name,mimeType,modifiedTime,size,version,webViewLink),incompleteSearch".into()),
                ("pageSize",max.to_string()),("orderBy","modifiedTime desc".into())];
            if let Some(q)=query(args,"query",1000)?{
                let escaped=q.replace('\\',"\\\\").replace('\'',"\\'");
                params.push(("q",format!("name contains '{escaped}' and trashed = false")));
            }else {params.push(("q","trashed = false".into()));}
            if let Some(p)=query(args,"pageToken",1024)?{params.push(("pageToken",p));}
            call(client,token,Method::GET,url(bases[1],&["drive","v3","files"],&params)?,None).await
        }
        ("drive","get",false)=>{
            let id=identifier(arg(args,"id")?)?;
            let info=call(client,token,Method::GET,
                url(bases[1],&["drive","v3","files",id],
                    &[("fields","id,name,mimeType,version,size,webViewLink,modifiedTime".into())])?,None).await?;
            let native=info["mimeType"].as_str().unwrap_or("").starts_with("application/vnd.google-apps.");
            let target=if native{
                url(bases[1],&["drive","v3","files",id,"export"],&[("mimeType","text/plain".into())])?
            }else{url(bases[1],&["drive","v3","files",id],&[("alt","media".into())])?};
            let content=response_text(client.get(target).bearer_auth(token).send()
                .await.map_err(|e|e.to_string())?).await?;
            Ok(json!({"metadata":info,"content":bounded(&content,MAX_TEXT),
                "truncated":content.chars().count()>MAX_TEXT}))
        }
        ("drive","create_text",true)=>{
            let name=text(args,"name",180)?;
            let body=text(args,"content",100_000)?;
            let boundary=format!("dominion-{}",uuid::Uuid::new_v4().simple());
            let metadata=json!({"name":name,"mimeType":"text/plain"});
            let upload=format!("--{boundary}\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n{}\r\n--{boundary}\r\nContent-Type: text/plain; charset=UTF-8\r\n\r\n{body}\r\n--{boundary}--\r\n",
                metadata);
            let target=url(bases[2],&["upload","drive","v3","files"],&[
                ("uploadType","multipart".into()),
                ("fields","id,name,version,mimeType,webViewLink".into())])?;
            let result=response_json(client.post(target).bearer_auth(token)
                .header("Content-Type",format!("multipart/related; boundary={boundary}"))
                .body(upload).send().await.map_err(|e|e.to_string())?).await?;
            let id=identifier(result["id"].as_str().ok_or("Google Drive create response lacks ID")?)?;
            let metadata=call(client,token,Method::GET,url(bases[1],&["drive","v3","files",id],
                &[("fields","id,name,version,mimeType,webViewLink".into())])?,None).await?;
            if metadata["id"]!=id||metadata["name"]!=name{
                return Err("Drive accepted create but independent readback differs; do not retry automatically".into());
            }
            Ok(json!({"created":true,"file":metadata,"verified":true}))
        }
        ("drive","update_text",true)=>{
            let id=identifier(arg(args,"id")?)?;
            let expected=arg(args,"expectedVersion")?;
            let content=text(args,"content",100_000)?;
            let before=call(client,token,Method::GET,url(bases[1],&["drive","v3","files",id],
                &[("fields","id,mimeType,version,name".into())])?,None).await?;
            if before["mimeType"]!="text/plain"{
                return Err("Drive update_text supports plain text files only; use docs append for Google Docs".into());
            }
            if before["version"].as_str()!=Some(expected){
                return Err("Drive file changed since read; re-read version before editing".into());
            }
            let target=url(bases[2],&["upload","drive","v3","files",id],
                &[("uploadType","media".into()),("fields","id,version,name".into())])?;
            let posted=response_json(client.patch(target).bearer_auth(token)
                .header("Content-Type","text/plain; charset=UTF-8")
                .body(content.clone()).send().await.map_err(|e|e.to_string())?).await?;
            let actual=response_text(client.get(url(bases[1],&["drive","v3","files",id],
                &[("alt","media".into())])?).bearer_auth(token).send()
                .await.map_err(|e|e.to_string())?).await?;
            if actual!=content{
                return Err("Drive update accepted but remote content differs; do not retry automatically".into());
            }
            Ok(json!({"updated":true,"id":id,"version":posted["version"],"verified":true}))
        }
        ("docs","get",false)=>{
            let id=identifier(arg(args,"id")?)?;
            let document=call(client,token,Method::GET,
                url(bases[4],&["v1","documents",id],&[
                    ("fields","documentId,title,body(content)".into())])?,None).await?;
            let mut content=String::new();
            document_text(&document["body"],&mut content,0);
            Ok(json!({"id":document["documentId"],"title":document["title"],
                "content":content,"truncated":content.len()>=MAX_TEXT}))
        }
        ("docs","append",true)=>{
            let id=identifier(arg(args,"id")?)?;
            let content=text(args,"content",30_000)?;
            call(client,token,Method::POST,
                url(bases[4],&["v1","documents",&format!("{id}:batchUpdate")],&[])?,
                Some(json!({"requests":[{"insertText":{
                    "endOfSegmentLocation":{},"text":content}}]}))).await?;
            let after=call(client,token,Method::GET,
                url(bases[4],&["v1","documents",id],&[
                    ("fields","documentId,title,body(content)".into())])?,None).await?;
            let mut after_text=String::new();
            document_text(&after["body"],&mut after_text,0);
            if !after_text.contains(&content){
                return Err("Docs accepted update but appended text not found in independent readback; do not retry automatically".into());
            }
            Ok(json!({"updated":true,"id":id,"verified":true}))
        }
        ("calendar","list",false)=>{
            let calendar=identifier(args["calendarId"].as_str().unwrap_or("primary"))?;
            let mut params=vec![("maxResults",max.to_string()),("singleEvents","true".into()),
                ("orderBy","startTime".into())];
            if let Some(v)=query(args,"timeMin",128)?{
                DateTime::parse_from_rfc3339(&v).map_err(|_|"Invalid timeMin RFC3339")?;
                params.push(("timeMin",v));
            }
            if let Some(p)=query(args,"pageToken",1024)? {params.push(("pageToken",p));}
            call(client,token,Method::GET,url(bases[3],
                &["calendar","v3","calendars",calendar,"events"],&params)?,None).await
        }
        ("calendar","get",false)=>{
            let cal=identifier(args["calendarId"].as_str().unwrap_or("primary"))?;
            let id=identifier(arg(args,"id")?)?;
            call(client,token,Method::GET,url(bases[3],
                &["calendar","v3","calendars",cal,"events",id],&[])?,None).await
        }
        ("calendar","create",true)=>{
            let cal=identifier(args["calendarId"].as_str().unwrap_or("primary"))?;
            let summary=text(args,"summary",500)?;
            let start=event_date(args,"start")?;
            let end=event_date(args,"end")?;
            if DateTime::parse_from_rfc3339(&start).unwrap()>=DateTime::parse_from_rfc3339(&end).unwrap(){
                return Err("Calendar end must be later than start".into());
            }
            let body=json!({"summary":summary,
                "description":query(args,"description",5000)?.unwrap_or_default(),
                "start":{"dateTime":start},"end":{"dateTime":end}});
            let created=call(client,token,Method::POST,url(bases[3],
                &["calendar","v3","calendars",cal,"events"],&[])?,Some(body)).await?;
            let id=identifier(created["id"].as_str().ok_or("Created calendar event ID missing")?)?;
            let verified=call(client,token,Method::GET,url(bases[3],
                &["calendar","v3","calendars",cal,"events",id],&[])?,None).await?;
            if verified["id"]!=id||verified["summary"]!=summary {
                return Err("Calendar accepted event but remote verification differs; do not retry automatically".into());
            }
            Ok(json!({"created":true,"event":verified,"verified":true}))
        }
        ("calendar","update",true)=>{
            let cal=identifier(args["calendarId"].as_str().unwrap_or("primary"))?;
            let id=identifier(arg(args,"id")?)?;
            let mut patch=json!({});
            if let Some(s)=query(args,"summary",500)?{patch["summary"]=json!(s);}
            if let Some(s)=query(args,"description",5000)?{patch["description"]=json!(s);}
            if let Some(s)=query(args,"start",128)?{
                DateTime::parse_from_rfc3339(&s).map_err(|_|"Invalid calendar start")?;
                patch["start"]=json!({"dateTime":s});
            }
            if let Some(s)=query(args,"end",128)?{
                DateTime::parse_from_rfc3339(&s).map_err(|_|"Invalid calendar end")?;
                patch["end"]=json!({"dateTime":s});
            }
            if patch.as_object().is_none_or(|v|v.is_empty()){return Err("No calendar fields to update".into());}
            let result=call(client,token,Method::PATCH,url(bases[3],
                &["calendar","v3","calendars",cal,"events",id],&[])?,Some(patch.clone())).await?;
            let verified=call(client,token,Method::GET,url(bases[3],
                &["calendar","v3","calendars",cal,"events",id],&[])?,None).await?;
            for (key,value) in patch.as_object().unwrap() {
                let matches=if key=="start"||key=="end" {
                    let expected=value["dateTime"].as_str().and_then(|t|DateTime::parse_from_rfc3339(t).ok());
                    let actual=verified[key]["dateTime"].as_str().and_then(|t|DateTime::parse_from_rfc3339(t).ok());
                    expected.is_some()&&expected==actual
                }else{verified[key]==*value};
                if !matches{
                    return Err("Calendar update accepted but remote readback disagrees; do not retry automatically".into());
                }
            }
            Ok(json!({"updated":true,"id":id,"verified":true,
                "event":verified,"updateResponseId":result["id"]}))
        }
        ("calendar","delete",true)=>{
            // Delete outcome is unknown if the HTTP response is lost; the
            // general kernel will not automatically retry this effect.
            let cal=identifier(args["calendarId"].as_str().unwrap_or("primary"))?;
            let id=identifier(arg(args,"id")?)?;
            let target=url(bases[3],&["calendar","v3","calendars",cal,"events",id],&[])?;
            let res=client.delete(target).bearer_auth(token).send().await
                .map_err(|e|format!("Google Calendar deletion outcome uncertain: {e}"))?;
            if !res.status().is_success(){
                return Err(format!("Calendar delete denied: HTTP {}",res.status()));
            }
            // A read-only GET after DELETE establishes absence without
            // reissuing a potentially non-idempotent mutation.
            let check=client.get(url(bases[3],
                &["calendar","v3","calendars",cal,"events",id],&[])?)
                .bearer_auth(token).send().await.map_err(|e|e.to_string())?;
            if check.status()!=reqwest::StatusCode::NOT_FOUND{
                return Err("Calendar acknowledged deletion but absence was not verified; do not retry automatically".into());
            }
            Ok(json!({"deleted":true,"id":id,"verified":true}))
        }
        _=>Err("Unsupported Google operation".into()),
    }
}
pub async fn execute(state:&AppState,args:&Value,write:bool)->Result<Value,String>{
    let service=arg(args,"service")?;
    let operation=arg(args,"operation")?;
    let scope=scope_for(service,operation,write)?;
    ensure_granted(state,scope)?;
    let token=crate::engine::oauth::fresh_access_token(state,"google").await
        .map_err(|e|format!("Reconnect Google to grant Workspace permissions: {e}"))?;
    let client=state.provider_http_client(30)?;
    execute_with(&client,&token,&[GMAIL,DRIVE,UPLOAD,CALENDAR,DOCS],args,write).await
}

#[cfg(test)]
mod tests{
    use super::*;
    #[derive(Default)]
    struct FakeGoogle {
        mail_sent:usize,
        drive_uploads:usize,
        doc_updates:usize,
        events_created:usize,
    }
    type FakeState=std::sync::Arc<std::sync::Mutex<FakeGoogle>>;
    async fn fake_mail_send(
        axum::extract::State(state):axum::extract::State<FakeState>,
        axum::Json(body):axum::Json<Value>,
    )->axum::Json<Value>{
        let raw=body["raw"].as_str().unwrap();
        let decoded=URL_SAFE_NO_PAD.decode(raw).unwrap();
        let message=String::from_utf8(decoded).unwrap();
        assert!(message.contains("To: dev@example.com\r\n"));
        assert!(message.contains("Content-Type: text/plain"));
        state.lock().unwrap().mail_sent+=1;
        axum::Json(json!({"id":"mail123","threadId":"thread123"}))
    }
    async fn fake_drive_upload(
        axum::extract::State(state):axum::extract::State<FakeState>,
        body:String,
    )->axum::Json<Value>{
        assert!(body.contains("notes.txt"));
        assert!(body.contains("persisted content"));
        state.lock().unwrap().drive_uploads+=1;
        axum::Json(json!({"id":"drive123","version":"1"}))
    }
    async fn fake_docs_update(
        axum::extract::State(state):axum::extract::State<FakeState>,
        axum::Json(body):axum::Json<Value>,
    )->axum::Json<Value>{
        assert_eq!(body["requests"][0]["insertText"]["text"],"Hello inserted");
        state.lock().unwrap().doc_updates+=1;
        axum::Json(json!({"replies":[{}],"writeControl":{}}))
    }
    async fn fake_calendar_create(
        axum::extract::State(state):axum::extract::State<FakeState>,
        axum::Json(body):axum::Json<Value>,
    )->axum::Json<Value>{
        assert_eq!(body["summary"],"Meeting");
        state.lock().unwrap().events_created+=1;
        axum::Json(json!({"id":"event123"}))
    }
    #[tokio::test]
    async fn connected_google_account_sends_mail_writes_drive_docs_and_calendar_over_http(){
        let state:FakeState=Default::default();
        let router=axum::Router::new()
            .route("/gmail/v1/users/me/messages",axum::routing::get(||
                async{axum::Json(json!({"messages":[{"id":"mail123"}],"resultSizeEstimate":1}))}))
            .route("/gmail/v1/users/me/messages/mail123",axum::routing::get(||
                async{axum::Json(json!({"id":"mail123","threadId":"thread123",
                    "snippet":"hello",
                    "payload":{"headers":[{"name":"From","value":"dev@example.com"},
                        {"name":"Subject","value":"hi"}],
                        "body":{"data":URL_SAFE_NO_PAD.encode("Message body")}}}))}))
            .route("/gmail/v1/users/me/messages/send",axum::routing::post(fake_mail_send))
            .route("/drive/v3/files",axum::routing::get(||
                async{axum::Json(json!({"files":[{"id":"drive123","name":"notes.txt"}]}))}))
            .route("/drive/v3/files/drive123",axum::routing::get(||
                async{axum::Json(json!({"id":"drive123","name":"notes.txt",
                    "mimeType":"text/plain","version":"1"}))}))
            .route("/upload/drive/v3/files",axum::routing::post(fake_drive_upload))
            .route("/v1/documents/doc123",axum::routing::get(||
                async{axum::Json(json!({"documentId":"doc123","title":"Shared document",
                    "body":{"content":[{"paragraph":{"elements":[{"textRun":{
                        "content":"Hello inserted"}}]}}]}}))}))
            .route("/v1/documents/doc123:batchUpdate",axum::routing::post(fake_docs_update))
            .route("/calendar/v3/calendars/primary/events",axum::routing::get(||
                async{axum::Json(json!({"items":[{"id":"event123","summary":"Meeting"}]}))})
                .post(fake_calendar_create))
            .route("/calendar/v3/calendars/primary/events/event123",axum::routing::get(||
                async{axum::Json(json!({"id":"event123","summary":"Meeting",
                    "start":{"dateTime":"2026-11-18T09:00:00+09:00"},
                    "end":{"dateTime":"2026-11-18T10:00:00+09:00"}}))}))
            .with_state(state.clone());
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base=format!("http://{}/",listener.local_addr().unwrap());
        let server=tokio::spawn(async move{axum::serve(listener,router).await.unwrap();});
        let origins=[base.as_str();5];
        let client=Client::builder().timeout(std::time::Duration::from_secs(5)).build().unwrap();
        let token="google-test-access-token";
        let search=execute_with(&client,token,&origins,
            &json!({"service":"gmail","operation":"search","query":"subject:hi"}),false).await.unwrap();
        assert_eq!(search["messages"][0]["id"],"mail123");
        let message=execute_with(&client,token,&origins,
            &json!({"service":"gmail","operation":"get","id":"mail123"}),false).await.unwrap();
        assert_eq!(message["bodyText"],"Message body");
        let sent=execute_with(&client,token,&origins,
            &json!({"service":"gmail","operation":"send",
                "to":"dev@example.com","subject":"Report","body":"Checked"}),true).await.unwrap();
        assert_eq!(sent["sent"],true);
        let drive=execute_with(&client,token,&origins,
            &json!({"service":"drive","operation":"search","query":"notes"}),false).await.unwrap();
        assert_eq!(drive["files"][0]["name"],"notes.txt");
        let upload=execute_with(&client,token,&origins,
            &json!({"service":"drive","operation":"create_text",
                "name":"notes.txt","content":"persisted content"}),true).await.unwrap();
        assert_eq!(upload["verified"],true);
        let doc=execute_with(&client,token,&origins,
            &json!({"service":"docs","operation":"get","id":"doc123"}),false).await.unwrap();
        assert_eq!(doc["content"],"Hello inserted");
        let appended=execute_with(&client,token,&origins,
            &json!({"service":"docs","operation":"append",
                "id":"doc123","content":"Hello inserted"}),true).await.unwrap();
        assert_eq!(appended["verified"],true);
        let events=execute_with(&client,token,&origins,
            &json!({"service":"calendar","operation":"list"}),false).await.unwrap();
        assert_eq!(events["items"][0]["summary"],"Meeting");
        let created=execute_with(&client,token,&origins,
            &json!({"service":"calendar","operation":"create","summary":"Meeting",
                "start":"2026-11-18T09:00:00+09:00",
                "end":"2026-11-18T10:00:00+09:00"}),true).await.unwrap();
        assert_eq!(created["verified"],true);
        let counters=state.lock().unwrap();
        assert_eq!((counters.mail_sent,counters.drive_uploads,
            counters.doc_updates,counters.events_created),(1,1,1,1));
        server.abort();
    }

    #[test]
    fn workspace_actions_have_explicit_read_write_and_scope_authority(){
        assert_eq!(scope_for("gmail","search",false).unwrap(),
            "https://www.googleapis.com/auth/gmail.readonly");
        assert!(scope_for("gmail","send",false).is_err());
        assert!(scope_for("gmail","search",true).is_err());
        assert!(scope_for("calendar","create",false).is_err());
        assert!(scope_for("calendar","delete",true).is_ok());
        assert!(identifier("../../etc/passwd").is_err());
        assert!(email("fake@example.com\r\nBcc:bad@example.com").is_err());
        assert!(mime_subject("Injected\r\nHeader: evil").is_err());
        let url=url(DRIVE,&["drive","v3","files","file01"],&[("q",
            "name contains 'sample'".to_string())]).unwrap();
        assert_eq!(url.host_str(),Some("www.googleapis.com"));
    }
    #[test]
    fn google_docs_returns_and_verifies_plain_document_text(){
        let document=json!({"body":{"content":[
            {"paragraph":{"elements":[{"textRun":{"content":"Line 1\\n"}}]}},
            {"paragraph":{"elements":[{"textRun":{"content":"Line 2"}}]}}
        ]}});
        let mut output=String::new();
        document_text(&document["body"],&mut output,0);
        assert_eq!(output,"Line 1\\nLine 2");
        assert!(output.contains("Line 2"));
    }

    #[test]
    fn google_grants_do_not_inherit_later_requested_scopes(){
        use crate::{config::Config,db::Store,vault::Vault};
        let root=std::env::temp_dir().join(format!("dominion-grants-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let config=Config{port:0,data_dir:root.clone(),web_dir:root.clone(),workspace:root.clone()};
        let state=AppState::new(config,Store::open(&root.join("db.sqlite")).unwrap(),Vault::open(&root.join("key")));
        state.store.put("oauth_config",&json!({"id":"google","connected":true,
            "scopes":"openid email profile"}));
        assert!(ensure_granted(&state,"https://www.googleapis.com/auth/gmail.readonly").is_err());
        state.store.put("oauth_config",&json!({"id":"google","connected":true,
            "scopes":"openid email profile https://www.googleapis.com/auth/gmail.readonly"}));
        assert!(ensure_granted(&state,"https://www.googleapis.com/auth/gmail.readonly").is_ok());
        assert!(ensure_granted(&state,"https://www.googleapis.com/auth/gmail.send").is_err());
        let _=std::fs::remove_dir_all(root);
    }
}
