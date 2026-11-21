/** TypeScript SDK for the independent DOMINION Rust engine, not a second agent loop. */
export interface RecordItem {id:string;createdAt?:string;updatedAt?:string;[key:string]:unknown}
export interface Agent extends RecordItem {name:string;systemPrompt?:string;providerId?:string;model?:string}
export interface Provider extends RecordItem {name:string;kind?:string;enabled?:boolean;hasKey?:boolean}
export interface Session extends RecordItem {title:string;agentId?:string;providerId?:string;model?:string}
export interface Room extends RecordItem {name:string;agentIds:string[];sessionId?:string}
export interface RoomEvent {
  version:1;roomId:string;sessionId:string;seq:number;eventId:string;
  author:string;replyTo:string|null;originRun:string|null;kind:string;
}
export interface RoomAgentBinding {
  roomId:string;agentId:string;sessionId:string;readCursor:number;state:string;
}
export interface RoomEventPage {
  roomId:string;sessionId:string;events:RoomEvent[];
  nextCursor:number;hasMore:boolean;bindings:RoomAgentBinding[];
}
export interface WorkflowStep {name:string;prompt:string;agentId:string;providerId?:string;model?:string}
export interface Workflow extends RecordItem {name:string;steps:WorkflowStep[];description?:string}
/** Provider-reported token totals; calls missing usage are counted separately. */
export interface NativeModelUsage {
 inputTokens:number;outputTokens:number;totalTokens:number;
 cacheReadTokens:number;cacheWriteTokens:number;
 reportedCalls:number;unreportedCalls:number;cacheReportedCalls:number;
 configuredCostKRW?:number;pricedCalls?:number;unpricedCalls?:number;
 configuredPriceCurrency?:'KRW';costBasis?:string;
 source:string;estimated:false;
 last?:{providerId:string;model:string;inputTokens:number|null;
  outputTokens:number|null;cacheReadTokens:number|null;cacheWriteTokens:number|null;
  providerReported:boolean};
}
/** Fixed model identity and nonsecret price snapshot for one native job. */
export interface NativeRouteSelection {
 providerId:string;model:string;binding:string;kind?:string;
 currency:'KRW';priceSource:string;
 pricesKRWPerMillion?:{input:number;output:number;cachedInput?:number;cacheWrite?:number}|null;
 selection?:{mode:string;reason?:string;requestedMode?:string;
  estimatedPerTurnKRW?:number;selectedQuality?:number;qualifiedCandidates?:number};
}
export type RoutingMode='default'|'quality_cost';
export type AlgorithmId='standard-v1'|'focused-v1';
export interface Job extends RecordItem {kind:string;status:string;input?:unknown;result?:unknown;error?:string;sessionId?:string;stepIndex?:number;stepTotal?:number;
 nativeRoute?:NativeRouteSelection;nativeModelUsage?:NativeModelUsage;nativeAlgorithm?:{format:number;id:AlgorithmId;revision:string};nativeState?:{chargedTokens?:number;modelCalls?:number;modelInFlight?:boolean;algorithmId?:AlgorithmId}}
/** Persisted Rust native job state shared with the autonomous UI. */
export interface NativeRun {
 id:string;status:string;instruction:string;answer?:string|null;error?:string|null;
 algorithmId?:AlgorithmId;algorithmRevision?:string|null;
 checkpoint?:{
  state?:{state:string};rounds?:number;toolCalls?:number;answer?:string|null;
  usage?:{prompt_tokens:number;completion_tokens:number;cached_prompt_tokens:number}|null;
  pendingCall?:{id:string;name:string;arguments:unknown}|null;
  events?:Array<{kind:string;detail:unknown}>;
  verificationChecks?:Array<{criterion:{type:string;path?:string;contains?:string};satisfied:boolean;explanation:string}>;
 }|null;
}
export interface NativeRunRequest {
 instruction:string;providerId?:string;model?:string;verifyCommand?:string;maxRounds?:number;maxToolCalls?:number;
 algorithmId?:AlgorithmId;
}
export interface NativeReconciliation {applied:boolean;details:string}
export interface NativeReplay {
 runId:string;mode:string;offlineOnly:boolean;modelOrExternalToolCalls:number;
 traceConsistent:boolean;replayableTrace:boolean;decisionTapeAvailable:boolean;
 requiresManualReconciliation:boolean;
 hostJournal:{operationCount:number;matchedReceipts:number;consistent:boolean;
   anomalies:string[];sourceEvidence:unknown[]};
 acceptanceChecks:{satisfied:number;total:number;allSatisfied:boolean};
 cost:{configuredCostKRW:number|null;currency:'KRW';invoiceVerified:boolean};
 actions:Array<{turn:number;id:string;plannedTool:string;observedTool?:string|null;
   status:string;diagnostic?:string|null}>;
}
export interface NativePairedComparison {
 mode:string;externalCalls:0;sameTaskModelLimitsPermissions:boolean;
 baselineRunId:string;candidateRunId:string;qualityComparable:boolean;
 candidateEligibleForManualReview:boolean;autoPromotion:false;
 baseline:Record<string,unknown>;candidate:Record<string,unknown>;
 differences:{toolCalls:number;modelCalls:number;chargedTokens:number;
   configuredCostKRW:number|null;latencyMs:number|null};qualification:string;
}
/** Single durable execution projection, independent of model and UI surface. */
export type RunSurface='chat'|'room'|'team'|'sdk'|'external_agent';
export type RunPhase='queued'|'running'|'waiting_approval'|'verifying'|'finalizing'|
  'succeeded'|'failed'|'cancelled'|'interrupted'|'needs_review';
