//! Conservative owner-configured monetary estimates: no invented provider
//! tariff and no assumed cache discount. Missing rate cards remain unpriced.
//! Real billing/GPU/machine overhead require a separate measured receipt.
use serde_json::{json,Value};

fn rate(pricing:&Value, key:&str)->Option<f64>{
    pricing[key].as_f64().filter(|n|n.is_finite() && *n>=0.0 && *n<=100_000.0)
}

/// All configured rates are USD per million tokens. The result is an
/// *estimate from a user-entered tariff*, not a provider invoice.
pub fn estimate(
    provider:&Value,
    prompt_tokens:u64,
    output_tokens:u64,
    cached_prompt_tokens:Option<u64>,
    estimated_usage:bool,
)->Value{
    let pricing=&provider["pricing"];
    let Some(input)=rate(pricing,"inputUsdPerMillion") else{return Value::Null;};
    let Some(output)=rate(pricing,"outputUsdPerMillion") else{return Value::Null;};
    if prompt_tokens==0 && output_tokens==0{return Value::Null;}
    let cached=cached_prompt_tokens.unwrap_or(0).min(prompt_tokens);
    let cache_rate=if cached>0{
        let Some(rate)=rate(pricing,"cachedInputUsdPerMillion") else{
            // Counting cached tokens at full input rate would misstate cost.
            return Value::Null;
        };
        rate
    }else{input};
    let regular=prompt_tokens.saturating_sub(cached);
    let amount=(regular as f64*input+cached as f64*cache_rate
        +output_tokens as f64*output)/1_000_000.0;
    if !amount.is_finite(){return Value::Null;}
    json!({
        "amountUsd":amount,
        "currency":"USD",
        "source":"owner_configured_rate_card",
        "precision":"estimate",
        "tokenUsageEstimated":estimated_usage,
        "cacheMeasured":cached_prompt_tokens.is_some() && !estimated_usage,
        "cachedInputTokensPriced":cached,
        "unpriced":["cache_creation_unreported","gpu","hosting","storage","provider_invoice_adjustments"]
    })
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn exactly_priced_cached_input_is_not_charged_twice(){
        let p=json!({"pricing":{
            "inputUsdPerMillion":2.0,"cachedInputUsdPerMillion":0.5,
            "outputUsdPerMillion":8.0}});
        let c=estimate(&p,1000,200,Some(800),false);
        // 200*2 + 800*0.5 + 200*8 = 2400 / 1e6 USD
        assert!((c["amountUsd"].as_f64().unwrap()-0.0024).abs()<1e-10);
        assert_eq!(c["cachedInputTokensPriced"],800);
        assert_eq!(c["precision"],"estimate");
        assert!(estimate(&json!({}),1000,200,None,false).is_null());
        let incomplete=json!({"pricing":{
            "inputUsdPerMillion":2.0,"outputUsdPerMillion":8.0}});
        assert!(estimate(&incomplete,1000,200,Some(200),false).is_null());
        assert!(estimate(&json!({"pricing":{
            "inputUsdPerMillion":-1.0,"outputUsdPerMillion":8.0
        }}),1000,200,None,false).is_null());
    }
}
