/**
 * DOMINION TypeScript SDK — a thin client for DOMINION's own Rust API.
 * No model-specific SDK or third-party agent runtime is involved.
 */
export type JsonValue = null | boolean | number | string | JsonValue[] | { [key: string]: JsonValue };

export interface DominionRecord {
  id: string;
  [key: string]: unknown;
}
export interface Agent extends DominionRecord {
  name: string;
  systemPrompt?: string;
  providerId?: string;
  model?: string;
}
export interface Session extends DominionRecord {
  title: string;
  agentId?: string;
  providerId?: string;
  model?: string;
}
export interface Job extends DominionRecord {
  kind: string;
  status: string;
  input?: unknown;
  result?: unknown;
  error?: string;
}
export interface Goal extends DominionRecord {
  goal: string;
  jobId: string;
  status: string;
  phase?: string;
  iterations?: number;
  plan?: string;
  gaps?: string[];
  evidence?: unknown[];
}
export interface RoleModel {
  agentId?: string;
  providerId?: string;
  model?: string;
}
export type SuccessCriterion =
  | { type: 'file_exists' | 'file_absent' | 'file_changed'; path: string }
  | { type: 'file_contains'; path: string; contains: string }
  | { type: 'file_equals'; path: string; content: string }
  | { type: 'tool_succeeded'; tool: string }
  | { type: 'command_succeeded'; command: string };
export interface GoalRequest {
  goal: string;
  maxIterations?: number;
  maxReplans?: number;
  replanAfter?: number;
  executor?: RoleModel;
  planner?: RoleModel;
  reviewer?: RoleModel;
  successCriteria?: SuccessCriterion[];
}
export interface ChatRequest {
  sessionId?: string;
  agentId?: string;
  providerId?: string;
  model?: string;
  input: string;
  toolsEnabled?: boolean;
  toolsExecutionEnabled?: boolean;
  autoLearning?: boolean;
  reasoningEffort?: string;
  thinking?: boolean;
}
export interface StreamEvent {
  type: string;
  [key: string]: unknown;
}
export interface BusEvent {
  type: string;
  seq: number;
  [key: string]: unknown;
}
export interface DominionClientOptions {
  /** Default: http://127.0.0.1:4317 */
  baseUrl?: string;
  /** Optional DOMINION_ADMIN_TOKEN; never sent to an arbitrary third-party host. */
  token?: string;
  /** Useful for tests or custom runtimes. Defaults to global fetch. */
  fetch?: typeof globalThis.fetch;
}

export class DominionApiError extends Error {
  readonly name = 'DominionApiError';
  constructor(
    public readonly status: number,
    message: string,
    public readonly details?: unknown,
  ) {
    super(message);
  }
}

function idSegment(id: string): string {
  if (!id || id === '.' || id === '..' || id.includes('/') || id.includes('\\')) {
    throw new TypeError('A non-empty record ID is required');
  }
  return encodeURIComponent(id);
}

interface Frame {
  data: string;
  id?: string;
}

/** Handles chunk splits, CRLF, comments, multi-line data and SSE IDs. */
function parseFrame(text: string): Frame | undefined {
  const data: string[] = [];
  let id: string | undefined;
  for (const line of text.split(/\r?\n/)) {
    if (!line || line.startsWith(':')) continue;
    const colon = line.indexOf(':');
    const field = colon < 0 ? line : line.slice(0, colon);
    const value = colon < 0 ? '' : line.slice(colon + 1).replace(/^ /, '');
    if (field === 'data') data.push(value);
    if (field === 'id' && !value.includes('\0')) id = value;
  }
  return data.length ? { data: data.join('\n'), id } : undefined;
}

