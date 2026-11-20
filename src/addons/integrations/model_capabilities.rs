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
}
impl ModelCapabilities {
    /// Provider-wide defaults plus exact model-ID overrides. The caller may
    /// choose the same model through 2 different endpoints with different limits.
    pub fn for_route(provider:&Value,model:&str)->Self{
        let generic=&provider["capabilities"];
        let specific=&provider["modelCapabilities"][model];
        let read=|keys:&[&str]|->Option<bool>{
            for from in [specific,generic] {
                for key in keys {
                    if let Some(value)=from[*key].as_bool(){return Some(value);}
                }
            }
            None
        };
        Self{
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
        if !requires_tools{return Ok(configured.to_owned());}
        if matches!(configured,"native"|"auto") && self.native_tools==Some(false){
            return if self.text_tools==Some(true){
                Ok("compat".into())
            }else{
                Err("선택 모델은 네이티브 도구 호출을 지원하지 않으며 텍스트 도구 호환도 선언되지 않았습니다.".into())
            };
        }
        if configured=="compat"&&self.text_tools==Some(false){
            return Err("선택 모델은 텍스트 방식 도구 호출을 지원하지 않습니다.".into());
        }
        Ok(configured.to_owned())
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
