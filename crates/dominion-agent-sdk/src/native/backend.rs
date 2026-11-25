//! Adapter to the original branch2 Driver and SDK execution entry point.
use super::*;
use crate::{ModelPort,ToolPort,ModelResponse as PortResponse,ToolRequest as PortCall};
struct Audit<'a> { session:&'a Session<'a>, events:Mutex<Vec<Value>> }
impl Audit<'_> {
    fn record(&self,event:Value)->Result<(),String>{
        let mut events=self.events.lock().map_err(|_|"audit lock poisoned".to_string())?;
        events.push(event);
        self.session.save(json!({"phase":"driver","journal":&*events}))
    }
}
struct Model<'a>(&'a Audit<'a>);
impl ModelPort for Model<'_> {
    async fn complete(&mut self,history:&[Value],visible:&[Value])->Result<PortResponse,String>{
        self.0.record(json!({"phase":"model_intent","transcript":history,"catalog":visible}))?;
        let response=self.0.session.model(history.to_vec(),visible.to_vec(),vec![]).await?;
        self.0.record(json!({"phase":"model_observed","content":response.content,
            "calls":response.calls.iter().map(|c|json!({"id":c.id,"name":c.name,"arguments":c.arguments})).collect::<Vec<_>>(),
            "inputTokens":response.input_tokens,"outputTokens":response.output_tokens}))?;
        Ok(PortResponse{content:response.content,calls:response.calls.into_iter().map(|c|PortCall{id:c.id,name:c.name,arguments:c.arguments}).collect()})
    }
    fn cancelled(&self)->bool {self.0.session.cancelled()}
}
struct Tools<'a>(&'a Audit<'a>);
impl ToolPort for Tools<'_> {
    async fn dispatch(&mut self,call:&PortCall)->Result<Value,String>{
        self.0.record(json!({"phase":"tool_intent","id":call.id,"name":call.name,"arguments":call.arguments}))?;
        let result=self.0.session.tool(ToolCall{id:call.id.clone(),name:call.name.clone(),arguments:call.arguments.clone()}).await?;
        let value=result.observation();
        self.0.record(json!({"phase":"tool_observed","id":call.id,"result":value,"success":result.success}))?;
        Ok(value)
    }
    fn cancelled(&self)->bool {self.0.session.cancelled()}
}
pub(super) async fn execute(session:&Session<'_>)->Result<String,String>{
    let r=session.request;
    let audit=Audit{session,events:Mutex::new(vec![])};
    let mut model=Model(&audit);
    let mut tools=Tools(&audit);
    let mut policy=crate::workflows::ReActLoop::default();
    let mut history=r.initial_messages();
    let catalog=r.catalog();
    let result=crate::execute(&mut policy,&mut model,&mut tools,&mut history,&catalog,
        crate::RunOptions{max_model_rounds:r.limits.max_model_calls,tool_budget:r.limits.max_tool_calls,
            tools_enabled:!catalog.is_empty(),has_plan:false}).await?;
    audit.record(json!({"phase":"candidate","transcript":history,"answer":result.response}))?;
    session.verify(&result.response).await?;
    Ok(result.response)
}