async function* readSse(body: ReadableStream<Uint8Array>): AsyncGenerator<Frame> {
  const reader = body.getReader();
  const decoder = new TextDecoder();
  let buffer = '';
  try {
    while (true) {
      const { done, value } = await reader.read();
      buffer += done ? decoder.decode() : decoder.decode(value, { stream: true });
      let boundary: RegExpExecArray | null;
      while ((boundary = /\r?\n\r?\n/.exec(buffer)) !== null) {
        const frame = parseFrame(buffer.slice(0, boundary.index));
        buffer = buffer.slice(boundary.index + boundary[0].length);
        if (frame) yield frame;
      }
      if (done) break;
      if (buffer.length > 2_000_000) throw new Error('SSE frame too large');
    }
    const tail = parseFrame(buffer);
    if (tail) yield tail;
  } finally {
    await reader.cancel().catch(() => undefined);
  }
}

export class DominionClient {
  private readonly baseUrl: string;
  private readonly token?: string;
  private readonly transport: typeof globalThis.fetch;

  constructor(options: DominionClientOptions = {}) {
    const parsed = new URL(options.baseUrl ?? 'http://127.0.0.1:4317');
    if (!['http:', 'https:'].includes(parsed.protocol) || parsed.username || parsed.password ||
        parsed.search || parsed.hash || !['', '/'].includes(parsed.pathname)) {
      throw new TypeError('baseUrl must be a plain HTTP(S) origin');
    }
    if (options.token && parsed.protocol !== 'https:' &&
        !['127.0.0.1', 'localhost', '[::1]'].includes(parsed.hostname)) {
      throw new TypeError('Refusing to send an admin token over plaintext to a remote host');
    }
    this.baseUrl = parsed.origin;
    this.token = options.token;
    this.transport = options.fetch ?? globalThis.fetch;
    if (!this.transport) throw new TypeError('A fetch implementation is required');
  }

  private async send(
    method: 'GET' | 'POST' | 'DELETE',
    path: string,
    data?: unknown,
    signal?: AbortSignal,
    stream = false,
  ): Promise<Response> {
    if (!path.startsWith('/api/') || path.startsWith('//')) {
      throw new TypeError('Only DOMINION API paths are allowed');
    }
    const headers = new Headers({ Accept: stream ? 'text/event-stream' : 'application/json' });
    if (data !== undefined) headers.set('Content-Type', 'application/json');
    if (this.token) headers.set('X-Dominion-Token', this.token);
    const response = await this.transport(this.baseUrl + path, {
      method,
      headers,
      body: data === undefined ? undefined : JSON.stringify(data),
      signal,
      cache: 'no-store',
    });
    if (!response.ok) {
      const raw = await response.text();
      let parsed: unknown = raw;
      try { parsed = JSON.parse(raw); } catch { /* text error */ }
      const error = (parsed && typeof parsed === 'object' && 'error' in parsed)
        ? String((parsed as { error?: unknown }).error)
        : (raw && !raw.trimStart().startsWith('<') ? raw : response.statusText || 'HTTP error');
      throw new DominionApiError(response.status, error, parsed);
    }
    return response;
  }

  private async json<T>(
    method: 'GET' | 'POST' | 'DELETE', path: string,
    data?: unknown, signal?: AbortSignal,
  ): Promise<T> {
    const response = await this.send(method, path, data, signal);
    try {
      return (await response.json()) as T;
    } catch {
      throw new DominionApiError(response.status, 'Expected JSON from DOMINION Rust API');
    }
  }