export interface RunRef {
 version:1;surface:RunSurface;runId:string;parentRunId:string|null;
}
export interface RunView {
 ref:RunRef;phase:RunPhase;status:string;terminal:boolean;reviewRequired:boolean;ownerActionRequired:boolean;
 attempt:number|null;sessionId:string|null;providerId:string|null;model:string|null;
 startedAt:string|null;finishedAt:string|null;sourceJournal:'job'|'execution';
}
/** Native workers execute inside the shared Rust kernel; external bridge workers
 * remain independent read-only runtimes and never mint native tool receipts. */
export interface TeamTask {
  id:string;prompt:string;dependsOn?:string[];
  mode?:'read'|'write';runtime?:'native'|'codex'|'claude'|'qwen';
  isolation?:'shared'|'worktree';resumeId?:string;
  agentId?:string;providerId?:string;model?:string;
  allowedTools?:string[];checks?:unknown[];
}
export interface RemoteTask {
  id:string;status:string;phase:string;workerId?:string;
  result?:{answer?:string;usage?:unknown;verified?:false;evidenceGrade?:string};
  queuedAt?:string;startedAt?:string;finishedAt?:string;
  evidenceGrade?:string;
}
export interface TeamRequest {
  goal:string;tasks:TeamTask[];maxParallel?:number;maxReadRetries?:number;
}
export interface TeamChild {
  taskId:string;mode:'read'|'write';runtime:'native'|'codex'|'claude'|'qwen'|'remote';
  isolation:'shared'|'worktree';dependsOn:string[];childJobId:string;
  status:string|null;executionId?:string|null;verified:boolean;
  evidenceGrade?:string|null;worktree?:unknown;answerExcerpt?:string|null;
}
export interface TeamView {
  id:string;kind:'team';status:string;goal:string;maxParallel:number;
  maxReadRetries:number;tasks:TeamChild[];result:unknown;
  error?:string|null;createdAt?:string|null;teamState?:unknown;
}
export interface WorktreePreview {
  diffSha256:string;files:unknown[];[key:string]:unknown;
}
export interface Approval extends RecordItem {status:string;sessionId?:string;tool?:string;args?:unknown}
export interface EngineEvent {seq:number;type:string;kind?:string;record?:RecordItem;removed?:string;approval?:Approval;runRef?:RunRef|null;runState?:RunView|null;[key:string]:unknown}
export interface ChatEvent {type:string;[key:string]:unknown}
export interface ChatRequest {sessionId:string;agentId:string;input:string;providerId?:string;model?:string;toolsEnabled?:boolean;toolsExecutionEnabled?:boolean;autoLearning?:boolean;reasoningEffort?:string;reasoningLanguage?:string;thinking?:boolean}
export type CreateJob={kind:'prompt';input:{prompt:string;sessionId?:string;agentId?:string;providerId?:string;model?:string;toolsEnabled?:boolean;reasoningEffort?:string;thinking?:boolean;maxSteps?:number;maxAttempts?:number;maxModelCalls?:number;maxToolCalls?:number;tokenBudget?:number;parallelReads?:number;strategy?:"balanced"|"verify_first"|"low_cost";algorithmId?:AlgorithmId;ownerInstructions?:string;allowedTools?:string[];acceptanceChecks?:unknown[];requireEvidence?:boolean;allowFallback?:boolean;routingMode?:RoutingMode;minQuality?:number;maxQualityDrop?:number}}|{kind:'workflow';input:{workflowId:string;input?:string}};
export interface Options {baseUrl?:string;adminToken?:string;fetch?:typeof globalThis.fetch}
export interface WaitOptions {signal?:AbortSignal;intervalMs?:number;timeoutMs?:number}
export interface StreamOptions {after?:number;signal?:AbortSignal}
export class DominionApiError extends Error {
  constructor(readonly status:number,message:string,readonly detail?:unknown) {super(message);this.name='DominionApiError';}
}
export class DominionStreamError extends Error {
  constructor(message:string){super(message);this.name='DominionStreamError';}
}
/** Matches the Rust Runtime RunPhase terminal aliases; never infer
 * completion from a socket closing. */
