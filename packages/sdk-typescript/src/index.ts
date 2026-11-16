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

export type VideoEngine = 'native' | 'comfyui';
export interface VideoQuality {
  status: 'passed' | 'container_only';
  bytes: number;
  durationSeconds?: number;
  width?: number;
  height?: number;
  audioPresent?: boolean;
  warning?: string;
}
export interface VideoJob {
  id: string;
  kind: 'video' | 'youtube_upload';
  status: 'queued' | 'running' | 'completed' | 'failed' | 'cancelled' | 'needs_review';
  engine?: VideoEngine;
  title?: string;
  brief?: string;
  progress?: number;
  storyboard?: string[];
  quality?: VideoQuality | null;
  outputUrl?: string | null;
  deliveryJobId?: string | null;
  deliveryError?: string | null;
  result?: {
    quality?: VideoQuality;
    renderWallSeconds?: number;
    newSceneCalls?: number;
    reusedScenes?: number;
    outputUrl?: string;
    youtubeId?: string;
    url?: string;
    bytes?: number;
  } | null;
  error?: string | null;
}
export interface StartVideo {
  brief: string;
  title?: string;
  engine?: VideoEngine;
  duration?: number;
  aspectRatio?: '9:16' | '16:9' | '1:1';
  scenes?: string[];
  fps?: number;
  seed?: number;
  negativePrompt?: string;
  /** Explicit opt-in; queued upload is always private. */
  autoUpload?: boolean;
}
export interface VideoUpload {
  title?: string;
  description?: string;
  privacyStatus?: 'private' | 'unlisted' | 'public';
  tags?: string;
  madeForKids?: boolean;
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

  /** These media endpoints use the same durable Rust job engine as the UI. */
  listVideoJobs(signal?: AbortSignal): Promise<VideoJob[]> {
    return this.request('GET', '/api/video/jobs', undefined, signal);
  }
  getVideoJob(id: string, signal?: AbortSignal): Promise<VideoJob> {
    return this.request('GET', `/api/video/jobs/${encodeURIComponent(id)}`, undefined, signal);
  }
  createVideo(input: StartVideo, signal?: AbortSignal): Promise<VideoJob> {
    return this.request('POST', '/api/video/jobs', input, signal);
  }
  regenerateVideoScene(
    sourceJobId: string, sceneIndex: number, prompt: string, signal?: AbortSignal,
  ): Promise<VideoJob> {
    return this.request('POST',
      `/api/video/jobs/${encodeURIComponent(sourceJobId)}/regenerate`,
      {sceneIndex, prompt}, signal);
  }
  queueVideoUpload(videoJobId: string, input: VideoUpload, signal?: AbortSignal): Promise<VideoJob> {
    return this.request('POST', `/api/video/jobs/${encodeURIComponent(videoJobId)}/upload`,
      input, signal);
  }
}
