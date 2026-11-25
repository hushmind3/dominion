//! Adapter to the original branch2 Driver and SDK execution entry point.
use super::*;
pub(super) const ENGINE_ID:&str="dominion-branch2-native-v2";
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
    async fn verify_candidate(&mut self,answer:&str)->Result<Option<String>,String>{
        self.0.record(json!({"phase":"candidate","answer":answer}))?;
        Ok(match self.0.session.verify_candidate(answer).await?{
            Verification::Accept=>None,Verification::Reject(reason)=>Some(reason)
        })
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
pub(super) async fn execute(session:&Session<'_>,restored:Option<Value>)->Result<String,String>{
    let r=session.request;
    let (events,mut history)=match restored{
        Some(saved)=>{
            if saved["phase"]!="suspended_driver"{return Err("invalid suspended driver state".into());}
            (saved["journal"].as_array().ok_or("missing driver journal")?.clone(),
             saved["transcript"].as_array().ok_or("missing driver transcript")?.clone())
        },
        None=>(vec![],r.initial_messages()),
    };
    let used=session.meter()?;
    let audit=Audit{session,events:Mutex::new(events)};
    let mut model=Model(&audit);
    let mut tools=Tools(&audit);
    let mut policy=crate::workflows::ReActLoop{completed_rounds:used.model_calls};
    let catalog=r.catalog();
    let result=crate::execute(&mut policy,&mut model,&mut tools,&mut history,&catalog,
        crate::RunOptions{max_model_rounds:r.limits.max_model_calls.saturating_sub(used.model_calls),tool_budget:r.limits.max_tool_calls.saturating_sub(used.tool_calls),
            tools_enabled:!catalog.is_empty(),has_plan:false}).await?;
    audit.record(json!({"phase":"candidate","transcript":history,"answer":result.response}))?;
    Ok(result.response)
}

pub(super) fn suspend_state(state:&Value,_meter:Meter)->Result<Value,String>{
    let mut events=state["journal"].as_array().ok_or("missing driver journal")?.clone();
    let intent=events.pop().ok_or("missing driver model intent")?;
    if intent["phase"]!="model_intent"||!intent["transcript"].is_array(){
        return Err("not an undispatched driver model boundary".into());
    }
    Ok(json!({"phase":"suspended_driver","journal":events,"transcript":intent["transcript"]}))
}
