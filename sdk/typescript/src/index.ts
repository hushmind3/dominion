export interface TaskInput {
  prompt: string;
  /** Reuse this identifier after a lost HTTP response. */
  requestId?: string;
  /** Connect successive tasks to the same native memory transcript. */
  sessionId?: string | null;
  /** Explicit permission grants for this run. Both default to false. */
  allowWrite?: boolean;
  allowShell?: boolean;
  /** Explicit consent to expose operator-configured MCP tools in this run. */
  allowMcp?: boolean;
  /** Require a verified file write before reporting successful completion. */
  requireEvidence?: boolean;
  maxSteps?: number;
  tokenBudget?: number;
  /** Extra model planning turn only when needed (auto), always, or never. */
  planningMode?: 'auto' | 'always' | 'never';
}
export type TaskStatus = "queued" | "running" | "completed" |
  "failed" | "cancelled" | "needs_review";
export interface TaskRecord {
  id: string;
  requestId: string;
  input: TaskInput;
  status: TaskStatus;
  steps: number;
  tokens: number;
  error: string | null;
  result: {answer: string; verifiedWrites: number; verification: string} | null;
  cancelRequested: boolean;
  createdAt: string;
}
export interface TaskEvent {
  seq: number;
  taskId: string;
  type: string;
  detail: Record<string, unknown>;
}
export interface GroupParticipant {
  name: string;
  role: string;
  economy: boolean;
}
export interface GroupMessage {
  speaker: string;
  content: string;
  at_ms: number;
  model: string | null;
}
export interface GroupRoom {
  id: string;
  topic: string;
  participants: GroupParticipant[];
  messages: GroupMessage[];
  status: 'open' | 'finished';
  rounds: number;
  max_rounds: number;
  cost_krw: number;
  budget_krw: number;
}
export interface CreateGroupInput {
  topic: string;
  participants: GroupParticipant[];
  max_rounds?: number;
  budget_krw?: number;
}

