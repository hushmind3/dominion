//! 2026-07-28 stateless HTTP MCP envelope, routing and schema header contract.
//! The legacy stdio/HTTP handshake stays in the separate transport adapter.
use super::*;
use base64::Engine as _;

pub(super) const VERSION:&str="2026-07-28";

fn encode_header(value:&str)->String{
    let safe=!value.is_empty()&&value.trim()==value
        &&value.bytes().all(|b|(0x20..=0x7e).contains(&b))
        &&!(value.starts_with("=?base64?")&&value.ends_with("?="));
    if safe {value.to_string()} else {
        format!("=?base64?{}?=",base64::engine::general_purpose::STANDARD.encode(value))
    }
}

fn valid_header_name(name:&str)->bool{
    !name.is_empty()&&name.len()<=80&&name.bytes().all(|b|
        b.is_ascii_alphanumeric()||b"!#$%&'*+-.^_`|~".contains(&b))
}
fn schema_annotation_count(value:&Value)->usize{
    match value {
        Value::Object(map)=>map.iter().map(|(k,v)|
            usize::from(k=="x-mcp-header")+schema_annotation_count(v)).sum(),
        Value::Array(items)=>items.iter().map(schema_annotation_count).sum(),
        _=>0,
    }
}

fn annotated_headers(schema:&Value,args:&Value)->Result<Vec<(String,String)>,String>{
    fn visit(schema:&Value,args:&Value,seen:&mut std::collections::HashSet<String>,
        headers:&mut Vec<(String,String)>,annotations:&mut usize,depth:usize)->Result<(),String>{
        if depth>16{return Err("MCP x-mcp-header schema nesting too deep".into());}
        if let Some(annotation)=schema.get("x-mcp-header"){
            if depth==0{return Err("MCP x-mcp-header cannot annotate schema root".into());}
            *annotations+=1;
            let suffix=annotation.as_str().filter(|v|valid_header_name(v))
                .ok_or("Invalid MCP x-mcp-header name")?;
            let canonical=suffix.to_ascii_lowercase();
            if !seen.insert(canonical){return Err("Duplicate MCP x-mcp-header".into());}
            match schema["type"].as_str(){
                Some("string"|"integer"|"boolean")=>{},
                _=>return Err("MCP header must annotate a string, integer or boolean".into())
            }
            if !args.is_null() {
                let text=match schema["type"].as_str(){
                    Some("string")=>args.as_str().ok_or("MCP header expected string")?.to_string(),
                    Some("integer")=>{
                        let number=args.as_i64().ok_or("MCP header expected integer")?;
                        if number.unsigned_abs()>9_007_199_254_740_991 {
                            return Err("MCP header integer exceeds safe range".into());
                        }
                        number.to_string()
                    },
                    Some("boolean")=>args.as_bool().ok_or("MCP header expected boolean")?.to_string(),
                    _=>unreachable!()
                };
                if text.len()>8192{return Err("MCP header value exceeds limit".into());}
                headers.push((format!("Mcp-Param-{suffix}"),encode_header(&text)));
            }
        }
        if let Some(properties)=schema["properties"].as_object(){
            for (field,definition) in properties {
                visit(definition,&args[field],seen,headers,annotations,depth+1)?;
            }
        }
        Ok(())
    }
    let mut headers=Vec::new();
    let mut seen=std::collections::HashSet::new();
    let mut count=0;
    visit(schema,args,&mut seen,&mut headers,&mut count,0)?;
    if count!=schema_annotation_count(schema){
        return Err("MCP x-mcp-header must be reachable via properties only".into());
    }
    if headers.len()>64{return Err("Too many MCP tool routing headers".into());}
    Ok(headers)
}

/// Only an HTTP transport-level rejection without a recognized modern
/// JSON-RPC error demonstrates a legacy endpoint. Never fall back after auth,
/// server failure, timeout or a modern error with a protocol diagnostic.
pub(super) fn legacy_http_probe_rejection(error:&str)->bool{
    let recognized_status=["400 Bad Request","404 Not Found","405 Method Not Allowed"]
        .iter().any(|status|error.starts_with(&format!("MCP HTTP {status}:")));
    if !recognized_status{return false;}
    let body=error.split_once(": ").map(|(_,body)|body).unwrap_or("");
    if let Ok(value)=serde_json::from_str::<Value>(body){
        if value["jsonrpc"]=="2.0" && value["error"]["code"].is_number(){
            return false;
        }
    }
    true
}

/// An explicit modern UnsupportedProtocolVersionError may advertise only
/// handshake-era revisions. The probe is read-only, so selecting the newest
/// shared legacy version here is safe. Never infer a downgrade from 401/403/5xx.
pub(super) fn advertised_legacy_version(error:&str)->Option<&'static str>{
    if !error.starts_with("MCP HTTP 400 Bad Request:"){return None;}
    let body=error.split_once(": ")?.1;
    let value:Value=serde_json::from_str(body).ok()?;
    if value["jsonrpc"]!="2.0"||value["error"]["code"]!=-32022 {
        return None;
    }
    let versions=value["error"]["data"]["supported"].as_array()?;
    ["2025-11-25","2025-06-18","2025-03-26","2024-11-05"]
        .into_iter().find(|v|versions.iter().any(|item|item.as_str()==Some(v)))
}

/// An invalid annotated tool is omitted from tools/list, rather than silently
/// dropping a mandatory header and dispatching an unrouteable external effect.
pub(super) fn validate_tool_schema(schema:&Value)->Result<(),String>{
    annotated_headers(schema,&Value::Null).map(|_|())
}

