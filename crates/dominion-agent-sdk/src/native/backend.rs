//! Adapter to branch3's existing ReActLoop/Kernel, including native evidence.
use super::*;
pub(super) const ENGINE_ID:&str="dominion-branch3-native-v2";
use crate::{Action, AgentLoop, AgentPorts, BoxFuture, Criterion, Decision, Evidence, Journal,
    Model as ModelPort, ModelReply, ModelTurn, Receipt, Snapshot, Status, Tools as ToolPort};
struct Model<'a>(&'a Session<'a>);
impl ModelPort for Model<'_> {
    fn complete<'a>(&'a self,_:&'a str,_:&'a str)->BoxFuture<'a,Result<ModelReply,String>>{
        Box::pin(async {Err("native tool-aware model lane required".into())})
    }
    fn supports_native_tools(&self)->bool{true}
    fn complete_with_tools<'a>(&'a self,system:&'a str,prompt:&'a str,tools:&'a [crate::ToolSpec],receipts:&'a [Receipt])
        ->BoxFuture<'a,Result<ModelTurn,String>>{
        Box::pin(async move {
            // Preserve the kernel's prompt and full receipts separately. No
            // arbitrary truncation or replacement of the kernel's policy.
            let catalog=tools.iter().map(|t|json!({"type":"function","function":{"name":t.name,"description":t.description,"parameters":t.schema}})).collect();
            let observations=receipts.iter().map(|r|serde_json::to_value(r).map_err(|e|e.to_string())).collect::<Result<Vec<_>,_>>()?;
            let messages=vec![json!({"role":"system","content":system}),json!({"role":"user","content":prompt})];
            let reply=self.0.model(messages,catalog,observations).await?;
            let answer=if reply.calls.is_empty(){Some(reply.content.clone())}else{None};
            let actions=reply.calls.into_iter().map(|c|Action{id:c.id,tool:c.name,arguments:c.arguments,depends_on:vec![]}).collect();
            Ok(ModelTurn{reply:ModelReply{content:reply.content,input_tokens:reply.input_tokens,output_tokens:reply.output_tokens},
                decision:Some(Decision{plan:vec![],actions,answer})})
        })
    }
}
struct Tools<'a>(&'a Session<'a>);
impl ToolPort for Tools<'_>{
    fn definitions(&self)->Vec<crate::ToolSpec>{
        if self.0.request.limits.max_tool_calls==0 { return vec![]; }
        self.0.request.tools.iter().map(|t|crate::ToolSpec{name:t.name.clone(),description:t.description.clone(),
            schema:t.parameters.clone(),read_only:t.read_only}).collect()
    }
    fn execute<'a>(&'a self,action:&'a Action)->BoxFuture<'a,Result<Value,String>>{
        Box::pin(async move{
            let result=self.0.tool(ToolCall{id:action.id.clone(),name:action.tool.clone(),arguments:action.arguments.clone()}).await?;
            Ok(result.observation())
        })
    }
}
struct Proof<'a>(&'a Session<'a>);
impl Evidence for Proof<'_>{
    fn check<'a>(&'a self,criterion:&'a Criterion)->BoxFuture<'a,Result<bool,String>>{
        Box::pin(async move{
            self.0.evidence(serde_json::to_value(criterion).map_err(|e|e.to_string())?).await
        })
    }
    fn verify_answer<'a>(&'a self,answer:&'a str)->BoxFuture<'a,Result<Option<String>,String>>{
        Box::pin(async move{Ok(match self.0.verify_candidate(answer).await?{
            Verification::Accept=>None,Verification::Reject(reason)=>Some(reason)
        })})
    }
}
struct Store<'a>(&'a Session<'a>);
impl Journal for Store<'_>{
    fn save(&self,snapshot:&Snapshot)->Result<(),String>{
        self.0.save(serde_json::to_value(snapshot).map_err(|e|e.to_string())?)
    }
    fn cancelled(&self,_:&str)->bool{self.0.cancelled()}
}
pub(super) async fn execute(session:&Session<'_>,restored:Option<Value>)->Result<String,String>{
    let r=session.request;
    let model=Model(session);
    let tools=Tools(session);
    let evidence=Proof(session);
    let journal=Store(session);
    let algorithm=crate::StandardAlgorithm;
    let runner=crate::ReActLoop{policy:&algorithm};
    let request=crate::RunRequest{id:r.id.clone(),instruction:r.instruction.clone(),owner_instructions:r.system.clone(),
        strategy:crate::Strategy::Balanced,limits:crate::Limits{max_steps:r.limits.max_model_calls,
            max_model_calls:r.limits.max_model_calls,max_tool_calls:r.limits.max_tool_calls.max(1),
            max_estimated_tokens:r.limits.max_tokens.unwrap_or(20_000_000),..crate::Limits::default()},
        allowed_tools:tools.definitions().into_iter().map(|t|t.name).collect(),checks:vec![],require_evidence:true};
    let saved=restored.map(serde_json::from_value::<Snapshot>).transpose().map_err(|e|e.to_string())?;
    if let Some(snapshot)=&saved{
        if !matches!(snapshot.status,Status::Running)||snapshot.model_in_flight||!snapshot.in_flight.is_empty()
            ||snapshot.id!=r.id||snapshot.instruction!=r.instruction||snapshot.model_calls!=session.meter()?.model_calls{
            return Err("suspended kernel state is not a matching settled run".into());
        }
    }
    let snapshot=runner.execute(AgentPorts{model:&model,tools:&tools,evidence:&evidence,journal:&journal},request,saved).await?;
    if !matches!(snapshot.status,Status::Completed){
        return Err(snapshot.error.unwrap_or_else(||"native branch3 task not completed".into()));
    }
    let answer=snapshot.answer.ok_or("native completion missing answer")?;
    Ok(answer)
}

pub(super) fn suspend_state(state:&Value,meter:Meter)->Result<Value,String>{
    let mut snapshot:Snapshot=serde_json::from_value(state.clone()).map_err(|e|e.to_string())?;
    if !matches!(snapshot.status,Status::Running)||!snapshot.model_in_flight||!snapshot.in_flight.is_empty()
        ||snapshot.model_calls!=meter.model_calls||snapshot.step==0{
        return Err("not an undispatched kernel model boundary".into());}
    snapshot.model_in_flight=false;snapshot.step-=1;
    serde_json::to_value(snapshot).map_err(|e|e.to_string())
}
