use crate::engine;
use crate::state::AppState;
use axum::{extract::{Path,Query,State},http::StatusCode,response::{IntoResponse,Response},Json};
use serde_json::{json,Value};
use std::collections::HashMap;
use std::time::Duration;
use super::response::error;

async fn bounded_registry_json(mut response:reqwest::Response)->Result<Value,String>{
    const LIMIT:usize=4*1024*1024;
    if response.content_length().is_some_and(|length|length>LIMIT as u64){
        return Err("MCP registry response exceeded 4 MiB".into());
    }
    let mut bytes=Vec::new();
    while let Some(chunk)=response.chunk().await.map_err(|error|error.to_string())?{
        if bytes.len().saturating_add(chunk.len())>LIMIT{
            return Err("MCP registry response exceeded 4 MiB".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|error|error.to_string())
}

pub async fn list_tools(State(state): State<AppState>) -> Json<Value> {
    let mut items = engine::tools::definitions()
        .into_iter()
        .filter_map(|definition| {
            let function = definition.get("function")?;
            let name = function.get("name")?.as_str()?.to_string();
            Some(json!({
                "name": name,
                "description": function.get("description"),
                "parameters": function.get("parameters"),
                "approval": engine::tools::requires_approval(&name),
                "source": "builtin",
            }))
        })
        .collect::<Vec<_>>();

    for definition in crate::extension_manager::definitions(&state){
        let function=&definition["function"];
        items.push(json!({"name":function["name"],"description":function["description"],
            "parameters":function["parameters"],"approval":true,"source":"extension"}));
    }
    for tool in engine::mcp::tools(&state).await {
        let qualified = tool.qualified();
        items.push(json!({
            "name": qualified,
            "description": format!("[MCP · {}] {}", tool.server_name, tool.description),
            "parameters": tool.schema,
            "approval": engine::mcp::requires_approval(&state, &qualified),
            "source": "mcp",
            "server": tool.server_name,
        }));
    }
    Json(json!(items))
}

pub async fn mcp_presets() -> Json<Value> {
    // Filesystem, git, memory and shell are native Rust tools. Do not push
    // third-party npm MCP servers for capabilities the engine already owns.
    Json(json!([]))
}

pub async fn mcp_discover(State(state): State<AppState>) -> Json<Value> {
    let mut items=Vec::new();
    for (id,server,result) in engine::mcp::discover_servers(&state).await {
        match result {
            Ok(tools) if tools.is_empty()=>items.push(json!({
                "mcpId":id,"server":server,"name":format!("status:{id}"),
                "status":"empty","description":"Connected; no tools advertised"
            })),
            Err(error)=>items.push(json!({
                "mcpId":id,"server":server,"name":format!("status:{id}"),
                "status":"failed","error":error
            })),
            Ok(tools)=>for tool in tools {
                let qualified=tool.qualified();
                items.push(json!({
                    "mcpId":id,"server":tool.server_name,"name":qualified,
                    "tool":tool.tool,"description":tool.description,"parameters":tool.schema,
                    "approval":engine::mcp::requires_approval(&state,&qualified),
                    "status":"connected"
                }));
            }
        }
    }
    Json(json!(items))
}

pub async fn export_skill(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match state.store.get("skill", &id) {
        Some(skill) => Json(skill).into_response(),
        None => error(StatusCode::NOT_FOUND, "스킬을 찾을 수 없습니다."),
    }
}

pub async fn skill_catalog(State(state): State<AppState>) -> Json<Value> {
    Json(engine::skills::catalog(&state))
}

pub async fn skill_catalog_meta(State(state): State<AppState>) -> Json<Value> {
    Json(json!({
        "roots": engine::skills::roots(&state).into_iter().map(|(source, path)| json!({
            "source": source,
            "path": path.to_string_lossy(),
        })).collect::<Vec<_>>(),
    }))
}

pub async fn import_skill(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    let id = body
        .get("id")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    match engine::skills::import(&state, id) {
        Ok(record) => Json(record).into_response(),
        Err(message) => error(StatusCode::BAD_REQUEST, &message),
    }
}

pub async fn import_skill_repo(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    let repo = body
        .get("repo")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    let result = tokio::task::spawn_blocking(move || engine::skills::import_repo(&state, &repo))
        .await;
    match result {
        Ok(Ok(result)) => Json(result).into_response(),
        Ok(Err(message)) => error(StatusCode::BAD_REQUEST, &message),
        Err(join_error) => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("스킬 저장소 작업이 중단되었습니다: {join_error}"),
        ),
    }
}

pub async fn mcp_catalog(Query(query): Query<HashMap<String, String>>) -> Response {
    let mut endpoint =
        match reqwest::Url::parse("https://registry.modelcontextprotocol.io/v0.1/servers") {
            Ok(url) => url,
            Err(message) => return error(StatusCode::INTERNAL_SERVER_ERROR, &message.to_string()),
        };
    let search = query.get("q").cloned().unwrap_or_default();
    if search.len() >= 3 {
        endpoint.query_pairs_mut().append_pair("search", &search);
    }
    endpoint
        .query_pairs_mut()
        .append_pair("version", "latest")
        .append_pair("limit", "60");

    let client = match reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
    {
        Ok(client)=>client,
        Err(build_error)=>return error(StatusCode::INTERNAL_SERVER_ERROR,&build_error.to_string()),
    };
    let response = client
        .get(endpoint)
        .timeout(Duration::from_secs(15))
        .send()
        .await;
    let response = match response {
        Ok(response) if response.status().is_success() => response,
        Ok(response) => {
            return error(
                StatusCode::BAD_GATEWAY,
                &format!("MCP registry HTTP {}", response.status()),
            )
        }
        Err(message) => return error(StatusCode::BAD_GATEWAY, &message.to_string()),
    };
    let value: Value = match bounded_registry_json(response).await {
        Ok(value) => value,
        Err(message) => return error(StatusCode::BAD_GATEWAY, &message.to_string()),
    };
    let mut seen = std::collections::HashSet::new();
    let items = value
        .get("servers")
        .and_then(|value| value.as_array())
        .map(|servers| {
            servers
                .iter()
                .filter_map(|entry| {
                    let server = entry.get("server").unwrap_or(entry);
                    let name = server.get("name").and_then(|value| value.as_str())?;
                    if !seen.insert(name.to_string()) {
                        return None;
                    }
                    Some(registry_item(server,name))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Json(json!(items)).into_response()
}

// Only fully specified supported launch configurations are offered for
// automatic registration. Complex argument/header/env templates need review.
fn nonempty_field(value:&Value,key:&str)->bool {
    value.get(key).is_some_and(|v| !v.is_null() &&
        !v.as_array().is_some_and(Vec::is_empty) &&
        !v.as_object().is_some_and(serde_json::Map::is_empty))
}
fn package_identifier(value:&str,kind:&str)->bool{
    let valid=|part:&str| !part.is_empty() && part.as_bytes()[0].is_ascii_alphanumeric()
        && part.bytes().all(|b|b.is_ascii_alphanumeric()||matches!(b,b'-'|b'_'|b'.'));
    if kind=="npm" && value.starts_with('@') {
        value[1..].split_once('/').is_some_and(|(scope,name)|valid(scope)&&valid(name))
    }else{valid(value)}
}
fn package_version(value:&str)->bool{
    !value.is_empty() && value.len()<=128 && value.as_bytes()[0].is_ascii_digit()
        && value.bytes().all(|b|b.is_ascii_alphanumeric()||matches!(b,b'.'|b'-'|b'+'|b'_'))
        && !value.split('.').any(|part|part.eq_ignore_ascii_case("x"))
}
fn registry_item(server:&Value,name:&str)->Value{
    let mut item=json!({"name":name,"description":server["description"],
        "version":server["version"],"installable":false,"manualSetup":true});
    for remote in server["remotes"].as_array().into_iter().flatten(){
        let Some(raw)=remote["url"].as_str() else{continue};
        if !matches!(remote["type"].as_str(),Some("streamable-http"|"sse"))
            || nonempty_field(remote,"headers") || nonempty_field(remote,"variables")
            || raw.contains('{')||raw.contains('}') {continue;}
        let Ok(url)=reqwest::Url::parse(raw) else{continue};
        if url.scheme()!="https"||url.host_str().is_none()||!url.username().is_empty()
            ||url.password().is_some()||url.fragment().is_some() {continue;}
        item["url"]=json!(raw);
        item["transport"]=json!("http");
        item["installable"]=json!(true);
        item["manualSetup"]=json!(false);
        return item;
    }
    for package in server["packages"].as_array().into_iter().flatten(){
        let kind=package["registryType"].as_str()
            .or_else(||package["registry_type"].as_str()).unwrap_or("");
        let (runtime,base)=match kind{
            "npm"=>("npx","https://registry.npmjs.org"),
            "pypi"=>("uvx","https://pypi.org"),
            _=>continue,
        };
        let Some(identifier)=package["identifier"].as_str() else{continue};
        let Some(version)=package["version"].as_str() else{continue};
        let exact_npm=kind!="npm" || {
            let core=version.split(['-','+']).next().unwrap_or("");
            let fields=core.split('.').collect::<Vec<_>>();
            fields.len()==3 && fields.iter().all(|part|!part.is_empty()&&part.bytes().all(|b|b.is_ascii_digit()))
        };
        if !package_identifier(identifier,kind)||!package_version(version)||!exact_npm
            || package["transport"]["type"]!="stdio"
            || package["runtimeHint"].as_str().is_some_and(|hint|hint!=runtime)
            || package["registryBaseUrl"].as_str().is_some_and(|url|url.trim_end_matches('/')!=base)
            || ["runtimeArguments","packageArguments","environmentVariables","runtime_arguments",
                "package_arguments","environment_variables"].iter().any(|key|nonempty_field(package,key))
        {continue;}
        let spec=if kind=="npm"{format!("{identifier}@{version}")}else{format!("{identifier}=={version}")};
        let args=if kind=="npm"{vec!["-y".to_string(),spec.clone()]}else{vec![spec.clone()]};
        item["packageName"]=json!(spec);
        item["packageVersion"]=json!(version);
        item["transport"]=json!("stdio");
        item["command"]=json!(runtime);
        item["args"]=json!(args);
        item["installable"]=json!(true);
        item["manualSetup"]=json!(false);
        return item;
    }
    item
}

#[cfg(test)]
mod registry_tests{
    use super::*;
    #[test]
    fn concrete_remote_preferred_and_packages_pinned(){
        let package=json!({"registryType":"npm","identifier":"@sample/server","version":"2.1.0",
            "transport":{"type":"stdio"}});
        let remote=json!({"type":"streamable-http","url":"https://example.com/mcp"});
        let item=registry_item(&json!({"packages":[package.clone()],"remotes":[remote]}),"example/server");
        assert_eq!(item["transport"],"http");
        let item=registry_item(&json!({"packages":[package]}),"example/server");
        assert_eq!(item["args"],json!(["-y","@sample/server@2.1.0"]));
        let python=registry_item(&json!({"packages":[{"registryType":"pypi",
            "identifier":"sample-server","version":"2026.11.2","transport":{"type":"stdio"}}]}),"python");
        assert_eq!(python["command"],"uvx");
        assert_eq!(python["args"],json!(["sample-server==2026.11.2"]));
    }
    #[test]
    fn unsupported_or_templated_descriptors_require_manual_setup(){
        for package in [
            json!({"registryType":"npm","identifier":"--evil","version":"1.0.0","transport":{"type":"stdio"}}),
            json!({"registryType":"npm","identifier":"sample","version":"latest","transport":{"type":"stdio"}}),
            json!({"registryType":"npm","identifier":"sample","version":"1.x","transport":{"type":"stdio"}}),
            json!({"registryType":"npm","identifier":"sample","version":"1.2","transport":{"type":"stdio"}}),
            json!({"registryType":"npm","identifier":"sample","version":"1.0.0","transport":{"type":"stdio"},
                "environmentVariables":[{"name":"TOKEN","isRequired":true}]}),
            json!({"registryType":"npm","identifier":"sample","version":"1.0.0","transport":{"type":"stdio"},
                "packageArguments":[{"type":"positional","value":"some/path"}]})
        ]{
            let item=registry_item(&json!({"packages":[package]}),"manual");
            assert_eq!(item["installable"],false);
            assert!(item["command"].is_null());
        }
        let item=registry_item(&json!({"remotes":[{"type":"streamable-http",
            "url":"https://example.com/{tenant}/mcp","headers":[{"name":"Authorization"}]}]}),"manual");
        assert_eq!(item["manualSetup"],true);
    }
}
