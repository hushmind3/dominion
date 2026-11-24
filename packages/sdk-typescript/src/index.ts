/**
 * Optional TypeScript client for Dominion's shared native Rust runtime.
 * All control flow, policy, execution and journals remain in the Rust server.
 */
export type SdkRunStatus =
  | 'queued' | 'running' | 'awaiting_approval' | 'in_doubt'
  | 'interrupted' | 'completed' | 'cancelled' | 'exhausted'
  | 'denied' | 'failed' | 'needs_review';


/** Identical execution identity emitted by the SQLite event bus and SDK. */
export interface RunEventRef {
  schemaVersion: number;
  executionId: string;
  sessionId?: string;
  jobId?: string;
  parentExecutionId?: string;
  externalRunId?: string;
  taskId?: string;
  roomId?: string;
  agentId?: string;
  sourceMessageId?: string;
  /** Native execution lifecycle; this is not the SDK presentation status. */
  lifecycle?: 'queued' | 'running' | 'awaiting_approval' | 'paused'
    | 'needs_review' | 'interrupted' | 'completed' | 'failed' | 'cancelled';
}
export interface RunEventEnvelope {
  seq: number;
  type: string;
  runRef: RunEventRef;
  [field: string]: unknown;
}
export interface RunEventPage {
  schemaVersion: number;
  executionId: string;
  after: number;
  nextAfter: number;
  hasMore: boolean;
  earliestAvailableSeq: number | null;
  historyTruncated: boolean;
  events: RunEventEnvelope[];
}
/** Remote workers have independent native SDKs; a report is NOT a core receipt. */
export interface RemoteJob {
  id: string;
  kind: 'remote_sdk';
  instruction: string;
  status: 'queued' | 'leased' | 'needs_review' | 'owner_accepted'
    | 'owner_rejected' | 'cancelled';
  workerId?: string;
  targetWorkerId?: string | null;
  requiredCapabilities?: string[];
  leaseExpiresAt?: string;
  workerReport?: Record<string, unknown>;
  nativeVerified: false;
}
export interface RemoteJobRequest {
  instruction: string;
  verifyCommand?: string;
  maxRounds?: number;
  targetWorkerId?: string;
  requiredCapabilities?: string[];
}
/** Worker identity is configured by the owner, not self-attested by a model. */
export interface RemoteWorker {
  id: string;
  kind: 'remote_worker';
  status: 'enabled' | 'disabled';
  capabilities: string[];
  protocolVersion: number;
  registeredAt: string;
  lastSeenAt?: string | null;
}
/** One-time response; neither list nor event history returns this plaintext. */
export interface RemoteWorkerEnrollment {
  worker: RemoteWorker;
  token: string;
}
/** Untrusted cross-host Git patch: owner-reviewed evidence, never a core receipt. */
export interface RemoteArtifactManifest {
  id: string;
  jobId: string;
  workerId: string;
  baseCommit: string;
  patchSha256: string;
  patchBytes: number;
  status: 'quarantined' | 'owner_accepted' | 'owner_rejected';
  nativeVerified: false;
}
export interface RemoteArtifactInspection {
  manifest: RemoteArtifactManifest;
  patch: string;
}

/** Room-local committed delivery facts; seq is NOT the global SSE seq. */
export interface RoomEvent {
  schemaVersion: number;
  roomId: string;
  seq: number;
  eventId: string;
  author: string;
  replyTo: string | null;
  originRun: string | null;
  kind: 'owner_message' | 'agent_reply';
  createdAt: string;
}
export interface RoomEventPage {
  schemaVersion: number;
  roomId: string;
  after: number;
  nextAfter: number;
  hasMore: boolean;
  events: RoomEvent[];
}

export interface RunEventsOptions {
  after?: number;
  limit?: number;
}

