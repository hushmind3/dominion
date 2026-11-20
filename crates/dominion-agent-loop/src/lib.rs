//! Replaceable model/tool workflows. ReAct and plan-and-execute run through
//! the same core driver, tool gates, cancellation and host authority.
use dominion_turn_core::agent_loop::{
    AgentLoop,ModelPort,ModelResponse,RoundObservation,ToolPort,
};
use dominion_turn_core::driver::{Driver,Step};
use serde_json::{json,Value};

#[derive(Default)]
pub struct ReActLoop {
    pub completed_rounds:usize,
}
impl AgentLoop for ReActLoop{
    fn name(&self)->&'static str {"react"}
    fn after_model(&mut self,observation:RoundObservation)->Step{
        self.completed_rounds+=1;
        if observation.proposed_calls==0 {Step::Answer}
        else{Step::Execute{calls:observation.proposed_calls}}
    }
}
#[derive(Default)]
pub struct PlanExecuteLoop {
    pub completed_rounds:usize,
    pub plan_was_available:bool,
}
impl AgentLoop for PlanExecuteLoop{
    fn name(&self)->&'static str {"plan-execute"}
    fn after_model(&mut self,observation:RoundObservation)->Step{
        self.completed_rounds+=1;
        self.plan_was_available|=observation.has_plan;
        // The secondary planner is advisory. If it fails, ordinary tool
        // execution continues with identical admission and owner consent.
        if observation.proposed_calls==0 {Step::Answer}
        else{Step::Execute{calls:observation.proposed_calls}}
    }
}
pub enum ActiveLoop {
    ReAct(ReActLoop),
    PlanExecute(PlanExecuteLoop),
}
pub fn select(requested_plan:bool)->ActiveLoop{
    if requested_plan{ActiveLoop::PlanExecute(PlanExecuteLoop::default())}
    else{ActiveLoop::ReAct(ReActLoop::default())}
}
impl AgentLoop for ActiveLoop{
    fn name(&self)->&'static str{
        match self{Self::ReAct(p)=>p.name(),Self::PlanExecute(p)=>p.name()}
    }
    fn after_model(&mut self,observation:RoundObservation)->Step{
        match self{Self::ReAct(p)=>p.after_model(observation),
                   Self::PlanExecute(p)=>p.after_model(observation)}
    }
}
#[derive(Debug)]
pub struct RunResult {
    pub response:String,
    pub model_rounds:usize,
    pub tool_calls:usize,
    pub workflow:&'static str,
}
/// Standalone workflow implementation: any provider/tool host can implement
/// the two ports without importing the Dominion product or database.
pub async fn run<A:AgentLoop,M:ModelPort,T:ToolPort>(
    policy:&mut A,driver:&mut Driver,model:&mut M,tools:&mut T,
    history:&mut Vec<Value>,visible_tools:&[Value],tools_enabled:bool,
    has_plan:bool,
)->Result<RunResult,String>{
    let mut rounds=0usize;
    let mut dispatched=0usize;
    loop{
        let response:ModelResponse=driver.request_model(
            model.cancelled()||tools.cancelled(),
            model.complete(history,visible_tools),
        ).await?;
        rounds+=1;
        let step=driver.authorize_step(policy,RoundObservation{
            proposed_calls:response.calls.len(),has_plan,
        },tools_enabled).map_err(str::to_string)?;
        match step{
            Step::Answer=>{
                history.push(json!({"role":"assistant","content":response.content}));
                return Ok(RunResult{
                    response:response.content,model_rounds:rounds,
                    tool_calls:dispatched,workflow:policy.name(),
                });
            }
            Step::Execute{..}=>{
                // Reject the entire proposal before ANY host effect. A batch
                // with a visible read followed by a hidden mutation must not
                // partially execute, regardless of which strategy selected it.
                if !Driver::batch_advertised(
                    response.calls.iter().map(|call|call.name.as_str()),
                    visible_tools,
                ){
                    return Err("tool was not advertised to this model round".into());
                }
                let calls=response.calls.iter().map(|c|json!({
                    "id":c.id,"type":"function",
                    "function":{"name":c.name,"arguments":c.arguments.to_string()}
                })).collect::<Vec<_>>();
                history.push(json!({"role":"assistant","content":response.content,
                    "tool_calls":calls}));
                for call in &response.calls {
                    if model.cancelled()||tools.cancelled(){
                        return Err("execution cancelled".into());
                    }
                    driver.admit_tool(&call.name,visible_tools,tools_enabled)
                        .map_err(str::to_string)?;
                    let output=tools.dispatch(call).await?;
                    history.push(json!({"role":"tool","tool_call_id":call.id,
                        "content":output.to_string()}));
                    dispatched+=1;
                }
            }
        }
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    use dominion_turn_core::agent_loop::ToolRequest;
    use std::collections::VecDeque;
    struct Model{replies:VecDeque<ModelResponse>,called:usize,cancelled:bool}
    impl ModelPort for Model{
        async fn complete(&mut self,_:&[Value],_:&[Value])->Result<ModelResponse,String>{
            self.called+=1;
            self.replies.pop_front().ok_or_else(||"no provider response".to_string())
        }
        fn cancelled(&self)->bool{self.cancelled}
    }
    #[derive(Default)]
    struct Tools{executed:Vec<String>,cancelled:bool}
    impl ToolPort for Tools{
        async fn dispatch(&mut self,call:&ToolRequest)->Result<Value,String>{
            self.executed.push(call.name.clone());
            Ok(json!({"success":true}))
        }
        fn cancelled(&self)->bool{self.cancelled}
    }
    fn catalog()->Vec<Value>{
        vec![json!({"type":"function","function":{"name":"file_read"}})]
    }
    fn model_with_tool(name:&str)->Model{
        Model{replies:VecDeque::from(vec![
            ModelResponse{content:String::new(),calls:vec![ToolRequest{
                id:"call-1".into(),name:name.into(),arguments:json!({"path":"a.rs"}),
            }]},
            ModelResponse{content:"finished".into(),calls:vec![]},
        ]),called:0,cancelled:false}
    }
    #[tokio::test]
    async fn both_loops_execute_real_provider_tool_roundtrip_via_ports(){
        for wants_plan in [false,true] {
            let mut algorithm=select(wants_plan);
            let mut driver=Driver::new(3,3);
            let mut model=model_with_tool("file_read");
            let mut tools=Tools::default();
            let mut history=vec![json!({"role":"user","content":"read a.rs"})];
            let outcome=run(&mut algorithm,&mut driver,&mut model,&mut tools,
                &mut history,&catalog(),true,wants_plan).await.unwrap();
            assert_eq!(outcome.response,"finished");
            assert_eq!(outcome.model_rounds,2);
            assert_eq!(outcome.tool_calls,1);
            assert_eq!(model.called,2);
            assert_eq!(tools.executed,vec!["file_read"]);
            assert_eq!(history[2]["role"],"tool");
            assert_eq!(outcome.workflow,if wants_plan{"plan-execute"}else{"react"});
        }
    }
    #[tokio::test]
    async fn invisible_or_disabled_tools_cannot_reach_host(){
        for enabled in [true,false]{
            let mut algorithm=select(false);
            let mut driver=Driver::new(3,3);
            let mut model=model_with_tool("mcp__hidden__delete");
            let mut tools=Tools::default();
            let result=run(&mut algorithm,&mut driver,&mut model,&mut tools,
                &mut vec![],&catalog(),enabled,false).await;
            assert!(result.is_err());
            assert!(tools.executed.is_empty());
        }
    }
    #[tokio::test]
    async fn any_hidden_batch_member_prevents_every_host_side_effect(){
        let mut algorithm=select(false);
        let mut driver=Driver::new(3,3);
        let mut model=Model{
            replies:VecDeque::from(vec![ModelResponse{
                content:String::new(),
                calls:vec![
                    ToolRequest{id:"1".into(),name:"file_read".into(),arguments:json!({})},
                    ToolRequest{id:"2".into(),name:"mcp__hidden__write".into(),arguments:json!({})},
                ]
            }]),called:0,cancelled:false,
        };
        let mut tools=Tools::default();
        let result=run(&mut algorithm,&mut driver,&mut model,&mut tools,
            &mut vec![],&catalog(),true,false).await;
        assert!(result.unwrap_err().contains("not advertised"));
        assert!(tools.executed.is_empty());
    }
    #[tokio::test]
    async fn cancellation_and_round_limit_stop_model_and_tool_dispatch(){
        let mut algorithm=select(false);
        let mut driver=Driver::new(3,3);
        let mut model=model_with_tool("file_read");
        model.cancelled=true;
        let mut tools=Tools::default();
        assert!(run(&mut algorithm,&mut driver,&mut model,&mut tools,&mut vec![],
            &catalog(),true,false).await.is_err());
        assert_eq!(model.called,0);
        let mut algorithm=select(false);
        let mut driver=Driver::new(1,3);
        let mut model=model_with_tool("file_read");
        let mut tools=Tools::default();
        assert!(run(&mut algorithm,&mut driver,&mut model,&mut tools,&mut vec![],
            &catalog(),true,false).await.is_err());
        assert_eq!(model.called,1);
        assert_eq!(tools.executed,vec!["file_read"]);
    }
}
