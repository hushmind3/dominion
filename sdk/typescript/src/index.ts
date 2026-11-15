/**
 * Public SDK for the SAME /api/* Axum Rust server as the Dominion GUI.
 * No second agent engine, shadow job state or /api/v1 prototype endpoints.
 */
export type JobStatus = 'queued' | 'running' | 'completed' | 'failed' | 'cancelled' | 'needs_review' | 'interrupted';
export type RecordItem = { id: string; [key: string]: unknown };
export interface Job extends RecordItem {
  kind: string; status: JobStatus; input: Record<string, unknown>;
  attempts?: number; stepIndex?: number; stepTotal?: number;
  sessionId?: string; error?: string; result?: unknown;
  steeringRevision?: number;
  steering?: Array<{revision:number;instruction:string;mode?:'append'|'replace'}>;
  autonomousState?: {phase?:string;activeGoal?:string;attempt?:number;plan?:unknown;verification?:unknown};
}
export interface Execution extends RecordItem { status: string; sessionId?: string; startedAt?: string; finishedAt?: string }
export interface Agent extends RecordItem { name: string; systemPrompt?: string; providerId?: string; model?: string }
export interface Session extends RecordItem { title: string; agentId?: string; roomId?: string; providerId?: string; model?: string }
export interface Room extends RecordItem { name: string; agentIds: string[]; sessionId?: string; rounds?: number; tokenBudget?: number }
export interface Approval extends RecordItem { status: string; sessionId?: string; tool?: string; args?: unknown }
export interface StreamEvent { type: string; [key: string]: unknown }
export interface DurableEvent extends StreamEvent { seq: number }
export interface Health { ok: boolean; service: string; version: string; runtime: string; engine: string }
export interface ChatInput {
  sessionId: string; agentId: string; input: string;
  providerId?: string; model?: string; toolsEnabled?: boolean;
  toolsExecutionEnabled?: boolean; autoLearning?: boolean;
  reasoningLanguage?: string; reasoningEffort?: string; thinking?: boolean;
}
export interface RoomMessageInput {
  input: string; clientMessageId?: string; agentIds?: string[]; attachmentIds?: string[];
  reasoningLanguage?: 'ko' | 'default'; reasoningEffort?: string;
  thinking?: boolean; providerId?: string; model?: string;
}
export interface PromptJobInput {
  kind: 'prompt';
  input: { prompt: string; toolsEnabled?: boolean; agentId?: string; providerId?: string; model?: string; reasoningEffort?: string; thinking?: boolean;
    /** Engine-owned autonomous verification/replan (default true). */ autonomous?: boolean;
    /** Existing absolute project directory, scoped to this job only. */ workspaceDir?: string;
    /** Token budget for this one job, enforced before provider calls. */ tokenBudget?: number;
    /** Bounded Rust repair loop, 1..6, default 3. */ maxAutonomousAttempts?: number;
    executionStrategy?: 'focused' | 'conservative';
  };
}
export interface WorkflowJobInput { kind: 'workflow'; input: { workflowId: string; input?: unknown } }
/** Bounded, opt-in GPU budget measured in NEW remote scene renders. */
export interface VideoJobInput {
  brief: string; title?: string; engine?: 'native'|'comfyui';
  duration?: number; aspectRatio?: '9:16'|'16:9'|'1:1';
  scenes?: string[]; continuity?: string; fps?: number;
  width?: number; height?: number; seed?: number; negativePrompt?: string;
  reuseVideoJobId?: string; regenerateScenes?: number[];
  /** 0 forbids paid new scenes, while SHA256-verified previous scenes can be reused. */
  maxNewScenes?: number;
}
export interface VideoJob extends RecordItem {
  kind: 'video'|'youtube_upload'; status: JobStatus;
  engine?: 'native'|'comfyui'; title?: string; outputUrl?: string|null;
  maxNewScenes?: number|null; result?: unknown;
  renderWorkload?: {
    frames: number; pixelFrames: number; megapixelFrames: number;
    estimatedMoneyCost: null; unit: 'pixel_frames_not_currency';
  };
}
export type CreateJobInput = PromptJobInput | WorkflowJobInput;
export interface ClientOptions { baseUrl?: string; token?: string; fetch?: typeof globalThis.fetch }
export interface RequestOptions { signal?: AbortSignal }
export interface WaitOptions extends RequestOptions { intervalMs?: number; timeoutMs?: number }
export interface EventOptions extends RequestOptions {
  after?: number; maxReconnects?: number; reconnectDelayMs?: number;
}