export interface ToolCall {
  id: string;
  name: string;
  arguments: Record<string, unknown>;
}
export interface ModelUsage {
  prompt_tokens: number | null;
  completion_tokens: number | null;
  cached_prompt_tokens: number | null;
  estimated?: boolean;
}
export interface RunCheckpoint {
  state: { state: string; detail?: Record<string, unknown> };
  rounds: number;
  toolCalls: number;
  usage: ModelUsage | null;
  pendingCall: ToolCall | null;
  events: Array<{ kind: string; detail: unknown }>;
  answer: string | null;
  taskOutcome?: 'succeeded' | 'failed' | 'cancelled' | 'answered_unverified' | null;
  verified?: boolean;
  acceptancePassed?: boolean | null;
  failureClass?: string | null;
}
export interface RunActions {
  approve: boolean; deny: boolean; cancel: boolean; resume: boolean;
  reconcile: boolean; steer: boolean; refresh: boolean;
}
export interface AgentRun {
  actions?: RunActions;
  uncertainEffectCount?: number;
  sessionId?: string;
  readOnly?: boolean;
  id: string;
  status: SdkRunStatus;
  instruction: string;
  providerId?: string;
  model?: string;
  answer?: string;
  error?: string;
  /** True only after the Rust acceptance evaluator proved task success. */
  verified?: boolean;
  taskOutcome?: 'succeeded' | 'failed' | 'cancelled' | 'answered_unverified' | null;
  acceptancePassed?: boolean | null;
  failureClass?: string | null;
  evidenceExecutionId?: string;
  /** Shared native execution identity, when an execution has been admitted. */
  runRef?: RunEventRef;
  checkpoint?: RunCheckpoint | null;
}
export interface RunSummary {
  id: string; status: SdkRunStatus; sessionId?: string | null;
  source?: {kind: string; id: string} | null; taskOutcome?: AgentRun['taskOutcome'];
  verified?: boolean | null; createdAt?: string;
}
export interface InferenceRoute { providerId: string; model: string }
export interface InferenceSupport extends InferenceRoute { thinkingToggle: boolean; reasoningEfforts: string[] }
export interface ExtensionManifest {
  schemaVersion: 1; id: string; name: string; version: string; description?: string;
}
export interface ExtensionRegistration {
  id: string; manifest: ExtensionManifest; enabled: boolean;
}
export interface StartRun {
  instruction: string;
  providerId?: string;
  model?: string;
  agentId?: string;
  idempotencyKey?: string;
  source?: {kind: string; id: string};
  verifyCommand?: string;
  maxRounds?: number;
  workspaceDir?: string;
  tokenBudget?: number;
  budgetUsd?: number;
  shellSandbox?: 'required' | 'off';
}

export interface WaitRunOptions {
  /** Maximum elapsed time. A timeout never cancels the remote run. */
  timeoutMs?: number;
  /** Poll interval for durable checkpoints. */
  intervalMs?: number;
  signal?: AbortSignal;
  /** Called once for each authoritative snapshot received from Rust. */
  onUpdate?: (run: AgentRun) => void;
}

/** Returns when work is finished or the owner must take action. */
export const isRunSettled = (run: AgentRun): boolean =>
  run.status !== 'queued' && run.status !== 'running';

export interface ClientOptions {
  baseUrl?: string;
  adminToken?: string;
  fetch?: typeof fetch;
}

export class DominionSdkError extends Error {
  constructor(
    message: string,
    public readonly status: number,
  ) {
    super(message);
    this.name = 'DominionSdkError';
  }
}

export class DominionAgentClient {
  private readonly baseUrl: string;
  private readonly adminToken?: string;
  private readonly transport: typeof fetch;

  constructor(options: ClientOptions = {}) {
    this.baseUrl = (options.baseUrl ?? 'http://127.0.0.1:4317').replace(/\/$/, '');
    this.adminToken = options.adminToken;
    this.transport = options.fetch ?? fetch;
  }

