use crate::engine;
use crate::state::AppState;
use axum::{extract::{Path,Query,State},http::StatusCode,response::{IntoResponse,Response},Json};
use serde_json::{json,Value};
use std::collections::HashMap;
use std::time::Duration;
use super::response::error;
pub(crate) mod plugin_catalog;
pub use plugin_catalog::search as plugin_search;

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

    for tool in crate::integrations::registry::native_external_tools(&state).await {
        items.push(json!({"name":tool.name,"description":tool.description,"parameters":tool.schema,
            "approval":crate::integrations::registry::tool_requires_approval(&state,&tool.name),"source":"external"}));
    }
    Json(json!(items))
}

pub async fn mcp_runtime() -> Json<Value> { Json(json!({"available":cfg!(feature="legacy-mcp"),"legacy":true})) }

pub async fn mcp_presets() -> Json<Value> {
    // Filesystem, git, memory and shell are native Rust tools. Do not push
    // third-party npm MCP servers for capabilities the engine already owns.
    Json(json!([]))
}

pub async fn mcp_discover(State(state): State<AppState>) -> Json<Value> {
    let (tools, failures) = crate::integrations::mcp::tools_with_errors(&state).await;
    let mut items = tools
        .into_iter()
        .into_iter()
        .map(|tool| {
            let qualified = tool.qualified();
            json!({
                "mcpId":tool.server_id,
                "server": tool.server_name,
                "name": qualified,
                "tool": tool.tool,
                "description": tool.description,
                "parameters": tool.schema,
                "approval": crate::integrations::mcp::requires_approval(&state, &qualified),
            })
        })
        .collect::<Vec<_>>();
    items.extend(failures);
    Json(json!(items))
}

pub async fn export_skill(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match state.store.get("skill", &id) {
        Some(skill) => Json(skill).into_response(),
        None => error(StatusCode::NOT_FOUND, "스킬을 찾을 수 없습니다."),
    }
}

pub async fn skill_catalog(State(state): State<AppState>) -> Json<Value> {
    Json(crate::integrations::skills::catalog::catalog(&state))
}

