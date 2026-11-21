//! Stateless MCP 2026-07-28 Streamable HTTP transport.
//! Negotiation is performed with tools/list, never an effectful tools/call.
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde_json::{json,Value};
use std::{collections::HashSet,sync::OnceLock,time::Duration};

const VERSION:&str="2026-07-28";
#[derive(Debug)]
pub(super) enum ModernError{ Legacy(String), Failed(String) }
impl ModernError{
    pub(super) fn message(self)->String{
        match self{Self::Legacy(s)|Self::Failed(s)=>s}
    }
}
fn client()->&'static reqwest::Client{
    static CLIENT:OnceLock<reqwest::Client>=OnceLock::new();
    CLIENT.get_or_init(reqwest::Client::new)
}
fn encode_header(value:&str)->String{
    let safe=!value.is_empty()&&value.trim()==value
        &&value.bytes().all(|byte|(0x20..=0x7e).contains(&byte))
        &&!(value.starts_with("=?base64?")&&value.ends_with("?="));
    if safe{value.to_string()}
    else{format!("=?base64?{}?=",STANDARD.encode(value.as_bytes()))}
}
fn valid_token(value:&str)->bool{
    !value.is_empty()&&value.bytes().all(|b|
        b.is_ascii_alphanumeric()||b"!#$%&'*+-.^_\x60|~".contains(&b))
}
/// Only statically reachable object properties may mirror header parameters.
pub(super) fn header_specs(schema:&Value)->Result<Vec<(String,Vec<String>)>,String>{
    fn invalid_branch(value:&Value,depth:usize)->Result<(),String>{
        if depth>24{return Err("MCP schema nested too deeply".into());}
        if value.get("x-mcp-header").is_some(){
            return Err("x-mcp-header in a non-property schema branch".into());
        }
        match value{
            Value::Array(a)=>for v in a{invalid_branch(v,depth+1)?;},
            Value::Object(m)=>for v in m.values(){invalid_branch(v,depth+1)?;},
            _=>(),
        }
        Ok(())
    }
    fn scan(value:&Value,path:&mut Vec<String>,depth:usize,
        seen:&mut HashSet<String>,out:&mut Vec<(String,Vec<String>)>)
        ->Result<(),String>{
        if depth>24{return Err("MCP schema nested too deeply".into());}
        if let Some(annotation)=value.get("x-mcp-header"){
            let name=annotation.as_str().filter(|name|valid_token(name))
                .ok_or("invalid x-mcp-header name")?;
            if path.is_empty()||!matches!(value["type"].as_str(),
                Some("string"|"integer"|"boolean")){
                return Err("x-mcp-header must annotate a reachable primitive".into());
            }
            if !seen.insert(name.to_ascii_lowercase()){
                return Err("duplicate x-mcp-header name".into());
            }
            out.push((name.to_string(),path.clone()));
        }
        if let Some(map)=value.as_object(){
            for (key,child) in map{
                if key=="properties" {
                    if let Some(props)=child.as_object(){
                        for (name,property) in props{
                            path.push(name.clone());
                            scan(property,path,depth+1,seen,out)?;
                            path.pop();
                        }
                    }
                }else if key!="x-mcp-header"{invalid_branch(child,depth+1)?;}
            }
        }
        Ok(())
    }
    let mut result=Vec::new();
    scan(schema,&mut Vec::new(),0,&mut HashSet::new(),&mut result)?;
    Ok(result)
}
fn mirrored_headers(schema:Option<&Value>,arguments:&Value)
    ->Result<Vec<(String,String)>,ModernError>{
    let specs=match schema{Some(v)=>header_specs(v).map_err(ModernError::Failed)?,None=>vec![]};
    let mut out=Vec::new();
    for (name,path) in specs{
        let mut value=arguments;
        for key in path{value=&value[&key];}
        let raw=match value{
            Value::Null=>continue,
            Value::String(s)=>s.clone(),
            Value::Bool(b)=>b.to_string(),
            Value::Number(n) if n.is_i64()||n.is_u64()=>{
                let i=n.as_i64().map(i128::from)
                    .or_else(||n.as_u64().map(i128::from)).unwrap_or(i128::MAX);
                if i.abs()>9_007_199_254_740_991{
                    return Err(ModernError::Failed("MCP integer header outside JS safe range".into()));
                }
                n.to_string()
            },
            _=>return Err(ModernError::Failed("MCP annotated value is not primitive".into())),
        };
        out.push((format!("Mcp-Param-{name}"),encode_header(&raw)));
    }
    Ok(out)
}
fn final_response(text:&str,content_type:&str,id:&Value)->Result<Value,String>{
    if content_type.contains("text/event-stream"){
        let normalized=text.replace("\r\n","\n");
        for frame in normalized.split("\n\n"){
            let data=frame.lines().filter_map(|line|line.strip_prefix("data:"))
                .map(str::trim_start).collect::<Vec<_>>().join("\n");
            if data.is_empty(){continue;}
            if let Ok(value)=serde_json::from_str::<Value>(&data){
                if value["id"]==*id && (value.get("result").is_some()||value.get("error").is_some()){
                    return Ok(value);
                }
            }
        }
        return Err("MCP SSE missing matching final response".into());
    }
    let response=serde_json::from_str::<Value>(text).map_err(|e|e.to_string())?;
    if response["id"]!=*id ||
        (response.get("result").is_none()&&response.get("error").is_none()){
        return Err("MCP response envelope/id mismatch".into());
    }
    Ok(response)
}
async fn request_one(record:&Value,token:Option<&str>,method:&str,
    mut params:Value,schema:Option<&Value>)->Result<Value,ModernError>{
    let url=record["url"].as_str().filter(|u|!u.is_empty())
        .ok_or_else(||ModernError::Failed("MCP URL missing".into()))?;
    let parsed=reqwest::Url::parse(url).map_err(|e|ModernError::Failed(e.to_string()))?;
    if !matches!(parsed.scheme(),"http"|"https"){
        return Err(ModernError::Failed("MCP URL must use http(s)".into()));
    }
    if !params.is_object(){return Err(ModernError::Failed("MCP parameters must be object".into()));}
    params["_meta"]=json!({
        "io.modelcontextprotocol/protocolVersion":VERSION,
        "io.modelcontextprotocol/clientInfo":{"name":"dominion-rust","version":env!("CARGO_PKG_VERSION")},
        "io.modelcontextprotocol/clientCapabilities":{}
    });
    let id=json!(uuid::Uuid::new_v4().to_string());
    let envelope=json!({"jsonrpc":"2.0","id":id,"method":method,"params":params});
    let mut request=client().post(parsed).timeout(Duration::from_secs(60))
        .header("Content-Type","application/json")
        .header("Accept","application/json, text/event-stream")
        .header("MCP-Protocol-Version",VERSION).header("Mcp-Method",method).json(&envelope);
    if let Some(name)=match method{
        "tools/call"|"prompts/get"=>params["name"].as_str(),
        "resources/read"=>params["uri"].as_str(),_=>None,
    }{request=request.header("Mcp-Name",encode_header(name));}
    if method=="tools/call"{
        for (name,value) in mirrored_headers(schema,&params["arguments"])?{
            request=request.header(name,value);
        }
    }
    if let Some(token)=token{request=request.bearer_auth(token);}
    let response=request.send().await.map_err(|e|ModernError::Failed(format!(
        "MCP transport outcome unknown; do not replay an effectful call: {e}"
    )))?;
    let status=response.status();
    let content_type=response.headers().get("content-type")
        .and_then(|v|v.to_str().ok()).unwrap_or("").to_string();
    let bytes=response.bytes().await.map_err(|e|ModernError::Failed(format!(
        "MCP response lost, tool outcome unknown: {e}"
    )))?;
    if bytes.len()>4*1024*1024{
        return Err(ModernError::Failed("MCP response exceeds 4 MiB".into()));
    }
    let text=std::str::from_utf8(&bytes).map_err(|e|ModernError::Failed(e.to_string()))?;
    let parsed=final_response(text,&content_type,&id);
    if !status.is_success(){
        let modern=parsed.as_ref().ok()
            .and_then(|value|value["error"]["code"].as_i64())
            .is_some_and(|code|(-32099..=-32020).contains(&code)||code==-32601);
        if matches!(status.as_u16(),400|404|405)&&!modern{
            return Err(ModernError::Legacy(format!("legacy MCP endpoint responded {status}")));
        }
        return Err(ModernError::Failed(format!("MCP HTTP {status}: {}",
            text.chars().take(400).collect::<String>())));
    }
    let value=parsed.map_err(ModernError::Failed)?;
    if let Some(error)=value.get("error"){
        return Err(ModernError::Failed(format!("MCP RPC error: {error}")));
    }
    let result=value.get("result").cloned().unwrap_or(Value::Null);
    if result["resultType"]=="input_required"{
        return Err(ModernError::Failed("MCP input_required: owner input required before continuing".into()));
    }
    Ok(result)
}
pub(super) async fn list(record:&Value,token:Option<&str>)
    ->Result<Value,ModernError>{
    let mut all=Vec::new();
    let mut cursor:Option<String>=None;
    let mut seen=HashSet::new();
    for _ in 0..16{
        let params=cursor.as_ref().map(|v|json!({"cursor":v}))
            .unwrap_or_else(||json!({}));
        let response=request_one(record,token,"tools/list",params,None).await?;
        let page=response["tools"].as_array().ok_or_else(||
            ModernError::Failed("MCP tools/list missing tools".into()))?;
        if all.len()+page.len()>1000{
            return Err(ModernError::Failed("MCP tools/list exceeds 1000 tools".into()));
        }
        all.extend(page.iter().cloned());
        match response["nextCursor"].as_str(){
            None|Some("")=>return Ok(json!({"tools":all})),
            Some(next) if next.len()<=1024 && seen.insert(next.to_string())=>{
                cursor=Some(next.to_string());
            }
            _=>return Err(ModernError::Failed("MCP repeated/oversized list cursor".into())),
        }
    }
    Err(ModernError::Failed("MCP tools/list exceeds 16 pages".into()))
}
pub(super) async fn call(record:&Value,token:Option<&str>,
    params:Value,schema:Option<&Value>)->Result<Value,ModernError>{
    request_one(record,token,"tools/call",params,schema).await
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn rejects_bad_schema_and_escapes_unsafe_header_values(){
        let good=json!({"type":"object","properties":{"region":{
            "type":"string","x-mcp-header":"Region"},"nested":{
            "type":"object","properties":{"count":{"type":"integer","x-mcp-header":"Count"}}}}});
        assert_eq!(header_specs(&good).unwrap().len(),2);
        assert_eq!(encode_header("안녕"),"=?base64?7JWI64WV?=");
        assert_eq!(encode_header(" padded "),"=?base64?IHBhZGRlZCA=?=");
        assert!(header_specs(&json!({"type":"object","properties":{
            "x":{"type":"number","x-mcp-header":"Wrong"}}})).is_err());
        assert!(header_specs(&json!({"type":"object","properties":{
            "x":{"type":"string","x-mcp-header":"Same"},
            "y":{"type":"string","x-mcp-header":"same"}}})).is_err());
        let mirrored=mirrored_headers(Some(&good),
            &json!({"region":"ap-northeast-2","nested":{"count":3}})).unwrap();
        assert!(mirrored.iter().any(|(k,v)|k=="Mcp-Param-Count"&&v=="3"));
    }
    #[tokio::test]
    async fn modern_http_has_stateless_headers_pagination_and_call_param(){
        use axum::{extract::State,http::{HeaderMap,StatusCode},routing::post,Json,Router};
        use std::sync::{Arc,Mutex};
        #[derive(Clone)]struct Mock(Arc<Mutex<Vec<String>>>);
        async fn handler(State(state):State<Mock>,headers:HeaderMap,Json(body):Json<Value>)
            ->(StatusCode,Json<Value>){
            let method=body["method"].as_str().unwrap_or("");
            assert_eq!(headers.get("mcp-protocol-version").unwrap(),VERSION);
            assert_eq!(headers.get("mcp-method").unwrap(),method);
            assert_eq!(body["params"]["_meta"]["io.modelcontextprotocol/protocolVersion"],VERSION);
            assert_eq!(body["params"]["_meta"]["io.modelcontextprotocol/clientInfo"]["name"],"dominion-rust");
            assert!(headers.get("mcp-session-id").is_none());
            state.0.lock().unwrap().push(method.to_string());
            let result=if method=="tools/list"{
                if body["params"]["cursor"]=="page-two"{
                    json!({"tools":[{"name":"검색","inputSchema":{"type":"object","properties":{
                        "region":{"type":"string","x-mcp-header":"Region"}}}}]})
                }else{json!({"tools":[{"name":"echo","inputSchema":{"type":"object"}}],
                    "nextCursor":"page-two"})}
            }else{
                assert_eq!(headers.get("mcp-name").unwrap(),"=?base64?6rKA7IOJ?=");
                assert_eq!(headers.get("mcp-param-region").unwrap(),"ap-northeast-2");
                json!({"resultType":"complete","content":[{"type":"text","text":"ok"}]})
            };
            (StatusCode::OK,Json(json!({"jsonrpc":"2.0","id":body["id"],"result":result})))
        }
        let state=Mock(Arc::new(Mutex::new(Vec::new())));
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url=format!("http://{}/mcp",listener.local_addr().unwrap());
        let app=Router::new().route("/mcp",post(handler)).with_state(state.clone());
        let task=tokio::spawn(async move {axum::serve(listener,app).await.unwrap();});
        let record=json!({"url":url});
        let found=list(&record,None).await.unwrap();
        assert_eq!(found["tools"].as_array().unwrap().len(),2);
        let schema=found["tools"][1]["inputSchema"].clone();
        let response=call(&record,None,json!({"name":"검색",
            "arguments":{"region":"ap-northeast-2"}}),Some(&schema)).await.unwrap();
        assert_eq!(response["content"][0]["text"],"ok");
        assert_eq!(&*state.0.lock().unwrap(),&["tools/list","tools/list","tools/call"]);
        task.abort();
    }
}