  private async request<T>(
    method: 'GET' | 'POST' | 'DELETE',
    path: string,
    data?: unknown,
    signal?: AbortSignal,
  ): Promise<T> {
    const headers: Record<string, string> = {};
    if (data !== undefined) headers['content-type'] = 'application/json';
    if (this.adminToken) headers['x-dominion-token'] = this.adminToken;
    const response = await this.transport(this.baseUrl + path, {
      method, headers, body: data === undefined ? undefined : JSON.stringify(data), signal, redirect: 'error',
    });
    const raw = await response.text();
    let decoded: unknown;
    try { decoded = raw ? JSON.parse(raw) : null; }
    catch {
      throw new DominionSdkError('Dominion returned a non-JSON response', response.status);
    }
    if (!response.ok) {
      const detail = decoded && typeof decoded === 'object' && 'error' in decoded
        ? String((decoded as {error: unknown}).error) : `HTTP ${response.status}`;
      throw new DominionSdkError(detail, response.status);
    }
    return decoded as T;
  }

  listRemoteWorkers(signal?: AbortSignal): Promise<RemoteWorker[]> {
    return this.request('GET','/api/remote/workers',undefined,signal);
  }
  enrollRemoteWorker(workerId:string,capabilities:string[]=['sdk'],
      signal?:AbortSignal):Promise<RemoteWorkerEnrollment> {
    return this.request('POST','/api/remote/workers',{workerId,capabilities},signal);
  }
  rotateRemoteWorkerToken(id:string,signal?:AbortSignal):Promise<RemoteWorkerEnrollment> {
    return this.request('POST',
      `/api/remote/workers/${encodeURIComponent(id)}/rotate`,{},signal);
  }
  setRemoteWorkerEnabled(id:string,enabled:boolean,signal?:AbortSignal):Promise<RemoteWorker> {
    return this.request('POST',
      `/api/remote/workers/${encodeURIComponent(id)}/state`,{enabled},signal);
  }
  /** Submit work for an explicitly connected remote DOMINION Rust SDK host. */
  createRemoteJob(input: RemoteJobRequest, signal?: AbortSignal): Promise<RemoteJob> {
    return this.request('POST','/api/remote/jobs',input,signal);
  }
  listRemoteJobs(signal?: AbortSignal): Promise<RemoteJob[]> {
    return this.request('GET','/api/remote/jobs',undefined,signal);
  }
  getRemoteJob(id: string, signal?: AbortSignal): Promise<RemoteJob> {
    return this.request('GET',
      `/api/remote/jobs/${encodeURIComponent(id)}`,undefined,signal);
  }
  cancelRemoteJob(id: string, signal?: AbortSignal): Promise<RemoteJob> {
    return this.request('POST',
      `/api/remote/jobs/${encodeURIComponent(id)}/cancel`,{},signal);
  }
  /** Mark expired remote leases for review, never retry unknown effects. */
  reconcileRemoteLeases(signal?: AbortSignal): Promise<{count:number;needsReview:string[]}> {
    return this.request('POST','/api/remote/reconcile',{},signal);
  }
  /** Owner review acknowledges a report; it NEVER grants nativeVerified. */
  reviewRemoteJob(id: string, accept: boolean, signal?: AbortSignal): Promise<RemoteJob> {
    return this.request('POST',
      `/api/remote/jobs/${encodeURIComponent(id)}/review`,{accept},signal);
  }

  /** Read the immutable quarantined diff before deciding to accept it. */
  getRemoteArtifact(id:string,signal?:AbortSignal):Promise<RemoteArtifactInspection> {
    return this.request('GET',
      `/api/remote/jobs/${encodeURIComponent(id)}/artifact`,undefined,signal);
  }
  /** An owner's acceptance records provenance; it never merges or executes code. */
  reviewRemoteArtifact(id:string,accept:boolean,
      signal?:AbortSignal):Promise<RemoteArtifactManifest> {
    return this.request('POST',
      `/api/remote/jobs/${encodeURIComponent(id)}/artifact/review`,{accept},signal);
  }