export class DominionError extends Error {
  constructor(public readonly status: number, message: string, public readonly details?: unknown) {
    super(message); this.name = 'DominionError';
  }
}
export class StreamInterruptedError extends Error {
  constructor() { super('Stream interrupted before completion'); this.name = 'StreamInterruptedError'; }
}
function id(value: string): string {
  if (typeof value !== 'string' || !value.length) throw new TypeError('A nonempty ID is required');
  return encodeURIComponent(value);
}
function asApiPath(path: string): string {
  // Do not allow an external URL or a path traversal to receive the local admin token.
  if (!path.startsWith('/api/') || path.includes('\\') || path.includes('..') || path.startsWith('//') || /[\r\n]/.test(path)) {
    throw new TypeError('Dominion requests must use a relative /api/* path');
  }
  return path;
}
async function fail(response: Response): Promise<never> {
  const raw = await response.text();
  let body: any;
  try { body = JSON.parse(raw); } catch { body = undefined; }
  throw new DominionError(response.status, typeof body?.error === 'string' ? body.error :
    (raw && !raw.trimStart().startsWith('<') ? raw : 'HTTP ' + response.status), body);
}
/** Shared SSE parser: split UTF-8, CRLF, comment/keepalive and multiline data. */
export async function* parseSse(response: Response): AsyncGenerator<{id?: string; data: string}> {
  if (!response.ok) await fail(response);
  if (!response.headers.get('content-type')?.toLowerCase().startsWith('text/event-stream') || !response.body)
    throw new DominionError(response.status, 'Expected a text/event-stream response');
  const reader = response.body.getReader(), decoder = new TextDecoder();
  let buffer = '';
  try {
    while (true) {
      const { done, value } = await reader.read();
      if (value) buffer += decoder.decode(value, { stream: true });
      if (done) buffer += decoder.decode();
      let boundary: RegExpExecArray | null;
      while ((boundary = /\r\n\r\n|\n\n|\r\r/.exec(buffer)) !== null) {
        const frame = buffer.slice(0, boundary.index);
        buffer = buffer.slice(boundary.index + boundary[0].length);
        let eventId: string | undefined;
        const data: string[] = [];
        for (const line of frame.split(/\r\n|\n|\r/)) {
          if (line.startsWith(':')) continue;
          if (line.startsWith('id:')) eventId = line.slice(3).replace(/^ /, '');
          if (line === 'data' || line.startsWith('data:'))
            data.push(line === 'data' ? '' : line.slice(5).replace(/^ /, ''));
        }
        if (data.length) yield { id: eventId, data: data.join('\n') };
      }
      if (buffer.length > 4 * 1024 * 1024) throw new DominionError(0, 'SSE frame exceeds 4 MiB');
      if (done) return;
    }
  } finally { await reader.cancel().catch(() => {}); reader.releaseLock(); }
}
function abortError(): Error { const e = new Error('Request aborted'); e.name = 'AbortError'; return e; }
function sleep(ms: number, signal?: AbortSignal): Promise<void> {
  return new Promise((resolve, reject) => {
    if (signal?.aborted) return reject(signal.reason ?? abortError());
    const onAbort = () => { clearTimeout(timer); reject(signal?.reason ?? abortError()); };
    const timer = setTimeout(() => { signal?.removeEventListener('abort', onAbort); resolve(); }, ms);
    signal?.addEventListener('abort', onAbort, {once:true});
  });
}
export class Dominion {
  readonly baseUrl: string;
  private token: string;
  private readonly fetcher: typeof fetch;
  constructor(options: ClientOptions = {}) {
    this.baseUrl = (options.baseUrl ?? 'http://127.0.0.1:4317').replace(/\/+$/, '');
    if (this.baseUrl && !/^https?:\/\//i.test(this.baseUrl)) throw new TypeError('baseUrl must be HTTP(S) or empty for same-origin');
    this.token = options.token?.trim() ?? '';
    this.fetcher = options.fetch ?? globalThis.fetch.bind(globalThis);
  }
  setToken(token: string): void { this.token = token.trim(); }
  /** Raw fetch is used by the GUI too, including its authorization and cache rules. */
  raw(path: string, init: RequestInit = {}): Promise<Response> {
    const headers = new Headers(init.headers);
    if (typeof init.body === 'string' && !headers.has('Content-Type')) headers.set('Content-Type', 'application/json');
    if (this.token) headers.set('X-Dominion-Token', this.token);
    return this.fetcher(this.baseUrl + asApiPath(path), {...init, headers, cache:'no-store'});
  }
  async request<T>(method: 'GET'|'POST'|'DELETE', path: string, body?: unknown, options: RequestOptions = {}): Promise<T> {
    const response = await this.raw(path, {method, signal:options.signal,
      ...(body === undefined ? {} : {body:JSON.stringify(body)})});
    if (!response.ok) await fail(response);
    const raw = await response.text();
    try { return JSON.parse(raw) as T; }
    catch { throw new DominionError(response.status, 'Server response is not JSON'); }
  }
  readonly health = () => this.request<Health>('GET','/api/health');
  readonly status = () => this.request<Record<string,unknown>>('GET','/api/status');
  readonly jobs = {
    list: (options?: RequestOptions) => this.request<Job[]>('GET','/api/jobs',undefined,options),
    get: (key: string, options?: RequestOptions) => this.request<Job>('GET','/api/jobs/'+id(key),undefined,options),
    create: (input: CreateJobInput, options?: RequestOptions) => this.request<Job>('POST','/api/jobs',input,options),
    cancel: (key: string, options?: RequestOptions) => this.request<Job>('POST','/api/jobs/'+id(key)+'/cancel',{},options),
    /** Add a new binding instruction to the current goal. */
    steer: (key: string, instruction: string, options?: RequestOptions) => this.request<{jobId:string;steeringRevision:number;mode:'append';appliesAt:string}>('POST','/api/jobs/'+id(key)+'/steer',{instruction},options),
    /** Replace unfinished work in the SAME durable Rust job. */
    replaceGoal: (key: string, instruction: string, options?: RequestOptions) => this.request<{jobId:string;steeringRevision:number;mode:'replace';appliesAt:string}>('POST','/api/jobs/'+id(key)+'/steer',{instruction,mode:'replace'},options),
  };
  readonly executions = {
    list: (limit=200,offset=0,options?:RequestOptions) => this.request<Execution[]>('GET','/api/executions?limit='+limit+'&offset='+offset,undefined,options),
    get: (key:string,options?:RequestOptions) => this.request<Execution>('GET','/api/executions/'+id(key),undefined,options),
    operations: (key:string,options?:RequestOptions) => this.request<RecordItem[]>('GET','/api/executions/'+id(key)+'/operations',undefined,options),
    cancel: (key:string,options?:RequestOptions) => this.request<Execution>('POST','/api/executions/'+id(key)+'/cancel',{},options),
  };
  readonly sessions = {
    list: (options?:RequestOptions) => this.request<Session[]>('GET','/api/sessions',undefined,options),
    create: (input:{title?:string;agentId?:string;providerId?:string;model?:string},options?:RequestOptions) => this.request<Session>('POST','/api/sessions',input,options),
    get: (key:string,options?:RequestOptions) => this.request<Session>('GET','/api/sessions/'+id(key),undefined,options),
    messages: (key:string,options?:RequestOptions) => this.request<RecordItem[]>('GET','/api/sessions/'+id(key)+'/messages',undefined,options),
    usage: (key:string,options?:RequestOptions) => this.request<unknown>('GET','/api/sessions/'+id(key)+'/usage',undefined,options),
    cancel: (key:string,options?:RequestOptions) => this.request<{cancelled:boolean}>('POST','/api/sessions/'+id(key)+'/cancel',{},options),
    fork: (key:string,options?:RequestOptions) => this.request<Session>('POST','/api/sessions/'+id(key)+'/fork',{},options),
    compact: (key:string,body:Record<string,unknown>={},options?:RequestOptions) => this.request<unknown>('POST','/api/sessions/'+id(key)+'/compact',body,options),
  };
  readonly agents = {
    list: (options?:RequestOptions) => this.request<Agent[]>('GET','/api/agents',undefined,options),
    save: (input:Partial<Agent>&{name:string},options?:RequestOptions) => this.request<Agent>('POST','/api/agents',input,options),
  };
  readonly rooms = {
    list: (options?:RequestOptions) => this.request<Room[]>('GET','/api/rooms',undefined,options),
    get: (key:string,options?:RequestOptions) => this.request<Room>('GET','/api/rooms/'+id(key),undefined,options),
    save: (input:Partial<Room>&{name:string},options?:RequestOptions) => this.request<Room>('POST','/api/rooms',input,options),
    messages: (key:string,options?:RequestOptions) => this.request<RecordItem[]>('GET','/api/rooms/'+id(key)+'/messages',undefined,options),
    /** clientMessageId is the Rust engine's durable deduplication key. */
    send: (key:string,input:RoomMessageInput,options?:RequestOptions) => this.request<{jobId:string;status:string;sessionId:string;job:Job}>('POST','/api/rooms/'+id(key)+'/messages',input,options),
    stop: (key:string,options?:RequestOptions) => this.request<unknown>('POST','/api/rooms/'+id(key)+'/stop',{},options),
  };
  readonly approvals = {
    list: (options?:RequestOptions) => this.request<Approval[]>('GET','/api/approvals',undefined,options),
    resolve: (key:string,allow:boolean,options?:RequestOptions) => this.request<{resolved:boolean;status:string}>('POST','/api/approvals',{id:key,allow},options),
  };
  readonly video = {
    status: (options?:RequestOptions) => this.request<unknown>('GET','/api/video/status',undefined,options),
    config: (options?:RequestOptions) => this.request<unknown>('GET','/api/video/config',undefined,options),
    saveConfig: (input:Record<string,unknown>,options?:RequestOptions) => this.request<unknown>('POST','/api/video/config',input,options),
    list: (options?:RequestOptions) => this.request<VideoJob[]>('GET','/api/video/jobs',undefined,options),
    get: (key:string,options?:RequestOptions) => this.request<VideoJob>('GET','/api/video/jobs/'+id(key),undefined,options),
    create: (input:VideoJobInput,options?:RequestOptions) => this.request<VideoJob>('POST','/api/video/jobs',input,options),
    outputs: (options?:RequestOptions) => this.request<unknown[]>('GET','/api/video/outputs',undefined,options),
    download: async (key:string,options?:RequestOptions) => {
      const response = await this.raw('/api/video/jobs/'+id(key)+'/content',{signal:options?.signal});
      if (!response.ok) await fail(response);
      return response.blob();
    },
  };
  readonly rsi = {
    list: (options?:RequestOptions) => this.request<RecordItem[]>('GET','/api/rsi',undefined,options),
    campaigns: (options?:RequestOptions) => this.request<RecordItem[]>('GET','/api/rsi/campaigns',undefined,options),
    campaign: (key:string,options?:RequestOptions) => this.request<RecordItem>('GET','/api/rsi/campaigns/'+id(key),undefined,options),
  };
  /** Chat POST is never retried, to avoid duplicate tool/model actions. */
  async streamChat(input:ChatInput,onEvent:(event:StreamEvent)=>void,options:RequestOptions={}):Promise<void> {
    const response=await this.raw('/api/chat',{method:'POST',body:JSON.stringify(input),signal:options.signal});
    for await (const frame of parseSse(response)) {
      const event=JSON.parse(frame.data) as StreamEvent;
      onEvent(event);
      if (event.type==='done') return;
    }
    if (options.signal?.aborted) throw options.signal.reason ?? abortError();
    throw new StreamInterruptedError();
  }
  /** Replays Rust durable events from their last sequence after transport loss. */
  async *events(options:EventOptions={}):AsyncGenerator<DurableEvent> {
    let cursor=Math.max(0,Math.floor(options.after??0));
    const maxReconnects=Math.max(0,options.maxReconnects??3);
    let attempts=0;
    while(true) {
      if(options.signal?.aborted) throw options.signal.reason??abortError();
      try {
        const response=await this.raw('/api/events?after='+cursor,{
          headers:{Accept:'text/event-stream','Last-Event-ID':String(cursor)},signal:options.signal
        });
        for await(const frame of parseSse(response)) {
          const event=JSON.parse(frame.data) as DurableEvent;
          if(!Number.isSafeInteger(event.seq)||event.seq<=cursor) continue;
          cursor=event.seq;
          attempts=0;
          yield event;
        }
      } catch(error) {
        if(options.signal?.aborted) throw options.signal.reason??abortError();
        if(error instanceof DominionError&&error.status>=400&&error.status<500) throw error;
        if(attempts>=maxReconnects) throw error;
      }
      if(options.signal?.aborted) throw options.signal.reason??abortError();
      if(attempts>=maxReconnects) throw new StreamInterruptedError();
      attempts++;
      await sleep(Math.min(30000,(options.reconnectDelayMs??500)*2**(attempts-1)),options.signal);
    }
  }
  /** Durable state is server-owned, so callers can call this after a new process starts. */
  async waitForJob(key:string,options:WaitOptions={}):Promise<Job> {
    const interval=Math.max(100,options.intervalMs??800);
    const deadline=options.timeoutMs==null?Infinity:Date.now()+Math.max(0,options.timeoutMs);
    while(true) {
      if(options.signal?.aborted) throw options.signal.reason??abortError();
      const job=await this.jobs.get(key,options);
      if(['completed','failed','cancelled','needs_review','interrupted'].includes(job.status)) return job;
      const remaining=deadline-Date.now();
      if(remaining<=0) throw new DominionError(408,'Timed out waiting for durable job');
      await sleep(Math.min(interval,remaining),options.signal);
    }
  }
}
export default Dominion;
