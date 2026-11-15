//! Opt-in native agent quality-floor routing. Never infer that a model is
//! inexpensive, capable, or free from its name. Provider prices are owner-
//! configured KRW per million tokens; quality is explicitly declared.
use crate::{engine::{gateway, gateway::Route},state::AppState};
use serde_json::{json,Value};
use sha2::{Digest,Sha256};

#[derive(Clone,Copy,Debug)]
struct Prices {input:f64,output:f64,cached:Option<f64>,write:Option<f64>}
fn nonnegative(value:&Value)->Option<f64>{
    value.as_f64().filter(|v|v.is_finite()&&*v>=0.0&&*v<=1_000_000_000.0)
}
fn prices(value:&Value)->Option<Prices>{
    Some(Prices {
        input:nonnegative(&value["input"])?,
        output:nonnegative(&value["output"])?,
        cached:nonnegative(&value["cachedInput"]),
        write:nonnegative(&value["cacheWrite"]),
    })
}
fn estimate(prices:Prices,input:u64,output:u64)->f64 {
    (prices.input*input as f64+prices.output*output as f64)/1_000_000.0
}
/// A provider profile is immutable for a native job once selected.
pub(crate) fn binding(route:&Route)->String {
    let value=json!({"providerId":route.id,"model":route.model,
        "kind":route.kind,"baseUrl":route.base_url,"accountId":route.account_id});
    format!("{:x}",Sha256::digest(value.to_string().as_bytes()))
}
fn route_record(route:&Route,decision:Value,profile:&Value)->Value {
    json!({
        "providerId":route.id,"model":route.model,"binding":binding(route),
        "kind":route.kind,"selection":decision,
        "pricesKRWPerMillion":profile["pricesKRWPerMillion"],
        "currency":"KRW","priceSource":"owner_configured",
        "pinnedAt":chrono::Utc::now().to_rfc3339(),
    })
}
fn valid_quality(value:&Value)->Option<f64>{
    value.as_f64().filter(|v|v.is_finite()&& (0.0..=1.0).contains(v))
}
/// A chosen provider, agent role or model is an explicit user instruction,
/// never silently replaced. Only owner-enabled quality_cost + automatic model
/// selection can evaluate alternative native-capable providers.
pub(crate) fn choose(
    state:&AppState,requested_provider:&str,requested_model:Option<&str>,
    policy:&Value,task:&str,owner_instructions:&str,
)->Result<(Route,Value),String>{
    let explicit=!requested_provider.is_empty() || requested_model.is_some();
    let route_default=||gateway::resolve(state,requested_provider,requested_model);
    if policy["mode"]!="quality_cost" || explicit {
        let route=route_default()?;
        let profile=state.store.get("provider",&route.id)
            .unwrap_or(Value::Null)["nativeRouting"].clone();
        let reason=if explicit{"explicit_provider_or_model"}else{"default_policy"};
        let record=route_record(&route,json!({"mode":"pinned","reason":reason}),&profile);
        return Ok((route,record));
    }
    let min_quality=policy["minQuality"].as_f64()
        .filter(|x|x.is_finite()&&(0.0..=1.0).contains(x))
        .unwrap_or(0.80);
    let max_drop=policy["maxQualityDrop"].as_f64()
        .filter(|x|x.is_finite()&&(0.0..=0.50).contains(x))
        .unwrap_or(0.10);
    // Conservative projection. Routing itself makes no additional paid call.
    // Every candidate is compared using the SAME input/output work shape.
    let input=crate::engine::chat::estimate_text_tokens(&format!(
        "{task}\n{owner_instructions}")).saturating_add(1024).max(1024);
    let output=2048u64;
    let mut candidates=Vec::<(Route,f64,f64,Value)>::new();
    for profile in state.store.all("provider") {
        let Some(q)=valid_quality(&profile["nativeRouting"]["quality"]) else{continue};
        if q<min_quality {continue;}
        let Some(price)=prices(&profile["nativeRouting"]["pricesKRWPerMillion"]) else{continue};
        let Some(id)=profile["id"].as_str() else{continue};
        let Ok(route)=gateway::route_from_provider(state,&profile,None) else{continue};
        // Do not substitute CLI compatibility/manual tools for the native SDK
        // and never equate OAuth subscriptions with metered API prices.
        if route.kind=="cli"||route.kind=="codex"||route.tool_mode=="compat"{
            continue;
        }
        if !route.local && route.api_key.is_empty(){continue;}
        if route.context_tokens.is_some_and(|cap|cap<input.saturating_add(output)){continue;}
        let _=id;
        let estimated=estimate(price,input,output);
        candidates.push((route,q,estimated,profile["nativeRouting"].clone()));
    }
    if candidates.is_empty() {
        let route=route_default()?;
        let profile=state.store.get("provider",&route.id)
            .unwrap_or(Value::Null)["nativeRouting"].clone();
        return Ok((route.clone(),route_record(&route,json!({
            "mode":"pinned","requestedMode":"quality_cost",
            "reason":"no_qualified_priced_native_provider",
            "minimumQuality":min_quality,"estimatedSavings":false,
        }),&profile)));
    }
    let best_quality=candidates.iter().map(|(_,q,_,_)|*q).fold(0.0,f64::max);
    let floor=min_quality.max(best_quality-max_drop);
    candidates.retain(|(_,q,_,_)|*q+1e-10>=floor);
    candidates.sort_by(|left,right|left.2.total_cmp(&right.2)
        .then_with(||right.1.total_cmp(&left.1))
        .then_with(||left.0.id.cmp(&right.0.id))
        .then_with(||left.0.model.cmp(&right.0.model)));
    let count=candidates.len();
    let (route,q,cost,profile)=candidates.remove(0);
    let decision=json!({
        "mode":"quality_cost","reason":"quality_floor_then_estimated_price",
        "minimumQuality":min_quality,"maximumQualityDrop":max_drop,
        "qualityFloorApplied":floor,"bestDeclaredQuality":best_quality,
        "selectedQuality":q,"qualifiedCandidates":count,
        "estimateInputTokens":input,"estimateOutputTokens":output,
        "estimatedPerTurnKRW":cost,"estimatedSavings":false,
        "qualityIsOwnerDeclared":true,"pricesAreOwnerConfigured":true,
    });
    let record=route_record(&route,decision,&profile);
    Ok((route,record))
}
/// Compute owner-configured price × provider-reported input/output/cache usage.
/// Null means unknown: absent cache-write or cache-read tariff, malformed
/// values, or missing usage must NEVER be treated as zero-cost.
pub(crate) fn observed_cost(
    route_record:&Value,input:Option<u64>,output:Option<u64>,
    cache_read:Option<u64>,cache_write:Option<u64>,
)->Option<f64>{
    let input=input?;let output=output?;
    let p=prices(&route_record["pricesKRWPerMillion"])?;
    let read=cache_read.unwrap_or(0);
    let write=cache_write.unwrap_or(0);
    if read.saturating_add(write)>input {return None;}
    let cached_price=if read>0 {p.cached?}else{0.0};
    let write_price=if write>0 {p.write?}else{0.0};
    let base=input.saturating_sub(read).saturating_sub(write);
    Some((p.input*base as f64+cached_price*read as f64+
        write_price*write as f64+p.output*output as f64)/1_000_000.0)
}
pub(crate) fn validate_profile(value:&Value)->Result<(),String>{
    if value.is_null(){return Ok(());}
    if !value.is_object(){return Err("nativeRouting must be an object".into());}
    if valid_quality(&value["quality"]).is_none(){
        return Err("nativeRouting.quality must be 0.0 through 1.0".into());
    }
    if prices(&value["pricesKRWPerMillion"]).is_none(){
        return Err("nativeRouting prices require nonnegative finite input/output KRW per million".into());
    }
    for field in ["cachedInput","cacheWrite"] {
        if !value["pricesKRWPerMillion"][field].is_null() &&
            nonnegative(&value["pricesKRWPerMillion"][field]).is_none(){
            return Err(format!("nativeRouting {field} price must be nonnegative KRW per million"));
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::Config,db::Store,vault::Vault};
    fn state()->AppState {
        let dir=std::env::temp_dir().join(format!("dominion-route-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config=Config{port:0,data_dir:dir.clone(),web_dir:dir.join("web"),workspace:dir};
        AppState::new(config.clone(),Store::open(&config.db_path()).unwrap(),
            Vault::open(&config.key_path()))
    }
    fn add(state:&AppState,id:&str,quality:Option<f64>,input:Option<f64>,output:f64){
        let mut p=json!({"id":id,"model":format!("{id}-model"),
            "kind":"openai","enabled":true,"toolMode":"native","local":true,
            "baseUrl":format!("http://127.0.0.1:12{}/v1",id.len()*99)});
        if let (Some(q),Some(i))=(quality,input){
            p["nativeRouting"]=json!({"quality":q,
                "pricesKRWPerMillion":{"input":i,"output":output,
                    "cachedInput":i/2.0,"cacheWrite":i*1.25}});
        }
        state.store.put("provider",&p);
    }
    #[test]
    fn native_quality_floor_prevents_cheap_unqualified_model_and_honors_explicit_owner() {
        let s=state();
        add(&s,"weak",Some(0.30),Some(100.0),100.0);
        add(&s,"good",Some(0.90),Some(1800.0),12000.0);
        add(&s,"best",Some(0.98),Some(2700.0),15000.0);
        let policy=json!({"mode":"quality_cost","minQuality":0.8,"maxQualityDrop":0.1});
        let (selected,receipt)=choose(&s,"",None,&policy,"modify code","cargo test").unwrap();
        assert_eq!(selected.id,"good");
        assert_eq!(receipt["selection"]["mode"],"quality_cost");
        assert_eq!(receipt["selection"]["qualifiedCandidates"],2);
        let (explicit,pinned)=choose(&s,"weak",None,&policy,"modify code","").unwrap();
        assert_eq!(explicit.id,"weak");
        assert_eq!(pinned["selection"]["reason"],"explicit_provider_or_model");
    }
    #[test]
    fn native_router_never_guesses_missing_prices_or_quality_and_is_deterministic() {
        let s=state();
        add(&s,"default",None,None,0.0);
        add(&s,"unknown",Some(0.95),None,0.0);
        let (fallback,metadata)=choose(&s,"",None,
            &json!({"mode":"quality_cost"}),"inspect","").unwrap();
        assert_eq!(metadata["selection"]["reason"],"no_qualified_priced_native_provider");
        assert_eq!(fallback.id,"default");
        add(&s,"priced_a",Some(0.92),Some(1000.0),2000.0);
        add(&s,"priced_b",Some(0.92),Some(1000.0),2000.0);
        let (one,_)=choose(&s,"",None,&json!({"mode":"quality_cost"}),"inspect","").unwrap();
        let (two,_)=choose(&s,"",None,&json!({"mode":"quality_cost"}),"inspect","").unwrap();
        assert_eq!(one.id,"priced_a");
        assert_eq!(one.id,two.id);
    }
    #[test]
    fn native_price_is_measured_cache_aware_and_unpriced_usage_is_unknown() {
        let record=json!({"pricesKRWPerMillion":{
            "input":1000.0,"cachedInput":100.0,"cacheWrite":1500.0,"output":10000.0}});
        let cost=observed_cost(&record,Some(10_000),Some(2_000),
            Some(6000),Some(2000)).unwrap();
        let expected=(2000.0*1000.0+6000.0*100.0+
            2000.0*1500.0+2000.0*10000.0)/1_000_000.0;
        assert!((cost-expected).abs()<1e-6);
        assert_eq!(observed_cost(&record,None,Some(1),None,None),None);
        assert_eq!(observed_cost(&record,Some(1),Some(1),Some(2),None),None);
        let incomplete=json!({"pricesKRWPerMillion":{"input":1000,"output":10000}});
        assert_eq!(observed_cost(&incomplete,Some(10),Some(2),Some(5),None),None);
        assert!(validate_profile(&json!({"quality":0.8,
            "pricesKRWPerMillion":{"input":1.0,"output":2.0}})).is_ok());
        assert!(validate_profile(&json!({"quality":1.2,
            "pricesKRWPerMillion":{"input":1.0,"output":2.0}})).is_err());
    }
}