  runsBySource(kind: string, signal?: AbortSignal): Promise<RunSummary[]> {
    return this.request('GET', `/api/sdk/runs/source/${encodeURIComponent(kind)}`, undefined, signal);
  }
  inferenceSupport(route: InferenceRoute, signal?: AbortSignal): Promise<InferenceSupport> {
    return this.request('GET', '/api/chat/inference-support?' + new URLSearchParams({providerId:route.providerId,model:route.model}).toString(), undefined, signal);
  }
  inferenceSupportBatch(routes: InferenceRoute[], signal?: AbortSignal): Promise<InferenceSupport[]> {
    return this.request('POST', '/api/chat/inference-support', routes, signal);
  }
  listRuns(signal?: AbortSignal): Promise<AgentRun[]> {
    return this.request('GET', '/api/sdk/runs', undefined, signal);
  }
  getRun(id: string, signal?: AbortSignal): Promise<AgentRun> {
    return this.request('GET', `/api/sdk/runs/${encodeURIComponent(id)}`, undefined, signal);
  }

  /** Durable room-local event cursor, independently of global run seq. */
  getRoomEvents(
    roomId: string, options: RunEventsOptions = {}, signal?: AbortSignal,
  ): Promise<RoomEventPage> {
    const params = new URLSearchParams();
    if (options.after !== undefined) params.set('after', String(options.after));
    if (options.limit !== undefined) params.set('limit', String(options.limit));
    const query = params.toString();
    return this.request('GET',
      `/api/rooms/${encodeURIComponent(roomId)}/events${query ? `?${query}` : ''}`,
      undefined, signal);
  }

  /** Replay a durable, execution-scoped timeline without guessing a job/room ID. */
  getExecutionEvents(
    executionId: string, options: RunEventsOptions = {}, signal?: AbortSignal,
  ): Promise<RunEventPage> {
    const params = new URLSearchParams();
    if (options.after !== undefined) params.set('after', String(options.after));
    if (options.limit !== undefined) params.set('limit', String(options.limit));
    const query = params.toString();
    return this.request('GET',
      `/api/executions/${encodeURIComponent(executionId)}/events${query ? `?${query}` : ''}`,
      undefined, signal);
  }

  /** Resolve an SDK facade run to its independently recorded native execution. */
  async getRunEvents(
    sdkRunId: string, options: RunEventsOptions = {}, signal?: AbortSignal,
  ): Promise<RunEventPage> {
    const run = await this.getRun(sdkRunId, signal);
    const executionId = run.runRef?.executionId ?? run.evidenceExecutionId;
    if (!executionId) {
      throw new DominionSdkError('Native execution has not been admitted yet', 409);
    }
    return this.getExecutionEvents(executionId, options, signal);
  }

  listExtensions(signal?:AbortSignal):Promise<ExtensionRegistration[]> {
    return this.request('GET','/api/extensions/apps',undefined,signal);
  }
  registerExtension(manifest:ExtensionManifest,signal?:AbortSignal):Promise<ExtensionRegistration>{
    return this.request('POST','/api/extensions/apps',manifest,signal);
  }
  setExtensionEnabled(id:string,enabled:boolean,signal?:AbortSignal):Promise<ExtensionRegistration>{
    return this.request('POST','/api/extensions/apps/'+encodeURIComponent(id)+'/state',{enabled},signal);
  }
  async unregisterExtension(id:string,signal?:AbortSignal):Promise<void>{
    await this.request('DELETE','/api/extensions/apps/'+encodeURIComponent(id),undefined,signal);
  }
  startExtensionRun(id:string,input:Omit<StartRun,'source'>,signal?:AbortSignal):Promise<AgentRun>{
    return this.startRun({...input,source:{kind:'extension',id}},signal);
  }
  startRun(input: StartRun, signal?: AbortSignal): Promise<AgentRun> {
    return this.request('POST', '/api/sdk/runs', input, signal);
  }
  steerRun(id: string, instruction: string, mode: 'append' | 'replace' = 'append', signal?: AbortSignal): Promise<{id: string; revision: number; appliesAt: string}> {
    return this.request('POST', `/api/sdk/runs/${encodeURIComponent(id)}/steer`, {instruction, mode}, signal);
  }
  approveCall(id: string, callId: string, signal?: AbortSignal): Promise<AgentRun> {
    return this.request('POST', `/api/sdk/runs/${encodeURIComponent(id)}/approve`,
      {callId}, signal);
  }
  async denyCall(id: string, callId: string, signal?: AbortSignal): Promise<AgentRun> {
    const current = await this.getRun(id, signal);
    if (!current.actions?.deny || current.checkpoint?.pendingCall?.id !== callId)
      throw new DominionSdkError('Not the run’s pending approval', 409);
    const result = await this.request<{status:string}>('POST','/api/approvals',{id:callId,allow:false},signal);
    if (result.status !== 'denied') throw new DominionSdkError('Approval was not denied',409);
    return this.getRun(id,signal);
  }
  reconcileRun(
    id: string,
    observation: {applied: boolean; details: string},
    signal?: AbortSignal,
  ): Promise<AgentRun> {
    return this.request('POST', `/api/sdk/runs/${encodeURIComponent(id)}/reconcile`,
      observation, signal);
  }
  resumeRun(id: string, signal?: AbortSignal): Promise<AgentRun> {
    return this.request('POST', `/api/sdk/runs/${encodeURIComponent(id)}/resume`,
      {}, signal);
  }
  cancelRun(id: string, signal?: AbortSignal): Promise<AgentRun> {
    return this.request('POST', `/api/sdk/runs/${encodeURIComponent(id)}/cancel`,
      {}, signal);
  }