pub(super) fn decorate(payload:&Value,tool_schema:Option<&Value>)
    ->Result<(Value,Vec<(String,String)>),String>{
    let method=payload["method"].as_str()
        .filter(|v|!v.is_empty()&&v.len()<=128)
        .ok_or("Missing MCP method")?;
    let mut params=payload["params"].as_object().cloned().unwrap_or_default();
    let mut meta=params.get("_meta").and_then(Value::as_object)
        .cloned().unwrap_or_default();
    meta.insert("io.modelcontextprotocol/protocolVersion".into(),json!(VERSION));
    meta.insert("io.modelcontextprotocol/clientInfo".into(),
        json!({"name":"dominion-rust","version":env!("CARGO_PKG_VERSION")}));
    meta.insert("io.modelcontextprotocol/clientCapabilities".into(),json!({}));
    params.insert("_meta".into(),Value::Object(meta));
    let mut body=payload.clone();
    body["params"]=Value::Object(params);
    let mut headers=vec![
        ("MCP-Protocol-Version".into(),VERSION.into()),
        ("Mcp-Method".into(),method.into())
    ];
    if matches!(method,"tools/call"|"prompts/get"|"resources/read"){
        let key=if method=="resources/read"{"uri"}else{"name"};
        let name=body["params"][key].as_str()
            .filter(|v|!v.is_empty()&&v.len()<=8192)
            .ok_or("Missing MCP routed tool, resource or prompt name")?;
        headers.push(("Mcp-Name".into(),encode_header(name)));
    }
    if method=="tools/call" {
        let schema=tool_schema.ok_or("Modern MCP tool call requires a fresh input schema")?;
        headers.extend(annotated_headers(schema,&body["params"]["arguments"])?);
    }
    Ok((body,headers))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn modern_mcp_request_carries_matching_version_identity_and_route(){
        let (body,headers)=decorate(&json!({"jsonrpc":"2.0","id":10,
            "method":"tools/call","params":{"name":"검색🧠",
            "arguments":{"zone":"서울","query":"q"}}}),
            Some(&json!({"type":"object","properties":{"zone":{
                "type":"string","x-mcp-header":"Region"}}}))).unwrap();
        assert_eq!(body["params"]["_meta"]["io.modelcontextprotocol/protocolVersion"],VERSION);
        assert_eq!(body["params"]["_meta"]["io.modelcontextprotocol/clientCapabilities"],json!({}));
        assert!(headers.iter().any(|(k,v)|k=="Mcp-Method"&&v=="tools/call"));
        assert!(headers.iter().any(|(k,v)|k=="Mcp-Name"&&v.starts_with("=?base64?")));
        assert!(headers.iter().any(|(k,v)|k=="Mcp-Param-Region"&&v.starts_with("=?base64?")));
    }
    #[test]
    fn modern_header_rejects_unreachable_duplicate_and_unsafe_annotations(){
        for schema in [
            json!({"type":"object","properties":{"a":{"type":"string","x-mcp-header":"X"},
                "b":{"type":"string","x-mcp-header":"x"}}}),
            json!({"type":"object","items":{"x-mcp-header":"X","type":"string"}}),
            json!({"type":"object","properties":{"a":{"x-mcp-header":"Bad\rHeader","type":"string"}}}),
            json!({"type":"object","properties":{"a":{"x-mcp-header":"X","type":"number"}}})
        ]{assert!(validate_tool_schema(&schema).is_err());}
    }
    #[test]
    fn modern_version_mismatch_advertises_explicit_safe_legacy_downgrade(){
        let error=r#"MCP HTTP 400 Bad Request: {"jsonrpc":"2.0","id":1,"error":{"code":-32022,"data":{"supported":["2025-06-18","2025-11-25"]}}}"#;
        assert_eq!(advertised_legacy_version(error),Some("2025-11-25"));
        assert_eq!(advertised_legacy_version(
            r#"MCP HTTP 400 Bad Request: {"jsonrpc":"2.0","error":{"code":-32020,"data":{"supported":["2025-11-25"]}}}"#),
            None);
        assert_eq!(advertised_legacy_version(
            r#"MCP HTTP 401 Unauthorized: {"error":"bad"}"#),None);
    }
    #[test]
    fn modern_jsonrpc_error_is_not_evidence_for_legacy_fallback(){
        assert!(legacy_http_probe_rejection("MCP HTTP 404 Not Found: not found"));
        assert!(legacy_http_probe_rejection("MCP HTTP 400 Bad Request: old handler"));
        assert!(!legacy_http_probe_rejection(
            "MCP HTTP 404 Not Found: {\"jsonrpc\":\"2.0\",\"error\":{\"code\":-32601}}"));
        assert!(!legacy_http_probe_rejection("MCP HTTP 401 Unauthorized: auth required"));
        assert!(!legacy_http_probe_rejection("MCP HTTP 500 Internal Server Error: offline"));
    }
    #[test]
    fn modern_header_escapes_spaces_unicode_newlines_and_sentinel(){
        for (value,encoded) in [
            (" normal ","=?base64?"),
            ("개행\n금지","=?base64?"),
            ("=?base64?abc?=","=?base64?")
        ]{assert!(encode_header(value).starts_with(encoded));}
        assert_eq!(encode_header("us-west1"),"us-west1");
        assert!(!valid_header_name("Bad\rName"));
    }
}
