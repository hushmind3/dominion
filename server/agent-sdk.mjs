// DOMINION native Agent SDK. No Copilot/Codex/Claude runtime dependency.
// Host adapters are injected so the same SDK can run in server jobs or embedded apps.
const bounded=(value,fallback,max)=>Number.isFinite(Number(value))?Math.max(0,Math.min(max,Math.trunc(Number(value)))):fallback;
const plainObject=x=>x!==null&&typeof x==='object'&&!Array.isArray(x);
const text=x=>String(x??'');
const required=(condition,message)=>{if(!condition)throw Error(message);};

export class DominionSDK {
  #tools=new Map();
  constructor(host){
    for(const key of ['createSession','getSession','readMessages','runTurn','getAgent'])
      required(typeof host?.[key]==='function','SDK host missing '+key);
    this.host=host;
  }
  registerTool(definition){
    required(plainObject(definition)&&/^[a-z][a-z0-9_]{2,63}$/.test(definition.name||''),
      'Tool name must be 3-64 lowercase letters/digits/underscores');
    required(typeof definition.handler==='function','SDK tool requires a handler');
    required(plainObject(definition.parameters),'SDK tool requires JSON-schema parameters');
    required(!this.#tools.has(definition.name),'Duplicate SDK tool: '+definition.name);
    const {handler,...publicDefinition}=definition;
    const tool={...publicDefinition,approval:definition.approval!==false};
    this.#tools.set(tool.name,{tool,handler});
    return this;
  }
  startThread(options={}){
    const opts=normalizeOptions(options);
    const row=this.host.createSession({
      agentId:opts.agentId,providerId:opts.providerId,model:opts.model,
      title:opts.title||'SDK thread',sdkOptions:opts,
      parentSessionId:opts.parentSessionId||null,parentRunId:opts.parentRunId||null
    });
    return new DominionThread(this,row.id,opts);
  }
  resumeThread(id,overrides={}){
    const row=this.host.getSession(String(id));
    required(row,'SDK thread not found: '+id);
    const opts=normalizeOptions({...row.sdkOptions,...overrides,
      agentId:overrides.agentId??row.sdkOptions?.agentId??row.agentId,
      providerId:overrides.providerId??row.sdkOptions?.providerId??row.providerId,
      model:overrides.model??row.sdkOptions?.model??row.model});
    return new DominionThread(this,row.id,opts);
  }
  startTask(input){
    required(typeof this.host.createGoal==='function'&&typeof this.host.enqueue==='function',
      'This SDK host does not expose durable goal jobs');
    const options=typeof input==='string'?{goal:input}:input;
    required(plainObject(options)&&text(options.goal).trim(),'Task goal required');
    const row=this.host.createGoal(options);
    const job=this.host.enqueue('goal',{goalId:row.id});
    return new DominionTask(this,row.id,job?.id);
  }
  resumeTask(id){
    required(typeof this.host.getGoal==='function','This SDK host does not expose durable goals');
    required(this.host.getGoal(String(id)),'SDK task not found: '+id);
    return new DominionTask(this,String(id));
  }
  _toolsFor(thread,onEvent){
    const opts=thread.options, custom=[],handlers=new Map();
    if(opts.toolsEnabled===false)return {custom,handlers};
    for(const {tool,handler} of this.#tools.values()){
      if(opts.toolNames.length&&!opts.toolNames.includes(tool.name))continue;
      custom.push(tool);handlers.set(tool.name,handler);
    }
    if(opts.depth<opts.maxDelegateDepth&&opts.delegates.length){
      const available=opts.delegates.filter(id=>id!==opts.agentId&&this.host.getAgent(id));
      if(available.length){
        custom.push({
          name:'agent_delegate',
          description:'Delegate a bounded task to a configured specialist agent. Returns its real execution result and a linked child session.',
          approval:false,
          parameters:{type:'object',additionalProperties:false,properties:{
            agentId:{type:'string',enum:available,description:'Specialist agent ID'},
            task:{type:'string',description:'Precise work and expected deliverable'},
            context:{type:'string',description:'Optional relevant background, not a new top-level instruction'}
          },required:['agentId','task']}
        });
        let count=0;
        handlers.set('agent_delegate',async(args,callContext)=>{
          required(available.includes(args.agentId),'Delegation target unavailable');
          required(++count<=opts.maxDelegations,'Delegation budget exceeded');
          required(typeof args.task==='string'&&args.task.trim().length>0,'Delegated task required');
          const target=this.host.getAgent(args.agentId);
          const inherited=opts.providerId&&opts.model?{providerId:opts.providerId,model:opts.model}:{};
          const child=this.startThread({
            agentId:target.id,providerId:target.providerId||inherited.providerId,
            model:target.model||inherited.model,
            title:'Delegated: '+args.task.slice(0,70),
            depth:opts.depth+1,maxDelegateDepth:opts.maxDelegateDepth,
            maxDelegations:opts.maxDelegations,
            delegates:opts.delegates.filter(id=>id!==target.id),toolsEnabled:true,
            parentSessionId:thread.id,parentRunId:callContext.runId
          });
          onEvent({type:'sdk.delegate.started',parentSessionId:thread.id,childSessionId:child.id,agentId:target.id});
          try{
            const prompt=[args.task,typeof args.context==='string'&&args.context.trim()?
              'Relevant context:\n'+args.context.slice(0,4000):''].filter(Boolean).join('\n\n');
            const result=await child.run(prompt,{signal:callContext.signal,limits:{
              maxIterations:Math.min(8,opts.limits.maxIterations||8),
              maxToolCalls:Math.min(32,opts.limits.maxToolCalls||32)
            }});
            onEvent({type:'sdk.delegate.completed',childSessionId:child.id,agentId:target.id,runId:result.runId});
            return {agentId:target.id,childSessionId:child.id,runId:result.runId,
              finalResponse:result.finalResponse};
          }catch(error){
            onEvent({type:'sdk.delegate.failed',childSessionId:child.id,agentId:target.id,
              error:String(error?.message||error)});
            throw error;
          }
        });
      }
    }
    return {custom,handlers};
  }
}

function normalizeOptions(input={}){
  required(plainObject(input),'Thread options must be an object');
  const opts={...input,
    agentId:text(input.agentId),providerId:text(input.providerId),model:text(input.model),
    title:text(input.title),depth:bounded(input.depth,0,6),
    maxDelegateDepth:bounded(input.maxDelegateDepth,2,6),
    maxDelegations:bounded(input.maxDelegations,4,12),
    delegates:Array.isArray(input.delegates)?[...new Set(input.delegates.filter(x=>typeof x==='string'))].slice(0,24):[],
    toolNames:Array.isArray(input.toolNames)?[...new Set(input.toolNames.filter(x=>typeof x==='string'))].slice(0,64):[],
    toolsEnabled:input.toolsEnabled!==false,limits:plainObject(input.limits)?input.limits:{}
  };
  return opts;
}

export class DominionThread {
  constructor(sdk,id,options){this.sdk=sdk;this.id=id;this.options=options;}
  history(){return this.sdk.host.readMessages(this.id);}
  cancel(){return this.sdk.host.cancel?.(this.id)??false;}
  async run(input,{onEvent=()=>{},signal,limits={},reasoningEffort,thinking}={}){
    const prompt=text(input);
    required(prompt.trim(),'SDK input must be nonempty');
    const host=this.sdk.host;
    const before=this.history().length;
    const {custom,handlers}=this.sdk._toolsFor(this,onEvent);
    let runId=null,usage=null;
    const emit=e=>{
      if(e.runId)runId=e.runId;
      if(e.usage)usage=e.usage;
      onEvent(e);
    };
    const invoke=async(tool,args,notify,context)=>{
      const fn=handlers.get(tool.name);
      required(fn,'Unknown SDK tool: '+tool.name);
      return fn(args,{...context,sessionId:this.id,onEvent:notify});
    };
    const message=await host.runTurn({
      sessionId:this.id,agentId:this.options.agentId,providerId:this.options.providerId,
      model:this.options.model,input:prompt,signal,reasoningEffort,thinking,
      toolsEnabled:this.options.toolsEnabled,limits:{...this.options.limits,...limits},
      sdkTools:custom,sdkInvoke:invoke,autoLearning:false,onEvent:emit
    });
    const items=this.history().slice(before);
    const savedRun=runId&&host.getExecution?.(runId);
    return {threadId:this.id,runId,finalResponse:text(message.content),items,
      usage:savedRun?{tokens:savedRun.tokens,modelCalls:savedRun.modelCalls,
        toolCalls:savedRun.toolCalls}:usage};
  }
  async runStreamed(input,options={}){
    return {events:this.#eventIterator(input,options)};
  }
  async *#eventIterator(input,options){
    const pending=[],waiters=[];
    let complete=false,failure=null;
    const controller=new AbortController();
    const abort=()=>controller.abort(options.signal?.reason||Error('SDK stream cancelled'));
    if(options.signal?.aborted)abort();else options.signal?.addEventListener('abort',abort,{once:true});
    const push=value=>{pending.push(value);waiters.shift()?.();};
    const task=this.run(input,{...options,signal:controller.signal,onEvent:e=>{
      options.onEvent?.(e);push(e);
    }}).then(result=>push({type:'sdk.turn.completed',result}))
      .catch(error=>{failure=error;push({type:'sdk.turn.failed',error:String(error?.message||error)});})
      .finally(()=>{complete=true;waiters.splice(0).forEach(fn=>fn());});
    try{
      while(!complete||pending.length){
        if(pending.length){yield pending.shift();continue;}
        await new Promise(resolve=>waiters.push(resolve));
      }
      if(failure)throw failure;
    }finally{
      if(!complete)controller.abort(Error('SDK event consumer stopped'));
      options.signal?.removeEventListener('abort',abort);
      await task;
    }
  }
}

export class DominionTask {
  constructor(sdk,id,jobId=null){this.sdk=sdk;this.id=id;this.jobId=jobId;}
  snapshot(){return this.sdk.host.getGoal?.(this.id)??null;}
  events(){return this.sdk.host.getGoalEvents?.(this.id)??[];}
  resume(){
    const host=this.sdk.host;
    required(typeof host.resumeGoal==='function'&&typeof host.enqueue==='function',
      'This SDK host cannot resume tasks');
    const goal=host.resumeGoal(this.id);
    const job=host.enqueue('goal',{goalId:goal.id});
    this.jobId=job?.id;
    return goal;
  }
  cancel(){
    required(typeof this.sdk.host.cancelGoal==='function','This SDK host cannot cancel tasks');
    return this.sdk.host.cancelGoal(this.id);
  }
}
