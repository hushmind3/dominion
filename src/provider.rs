//! Native provider adapter used by the Rust execution loop, groups and video.
//! No provider SDK or external agent runtime controls task execution.
use reqwest::Client;
use serde_json::{json, Value};
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct ModelConfig {
    pub endpoint: String,
    pub name: String,
    pub api_key: Option<String>,
    /// Configured price in KRW per million tokens, not an asserted provider rate.
    pub input_krw_per_million: f64,
    pub cached_input_krw_per_million: f64,
    pub output_krw_per_million: f64,
}
impl ModelConfig {
    pub fn basic(endpoint: String, name: String, api_key: Option<String>) -> Self {
        Self {endpoint, name, api_key,
            input_krw_per_million:0.0,cached_input_krw_per_million:0.0,
            output_krw_per_million:0.0}
    }
}

#[derive(Debug, Clone)]
pub struct Reply {
    pub raw: Value,
    pub text: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cached_tokens: u64,
    /// True when the provider omitted token usage and a conservative size estimate was used.
    pub usage_estimated: bool,
    pub cost_krw: f64,
    pub model: String,
}

#[derive(Clone)]
pub struct ModelClient {
    http: Client,
    strong: ModelConfig,
    economy: Option<ModelConfig>,
}
impl ModelClient {
    pub fn new(strong: ModelConfig, economy: Option<ModelConfig>) -> Result<Self, String> {
        for item in std::iter::once(&strong).chain(economy.iter()) {
            let url=reqwest::Url::parse(&item.endpoint)
                .map_err(|error|format!("invalid model endpoint: {error}"))?;
            if !matches!(url.scheme(),"http"|"https") || !url.username().is_empty()
                || url.password().is_some() || url.fragment().is_some() {
                return Err("model endpoint must be HTTP(S) without embedded credentials".into());
            }
            if item.name.trim().is_empty() || item.name.len()>256 {
                return Err("model name must be non-empty and bounded".into());
            }
            if !item.input_krw_per_million.is_finite() || !item.output_krw_per_million.is_finite()
                || !item.cached_input_krw_per_million.is_finite()
                || item.input_krw_per_million<0.0 || item.output_krw_per_million<0.0
                || item.cached_input_krw_per_million<0.0 {
                return Err("model KRW token rates must be finite and nonnegative".into());
            }
        }
        Ok(Self {
            http: Client::builder().timeout(Duration::from_secs(90))
                .build().map_err(|error|error.to_string())?,
            strong,economy,
        })
    }

    async fn request(&self, provider: &ModelConfig, messages: &[Value], tools: &[Value])
        -> Result<Reply,String> {
        let mut payload=json!({"model":provider.name,"messages":messages,"stream":false});
        if !tools.is_empty() {
            payload["tools"]=json!(tools);
            payload["tool_choice"]=json!("auto");
        }
        let input_chars=messages.iter().map(|message|message.to_string().chars().count())
            .sum::<usize>()+tools.iter().map(|tool|tool.to_string().chars().count()).sum::<usize>();
        let mut req=self.http.post(&provider.endpoint).json(&payload);
        if let Some(token)=&provider.api_key {req=req.bearer_auth(token);}
        let response=req.send().await.map_err(|error|format!("model transport: {error}"))?;
        let status=response.status();
        if !status.is_success(){
            let body=response.text().await.unwrap_or_default();
            return Err(format!("model HTTP {status}: {}",body.chars().take(400).collect::<String>()));
        }
        let raw=response.json::<Value>().await.map_err(|error|format!("model JSON: {error}"))?;
        let message=&raw["choices"][0]["message"];
        if !message.is_object() {return Err("model response missing assistant message".into());}
        let usage_estimated=raw["usage"]["prompt_tokens"].as_u64().is_none()
            || raw["usage"]["completion_tokens"].as_u64().is_none();
        let input_tokens=raw["usage"]["prompt_tokens"].as_u64()
            .unwrap_or_else(||(input_chars as u64).saturating_add(3)/4);
        let output_tokens=raw["usage"]["completion_tokens"].as_u64()
            .unwrap_or_else(||(message.to_string().chars().count() as u64).saturating_add(3)/4);
        let cached_tokens=raw.pointer("/usage/prompt_tokens_details/cached_tokens")
            .and_then(Value::as_u64).unwrap_or(0).min(input_tokens);
        let cost_krw=calculate_cost(
            provider, input_tokens, cached_tokens, output_tokens
        );
        Ok(Reply {
            text:message["content"].as_str().unwrap_or_default().into(),
            raw,input_tokens,output_tokens,cached_tokens,usage_estimated,cost_krw,
            model:provider.name.clone()
        })
    }

    /// Select economy only when explicitly requested, with a single bounded
    /// failover to the strong model on error. No automatic paid reruns on the
    /// normal strong path.
    pub async fn complete(&self,messages:&[Value],tools:&[Value],economy:bool)
        -> Result<Reply,String> {
        if economy {
            if let Some(cheap)=&self.economy {
                match self.request(cheap,messages,tools).await {
                    Ok(reply)=>return Ok(reply),
                    Err(error) if cheap.endpoint==self.strong.endpoint && cheap.name==self.strong.name=>{
                        return Err(error);
                    },
                    Err(_)=>{},
                }
            }
        }
        self.request(&self.strong,messages,tools).await
    }

    pub async fn complete_raw(&self,messages:&[Value],tools:&[Value])
        -> Result<Value,String> {
        Ok(self.complete(messages,tools,false).await?.raw)
    }
}

/// Meter cached and uncached prompt tokens separately. Cached tokens form a
/// subset of input tokens, not an extra billable quantity.
fn calculate_cost(provider:&ModelConfig,input:u64,cached:u64,output:u64)->f64{
    let cached=cached.min(input);
    let uncached=input.saturating_sub(cached);
    (uncached as f64*provider.input_krw_per_million
        +cached as f64*provider.cached_input_krw_per_million
        +output as f64*provider.output_krw_per_million)/1_000_000.0
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cache_pricing_never_double_charges_the_input_tokens() {
        let mut cfg=ModelConfig::basic("http://localhost:1234/v1/chat/completions".into(),
            "test".into(),None);
        cfg.input_krw_per_million=2000.0;
        cfg.cached_input_krw_per_million=300.0;
        cfg.output_krw_per_million=5000.0;
        // 800 uncached + 200 cached + 100 generated
        let expected=(800.0*2000.0+200.0*300.0+100.0*5000.0)/1_000_000.0;
        assert!((calculate_cost(&cfg,1000,200,100)-expected).abs()<0.000001);
        assert_eq!(calculate_cost(&cfg,100,200,0),0.03);
    }
    #[test]
        fn rejects_non_http_or_invalid_rates() {
        assert!(ModelClient::new(ModelConfig::basic(
            "file:///local".into(),"test".into(),None),None).is_err());
        let mut provider=ModelConfig::basic(
            "http://127.0.0.1:11434/v1/chat/completions".into(),"local".into(),None);
        provider.input_krw_per_million=f64::NAN;
        assert!(ModelClient::new(provider,None).is_err());
    }
}