export const terminalHostStatus=(status:string):boolean=>
  ['completed','succeeded','failed','denied','exhausted','cancelled',
   'interrupted','needs_review','in_doubt'].includes(status);
const id=(value:string)=>encodeURIComponent(value);
const aborted=()=>new DOMException('Operation aborted','AbortError');
function sleep(ms:number,signal?:AbortSignal):Promise<void>{
  if(signal?.aborted)return Promise.reject(aborted());
  return new Promise((resolve,reject)=>{
    const timer=setTimeout(()=>{signal?.removeEventListener('abort',abort);resolve();},ms);
    const abort=()=>{clearTimeout(timer);reject(aborted());};
    signal?.addEventListener('abort',abort,{once:true});
  });
}
/** SSE parser handles packet fragmentation, comments, CRLF and Unicode boundaries. */
async function* frames(body:ReadableStream<Uint8Array>):AsyncGenerator<unknown>{
  const reader=body.getReader(),decoder=new TextDecoder();
  let buffer='';
  try{
    while(true){
      const {done,value}=await reader.read();if(done)break;
      buffer+=decoder.decode(value,{stream:true});
      // Coalesced packets may contain multiple independent, bounded SSE frames.
      let match:RegExpExecArray|null;
      while((match=/(?:\r\n|\r|\n){2}/.exec(buffer))){
        if(match.index>2*1024*1024)throw new DominionStreamError('SSE frame larger than 2 MiB');
        const raw=buffer.slice(0,match.index).replace(/^\uFEFF/,'');
        buffer=buffer.slice(match.index+match[0].length);
        const data=raw.split(/\r\n|\r|\n/).filter(x=>x.startsWith('data:'))
          .map(x=>x.slice(5).replace(/^ /,'')).join('\n');
        if(!data)continue;
        try{yield JSON.parse(data) as unknown;}
        catch{throw new DominionStreamError('Invalid JSON in SSE frame');}
      }
      if(buffer.length>2*1024*1024)throw new DominionStreamError('SSE frame larger than 2 MiB');
    }
  }finally{await reader.cancel().catch(()=>undefined);reader.releaseLock();}
}
/** An SDK handle to one engine. Does not run external agents or shadow engine state. */
export class Dominion{
  readonly baseUrl:string;
  private readonly fetcher:typeof fetch;
  private readonly token?:string;
  constructor(options:Options={}){
    this.baseUrl=(options.baseUrl??'http://127.0.0.1:4317').replace(/\/+$/,'');
    if(!/^https?:\/\//.test(this.baseUrl))throw new TypeError('baseUrl must use HTTP(S)');
    this.token=options.adminToken;
    this.fetcher=options.fetch??globalThis.fetch.bind(globalThis);
  }
  private async open(method:'GET'|'POST'|'DELETE',path:string,body?:unknown,signal?:AbortSignal,sse=false):Promise<Response>{
    if(!path.startsWith('/api/'))throw new TypeError('Only engine API paths are supported');
    const headers:Record<string,string>={Accept:sse?'text/event-stream':'application/json'};
    if(body!==undefined)headers['Content-Type']='application/json';
    if(this.token)headers['X-Dominion-Token']=this.token;
    const result=await this.fetcher(this.baseUrl+path,{
      method,headers,signal,cache:'no-store',
      body:body===undefined?undefined:JSON.stringify(body)
    });
    if(!result.ok){
      const raw=await result.text().catch(()=>'');
      let detail:unknown;
      try{detail=JSON.parse(raw);}catch{detail=undefined;}
      const message=detail&&typeof detail==='object'&&'error' in detail&&typeof detail.error==='string'
        ?detail.error:raw.slice(0,300)||result.statusText||'HTTP '+result.status;
      throw new DominionApiError(result.status,message,detail);
    }
    return result;
  }
  async request<T>(method:'GET'|'POST'|'DELETE',path:string,body?:unknown,signal?:AbortSignal):Promise<T>{
    const response=await this.open(method,path,body,signal);
    try{return await response.json() as T;}
    catch{throw new DominionStreamError('Expected JSON response: '+path);}
  }
  readonly health=()=>this.request<{ok:boolean;engine:string;runtime:string}>('GET','/api/health');
  readonly remoteWorkers={
    submit:(prompt:string,options:{parentRunId?:string;maxTokens?:number}={})=>
      this.request<{id:string;runId:string;status:string;sourceJournal:string}>(
        'POST','/api/remote/tasks',{prompt,...options}),
    list:()=>this.request<RemoteTask[]>('GET','/api/remote/tasks'),
    get:(taskId:string)=>this.request<RemoteTask>('GET','/api/remote/tasks/'+id(taskId)),
    cancel:(taskId:string)=>this.request<{id:string;status:string}>(
      'POST','/api/remote/tasks/'+id(taskId)+'/cancel',{})
  };
  readonly status=()=>this.request<Record<string,unknown>>('GET','/api/status');
  readonly agents={
    list:()=>this.request<Agent[]>('GET','/api/agents'),
    save:(agent:Partial<Agent>&{name:string})=>this.request<Agent>('POST','/api/agents',agent),
    remove:(agentId:string)=>this.request<unknown>('DELETE','/api/agents/'+id(agentId))
  };
  readonly providers={
    list:()=>this.request<Provider[]>('GET','/api/providers'),
    save:(provider:Record<string,unknown>)=>this.request<Provider>('POST','/api/providers',provider),
    models:(providerId:string)=>this.request<unknown>('GET','/api/providers/'+id(providerId)+'/models')
  };
  readonly sessions={
    list:()=>this.request<Session[]>('GET','/api/sessions'),
    create:(data:{title?:string;agentId?:string;providerId?:string;model?:string}={})=>this.request<Session>('POST','/api/sessions',data),
    get:(sessionId:string)=>this.request<Session>('GET','/api/sessions/'+id(sessionId)),
    messages:(sessionId:string)=>this.request<unknown[]>('GET','/api/sessions/'+id(sessionId)+'/messages'),
    usage:(sessionId:string)=>this.request<unknown>('GET','/api/sessions/'+id(sessionId)+'/usage'),
    fork:(sessionId:string)=>this.request<Session>('POST','/api/sessions/'+id(sessionId)+'/fork',{}),
    compact:(sessionId:string)=>this.request<unknown>('POST','/api/sessions/'+id(sessionId)+'/compact',{}),
    cancel:(sessionId:string)=>this.request<unknown>('POST','/api/sessions/'+id(sessionId)+'/cancel',{}),
    remove:(sessionId:string)=>this.request<unknown>('DELETE','/api/sessions/'+id(sessionId))
  };
  readonly jobs={
    list:()=>this.request<Job[]>('GET','/api/jobs'),
    create:(job:CreateJob)=>this.request<Job>('POST','/api/jobs',job),
    get:(jobId:string)=>this.request<Job>('GET','/api/jobs/'+id(jobId)),
    replay:(jobId:string)=>this.request<NativeReplay>('GET','/api/jobs/'+id(jobId)+'/replay'),
    compare:(baselineId:string,candidateId:string)=>this.request<NativePairedComparison>(
      'POST','/api/jobs/'+id(baselineId)+'/compare',{candidateRunId:candidateId}),
    cancel:(jobId:string)=>this.request<Job>('POST','/api/jobs/'+id(jobId)+'/cancel',{}),
    steer:(jobId:string,instruction:string)=>this.request<{ok:boolean;jobId:string;directive:string}>('POST','/api/jobs/'+id(jobId)+'/directive',{instruction})
  };
  /** Public controls for the same Rust native jobs as the React work panel. */
  /** Canonical read-only lifecycle across Chat, Room, Team and native jobs. */
  readonly runs={
    get:(runId:string)=>this.request<RunView>('GET','/api/runs/'+id(runId))
  };
  /** Task Team is a durable DAG supervisor, not another model execution loop. */
  readonly taskTeams={
    list:()=>this.request<TeamView[]>('GET','/api/task-teams'),
    create:(plan:TeamRequest)=>this.request<TeamView>('POST','/api/task-teams',plan),
    get:(teamId:string)=>this.request<TeamView>('GET','/api/task-teams/'+id(teamId)),
    cancel:(teamId:string)=>this.request<TeamView>('POST','/api/task-teams/'+id(teamId)+'/cancel',{}),
    bridges:()=>this.request<unknown>('GET','/api/task-teams/bridges'),
    retry:(teamId:string)=>this.request<{newTeamId:string;retryOf:string;freshRun:true;status:string;team:TeamView}>(
      'POST','/api/task-teams/'+id(teamId)+'/retry',{confirm:true}),
    preview:(teamId:string,taskId:string)=>this.request<WorktreePreview>('GET',
      '/api/task-teams/'+id(teamId)+'/worktrees/'+id(taskId)+'/preview'),
    /** Stages an exact, owner-reviewed Git patch; it does not commit or push. */
    promote:(teamId:string,taskId:string,expectedDiffSha256:string)=>this.request<unknown>('POST',
      '/api/task-teams/'+id(teamId)+'/worktrees/'+id(taskId)+'/promote',
      {confirm:true,expectedDiffSha256})
  };
  readonly nativeRuns={
    list:()=>this.request<NativeRun[]>('GET','/api/sdk/runs'),
    start:(input:NativeRunRequest)=>this.request<NativeRun>('POST','/api/sdk/runs',input),
    get:(runId:string)=>this.request<NativeRun>('GET','/api/sdk/runs/'+id(runId)),
    replay:(runId:string)=>this.request<NativeReplay>('GET','/api/sdk/runs/'+id(runId)+'/replay'),
    compare:(baselineId:string,candidateId:string)=>this.request<NativePairedComparison>(
      'POST','/api/sdk/runs/'+id(baselineId)+'/compare',{candidateRunId:candidateId}),
    approve:(runId:string,callId:string)=>this.request<NativeRun>('POST','/api/sdk/runs/'+id(runId)+'/approve',{callId}),
    cancel:(runId:string)=>this.request<NativeRun>('POST','/api/sdk/runs/'+id(runId)+'/cancel',{}),
    resume:(runId:string)=>this.request<NativeRun>('POST','/api/sdk/runs/'+id(runId)+'/resume',{}),
    reconcile:(runId:string,observation:NativeReconciliation)=>this.request<NativeRun>(
      'POST','/api/sdk/runs/'+id(runId)+'/reconcile',observation),
    /** Stop at an actionable pause, not only at final completion.
     * Approval and uncertain-effect recovery need an owner decision. */
    wait:async(runId:string,options:WaitOptions={}):Promise<NativeRun>=>{
      const interval=Math.max(100,options.intervalMs??1000);
      const deadline=options.timeoutMs===undefined?Infinity:Date.now()+options.timeoutMs;
      const actionable=new Set([
        'completed','succeeded','failed','cancelled','exhausted','denied',
        'awaiting_approval','waiting_approval','in_doubt','interrupted','needs_review'
      ]);
      while(true){
        if(options.signal?.aborted)throw aborted();
        const run=await this.request<NativeRun>(
          'GET','/api/sdk/runs/'+id(runId),undefined,options.signal);
        if(actionable.has(run.status))return run;
        if(Date.now()>=deadline)throw new DominionStreamError('Native run wait timed out');
        await sleep(Math.min(interval,deadline-Date.now()),options.signal);
      }
    }
  };
  readonly workflows={
    list:()=>this.request<Workflow[]>('GET','/api/workflows'),
    save:(flow:Partial<Workflow>&{name:string;steps:WorkflowStep[]})=>this.request<Workflow>('POST','/api/workflows',flow),
    run:(workflowId:string,input='')=>this.request<Job>('POST','/api/workflows/'+id(workflowId)+'/run',{input})
  };
  readonly rooms={
    list:()=>this.request<Room[]>('GET','/api/rooms'),
    get:(roomId:string)=>this.request<Room>('GET','/api/rooms/'+id(roomId)),
    run:(roomId:string,goal:string)=>this.request<unknown>('POST','/api/rooms/'+id(roomId)+'/run',{goal}),
    stop:(roomId:string)=>this.request<unknown>('POST','/api/rooms/'+id(roomId)+'/stop',{}),
    messages:(roomId:string)=>this.request<unknown[]>('GET','/api/rooms/'+id(roomId)+'/messages'),
    events:(roomId:string,after=0,limit=100)=>{
      if(!Number.isSafeInteger(after)||after<0)throw new RangeError('Invalid Room event cursor');
      if(!Number.isSafeInteger(limit)||limit<1||limit>200)
        throw new RangeError('Room event page must contain 1-200 items');
      return this.request<RoomEventPage>('GET','/api/rooms/'+id(roomId)+
        '/events?after='+after+'&limit='+limit);
    }
  };
  readonly approvals={
    pending:()=>this.request<Approval[]>('GET','/api/approvals'),
    resolve:(approvalId:string,allow:boolean)=>this.request<{resolved:boolean;resumed?:boolean;status:string}>(
      'POST','/api/approvals',{id:approvalId,allow})
  };
  readonly executions={
    list:(limit=200,offset=0)=>this.request<unknown[]>('GET','/api/executions?limit='+limit+'&offset='+offset),
    get:(executionId:string)=>this.request<unknown>('GET','/api/executions/'+id(executionId)),
    operations:(executionId:string)=>this.request<unknown[]>('GET','/api/executions/'+id(executionId)+'/operations'),
    cancel:(executionId:string)=>this.request<unknown>('POST','/api/executions/'+id(executionId)+'/cancel',{})
  };
  private async *stream(path:string,method:'GET'|'POST',body?:unknown,signal?:AbortSignal):AsyncGenerator<unknown>{
    const response=await this.open(method,path,body,signal,true);
    if(!response.body||!response.headers.get('content-type')?.includes('text/event-stream'))
      throw new DominionStreamError('Expected SSE: '+path);
    yield* frames(response.body);
  }
  /** Uses Rust's live chat model/tool loop, including approvals. */
  async *chat(input:ChatRequest,signal?:AbortSignal):AsyncGenerator<ChatEvent>{
    let completed=false;
    for await(const value of this.stream('/api/chat','POST',input,signal)){
      const event=value as ChatEvent;
      if(event?.type)yield event;
      if(event?.type==='done'){completed=true;break;}
    }
    if(!completed&&!signal?.aborted)throw new DominionStreamError('Chat SSE ended before done');
  }
  /** Replays durable Rust bus events from exclusive sequence number. */
  async *events(options:StreamOptions={}):AsyncGenerator<EngineEvent>{
    let cursor=options.after??0;
    if(!Number.isSafeInteger(cursor)||cursor<0)throw new RangeError('Invalid event cursor');
    for await(const value of this.stream('/api/events?after='+cursor,'GET',undefined,options.signal)){
      const event=value as EngineEvent;
      if(!Number.isSafeInteger(event?.seq)||event.seq<=cursor)continue;
      cursor=event.seq;yield event;
    }
  }
  /** Automatically reconnects and deduplicates using the latest durable cursor. */
  async *watchEvents(options:StreamOptions&{retryDelayMs?:number}={}):AsyncGenerator<EngineEvent>{
    let after=options.after??0,delay=Math.max(100,options.retryDelayMs??1000);
    while(!options.signal?.aborted){
      try{
        for await(const event of this.events({...options,after})){
          after=event.seq;yield event;
        }
      }catch(error){
        if(options.signal?.aborted)return;
        if(error instanceof DominionApiError&&error.status>=400&&error.status<500)throw error;
      }
      if(!options.signal?.aborted){try{await sleep(delay,options.signal);}catch{return;}}
    }
  }
  /** Polls persisted job state; never conflates a dropped connection with success. */
  async waitJob(jobId:string,options:WaitOptions={}):Promise<Job>{
    const interval=Math.max(100,options.intervalMs??1000);
    const deadline=options.timeoutMs===undefined?Infinity:Date.now()+options.timeoutMs;
    while(true){
      if(options.signal?.aborted)throw aborted();
      const job=await this.request<Job>('GET','/api/jobs/'+id(jobId),undefined,options.signal);
      if(terminalHostStatus(job.status))return job;
      if(Date.now()>=deadline)throw new DominionStreamError('Job wait timed out');
      await sleep(Math.min(interval,deadline-Date.now()),options.signal);
    }
  }
}
export default Dominion;
