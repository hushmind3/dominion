/**
 * First-party high-level SDK for the DOMINION Rust runtime.
 *
 * All model decisions, tools, permissions, state and completion are performed
 * by the native runtime. Threads here identify its durable SQLite transcript.
 */
import { DominionClient } from './index.js';
import type { ClientOptions, TaskEvent, TaskInput, TaskRecord, TaskStatus } from './index.js';

export interface ThreadOptions {
  /** Stable identifier for a native conversation transcript. */
  sessionId?: string;
  /** Capabilities are opt-in, and cannot exceed the Rust server's permissions. */
  allowWrite?: boolean;
  allowShell?: boolean;
  allowMcp?: boolean;
  allowSubagents?: boolean;
  requireEvidence?: boolean;
  maxSteps?: number;
  tokenBudget?: number;
  planningMode?: 'auto' | 'always' | 'never';
}

export interface RunOptions extends Partial<Omit<TaskInput, 'prompt' | 'sessionId'>> {
  /** Abort waiting/streaming; use cancel() to stop server-side work. */
  signal?: AbortSignal;
  /** Resume durable events after a known sequence (default: replay from 0). */
  after?: number;
}

export interface RunResult {
  sessionId: string;
  taskId: string;
  status: TaskStatus;
  finalResponse: string | null;
  record: TaskRecord;
  events: TaskEvent[];
}

export interface StreamedRun {
  readonly sessionId: string;
  readonly taskId: string;
  readonly record: TaskRecord;
  readonly events: AsyncGenerator<TaskEvent>;
}

export class DominionRuntime {
  readonly client: DominionClient;
  constructor(options: ClientOptions | DominionClient = {}) {
    this.client = options instanceof DominionClient ? options : new DominionClient(options);
  }

  /** No model or tool work starts until run()/runStreamed() is called. */
  startThread(options: ThreadOptions = {}): DominionThread {
    return new DominionThread(this.client, options.sessionId ?? crypto.randomUUID(), options);
  }

  /** Reuse a server-owned transcript; no prompt is replayed as a side effect. */
  resumeThread(sessionId: string, options: Omit<ThreadOptions, 'sessionId'> = {}): DominionThread {
    return new DominionThread(this.client, sessionId, options);
  }
}

export class DominionThread {
  readonly id: string;
  private readonly defaults: Omit<ThreadOptions, 'sessionId'>;
  private latest?: string;

  constructor(
    private readonly client: DominionClient,
    sessionId: string,
    options: Omit<ThreadOptions, 'sessionId'> = {},
  ) {
    if (!sessionId || sessionId.length > 128 || /[\r\n\0]/.test(sessionId)) {
      throw new TypeError('A valid native session ID is required');
    }
    this.id = sessionId;
    this.defaults = {
      allowWrite: options.allowWrite ?? false,
      allowShell: options.allowShell ?? false,
      allowMcp: options.allowMcp ?? false,
      allowSubagents: options.allowSubagents ?? false,
      requireEvidence: options.requireEvidence ?? false,
      maxSteps: options.maxSteps,
      tokenBudget: options.tokenBudget,
      planningMode: options.planningMode ?? 'auto',
    };
  }

  get lastTaskId(): string | undefined { return this.latest; }

  /**
   * Submit exactly once; subsequent SSE reads never resubmit or pay for another
   * model call. The supplied requestId can recover a lost POST response.
   */
  async runStreamed(prompt: string, options: RunOptions = {}): Promise<StreamedRun> {
    if (!prompt.trim()) throw new TypeError('A non-empty instruction is required');
    const {signal, after, ...overrides} = options;
    const input: TaskInput = {
      ...this.defaults,
      ...overrides,
      prompt,
      sessionId: this.id,
    };
    const record = await this.client.submit(input, signal);
    this.latest = record.id;
    const events = this.untilFinished(record.id, after, signal);
    return {sessionId: this.id, taskId: record.id, record, events};
  }

  private async *untilFinished(
    taskId: string, after: number | undefined, signal: AbortSignal | undefined,
  ): AsyncGenerator<TaskEvent> {
    // A previously completed task may be returned for an idempotent request.
    // Replay its persisted events instead of starting another native execution.
    for await (const event of this.client.watch(taskId, {after, signal, reconnect: true})) {
      yield event;
      if (event.type === 'finished') return;
    }
    // An explicit AbortSignal stops waiting without asserting the native task
    // was cancelled. Calling cancel() is a separate durable server action.
    if (!signal?.aborted) {
      throw new Error('The durable task event stream ended without a terminal event');
    }
  }

  /** Wait for Rust's persisted terminal record; a model draft is not success. */
  async run(prompt: string, options: RunOptions = {}): Promise<RunResult> {
    const streamed = await this.runStreamed(prompt, options);
    const events: TaskEvent[] = [];
    let finished = false;
    for await (const event of streamed.events) {
      // Keep the last 2,000 events to bound a long running caller's memory.
      if (events.length === 2_000) events.shift();
      events.push(event);
      if (event.type === 'finished') finished = true;
    }
    if (!finished) {
      if (options.signal?.aborted) {
        throw new Error('Client stopped waiting; the native task may still be running. Call cancel() to stop it.');
      }
      throw new Error('Native task stopped streaming without a terminal record');
    }
    const record = await this.client.get(streamed.taskId, options.signal);
    if (!['completed','failed','cancelled','needs_review'].includes(record.status)) {
      throw new Error('Native terminal event was not followed by a terminal task record');
    }
    return {
      sessionId: this.id,
      taskId: record.id,
      status: record.status,
      finalResponse: record.status === 'completed' ? record.result?.answer ?? null : null,
      record,
      events,
    };
  }

  /** Steering is applied at the next native model step, not in this SDK. */
  async steer(text: string, taskId = this.latest, signal?: AbortSignal): Promise<TaskRecord> {
    if (!taskId) throw new Error('No task has been started in this thread');
    return this.client.steer(taskId, text, signal);
  }

  /** Cancellation changes the native persisted task; it is not AbortSignal. */
  async cancel(taskId = this.latest, signal?: AbortSignal): Promise<TaskRecord> {
    if (!taskId) throw new Error('No task has been started in this thread');
    return this.client.cancel(taskId, signal);
  }

  /** Read-only state lookup for UI clients that resume independently. */
  async get(taskId = this.latest, signal?: AbortSignal): Promise<TaskRecord> {
    if (!taskId) throw new Error('No task has been started in this thread');
    return this.client.get(taskId, signal);
  }
}
