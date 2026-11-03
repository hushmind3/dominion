// Shared runtime for chat, agents, meetings and workflows.
// A workflow may call this runtime, but the runtime knows nothing about goals or plans.
const clamp=(n,min,max,fallback)=>Number.isFinite(Number(n))?Math.max(min,Math.min(max,Math.trunc(Number(n)))):fallback;
const abortError=()=>Object.assign(Error('Execution cancelled'),{code:'CANCELLED'});
export class ExecutionRuntime {
  #sessions=new Map();
  #runs=new Map();
  constructor({complete,discoverTools,invokeTool,readHistory,convertMessage,saveMessage,saveRun,saveOperation}){
    Object.assign(this,{complete,discoverTools,invokeTool,readHistory,convertMessage,saveMessage,saveRun,saveOperation});
  }
  isActive(sessionId){return this.#sessions.has(sessionId);}
  active(){return [...this.#runs.values()].map(x=>({runId:x.id,sessionId:x.sessionId,status:x.status}));}
  cancel(id){
    const run=this.#runs.get(id)||this.#sessions.get(id);
    if(!run)return false;
    run.controller.abort(abortError());
    return true;
  }
  async execute({sessionId,agentId,providerId,model,system,onEvent=()=>{},signal,toolsEnabled=true,speakerLabels=false,
    limits={}}){
    if(this.#sessions.has(sessionId))throw Error('Session is already running');
    const controller=new AbortController(),start=Date.now(),runId=crypto.randomUUID();
    const maxIterations=clamp(limits.maxIterations,1,64,8);
    const maxToolCalls=clamp(limits.maxToolCalls,1,512,64);
    const maxElapsedMs=clamp(limits.maxElapsedMs,1000,86400000,900000);
    const maxTokens=clamp(limits.maxTokens,100,2000000,100000);
    const run={id:runId,sessionId,agentId,providerId,model,status:'running',startedAt:new Date().toISOString(),
      updatedAt:new Date().toISOString(),iterations:0,modelCalls:0,toolCalls:0,tokens:0,reason:'',controller};
    const persisted=()=>{const {controller:_,...data}=run;this.saveRun({...data,updatedAt:new Date().toISOString()});};
    const emit=(type,detail={})=>{persisted();onEvent({type,runId,sessionId,...detail});};
    const check=()=>{
      if(controller.signal.aborted)throw abortError();
      if(Date.now()-start>maxElapsedMs||run.tokens>=maxTokens||run.toolCalls>maxToolCalls)throw Object.assign(Error('Execution budget exceeded'),{code:'BUDGET'});
    };
    const externalAbort=()=>controller.abort(signal?.reason||abortError());
    if(signal?.aborted)externalAbort();
    else signal?.addEventListener('abort',externalAbort,{once:true});
    this.#sessions.set(sessionId,run);this.#runs.set(runId,run);
    try{
      check();emit('runtime.started');
      // A persisted unanswered tool call may have had side effects before a crash.
      // Never repeat such calls automatically when a session resumes.
      const prior=this.readHistory(sessionId);
      const answered=new Set(prior.filter(x=>x.role==='tool').map(x=>x.tool_call_id));
      const outstanding=prior.filter(x=>x.role==='assistant').flatMap(x=>x.tool_calls||[])
        .filter(x=>!answered.has(x.id));
      if(outstanding.length)throw Object.assign(Error('Unresolved tool calls require review before resuming'),{code:'INTERRUPTED'});
      const tools=await this.discoverTools(toolsEnabled);
      check();
      let assistant=null;
      for(let iteration=0;iteration<maxIterations;iteration++){
        check();run.iterations=iteration+1;
        const history=[{role:'system',content:system},...this.readHistory(sessionId).map(x=>this.convertMessage(x,speakerLabels))];
        emit('runtime.model_started',{iteration:iteration+1});
        const reply=await this.complete({providerId,model,messages:history,tools,signal:controller.signal,
          onEvent:e=>{if(!controller.signal.aborted)onEvent(e);}});
        check();
        run.modelCalls++;
        run.tokens+=Number(reply.usage?.total_tokens||Number(reply.usage?.prompt_tokens||0)+Number(reply.usage?.completion_tokens||0))||0;
        assistant=this.saveMessage({session_id:sessionId,role:'assistant',content:reply.content,
          reasoning:reply.reasoning_content,tool_calls:reply.tool_calls?.length?reply.tool_calls:null,agent_id:agentId});
        onEvent({type:'assistant_saved',message:assistant,usage:reply.usage,runId});
        emit('runtime.model_finished',{iteration:iteration+1,toolCalls:reply.tool_calls?.length||0});
        if(!reply.tool_calls?.length){run.status='completed';emit('runtime.completed');onEvent({type:'done',sessionId,usage:reply.usage,runId});return assistant;}
        for(const [index,call] of reply.tool_calls.entries()){
          check();
          if(run.toolCalls>=maxToolCalls)throw Object.assign(Error('Tool call limit exceeded'),{code:'BUDGET'});
          const callId=call.id||runId+':'+iteration+':'+index;
          const tool=tools.find(x=>x.name===call.function?.name);
          this.saveOperation({id:runId+':'+callId,runId,sessionId,callId,tool:call.function?.name||'',status:'started',startedAt:new Date().toISOString()});
          emit('runtime.tool_started',{name:call.function?.name,callId});
          let output;
          try{
            if(!tool)throw Error('Tool not enabled: '+call.function?.name);
            const args=JSON.parse(call.function?.arguments||'{}');
            onEvent({type:'tool_start',name:tool.name,args,callId});
            output=await this.invokeTool(tool,args,onEvent,{signal:controller.signal,runId,callId});
            this.saveOperation({id:runId+':'+callId,runId,sessionId,callId,tool:tool.name,status:'completed',finishedAt:new Date().toISOString()});
            onEvent({type:'tool_end',name:tool.name,callId,result:output});
          }catch(error){
            const message=String(error?.message||error);
            output={error:message};
            this.saveOperation({id:runId+':'+callId,runId,sessionId,callId,tool:call.function?.name||'',status:controller.signal.aborted?'uncertain':'failed',reason:message,finishedAt:new Date().toISOString()});
            onEvent({type:'tool_error',name:call.function?.name,callId,error:message});
          }
          // Store tool output even if cancellation arrives just after the call completes.
          this.saveMessage({session_id:sessionId,role:'tool',tool_call_id:callId,
            content:typeof output==='string'?output:JSON.stringify(output).slice(0,30000)});
          run.toolCalls++;
          emit('runtime.tool_finished',{name:call.function?.name,callId,error:Boolean(output?.error)});
          check();
        }
      }
      throw Object.assign(Error('Model iteration limit reached'),{code:'LIMIT'});
    }catch(error){
      run.status=controller.signal.aborted?'cancelled':error?.code==='BUDGET'?'budget_exceeded':
        error?.code==='INTERRUPTED'?'needs_review':'failed';
      run.reason=String(error?.message||error).slice(0,1000);
      emit('runtime.'+run.status,{reason:run.reason});
      throw error;
    }finally{
      signal?.removeEventListener('abort',externalAbort);
      run.finishedAt=new Date().toISOString();persisted();
      this.#sessions.delete(sessionId);this.#runs.delete(runId);
    }
  }
}
