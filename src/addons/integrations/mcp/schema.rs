//! MCP 2026-07-28 `x-mcp-header` JSON Schema admission and wire projection.
//! Tool schemas are untrusted: reject invalid annotations before tool exposure.
use serde_json::Value;
use std::collections::HashSet;

#[derive(Clone, Copy)]
enum Primitive { String, Integer, Boolean }
struct HeaderBinding { name: String, path: Vec<String>, kind: Primitive }

fn is_http_token(value:&str)->bool {
    !value.is_empty() && value.bytes().all(|byte|
        byte.is_ascii_alphanumeric()
        || matches!(byte,b'!'|b'#'|b'$'|b'%'|b'&'|b'\''|b'*'|b'+'|b'-'|b'.'|b'^'|b'_'|b'`'|b'|'|b'~'))
}

fn visit(node:&Value, reachable:bool, path:&mut Vec<String>,
    headers:&mut Vec<HeaderBinding>, seen:&mut HashSet<String>) -> Result<(),String> {
    if let Some(object)=node.as_object() {
        if let Some(header)=object.get("x-mcp-header") {
            let name=header.as_str().ok_or("x-mcp-header must be a string")?;
            if !reachable || path.is_empty() || !is_http_token(name) {
                return Err("invalid x-mcp-header location or name".into());
            }
            if !seen.insert(name.to_ascii_lowercase()) {
                return Err("duplicate case-insensitive x-mcp-header".into());
            }
            let kind=match object.get("type").and_then(Value::as_str) {
                Some("string")=>Primitive::String,
                Some("integer")=>Primitive::Integer,
                Some("boolean")=>Primitive::Boolean,
                _=>return Err("x-mcp-header requires a string, integer, or boolean property".into()),
            };
            headers.push(HeaderBinding{name:name.into(),path:path.clone(),kind});
        }
        for (key,value) in object {
            if key=="x-mcp-header" {continue;}
            if key=="properties" && reachable {
                let props=value.as_object().ok_or("JSON Schema properties must be an object")?;
                for (property,nested) in props {
                    path.push(property.clone());
                    visit(nested,true,path,headers,seen)?;
                    path.pop();
                }
            }else {
                // Annotations hidden under composition, items or refs cannot
                // be mapped to a single static request-argument property path.
                visit(value,false,path,headers,seen)?;
            }
        }
    }else if let Some(items)=node.as_array() {
        for item in items { visit(item,false,path,headers,seen)?; }
    }
    Ok(())
}

fn bindings(input_schema:&Value)->Result<Vec<HeaderBinding>,String>{
    if !input_schema.is_object(){return Err("MCP inputSchema must be a JSON Schema object".into());}
    let mut result=Vec::new();
    visit(input_schema,true,&mut Vec::new(),&mut result,&mut HashSet::new())?;
    Ok(result)
}
/// Validate tool headers without having to trust a server's schema annotations.
pub(super) fn validate(input_schema:&Value)->Result<(),String>{
    bindings(input_schema).map(|_|())
}

pub(super) fn encode_header(raw:&str)->String{
    let plain = !raw.is_empty() && raw.trim()==raw && raw.bytes().all(|byte|byte>=32&&byte<=126)
        && !(raw.starts_with("=?base64?")&&raw.ends_with("?="));
    if plain {return raw.into();}
    use base64::Engine as _;
    format!("=?base64?{}?=",base64::engine::general_purpose::STANDARD.encode(raw.as_bytes()))
}

pub(super) fn projected_headers(schema:&Value,args:&Value)->Result<Vec<(String,String)>,String>{
    let mut out=Vec::new();
    for binding in bindings(schema)? {
        let mut value=args;
        for key in &binding.path {
            let Some(child)=value.get(key) else {value=&Value::Null;break};
            value=child;
        }
        if value.is_null(){continue;}
        let raw=match binding.kind {
            Primitive::String=>value.as_str().ok_or("MCP header string argument has wrong type")?.to_string(),
            Primitive::Boolean=>value.as_bool().ok_or("MCP header boolean argument has wrong type")?.to_string(),
            Primitive::Integer=>{
                const MAX:i64=9_007_199_254_740_991;
                let number=value.as_i64().ok_or("MCP header integer argument has wrong type")?;
                if !(-MAX..=MAX).contains(&number){return Err("MCP header integer is outside JS safe range".into());}
                number.to_string()
            },
        };
        out.push((format!("Mcp-Param-{}",binding.name),encode_header(&raw)));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn mirrors_nested_primitive_headers_and_encodes_unicode_without_injection() {
        let schema=json!({"type":"object","properties":{"options":{"type":"object",
            "properties":{"region":{"type":"string","x-mcp-header":"Region"},
                "retry":{"type":"boolean","x-mcp-header":"Retry"}}}}});
        let headers=projected_headers(&schema,&json!({"options":{"region":"서울","retry":true}})).unwrap();
        assert_eq!(headers[0],("Mcp-Param-Region".into(),"=?base64?7ISc7Jq4?=".into()));
        assert_eq!(headers[1],("Mcp-Param-Retry".into(),"true".into()));
        assert_eq!(projected_headers(&schema,&json!({})).unwrap().len(),0);
    }
    #[test]
    fn rejects_unroutable_or_duplicate_annotations() {
        assert!(validate(&json!({"type":"object","properties":{"x":{"type":"number",
            "x-mcp-header":"Region"}}})).is_err());
        assert!(validate(&json!({"type":"object","properties":{"x":{"type":"string",
            "x-mcp-header":"Region"},"y":{"type":"string","x-mcp-header":"region"}}})).is_err());
        assert!(validate(&json!({"type":"object","allOf":[{"properties":{"x":{
            "type":"string","x-mcp-header":"Region"}}}]})).is_err());
        assert!(validate(&json!({"type":"object","properties":{"x":{"type":"string",
            "x-mcp-header":"bad\\r\\nheader"}}})).is_err());
    }
}