  /**
   * Waits for a terminal snapshot or an owner-action boundary, without
   * inventing completion when approval or effect reconciliation is required.
   */
  async waitForRun(id: string, options: WaitRunOptions = {}): Promise<AgentRun> {
    const timeoutMs = options.timeoutMs ?? 30 * 60_000;
    const intervalMs = options.intervalMs ?? 1_000;
    if (!Number.isFinite(timeoutMs) || timeoutMs < 0 || timeoutMs > 2_147_483_647 ||
        !Number.isFinite(intervalMs) || intervalMs < 0 || intervalMs > 2_147_483_647) {
      throw new DominionSdkError('Invalid wait duration', 400);
    }
    const deadline = Date.now() + timeoutMs;
    const controller = new AbortController();
    const timeout = new DominionSdkError('Dominion run wait timed out; the remote run is still durable', 408);
    const callerAbort = () => controller.abort(options.signal?.reason);
    options.signal?.throwIfAborted();
    options.signal?.addEventListener('abort', callerAbort, {once:true});
    const timer = setTimeout(() => controller.abort(timeout), timeoutMs);
    try {
      for (;;) {
        controller.signal.throwIfAborted();
        const snapshot = await this.getRun(id, controller.signal);
        controller.signal.throwIfAborted();
        options.onUpdate?.(snapshot);
        if (isRunSettled(snapshot)) return snapshot;
        const remaining = deadline - Date.now();
        if (remaining <= 0) throw timeout;
        await new Promise<void>((resolve,reject) => {
          const signal = controller.signal;
          if (signal.aborted) { reject(signal.reason); return; }
          const abort = () => { clearTimeout(delay); reject(signal.reason); };
          const delay = setTimeout(() => {
            signal.removeEventListener('abort',abort);
            resolve();
          }, Math.min(Math.max(50,intervalMs),remaining));
          signal.addEventListener('abort',abort,{once:true});
        });
      }
    } catch (error) {
      if (controller.signal.aborted) throw controller.signal.reason;
      throw error;
    } finally {
      clearTimeout(timer);
      options.signal?.removeEventListener('abort',callerAbort);
    }
  }

  /** Strict success gate: a text reply alone never constitutes verified work. */
  async waitForVerifiedRun(id: string, options: WaitRunOptions = {}): Promise<AgentRun> {
    const run = await this.waitForRun(id, options);
    if (run.status === 'completed' && run.taskOutcome === 'succeeded'
        && run.verified === true) return run;
    throw new DominionSdkError(
      run.error ?? `Dominion task is ${run.status}; verified success evidence is missing`,
      ['awaiting_approval','in_doubt','interrupted','needs_review'].includes(run.status) ? 409 : 422,
    );
  }
}
