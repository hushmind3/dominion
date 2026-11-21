/**
 * Optional TypeScript client for Dominion's shared native Rust runtime.
 * All control flow, policy, execution and journals remain in the Rust server.
 */
export type SdkRunStatus =
  | 'queued' | 'running' | 'awaiting_approval' | 'in_doubt'
  | 'interrupted' | 'completed' | 'cancelled' | 'exhausted'
  | 'denied' | 'failed';


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
  prompt_tokens: number;
  completion_tokens: number;
  cached_prompt_tokens: number;
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
export interface AgentRun {
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
export interface StartRun {
  instruction: string;
  providerId?: string;
  model?: string;
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
  new Set<SdkRunStatus>([
    'completed', 'failed', 'cancelled', 'exhausted', 'denied',
    'awaiting_approval', 'in_doubt', 'interrupted',
  ]).has(run.status);

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
    method: 'GET' | 'POST',
    path: string,
    data?: unknown,
    signal?: AbortSignal,
  ): Promise<T> {
    const headers: Record<string, string> = {};
    if (data !== undefined) headers['content-type'] = 'application/json';
    if (this.adminToken) headers['x-dominion-token'] = this.adminToken;
    const response = await this.transport(this.baseUrl + path, {
      method, headers, body: data === undefined ? undefined : JSON.stringify(data), signal,
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

  listRuns(signal?: AbortSignal): Promise<AgentRun[]> {
    return this.request('GET', '/api/sdk/runs', undefined, signal);
  }
  getRun(id: string, signal?: AbortSignal): Promise<AgentRun> {
    return this.request('GET', `/api/sdk/runs/${encodeURIComponent(id)}`, undefined, signal);
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
    const timeoutMs = Math.max(0, options.timeoutMs ?? 30 * 60_000);
    const intervalMs = Math.max(50, options.intervalMs ?? 1_000);
    const deadline = Date.now() + timeoutMs;
    for (;;) {
      options.signal?.throwIfAborted();
      const snapshot = await this.getRun(id, options.signal);
      options.onUpdate?.(snapshot);
      if (isRunSettled(snapshot)) return snapshot;
      const remaining = deadline - Date.now();
      if (remaining <= 0) {
        throw new DominionSdkError('Dominion run wait timed out; the remote run is still durable', 408);
      }
      await new Promise<void>((resolve, reject) => {
        const signal = options.signal;
        let timer: ReturnType<typeof setTimeout> | undefined;
        const abort = () => {
          if (timer !== undefined) clearTimeout(timer);
          signal?.removeEventListener('abort', abort);
          reject(signal?.reason ?? new DOMException('Aborted', 'AbortError'));
        };
        if (signal?.aborted) { abort(); return; }
        timer = setTimeout(() => {
          signal?.removeEventListener('abort', abort);
          resolve();
        }, Math.min(intervalMs, remaining));
        signal?.addEventListener('abort', abort, {once: true});
      });
    }
  }

  /** Strict success gate: a text reply alone never constitutes verified work. */
  async waitForVerifiedRun(id: string, options: WaitRunOptions = {}): Promise<AgentRun> {
    const run = await this.waitForRun(id, options);
    if (run.status === 'completed' && run.taskOutcome === 'succeeded'
        && run.verified === true) return run;
    throw new DominionSdkError(
      run.error ?? `Dominion task is ${run.status}; verified success evidence is missing`,
      run.status === 'awaiting_approval' || run.status === 'in_doubt' ? 409 : 422,
    );
  }
}