  health(signal?: AbortSignal) {
    return this.json<{ ok: boolean; runtime: string; version: string }>('GET', '/api/health', undefined, signal);
  }
  status(signal?: AbortSignal) {
    return this.json<Record<string, unknown>>('GET', '/api/status', undefined, signal);
  }
  agents(signal?: AbortSignal) {
    return this.json<Agent[]>('GET', '/api/agents', undefined, signal);
  }
  sessions(signal?: AbortSignal) {
    return this.json<Session[]>('GET', '/api/sessions', undefined, signal);
  }
  createSession(input: { title?: string; agentId?: string; providerId?: string; model?: string }, signal?: AbortSignal) {
    return this.json<Session>('POST', '/api/sessions', input, signal);
  }
  sessionMessages(id: string, signal?: AbortSignal) {
    return this.json<unknown[]>('GET', `/api/sessions/${idSegment(id)}/messages`, undefined, signal);
  }
  cancelSession(id: string, signal?: AbortSignal) {
    return this.json<{ cancelled: boolean }>('POST', `/api/sessions/${idSegment(id)}/cancel`, {}, signal);
  }
  jobs(signal?: AbortSignal) {
    return this.json<Job[]>('GET', '/api/jobs', undefined, signal);
  }
  job(id: string, signal?: AbortSignal) {
    return this.json<Job>('GET', `/api/jobs/${idSegment(id)}`, undefined, signal);
  }
  createJob(input: { kind: 'prompt' | 'workflow'; input: Record<string, unknown> }, signal?: AbortSignal) {
    return this.json<Job>('POST', '/api/jobs', input, signal);
  }
  cancelJob(id: string, signal?: AbortSignal) {
    return this.json<Job>('POST', `/api/jobs/${idSegment(id)}/cancel`, {}, signal);
  }
  goals(signal?: AbortSignal) {
    return this.json<Goal[]>('GET', '/api/goals', undefined, signal);
  }
  goal(id: string, signal?: AbortSignal) {
    return this.json<Goal>('GET', `/api/goals/${idSegment(id)}`, undefined, signal);
  }
  createGoal(input: GoalRequest, signal?: AbortSignal) {
    return this.json<{ goal: Goal; job: Job }>('POST', '/api/goals', input, signal);
  }
  cancelGoal(id: string, signal?: AbortSignal) {
    return this.json<Goal>('POST', `/api/goals/${idSegment(id)}/cancel`, {}, signal);
  }
  approvals(signal?: AbortSignal) {
    return this.json<DominionRecord[]>('GET', '/api/approvals', undefined, signal);
  }
  resolveApproval(id: string, allow: boolean, signal?: AbortSignal) {
    return this.json<{ resolved: boolean; status: string; error?: string }>(
      'POST', '/api/approvals', { id, allow }, signal,
    );
  }
  rooms(signal?: AbortSignal) {
    return this.json<DominionRecord[]>('GET', '/api/rooms', undefined, signal);
  }
  modelUsage(signal?: AbortSignal) {
    return this.json<Record<string, unknown>>('GET', '/api/model-usage', undefined, signal);
  }

  /** Yields the actual Rust turn events. Rejects interrupted streams rather than claiming success. */
  async *chat(input: ChatRequest, signal?: AbortSignal): AsyncGenerator<StreamEvent> {
    const response = await this.send('POST', '/api/chat', input, signal, true);
    if (!response.body || !response.headers.get('content-type')?.startsWith('text/event-stream')) {
      throw new DominionApiError(response.status, 'Expected a streaming Rust chat response');
    }
    for await (const frame of readSse(response.body)) {
      if (frame.data === '[DONE]') return;
      let event: StreamEvent;
      try { event = JSON.parse(frame.data) as StreamEvent; }
      catch { throw new Error('Malformed DOMINION chat stream event'); }
      yield event;
      if (event.type === 'done') return;
    }
    if (!signal?.aborted) throw new Error('DOMINION chat stream ended before done');
  }

  /** Read events from the durable Rust bus; reconnect with the last seq after a network interruption. */
  async *events(options: { after?: number; signal?: AbortSignal } = {}): AsyncGenerator<BusEvent> {
    const after = options.after ?? 0;
    if (!Number.isSafeInteger(after) || after < 0) throw new TypeError('after must be a nonnegative integer');
    const response = await this.send('GET', `/api/events?after=${after}`, undefined, options.signal, true);
    if (!response.body || !response.headers.get('content-type')?.startsWith('text/event-stream')) {
      throw new DominionApiError(response.status, 'Expected a streaming Rust event response');
    }
    for await (const frame of readSse(response.body)) {
      let event: BusEvent;
      try { event = JSON.parse(frame.data) as BusEvent; }
      catch { throw new Error('Malformed DOMINION bus event'); }
      if (!Number.isSafeInteger(event.seq) || event.seq <= after) continue;
      yield event;
    }
  }
}
