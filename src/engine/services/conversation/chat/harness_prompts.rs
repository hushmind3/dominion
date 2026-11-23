//! Sub2-specific, persisted model-facing harness instructions.
//! Runtime permissions and execution admission remain enforced by Rust.
use crate::state::AppState;
use serde_json::{json,Value};

#[derive(Clone,Copy)]
pub(crate) struct PromptSpec {
    pub id:&'static str,pub category:&'static str,pub title:&'static str,
    pub location:&'static str,pub default:&'static str,
    pub required:&'static [&'static str],
}
pub(crate) const SPECS:&[PromptSpec]=&[
    PromptSpec{id:"identity.chat",category:"대화",title:"일반 대화 정체성",location:"chat_context.rs",default:"DOMINION 대화형 AI입니다. 사용자의 질문에 자연스럽게 답하십시오. 이번 대화에는 도구나 외부 실행 권한이 없습니다.",required:&[]} ,
    PromptSpec{id:"identity.web",category:"대화",title:"웹 검색 대화 정체성",location:"chat_context.rs",default:"DOMINION 대화형 AI입니다. 사용자 요청 시 공개 웹 검색과 페이지 읽기만 할 수 있습니다. 파일·셸·MCP·메모리 쓰기·외부 작업 권한은 없습니다.",required:&[]} ,
    PromptSpec{id:"identity.agent",category:"에이전트",title:"에이전트 정체성",location:"chat_context.rs / chat_prompt_sources.rs",default:"DOMINION 지속 에이전트. 정체성과 개인 경험은 에이전트 ID에 속하며 대화 장소가 바뀌어도 유지한다. 실제 권한과 도구는 현재 실행 경계에 따른다.",required:&[]} ,
    PromptSpec{id:"chat.lightweight",category:"대화",title:"짧은 대화 시작",location:"chat_context.rs",default:"DOMINION. 당신은 사용자의 일반 대화를 돕는 AI 어시스턴트입니다. 사용자의 언어로 자연스럽고 정확하게 답하십시오.",required:&[]} ,
    PromptSpec{id:"chat.lightweight_alt",category:"대화",title:"간단 호출의 시작",location:"chat_context.rs",default:"DOMINION. 사용자의 언어로 자연스럽고 간결하게 답하십시오.",required:&[]} ,
    PromptSpec{id:"chat.general",category:"대화",title:"일반 대화 기본",location:"chat_context.rs",default:"당신은 사용자의 일반 대화를 돕는 AI 어시스턴트입니다. 사용자의 언어로 대답하십시오.",required:&[]} ,
    PromptSpec{id:"chat.web_scope",category:"대화",title:"웹 검색 설명",location:"chat_context.rs",default:"이번 턴에는 공개 웹 검색(web_search)과 해당 웹페이지 읽기(http_fetch)만 허용됩니다. 검색 결과의 출처 URL을 명시하십시오. 파일·셸·계정·MCP·자동 실행 권한은 없습니다.",required:&[]} ,
    PromptSpec{id:"chat.no_tools",category:"대화",title:"도구 비활성 안내",location:"chat_context.rs",default:"이번 턴에는 도구가 비활성화되어 있습니다. 외부 작업을 수행했다고 말하지 마십시오.",required:&[]} ,
    PromptSpec{id:"chat.reasoning_ko",category:"대화",title:"한국어 추론 지침",location:"chat_context.rs",default:"Write every internal reasoning / thinking step in Korean (한국어). The final answer must also be in Korean unless the user asks otherwise.",required:&[]} ,
    PromptSpec{id:"chat.lightweight_ko",category:"대화",title:"간단 한국어 추론",location:"chat_context.rs",default:"추론과 최종 답변은 다른 언어 요청이 없으면 한국어로 작성하십시오.",required:&[]} ,
    PromptSpec{id:"agent.general",category:"에이전트",title:"작업 에이전트 역할",location:"chat_context.rs",default:"당신은 Rust 엔진에서 동작하는 에이전트형 AI 작업 공간 DOMINION입니다. 사용자가 다른 언어로 쓰지 않는 한 한국어로 답하십시오.",required:&[]} ,
    PromptSpec{id:"agent.tools",category:"에이전트",title:"도구 사용·결과 보고",location:"chat_context.rs",default:"작업에 필요하면 사용 가능한 도구를 사용하십시오. 도구 결과를 정확히 보고하십시오.",required:&[]} ,
    PromptSpec{id:"agent.plan",category:"실행",title:"계획 실행",location:"turn_body.rs",default:"Internal execution plan. Verify every step with real tools, never claims:\n{plan}",required:&["plan"]} ,
    PromptSpec{id:"agent.decomposition",category:"실행",title:"작업 분해",location:"turn_body.rs",default:"Engine-owned task decomposition. Complete these subtasks in order and verify each with real tools before claiming done:\n{subtasks}",required:&["subtasks"]} ,
    PromptSpec{id:"agent.program_review",category:"검증",title:"검토가 필요한 프로그램",location:"turn_body.rs",default:"프로그램 실행이 needs_review 상태로 종료되었습니다. 이미 일어난 외부 효과는 유지되고, 추가 도구 호출은 자동 실행하지 않습니다. 검증 실패 원인과 확인이 필요한 사항만 보고하세요.",required:&[]} ,
    PromptSpec{id:"agent.procedure_retry",category:"검증",title:"절차 위반 수정",location:"turn_body.rs",default:"방금 실행은 학습된 절차를 위반했습니다. 도구를 다시 호출해 아래 위반을 수정한 뒤 최종 답변을 작성하세요. 위반: {violations}",required:&["violations"]} ,
    PromptSpec{id:"agent.acceptance_retry",category:"검증",title:"검증 실패 후 재시도",location:"turn_body.rs",default:"자동 검증 결과 아직 완료되지 않았습니다:\n- {gaps}\n\n{guidance}\n말로만 완료하지 말고, 실패 원인을 바꾸는 실제 도구를 호출한 뒤 결과를 검증하세요.",required:&["gaps","guidance"]} ,
    PromptSpec{id:"agent.acceptance_complete",category:"검증",title:"검증 완료 후 마무리",location:"turn_body.rs",default:"모든 검증 조건이 충족되었습니다. 추가 도구 호출 없이 최종 답변만 작성하세요.",required:&[]} ,
    PromptSpec{id:"agent.procedure_observation",category:"실행",title:"절차 참고 안내",location:"turn_body.rs",default:"절차 관찰: {error}. 이 전략은 강제가 아니므로 지금 실행을 계속할 수 있습니다. 필요하면 순서를 바꾸세요.",required:&["error"]} ,
    PromptSpec{id:"agent.read_before_write",category:"실행",title:"읽기 후 쓰기 권장",location:"turn_body.rs",default:"도구 선택 알고리즘(read-before-write): 쓰기 전에 workspace_list/workspace_read로 대상 상태를 먼저 확인하는 것이 권장됩니다. 이 힌트는 실행을 막지 않습니다.",required:&[]} ,
    PromptSpec{id:"agent.blocked_repeat",category:"복구",title:"동일 실패 재시도",location:"turn_body.rs",default:"직전 실패와 동일한 호출이 차단되었습니다. 다른 도구/인자로 실행하거나, 원인을 확인하는 probe를 선택하거나, 실제로 막혔다면 그 이유를 최종 답변에 쓰세요.",required:&[]} ,
    PromptSpec{id:"agent.repeat_feedback",category:"복구",title:"도구 반복 알림",location:"turn_body.rs",default:"`{tool}` 호출이 {count}회 반복되었습니다. 같은 결과가 나온다면 다른 접근을 선택하세요.",required:&["tool","count"]} ,
    PromptSpec{id:"agent.adaptation_result",category:"복구",title:"적응 실행의 결과",location:"turn_body.rs",default:"적응 실행: 선택된 개입 `{id}` (tool={tool}) 결과={result}. 근거 제한 계약={certificate} 검사 관측={probes} 가설 판정={hypotheses}. 같은 실패 호출을 반복하지 마세요.",required:&["id","tool","result","certificate","probes","hypotheses"]} ,
    PromptSpec{id:"agent.adaptation_error",category:"복구",title:"적응 실패",location:"turn_body.rs",default:"적응 후보 생성/실행에 실패했습니다: {error}. 모델은 원인 가설과 다른 도구/인자를 직접 선택하세요.",required:&["error"]} ,
    PromptSpec{id:"agent.adaptation_no_progress",category:"복구",title:"진전 없는 적응",location:"turn_body.rs",default:"연속 적응 실행이 진전 없이 실패했습니다. 추가 반복을 중단하고 실제 관측과 막힌 원인을 최종 답변에 쓰세요.",required:&[]} ,
    PromptSpec{id:"compat.tool_header",category:"로컬 모델",title:"텍스트 도구 프로토콜",location:"protocol.rs",default:"You cannot use native function calling. To call a tool, output exactly one line:\n<tool_call>{\"name\":\"tool_name\",\"arguments\":{...}}</tool_call>\nAvailable tools:",required:&[]} ,
    PromptSpec{id:"compat.tool_example",category:"로컬 모델",title:"도구 호출 예시",location:"protocol.rs",default:"Example: <tool_call>{\"name\":\"workspace_read\",\"arguments\":{\"path\":\"a.txt\"}}</tool_call>",required:&[]} ,
];
pub(crate) fn spec(id:&str)->Option<&'static PromptSpec>{
    SPECS.iter().find(|s|s.id==id)
}
fn stored(state:&AppState,id:&str)->Option<Value>{
    spec(id)?;
    state.store.get("harness_prompt_override",id)
}
pub(crate) fn revision(state:&AppState,id:&str)->String{
    stored(state,id).and_then(|v|v["revision"].as_str().map(str::to_string))
        .unwrap_or_else(||"builtin".to_string())
}
pub(crate) fn effective(state:&AppState,id:&str)->String{
    let default=spec(id).map(|s|s.default).unwrap_or("");
    stored(state,id).and_then(|v|v["text"].as_str().map(str::to_string))
        .unwrap_or_else(||default.to_string())
}
pub(crate) fn render(state:&AppState,id:&str,vars:&[(&str,&str)])->String{
    let mut result=effective(state,id);
    for (key,value) in vars {result=result.replace(&format!("{{{key}}}"),value);}
    result
}
pub(crate) fn validate(spec:&PromptSpec,text:&str)->Result<(),String>{
    if text.chars().count()>12_000 {return Err("하네스 지침은 항목당 12,000자 이하여야 합니다.".into());}
    for key in spec.required {
        if !text.contains(&format!("{{{key}}}")) {
            return Err(format!("실제 실행 데이터를 넣을 {{{key}}} 자리표시자가 필요합니다."));
        }
    }
    Ok(())
}
pub(crate) fn catalog(state:&AppState)->Value{
    json!({"items":SPECS.iter().map(|item|{
        let row=stored(state,item.id);
        json!({"id":item.id,"title":item.title,"category":item.category,"location":item.location,
            "default":item.default,
            "text":row.as_ref().and_then(|v|v["text"].as_str()).unwrap_or(item.default),
            "customized":row.is_some(),
            "revision":row.as_ref().and_then(|v|v["revision"].as_str()).unwrap_or("builtin"),
            "required":item.required,
            "updatedAt":row.as_ref().and_then(|v|v.get("updatedAt")).cloned().unwrap_or(Value::Null)})
    }).collect::<Vec<_>>(),"scope":"sub2-harness","runtimeGuardsEditable":false})
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn registry_has_unique_valid_templates(){
        let mut seen=std::collections::HashSet::new();
        for item in SPECS {assert!(seen.insert(item.id));validate(item,item.default).unwrap();}
    }
    #[test]
    fn required_evidence_slots_cannot_be_silently_removed(){
        assert!(validate(spec("agent.plan").unwrap(),"No plan evidence").is_err());
        assert!(validate(spec("agent.plan").unwrap(),"New strategy: {plan}").is_ok());
    }
}
