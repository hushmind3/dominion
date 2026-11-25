//! Adapter to the existing embedded execution path, not an agent loop.
use super::*;
pub(super) const ENGINE_ID:&str="dominion-branch1-native-v2";
use crate::{Snapshot, CheckpointStore, Journal, ModelPort, ModelTurn, TurnDecision, Dispatcher,
    DispatchResult, EmbeddedLimits, Action, BoxFuture, Verifier};
struct Store<'a> { session:&'a Session<'a>, snapshot:Mutex<Snapshot> }
impl Journal for Store<'_> {
    fn save(&self,snapshot:&Snapshot)->Result<(),String>{
        self.session.save(serde_json::to_value(snapshot).map_err(|e|e.to_string())?)?;
        *self.snapshot.lock().map_err(|_|"snapshot lock poisoned".to_string())?=snapshot.clone();
        Ok(())
    }
    fn cancelled(&self,_:&str)->bool { self.session.cancelled() }
}
impl CheckpointStore for Store<'_> {
    fn load(&self)->Result<Snapshot,String> {
        Ok(self.snapshot.lock().map_err(|_|"snapshot lock poisoned".to_string())?.clone())
    }
}
struct Model<'a>(&'a Session<'a>);
impl ModelPort for Model<'_> {
    fn decide<'a>(&'a mut self,snapshot:&'a Snapshot)->BoxFuture<'a,Result<ModelTurn,String>> {
        Box::pin(async move {
            let mut messages=snapshot.native.messages.clone();
            // Keep exact assistant/tool envelopes and IDs. Repair feedback is
            // provided as an owner-side message, never as fabricated tool output.
            if let Some(event)=snapshot.native.events.iter().rev().find(|e|e.kind=="embedded_retry") {
                if let Some(reason)=event.detail.as_ref().and_then(|v|v["reason"].as_str()) {
                    messages.push(json!({"role":"user","content":format!("Verification rejected: {reason}")}));
                }
            }
            let observations=snapshot.receipts.iter().map(|r|serde_json::to_value(r).map_err(|e|e.to_string())).collect::<Result<Vec<_>,_>>()?;
            let reply=self.0.model(messages,snapshot.native.advertised_tools.clone(),observations).await?;
            let charged_tokens=reply.input_tokens.unwrap_or(0).saturating_add(reply.output_tokens.unwrap_or(0));
            let decision=if reply.calls.is_empty() { TurnDecision::Answer(reply.content) }
            else { TurnDecision::Tools(reply.calls.into_iter().map(|c|Action{id:c.id,tool:c.name,arguments:c.arguments,depends_on:vec![]}).collect()) };
            Ok(ModelTurn{decision,charged_tokens})
        })
    }
}
struct Tools<'a>(&'a Session<'a>);
impl Dispatcher for Tools<'_> {
    type Output=();
    fn definitions(&self)->Vec<Value>{self.0.request.catalog()}
    fn execute<'a>(&'a self,action:&'a Action)->BoxFuture<'a,Result<DispatchResult<()>,String>> {
        Box::pin(async move {
            let result=self.0.tool(ToolCall{id:action.id.clone(),name:action.tool.clone(),arguments:action.arguments.clone()}).await?;
            Ok(DispatchResult{output:(),observation:result.observation(),success:result.success})
        })
    }
}
struct Verify<'a>(&'a Session<'a>);
impl Verifier for Verify<'_> {
    fn verify<'a>(&'a mut self,_:&'a Snapshot,answer:&'a str)->BoxFuture<'a,Result<crate::Verification,String>> {
        Box::pin(async move{Ok(match self.0.verify_candidate(answer).await?{
            Verification::Accept=>crate::Verification::Accept,Verification::Reject(reason)=>crate::Verification::Retry(reason)
        })})
    }
}
pub(super) async fn execute(session:&Session<'_>,restored:Option<Value>)->Result<String,String>{
    let r=session.request;
    let snapshot=if let Some(saved)=restored{
        let snapshot:Snapshot=serde_json::from_value(saved).map_err(|e|e.to_string())?;
        let expected=Snapshot::new(&r.id,&r.instruction,r.limits.max_model_calls)?;
        if !snapshot.settled()||snapshot.native.status!=crate::runtime::domain::RunStatus::Running
            ||snapshot.native.id!=expected.native.id||snapshot.native.instruction!=r.instruction||snapshot.model_calls!=session.meter()?.model_calls{
            return Err("suspended snapshot identity or settlement mismatch".into());}
        snapshot
    }else{
        let mut snapshot=Snapshot::new(&r.id,&r.instruction,r.limits.max_model_calls)?;
        snapshot.native.messages=r.initial_messages();
        snapshot.native.model_only=r.catalog().is_empty();
        snapshot
    };
    let store=Store{session,snapshot:Mutex::new(snapshot)};
    let mut model=Model(session);
    let tools=Tools(session);
    let mut verifier=Verify(session);
    let snapshot=crate::run_embedded(&store,&mut model,&tools,&mut verifier,
        EmbeddedLimits{max_rounds:r.limits.max_model_calls,max_tools:r.limits.max_tool_calls,max_elapsed:r.limits.max_elapsed}).await?;
    snapshot.native.answer.ok_or_else(||"native completion missing answer".into())
}

/// Cancel exactly the undispatched native model intent, not a paid request.
pub(super) fn suspend_state(state:&Value,meter:Meter)->Result<Value,String>{
    let mut snapshot:Snapshot=serde_json::from_value(state.clone()).map_err(|e|e.to_string())?;
    if !snapshot.model_in_flight||snapshot.native.status!=crate::runtime::domain::RunStatus::Running
        ||snapshot.model_calls!=meter.model_calls.saturating_add(1)||snapshot.step==0||snapshot.native.steps!=snapshot.step{
        return Err("not an undispatched native model boundary".into());}
    snapshot.model_in_flight=false;snapshot.model_calls-=1;snapshot.step-=1;snapshot.native.steps-=1;
    if !snapshot.settled(){return Err("native boundary still unsettled".into());}
    snapshot.native.event("sdk_suspended","SDK suspended before provider dispatch",None);
    serde_json::to_value(snapshot).map_err(|e|e.to_string())
}
