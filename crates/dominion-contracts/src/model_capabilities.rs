//! Endpoint/model-scoped capability negotiation before model budget admission.
//! Unknown properties retain legacy behavior; explicit false is authoritative.
//! A model-name heuristic never overrides a declared endpoint limitation.
use serde_json::Value;

#[derive(Clone,Debug,Default,PartialEq,Eq)]
pub struct ModelCapabilities {
    pub text:Option<bool>,
    pub native_tools:Option<bool>,
    pub text_tools:Option<bool>,
    pub vision:Option<bool>,
    pub reasoning:Option<bool>,
    pub streaming:Option<bool>,
    pub parallel_tools:Option<bool>,
    /// Declared, endpoint-specific input capacity. Never inferred from model name.
    pub context_window:Option<u64>,
    pub max_output_tokens:Option<u64>,
    pub prompt_cache:Option<bool>,
}
impl ModelCapabilities {
    /// Provider-wide defaults plus exact model-ID overrides. The caller may
    /// choose the same model through 2 different endpoints with different limits.
    pub fn for_route(provider:&Value,model:&str)->Self{
        let generic=&provider["capabilities"];
        let specific=&provider["modelCapabilities"][model];
        // Endpoint-specific declarations outrank model-wide defaults.
        // Each provider record may choose an explicit endpointId; otherwise
        // the normalized baseUrl uniquely identifies its network endpoint.
        let endpoint=provider["endpointId"].as_str()
            .filter(|id|!id.is_empty())
            .or_else(||provider["baseUrl"].as_str())
            .unwrap_or("");
        let endpoint_specific=&provider["endpointModelCapabilities"][endpoint][model];
        let endpoint_default=&provider["endpointCapabilities"][endpoint];
        let layers=[endpoint_specific,endpoint_default,specific,generic];
        let read=|keys:&[&str]|->Option<bool>{
            for from in layers {
                for key in keys {
                    if let Some(value)=from[*key].as_bool(){return Some(value);}
                }
            }
            None
        };
        let read_number=|keys:&[&str]|->Option<u64>{
            for from in layers {
                for key in keys {
                    if let Some(value)=from[*key].as_u64(){
                        return (value>0).then_some(value);
                    }
                }
            }
            None
        };
        Self{
            context_window:read_number(&["contextWindow","contextTokens"]),
            max_output_tokens:read_number(&["maxOutputTokens"]),
            prompt_cache:read(&["promptCache","promptCaching"]),
            text:read(&["text"]),
            native_tools:read(&["nativeToolCalls","toolCall","toolCalling"]),
            text_tools:read(&["textToolCalls","compatToolCalls"]),
            vision:read(&["vision","images"]),
            reasoning:read(&["reasoningEnabled","thinking"]),
            streaming:read(&["streaming"]),
            parallel_tools:read(&["parallelToolCalls","parallelTools"]),
        }
    }

    /// Do not send an unsupported native tool schema. Text-protocol tool calls
    /// need an explicit opt-in when native calls were explicitly denied.
    /// An unknown capability does not silently change existing routes.
    pub fn tool_mode(&self,configured:&str,requires_tools:bool)->Result<String,String>{
        // The selected route must actually be able to answer text turns.
        // An explicit negative capability cannot be bypassed by picking an
        // alternate tool protocol or by removing the tool catalog.
        if self.text==Some(false){
            return Err("선택 모델 엔드포인트는 텍스트 응답을 지원하지 않습니다.".into());
        }
        if !requires_tools{return Ok(configured.to_owned());}
        if matches!(configured,"native"|"auto") && self.native_tools==Some(false){
            return if self.text_tools==Some(true){
                Ok("compat".into())
            }else{
                Err("선택 모델은 네이티브 도구 호출을 지원하지 않으며 텍스트 도구 호환도 선언되지 않았습니다.".into())
            };
        }
        // A manually configured compat route is not proof that text-tool
        // decoding is supported. Honor both explicit denials and the absence
        // of an opt-in when native calls have already been denied.
        if configured=="compat" && (self.text_tools==Some(false)
            || (self.native_tools==Some(false)&&self.text_tools!=Some(true))) {
            return Err("선택 모델은 텍스트 방식 도구 호출을 지원하지 않습니다.".into());
        }
        Ok(configured.to_owned())
    }

    pub fn require_vision(&self,images:bool)->Result<(),String>{
        if images&&self.vision!=Some(true){
            return Err("이 모델 엔드포인트가 이미지 입력을 지원한다고 선언하지 않았습니다.".into());
        }
        Ok(())
    }
    pub fn streaming(&self,stream_required:bool)->Result<(),String>{
        if stream_required&&self.streaming==Some(false){
            return Err("선택 모델 엔드포인트는 스트리밍 생성을 지원하지 않습니다.".into());
        }
        Ok(())
    }
}
pub fn negotiate_tool_mode(provider:&Value,model:&str,mode:&str,tools:bool)->Result<String,String>{
    ModelCapabilities::for_route(provider,model).tool_mode(mode,tools)
}
pub fn require_vision(provider:&Value,model:&str,images:bool)->Result<(),String>{
    ModelCapabilities::for_route(provider,model).require_vision(images)
}
pub fn require_streaming(provider:&Value,model:&str,required:bool)->Result<(),String>{
    ModelCapabilities::for_route(provider,model).streaming(required)
}