pub async fn skill_catalog_meta(State(state): State<AppState>) -> Json<Value> {
    Json(json!({
        "roots": crate::integrations::skills::catalog::roots(&state).into_iter().map(|(source, path)| json!({
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
    match crate::integrations::skills::catalog::import(&state, id) {
        Ok(record) => Json(record).into_response(),
        Err(message) => error(StatusCode::BAD_REQUEST, &message),
    }
}

pub async fn import_skill_repo(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    let repo = body
        .get("repo")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    match crate::integrations::skills::catalog::import_repo(&state, repo).await {
        Ok(result) => Json(result).into_response(),
        Err(message) => error(StatusCode::BAD_REQUEST, &message),
    }
}

// Only project launchable, pinned packages. A manifest that declares
// additional arguments, environment or integrity requirements needs manual
// configuration; never silently drop those requirements.
fn registry_package(package: &Value) -> Option<(&str, &str, &str)> {
    let registry = package.get("registryType").or_else(||package.get("registry_type"))?.as_str()?;
    let name = package.get("identifier")?.as_str()?;
    let version = package.get("version")?.as_str()?;
    let (runner, base) = match registry {
        "npm" => ("npx", "https://registry.npmjs.org"),
        "pypi" => ("uvx", "https://pypi.org"),
        _ => return None,
    };
    if package.pointer("/transport/type").and_then(Value::as_str) != Some("stdio")
        || package.get("runtimeHint").is_some_and(|v| v.as_str() != Some(runner))
        || package.get("registryBaseUrl").is_some_and(|v| v.as_str().map(|s|s.trim_end_matches('/')) != Some(base))
        || ["runtimeArguments","packageArguments","environmentVariables"].iter().any(|key|
            package.get(*key).is_some_and(|v| !v.as_array().is_some_and(Vec::is_empty)))
        || package.get("fileSha256").is_some()
    { return None; }
    let part = |s: &str| !s.is_empty() && s.as_bytes()[0].is_ascii_alphanumeric()
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b,b'-'|b'_'|b'.'));
    let name_ok = if registry == "npm" {
        if let Some(scoped) = name.strip_prefix('@') {
            scoped.split_once('/').is_some_and(|(scope, package)| part(scope) && part(package))
        } else { part(name) }
    } else { part(name) };
    if !name_ok || name.len() > 214 || version.len() > 100
        || !version.as_bytes().first().is_some_and(u8::is_ascii_digit)
        || !version.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b,b'.'|b'-'|b'+'|b'!'))
    { return None; }
    // npm dist-tags, ranges and shorthand major/minor releases are not pins.
    if registry == "npm" {
        let core = version.split(['-','+']).next()?;
        let parts = core.split('.').collect::<Vec<_>>();
        if parts.len() != 3 || parts.iter().any(|p| p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit())) {
            return None;
        }
    }
    Some((registry,name,version))
}

#[cfg(test)]
mod registry_package_tests {
    use super::*;
    #[test]
    fn only_pinned_simple_stdio_packages_are_projected() {
        let mut npm = json!({"registryType":"npm","identifier":"@scope/server","version":"1.2.3","transport":{"type":"stdio"}});
        assert_eq!(registry_package(&npm),Some(("npm","@scope/server","1.2.3")));
        for bad in ["latest","1","1.2","^1.2.3","1.2.3;echo"] {
            npm["version"]=json!(bad);
            assert!(registry_package(&npm).is_none(),"{bad}");
        }
        npm["version"]=json!("1.2.3");
        npm["environmentVariables"]=json!([{"name":"API_KEY","isRequired":true}]);
        assert!(registry_package(&npm).is_none());
        let mut py=json!({"registryType":"pypi","identifier":"mcp-server","version":"2.0.1","transport":{"type":"stdio"}});
        assert_eq!(registry_package(&py),Some(("pypi","mcp-server","2.0.1")));
        py["runtimeArguments"]=json!([{"type":"positional","value":"unreviewed"}]);
        assert!(registry_package(&py).is_none());
        py["runtimeArguments"]=json!([]);
        py["fileSha256"]=json!("unverified");
        assert!(registry_package(&py).is_none());
    }
}

pub async fn mcp_catalog(Query(query): Query<HashMap<String, String>>) -> Response {
    let mut endpoint =
        match reqwest::Url::parse("https://registry.modelcontextprotocol.io/v0.1/servers") {
            Ok(url) => url,
            Err(message) => return error(StatusCode::INTERNAL_SERVER_ERROR, &message.to_string()),
        };
    let search = query.get("q").cloned().unwrap_or_default();
    if search.len()>160||search.chars().any(char::is_control){return error(StatusCode::BAD_REQUEST,"Invalid MCP registry query")}
    if !search.trim().is_empty() {
        endpoint.query_pairs_mut().append_pair("search", &search);
    }
    endpoint
        .query_pairs_mut()
        .append_pair("version", "latest")
        .append_pair("limit", "60");

    let response = reqwest::Client::new()
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
    let value: Value = match crate::integrations::network::read_bounded_json(
        response,4*1024*1024,"MCP registry catalog").await {
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
                    let package = server.get("packages")
                        .and_then(Value::as_array)
                        .and_then(|items| items.iter().find_map(registry_package));
                    let remote = server
                        .get("remotes")
                        .and_then(|value| value.as_array())
                        .and_then(|items| {
                            items.iter().find(|remote| {
                                remote["type"]=="streamable-http"
                                    && remote.get("headers").is_none_or(|v|v.as_array().is_some_and(Vec::is_empty))
                                    && remote.get("url").and_then(Value::as_str).is_some_and(|url|
                                        !url.contains(['{','}']) && reqwest::Url::parse(url).is_ok_and(|u|
                                            u.scheme()=="https"&&u.host_str().is_some()&&u.username().is_empty()&&u.password().is_none()&&u.fragment().is_none()))
                            })
                        });
                    Some(json!({
                        "name": name,
                        "description": server.get("description"),
                        "packageName": package.as_ref().map(|p| p.1),
                        "packageVersion": package.as_ref().map(|p| p.2),
                        "packageRegistry": package.as_ref().map(|p| p.0),
                        "url": remote.and_then(|remote| remote.get("url")),
                    }))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Json(json!(items)).into_response()
}

/// Runtime toggles for optional extensions. The UI treats this as a replaceable
/// policy object, so a partial map merges and an empty map restores defaults.
pub async fn runtime_extensions(State(state): State<AppState>) -> Json<Value> {
    let configured = stored_extensions(&state);
    Json(json!({
        "enabled": configured,
        "configured": configured,
        "runtime": "optional-extensions-v1"
    }))
}

pub async fn set_runtime_extensions(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Response {
    let Some(patch) = body.as_object() else {
        return error(StatusCode::BAD_REQUEST, "설정 형식이 올바르지 않습니다.");
    };
    let mut configured = stored_extensions(&state);
    if patch.is_empty() {
        configured = json!({});
    } else {
        for (key, value) in patch {
            if let Some(on) = value.as_bool() {
                configured[key] = json!(on);
            }
        }
    }
    if state
        .store
        .put(
            "extension_policy",
            &json!({"id":"default","extensions":configured}),
        )
        .is_none()
    {
        return error(StatusCode::INTERNAL_SERVER_ERROR, "설정을 저장하지 못했습니다.");
    }
    Json(json!({
        "enabled": configured,
        "configured": configured,
        "runtime": "optional-extensions-v1"
    }))
    .into_response()
}

fn stored_extensions(state: &AppState) -> Value {
    state
        .store
        .get("extension_policy", "default")
        .and_then(|record| record.get("extensions").cloned())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}))
}
