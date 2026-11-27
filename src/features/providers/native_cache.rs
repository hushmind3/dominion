//! Provider prompt-cache hints for the native Rust agent, independent of
//! chat's participant-working-view cache. Hints are optional and never grant
//! permission to replay a request after uncertain network delivery.
use crate::{engine::gateway::Route,state::AppState};
use serde_json::{json,Value};
use sha2::{Digest,Sha256};

const MIN_CACHEABLE_PREFIX_TOKENS:u64=1024;
const DISABLE_SECONDS:i64=24*60*60;

fn feature_id(route:&Route)->String {
    let identity=json!({
        "providerId":route.id,"model":route.model,"baseUrl":route.base_url,
        "kind":route.kind,"accountId":route.account_id
    });
    let digest=format!("{:x}",Sha256::digest(identity.to_string().as_bytes()));
    format!("native-prompt-cache-{digest}")
}
fn cache_declared_off(state:&AppState,route:&Route)->bool {
    state.store.get("provider",&route.id)
        .is_some_and(|provider|provider["capabilities"]["promptCache"]==false
            ||provider["capabilities"]["promptCaching"]==false)
}
fn previously_rejected(state:&AppState,route:&Route)->bool {
    state.store.get("provider_wire_feature",&feature_id(route))
        .is_some_and(|feature|feature["accepted"]==false
            &&feature["expiresAt"].as_i64()
                .is_some_and(|until|until>chrono::Utc::now().timestamp()))
}
fn prefix_tokens(body:&Value)->u64 {
    let tools=body["tools"].to_string();
    let system=body["system"].as_str().unwrap_or("");
    crate::engine::chat::estimate_text_tokens(&format!("{tools}{system}"))
}
/// Cache only the unchanged tools and system, never unique user/task data or
/// the latest result. Explicit 5-minute ephemeral breakpoints avoid charging
/// for a new cache write after every dynamically updated task prompt.
pub(crate) fn hinted(
    state:&AppState,route:&Route,original:&Value
)->(Value,bool,u64) {
    let tokens=prefix_tokens(original);
    let local_opt_in=state.store.get("provider",&route.id).is_some_and(|provider|
        provider["capabilities"]["promptCaching"]==true);
    if route.kind!="anthropic" || (route.local && !local_opt_in)
        || cache_declared_off(state,route) || previously_rejected(state,route)
        ||tokens<MIN_CACHEABLE_PREFIX_TOKENS {
        return (original.clone(),false,tokens);
    }
    let mut body=original.clone();
    let mut applied=false;
    if let Some(tools)=body["tools"].as_array_mut() {
        if let Some(tool)=tools.last_mut() {
            tool["cache_control"]=json!({"type":"ephemeral"});
            applied=true;
        }
    }
    if let Some(system)=original["system"].as_str().filter(|s|!s.is_empty()){
        body["system"]=json!([{
            "type":"text","text":system,"cache_control":{"type":"ephemeral"}
        }]);
        applied=true;
    }
    (body,applied,tokens)
}
/// Explicit 400/422 feature errors indicate the provider rejected the optional
/// hint before generation. Other statuses, generic schema errors, quota
/// failures and network interruptions never authorize another paid request.
pub(crate) fn unsupported(status:reqwest::StatusCode,text:&str)->bool {
    if !matches!(status.as_u16(),400|422){return false;}
    let parsed=serde_json::from_str::<Value>(text).unwrap_or(Value::Null);
    let param=parsed["error"]["param"].as_str()
        .or_else(||parsed["param"].as_str()).unwrap_or("");
    let message=parsed["error"]["message"].as_str()
        .or_else(||parsed["message"].as_str()).unwrap_or(text);
    let lower=message.to_ascii_lowercase();
    (param.contains("cache_control")||lower.contains("cache_control"))
        &&["unsupported","not supported","unknown","unrecognized",
            "not allowed","not permitted","extra_forbidden","extra inputs",
            "invalid parameter","unexpected field"]
            .iter().any(|keyword|lower.contains(keyword))
}
pub(crate) fn record(
    state:&AppState,route:&Route,accepted:bool,status:u16,prefix_tokens:u64,
) {
    let now=chrono::Utc::now().timestamp();
    state.store.put("provider_wire_feature",&json!({
        "id":feature_id(route),"field":"cache_control",
        "providerId":route.id,"model":route.model,
        "accepted":accepted,"httpStatus":status,
        "observedAt":chrono::Utc::now().to_rfc3339(),
        "expiresAt":if accepted{Value::Null}else{json!(now+DISABLE_SECONDS)},
        "mode":"native_explicit_static_prefix",
    }));
    state.store.audit("native.prompt_cache_hint",json!({
        "providerId":route.id,"model":route.model,
        "accepted":accepted,"status":status,
        "estimatedCacheablePrefixTokens":prefix_tokens,
        "estimatedSavings":false,"rejectedHintRetriedOnlyAfterExplicit400":!accepted,
    }));
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::Config,db::Store,vault::Vault};
    fn fixture()->(AppState,Route,std::path::PathBuf) {
        let dir=std::env::temp_dir().join(format!("dominion-native-cache-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let cfg=Config{port:0,data_dir:dir.clone(),web_dir:dir.join("web"),workspace:dir.clone()};
        let state=AppState::new(cfg.clone(),Store::open(&cfg.db_path()).unwrap(),Vault::open(&cfg.key_path()));
        let provider=json!({"id":"cache-provider","kind":"anthropic","enabled":true,
            "model":"claude-test","baseUrl":"https://api.anthropic.com/v1"});
        state.store.put("provider",&provider);
        state.save_secret("provider:cache-provider", "test-only-cache-key");
        let route=crate::engine::gateway::route_from_provider(&state,&provider,None).unwrap();
        (state,route,dir)
    }
    fn body()->Value {
        json!({"model":"claude-test","system":"stable owner policies ".repeat(400),
            "tools":[{"name":"read","description":"read source","input_schema":{"type":"object"}},
                {"name":"write","description":"write source","input_schema":{"type":"object"}}],
            "messages":[{"role":"user","content":"Unique task 1"}]})
    }
    #[test]
    fn native_static_tools_and_system_cache_but_unique_task_never_does() {
        let (state,route,dir)=fixture();
        let original=body();
        let (first,hint_on,prefix)=hinted(&state,&route,&original);
        assert!(hint_on);
        assert!(prefix>=1024);
        assert_eq!(first["tools"][1]["cache_control"]["type"],"ephemeral");
        assert_eq!(first["system"][0]["cache_control"]["type"],"ephemeral");
        assert_eq!(first["messages"],original["messages"]);
        assert!(original["system"].is_string(),"never mutate canonical body");
        let mut next=original.clone();
        next["messages"][0]["content"]=json!("Unique task 2");
        let (second,_,_)=hinted(&state,&route,&next);
        assert_eq!(first["tools"],second["tools"]);
        assert_eq!(first["system"],second["system"]);
        assert_ne!(first["messages"],second["messages"]);
        record(&state,&route,false,400,prefix);
        let (disabled,hinted_again,_)=hinted(&state,&route,&original);
        assert!(!hinted_again);
        assert_eq!(disabled,original);
        let mut overridden=route.clone();
        overridden.model="different-model".into();
        assert!(hinted(&state,&overridden,&original).1,
            "negative cache capability is route/model scoped");
        crate::test_support::cleanup_dir(dir);
    }
    #[test]
    fn only_explicit_unsupported_hint_permits_safe_compatibility_retry() {
        assert!(unsupported(reqwest::StatusCode::BAD_REQUEST,
            r#"{"error":{"param":"system.0.cache_control","message":"not supported"}}"#));
        assert!(!unsupported(reqwest::StatusCode::BAD_REQUEST,
            r#"{"error":{"param":"tools.0.name","message":"invalid tool"}}"#));
        assert!(!unsupported(reqwest::StatusCode::TOO_MANY_REQUESTS,
            r#"{"error":{"message":"cache_control not supported"}}"#));
        assert!(!unsupported(reqwest::StatusCode::INTERNAL_SERVER_ERROR,
            r#"{"error":{"message":"cache_control not supported"}}"#));
    }
    #[test]
    fn small_uncacheable_prompts_and_owner_disabled_capability_have_no_hint() {
        let (state,route,dir)=fixture();
        let mut small=body();
        small["system"]=json!("short");
        assert!(!hinted(&state,&route,&small).1);
        let mut provider=state.store.get("provider",&route.id).unwrap();
        provider["capabilities"]=json!({"promptCaching":false});
        state.store.put("provider",&provider);
        state.save_secret("provider:cache-provider", "test-only-cache-key");
        assert!(!hinted(&state,&route,&body()).1);
        crate::test_support::cleanup_dir(dir);
    }
}
