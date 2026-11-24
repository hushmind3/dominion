//! Compile and exercise the public SDK's decision and runtime exports without
//! linking the product, SQLite, HTTP server or provider implementations.
use dominion_agent_sdk::{algorithms,runtime};
use serde_json::json;

#[test]
fn external_sdk_can_plan_select_tools_and_evaluate_observations(){
    let plan=json!({"steps":[{"id":"read","profile":"inspection","prompt":"inspect","agentId":"a"}]});
    assert_eq!(algorithms::team::validate(&plan,&[json!({"id":"a"})]).unwrap().len(),1);
    let authorized=vec![json!({"function":{"name":"workspace_read"}})];
    let selected=algorithms::tool_selection::select_room_tools(authorized,"파일 읽어줘",&Default::default(),true);
    assert_eq!(selected[0]["function"]["name"],"workspace_read");
    let result=runtime::predicate::predicate_verdict(&json!({"type":"equals","from":"read","path":"/ok","value":true}),&json!({"read":{}})).unwrap();
    assert_eq!(result,runtime::predicate::PredicateVerdict::Unknown);
    let verification=json!({"verification":{"type":"procedure","after":[{"kind":"file_exists","path":"out.md"}]}});
    assert_eq!(algorithms::procedure::evaluate(&verification,&[],|_,_|(false,true))["followed"],false);
}
