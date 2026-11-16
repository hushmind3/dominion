/**
 * Optional TypeScript client for Dominion's shared native Rust runtime.
 * All control flow, policy, execution and journals remain in the Rust server.
 */
export type SdkRunStatus =
  | 'queued' | 'running' | 'awaiting_approval' | 'in_doubt'
  | 'interrupted' | 'completed' | 'cancelled' | 'exhausted'
  | 'denied' | 'failed';

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
}
export interface AgentRun {
  id: string;
  status: SdkRunStatus;
  instruction: string;
  providerId?: string;
  model?: string;
  answer?: string;
  error?: string;
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


}