#[cfg(test)]
mod tests{
    use super::*;
    use serde_json::json;

    #[test]
    fn exact_model_and_endpoint_capabilities_win_without_guessing(){
        let provider=json!({"capabilities":{"toolCall":true,"streaming":true},
            "modelCapabilities":{"model-1":{"toolCall":false,"textToolCalls":true},
                "model-2":{"streaming":false,"toolCall":false}}});
        assert_eq!(negotiate_tool_mode(&provider,"model-1","native",true).unwrap(),"compat");
        assert_eq!(negotiate_tool_mode(&provider,"model-1","native",false).unwrap(),"native");
        assert!(negotiate_tool_mode(&provider,"model-2","native",true).is_err());
        assert!(require_streaming(&provider,"model-2",true).is_err());
        assert!(require_streaming(&provider,"model-1",true).is_ok());
        assert_eq!(negotiate_tool_mode(&provider,"model-3","native",true).unwrap(),"native");
        assert!(require_vision(&provider,"model-1",true).is_err());
        assert!(require_vision(&provider,"model-1",false).is_ok());
        let images=json!({"capabilities":{"vision":true},
            "modelCapabilities":{"text-only":{"vision":false}}});
        assert!(require_vision(&images,"vision-enabled",true).is_ok());
        assert!(require_vision(&images,"text-only",true).is_err());
    }
    #[test]
    fn explicit_text_or_native_denials_survive_manual_route_and_failover(){
        // Two endpoints may expose one model with different decoding support.
        let native_denied=json!({"capabilities":{"text":true,"toolCall":false}});
        assert!(negotiate_tool_mode(&native_denied,"same-model","compat",true).is_err());
        assert!(negotiate_tool_mode(&native_denied,"same-model","auto",true).is_err());
        assert!(negotiate_tool_mode(&native_denied,"same-model","compat",false).is_ok());
        let approved=json!({"capabilities":{"text":true,"toolCall":false,
            "textToolCalls":true}});
        assert_eq!(negotiate_tool_mode(&approved,"same-model","compat",true).unwrap(),"compat");
        assert_eq!(negotiate_tool_mode(&approved,"same-model","auto",true).unwrap(),"compat");
        let no_text=json!({"capabilities":{"text":false,"toolCall":true,
            "textToolCalls":true}});
        for (mode,tools) in [("native",true),("compat",true),("auto",false)] {
            assert!(negotiate_tool_mode(&no_text,"same-model",mode,tools).is_err());
        }
        // A model override is authoritative for only that endpoint's route.
        let per_model=json!({"capabilities":{"text":true,"toolCall":true},
            "modelCapabilities":{"same-model":{"text":false}}});
        assert!(negotiate_tool_mode(&per_model,"same-model","native",true).is_err());
        assert!(negotiate_tool_mode(&per_model,"other-model","native",true).is_ok());
    }
    #[test]
    fn endpoint_model_and_numeric_profiles_win_before_gateway_budget_admission(){
        let common=json!({
            "capabilities":{"text":true,"toolCall":true,
                "contextTokens":32768,"maxOutputTokens":4096,"promptCache":true},
            "modelCapabilities":{"shared":{"contextTokens":16384,"vision":true}},
            "endpointCapabilities":{
                "local-endpoint":{"contextTokens":4096,"toolCall":false,
                    "textToolCalls":true,"promptCache":false},
                "remote-endpoint":{"contextTokens":131072,"maxOutputTokens":8192}
            },
            "endpointModelCapabilities":{"remote-endpoint":{
                "shared":{"vision":false,"maxOutputTokens":2048}}}
        });
        let mut local=common.clone();
        local["endpointId"]=json!("local-endpoint");
        let local_caps=ModelCapabilities::for_route(&local,"shared");
        assert_eq!(local_caps.context_window,Some(4096));
        assert_eq!(local_caps.max_output_tokens,Some(4096));
        assert_eq!(local_caps.prompt_cache,Some(false));
        assert_eq!(local_caps.tool_mode("native",true).unwrap(),"compat");
        assert_eq!(local_caps.vision,Some(true));
        let mut remote=common.clone();
        remote["endpointId"]=json!("remote-endpoint");
        let remote_caps=ModelCapabilities::for_route(&remote,"shared");
        assert_eq!(remote_caps.context_window,Some(131072));
        assert_eq!(remote_caps.max_output_tokens,Some(2048));
        assert_eq!(remote_caps.prompt_cache,Some(true));
        assert_eq!(remote_caps.vision,Some(false));
        assert_eq!(remote_caps.native_tools,Some(true));
        let other=ModelCapabilities::for_route(&remote,"other");
        assert_eq!(other.max_output_tokens,Some(8192));
        assert_eq!(other.vision,None);
    }
    #[test]
    fn disabled_compat_mode_cannot_be_used_to_hide_unsupported_tools(){
        let provider=json!({"capabilities":{"toolCall":false,"textToolCalls":false}});
        assert!(negotiate_tool_mode(&provider,"m","auto",true).is_err());
        assert!(negotiate_tool_mode(&provider,"m","compat",true).is_err());
        assert!(negotiate_tool_mode(&provider,"m","auto",false).is_ok());
        assert!(ModelCapabilities::for_route(&json!({}),"m").streaming(true).is_ok());
    }
}
