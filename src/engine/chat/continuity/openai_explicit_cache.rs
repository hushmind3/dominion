//! 2026 first-party explicit prompt-cache checkpoint negotiation.
use super::*;

/// Only the first-party GPT-5.6+ API supports the 2026 explicit cache protocol.
pub(super) fn openai_explicit_prefix_supported(p:&ProviderRef)->bool {
    if !p.kind.eq_ignore_ascii_case("openai"){return false;}
    let Ok(url)=reqwest::Url::parse(&p.base_url) else{return false};
    if url.scheme()!="https" || url.host_str()!=Some("api.openai.com")
        || !url.path().starts_with("/v1") {return false;}
    let model=p.model.to_ascii_lowercase();
    let Some(version)=model.strip_prefix("gpt-") else{return false};
    let mut parts=version.split('-').next().unwrap_or("").split('.');
    let major=parts.next().and_then(|p|p.parse::<u32>().ok()).unwrap_or(0);
    let minor=parts.next().and_then(|p|p.parse::<u32>().ok()).unwrap_or(0);
    major>=6 || (major==5&&minor>=6)
}
pub(super) fn explicit_feature_id(p:&ProviderRef)->String {
    format!("wire-{}",stable_id(&["openai-explicit-prefix-2026",
        &p.id,&p.base_url,&p.model,p.account_id.as_deref().unwrap_or("")]))
}
pub(super) fn explicit_prefix_disabled(state:&AppState,p:&ProviderRef)->bool {
    state.store.get("provider_wire_feature",&explicit_feature_id(p))
        .is_some_and(|f|f["accepted"]==false
            && f["expiresAt"].as_i64().is_some_and(|t|t>chrono::Utc::now().timestamp()))
}
/// Mark only the unchanged system prefix; a changing owner message must never
/// generate a new paid cache write in the default implicit mode.
pub(super) fn mark_openai_explicit_prefix(body:&mut Value)->bool {
    let Some(messages)=body["messages"].as_array_mut() else{return false};
    let Some(first)=messages.first_mut() else{return false};
    if !matches!(first["role"].as_str(),Some("system"|"developer")){return false;}
    let Some(content)=first["content"].as_str().map(str::to_string) else{return false};
    if super::room_context::estimate(&content)<1024{return false;}
    first["content"]=json!([{"type":"text","text":content,
        "prompt_cache_breakpoint":{"mode":"explicit"}}]);
    body["prompt_cache_options"]=json!({"mode":"explicit"});
    true
}
pub(super) fn record_explicit_rejection(state:&AppState,p:&ProviderRef,status:u16){
    state.store.put("provider_wire_feature",&json!({
        "id":explicit_feature_id(p),"field":"prompt_cache_breakpoint",
        "accepted":false,"httpStatus":status,
        "observedAt":chrono::Utc::now().to_rfc3339(),
        "expiresAt":chrono::Utc::now().timestamp()+86400
    }));
}

