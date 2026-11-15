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
    pub output_krw_per_million: f64,
}
impl ModelConfig {
    pub fn basic(endpoint: String, name: String, api_key: Option<String>) -> Self {
        Self {endpoint, name, api_key,
            input_krw_per_million:0.0,output_krw_per_million:0.0}
    }
}

#[derive(Debug, Clone)]
pub struct Reply {
    pub raw: Value,
    pub text: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cached_tokens: u64,
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
                || item.input_krw_per_million<0.0 || item.output_krw_per_million<0.0 {
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
        let input_tokens=raw["usage"]["prompt_tokens"].as_u64().unwrap_or(0);
        let output_tokens=raw["usage"]["completion_tokens"].as_u64().unwrap_or(0);
        let cached_tokens=raw.pointer("/usage/prompt_tokens_details/cached_tokens")
            .and_then(Value::as_u64).unwrap_or(0).min(input_tokens);
        let cost_krw=(
            (input_tokens as f64)*provider.input_krw_per_million
           +(output_tokens as f64)*provider.output_krw_per_million
        )/1_000_000.0;
        Ok(Reply {
            text:message["content"].as_str().unwrap_or_default().into(),
            raw,input_tokens,output_tokens,cached_tokens,cost_krw,model:provider.name.clone()
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
                    Err(_) if cheap.endpoint==self.strong.endpoint && cheap.name==self.strong.name=>{
                        return self.request(&self.strong,messages,tools).await;
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

#[cfg(test)]
mod tests {
    use super::*;
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