export interface VideoScene { heading: string; narration: string; seconds: number }
export type VideoBackend = 'local' | 'veo';
export type VideoStatus = 'draft' | 'rendering' | 'completed' | 'failed';
export interface VideoRecord {
  id: string;
  prompt: string;
  scenes: VideoScene[];
  status: VideoStatus;
  backend: VideoBackend;
  cost_krw: number;
  output: string | null;
  error: string | null;
  render_seconds: number;
}
export interface VideoRequest {
  prompt: string;
  scenes?: VideoScene[];
  backend?: VideoBackend;
}
export interface ImprovementRequest {
  repository: string;
  target_file: string;
  instruction: string;
  test_command: string;
  score_command?: string | null;
  parent?: string | null;
}
export interface ImprovementMeasurement {
  tests_passed: boolean;
  score: number | null;
  log: string;
}
export interface ImprovementTrial {
  id: string;
  repository: string;
  target_file: string;
  instruction: string;
  status: 'awaiting_approval' | 'running' | 'evaluated' | 'rejected' | 'adopted' | 'failed';
  base_commit: string | null;
  candidate_commit: string | null;
  baseline: ImprovementMeasurement | null;
  candidate: ImprovementMeasurement | null;
  accepted: boolean;
  cost_krw: number;
  error: string | null;
  parent: string | null;
  reflection: string | null;
}
export type MemoryKind = 'fact' | 'correction' | 'preference' | 'procedure';
export interface MemoryRecord {
  id: string;
  sessionId: string | null;
  content: string;
  source: 'owner';
  kind: MemoryKind;
  status: 'active' | 'inactive';
  createdAt: string;
}
export interface SkillRecord {
  id: string;
  name: string;
  trigger: string;
  instructions: string;
  enabled: boolean;
  createdAt: string;
}
export interface ClientOptions {
  baseUrl?: string;
  fetch?: typeof fetch;
  /** A user-provided AbortSignal also stops a read-only event watch. */
  signal?: AbortSignal;
}
export class DominionError extends Error {
  constructor(readonly status: number, message: string) {
    super(message);
    this.name = "DominionError";
  }
}
export class DominionClient {
  private readonly base: string;
  private readonly fetcher: typeof fetch;
  constructor(options: ClientOptions = {}) {
    const u = new URL(options.baseUrl ?? "http://127.0.0.1:4317");
    if (!["http:", "https:"].includes(u.protocol) || u.username || u.password || u.search || u.hash) {
      throw new TypeError("An HTTP(S) base URL without credentials is required");
    }
    this.base = u.toString().replace(/\/+$/, "");
    this.fetcher = options.fetch ?? fetch;
  }
  private async request<T>(path: string, method = "GET", data?: unknown, signal?: AbortSignal): Promise<T> {
    const response = await this.fetcher(this.base + path, {
      method, signal, cache: "no-store",
      headers: {"Accept":"application/json", ...(data===undefined?{}:{"Content-Type":"application/json"})},
      body: data===undefined?undefined:JSON.stringify(data)
    });
    const text = await response.text();
    let payload: unknown;
    try { payload=JSON.parse(text); }
    catch { throw new DominionError(response.status,"Invalid JSON response"); }
    if (!response.ok) {
      const message = payload && typeof payload === "object" && "error" in payload
        && typeof payload.error === "string" ? payload.error : "HTTP " + response.status;
      throw new DominionError(response.status,message);
    }
    return payload as T;
  }
  health(signal?: AbortSignal) {
    return this.request<{ok:boolean;engine:string;model:string;localOnly:boolean}>("/api/health","GET",undefined,signal);
  }
  /** A POST is not automatically retried. A repeated requestId resolves to the original task. */
  submit(input: TaskInput, signal?: AbortSignal): Promise<TaskRecord> {
    if (!input || typeof input.prompt !== "string" || !input.prompt.trim())
      throw new TypeError("A non-empty prompt is required");
    const requestId = input.requestId ?? crypto.randomUUID();
    if (!/^[A-Za-z0-9_-]{8,128}$/.test(requestId))
      throw new TypeError("requestId must be 8-128 ASCII letters, numbers, _ or -");
    return this.request<TaskRecord>("/api/tasks","POST",{...input,requestId},signal);
  }
  list(signal?: AbortSignal) { return this.request<TaskRecord[]>("/api/tasks","GET",undefined,signal); }
  get(id:string, signal?: AbortSignal) {
    return this.request<TaskRecord>("/api/tasks/"+encodeURIComponent(id),"GET",undefined,signal);
  }
  cancel(id:string, signal?: AbortSignal) {
    return this.request<TaskRecord>("/api/tasks/"+encodeURIComponent(id)+"/cancel","POST",{},signal);
  }
  steer(id:string,text:string,signal?: AbortSignal) {
    if (!text.trim()) throw new TypeError("Owner instruction is empty");
    return this.request<TaskRecord>("/api/tasks/"+encodeURIComponent(id)+"/steer","POST",{text},signal);
  }
  /** Native Rust participant room. The TS client performs no speaker selection. */
  groupList(signal?: AbortSignal) {
    return this.request<GroupRoom[]>('/api/groups','GET',undefined,signal);
  }
  groupCreate(input: CreateGroupInput, signal?: AbortSignal) {
    return this.request<GroupRoom>('/api/groups','POST',input,signal);
  }
  groupGet(id: string, signal?: AbortSignal) {
    return this.request<GroupRoom>('/api/groups/'+encodeURIComponent(id),'GET',undefined,signal);
  }
  groupSay(id: string, content: string, signal?: AbortSignal) {
    return this.request<GroupRoom>('/api/groups/'+encodeURIComponent(id)+'/messages','POST',{content},signal);
  }
  groupNext(id: string, signal?: AbortSignal) {
    return this.request<GroupRoom>('/api/groups/'+encodeURIComponent(id)+'/next','POST',{},signal);
  }
  videoList(signal?: AbortSignal) {
    return this.request<VideoRecord[]>('/api/videos','GET',undefined,signal);
  }
  videoCreate(input: VideoRequest, signal?: AbortSignal) {
    return this.request<VideoRecord>('/api/videos','POST',input,signal);
  }
  videoGet(id: string, signal?: AbortSignal) {
    return this.request<VideoRecord>('/api/videos/'+encodeURIComponent(id),'GET',undefined,signal);
  }
  videoEditScene(id: string, index: number, scene: VideoScene, signal?: AbortSignal) {
    if (!Number.isSafeInteger(index) || index < 0) throw new TypeError('Invalid scene index');
    return this.request<VideoRecord>(
      '/api/videos/'+encodeURIComponent(id)+'/scenes/'+index,'POST',scene,signal);
  }
  videoRender(id: string, signal?: AbortSignal) {
    return this.request<VideoRecord>('/api/videos/'+encodeURIComponent(id)+'/render','POST',{},signal);
  }
  videoDownloadUrl(id: string) {
    return this.base + '/api/videos/'+encodeURIComponent(id)+'/content';
  }
  improvementList(signal?: AbortSignal) {
    return this.request<ImprovementTrial[]>('/api/improvements','GET',undefined,signal);
  }
  improvementCreate(input: ImprovementRequest, signal?: AbortSignal) {
    return this.request<ImprovementTrial>('/api/improvements','POST',input,signal);
  }
  improvementGet(id: string, signal?: AbortSignal) {
    return this.request<ImprovementTrial>('/api/improvements/'+encodeURIComponent(id),'GET',undefined,signal);
  }
  improvementEvaluate(id: string, signal?: AbortSignal) {
    return this.request<ImprovementTrial>('/api/improvements/'+encodeURIComponent(id)+'/evaluate','POST',{},signal);
  }
  improvementAdopt(id: string, signal?: AbortSignal) {
    return this.request<ImprovementTrial>('/api/improvements/'+encodeURIComponent(id)+'/adopt','POST',{},signal);
  }
  improvementDescendant(id: string, signal?: AbortSignal) {
    return this.request<ImprovementTrial>('/api/improvements/'+encodeURIComponent(id)+'/descendant','POST',{},signal);
  }
  memoryList(sessionId?: string, signal?: AbortSignal) {
    const query=sessionId ? '?sessionId='+encodeURIComponent(sessionId):'';
    return this.request<MemoryRecord[]>('/api/memory'+query,'GET',undefined,signal);
  }
  memorySearch(query: string, sessionId?: string, signal?: AbortSignal) {
    const params=new URLSearchParams({query});
    if(sessionId) params.set('sessionId',sessionId);
    return this.request<MemoryRecord[]>('/api/memory/search?'+params,'GET',undefined,signal);
  }
  remember(content: string, kind: MemoryKind='fact', sessionId?: string, signal?: AbortSignal) {
    return this.request<MemoryRecord>('/api/memory','POST',
      {content,kind,sessionId:sessionId??null},signal);
  }
  memoryDisable(id: string, signal?: AbortSignal) {
    return this.request<{ok:boolean}>('/api/memory/'+encodeURIComponent(id)+'/disable','POST',{},signal);
  }
  skillList(signal?: AbortSignal) {
    return this.request<SkillRecord[]>('/api/skills','GET',undefined,signal);
  }
  skillCreate(name: string, instructions: string, trigger='', enabled=false, signal?: AbortSignal) {
    return this.request<SkillRecord>('/api/skills','POST',
      {name,instructions,trigger,enabled},signal);
  }
  skillEnable(id:string, enabled:boolean, signal?: AbortSignal) {
    return this.request<{ok:boolean}>('/api/skills/'+encodeURIComponent(id)+'/enable',
      'POST',{enabled},signal);
  }
  /**
   * Reconnect only a read-only GET. The cursor is based on committed event seq;
   * reconnecting this watch cannot repeat model calls or filesystem effects.
   */
  async *watch(id:string, options: {after?:number;signal?:AbortSignal;reconnect?:boolean} = {}):
    AsyncGenerator<TaskEvent> {
    let cursor = options.after ?? 0;
    if (!Number.isSafeInteger(cursor) || cursor < 0) throw new TypeError("Invalid event cursor");
    const reconnect = options.reconnect ?? true;
    let attempts = 0;
    while (!options.signal?.aborted) {
      let response: Response;
      try {
        response=await this.fetcher(this.base+"/api/tasks/"+encodeURIComponent(id)+"/events?after="+cursor,
          {headers:{Accept:"text/event-stream"},signal:options.signal,cache:"no-store"});
        if (!response.ok) throw new DominionError(response.status,"Event connection failed");
        if (!response.headers.get("content-type")?.startsWith("text/event-stream") || !response.body)
          throw new Error("Expected an event stream");
        const reader = response.body.getReader();
        const decoder = new TextDecoder();
        let buffer = "";
        try {
          while (true) {
            const next = await reader.read();
            if (next.done) break;
            buffer += decoder.decode(next.value,{stream:true});
            if (buffer.length>2_000_000) throw new Error("Event frame too large");
            let match:RegExpExecArray|null;
            while ((match=/\r\n\r\n|\n\n|\r\r/.exec(buffer))) {
              const frame=buffer.slice(0,match.index);
              buffer=buffer.slice(match.index+match[0].length);
              const payload=frame.split(/\r\n|\r|\n/).filter(s=>s.startsWith("data:"))
                .map(s=>s.slice(5).trimStart()).join("\n");
              if (!payload) continue;
              const event=JSON.parse(payload) as TaskEvent;
              if (!event || !Number.isSafeInteger(event.seq) || event.seq<1 ||
                  event.taskId!==id || typeof event.type!=="string") throw new Error("Invalid durable event");
              if (event.seq<=cursor) continue;
              cursor=event.seq;
              attempts=0;
              yield event;
            }
          }
        } finally { await reader.cancel().catch(()=>{});reader.releaseLock(); }
      } catch (error) {
        if (options.signal?.aborted) return;
        if (error instanceof DominionError && error.status>=400 && error.status<500) throw error;
        if (!reconnect) throw error;
      }
      if (!reconnect || options.signal?.aborted) return;
      if (++attempts>8) throw new Error("Event reconnect limit exceeded; refresh task state");
      await new Promise<void>(resolve=>setTimeout(resolve,Math.min(2500,250*2**Math.min(attempts,4))));
    }
  }
}

export { DominionRuntime, DominionThread } from './runtime.js';
export type { ThreadOptions, RunOptions, RunResult, StreamedRun } from './runtime.js';
