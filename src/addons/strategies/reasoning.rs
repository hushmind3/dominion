//! Product host resolves endpoint/account metadata; pure policy belongs to the SDK algorithm layer.
use serde_json::Value;
pub use dominion_algorithms::reasoning::*;

/// Metadata for the same exact route used by the transport.
pub fn route_controls(state:&crate::state::AppState,provider:&Value,route:&crate::engine::gateway::Route)->Value {
    let caps = crate::engine::gateway::ModelCapabilities::for_route(provider, &route.model);
    let catalog = if route.kind == "codex" {
        state.store.get("codex_model_catalog", &route.id).filter(|catalog| {
            route.account_id.as_deref().is_some_and(|account| catalog["accountId"].as_str() == Some(account))
        })
    } else { None };
    let endpoint = provider["endpointId"].as_str().filter(|id| !id.is_empty())
        .or_else(|| provider["baseUrl"].as_str()).unwrap_or("");
    let declared = if route.kind == "codex" {
        catalog.as_ref().and_then(|row| row["models"].as_array())
            .and_then(|rows| rows.iter().find(|row| row["id"].as_str() == Some(route.model.as_str())))
            .and_then(|row| row["reasoningEfforts"].as_array())
    } else {
        [&provider["endpointModelCapabilities"][endpoint][route.model.as_str()],
         &provider["endpointCapabilities"][endpoint],
         &provider["modelCapabilities"][route.model.as_str()], &provider["capabilities"]]
            .into_iter().find_map(|layer| layer["reasoningEfforts"].as_array())
    };
    inference_controls(&route.kind, &route.reasoning_protocol,
        declared.map(Vec::as_slice).unwrap_or_default(), caps.reasoning)

}

