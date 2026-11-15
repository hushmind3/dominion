import { test } from 'node:test';
import assert from 'node:assert/strict';
import { DominionClient, DominionApiError } from '../dist/index.js';

function response(data, status = 200, headers = {}) {
  return new Response(typeof data === 'string' ? data : JSON.stringify(data), {
    status, headers: { 'content-type': 'application/json', ...headers },
  });
}
function sse(chunks) {
  return new Response(new ReadableStream({
    start(controller) {
      for (const chunk of chunks) controller.enqueue(new TextEncoder().encode(chunk));
      controller.close();
    },
  }), { headers: { 'content-type': 'text/event-stream' } });
}

test('goal request is sent to native Rust route and returns queued job without inventing completion', async () => {
  const calls = [];
  const client = new DominionClient({
    token: 'private-token',
    fetch: async (url, init) => {
      calls.push({ url, init });
      return response({goal:{id:'g1',jobId:'goal:g1',goal:'write report.md',status:'queued'},job:{id:'goal:g1',kind:'goal',status:'queued'}},202);
    },
  });
  const result = await client.createGoal({
    goal: 'write report.md', successCriteria: [{type:'file_exists',path:'report.md'}],
  });
  assert.equal(result.job.status,'queued');
  assert.equal(calls[0].url,'http://127.0.0.1:4317/api/goals');
  assert.equal(calls[0].init.headers.get('X-Dominion-Token'),'private-token');
  assert.equal(JSON.parse(calls[0].init.body).successCriteria[0].path,'report.md');
});

test('chat reconstructs multi-line CRLF SSE frames across arbitrary chunks', async () => {
  const client = new DominionClient({
    fetch: async () => sse([': keepalive\r\n\r\nda', 'ta: {"type":"delta",\r\ndata: "text":"안녕"}\r', '\n\r\ndata: {"type":"done"}\n\n']),
  });
  const messages = [];
  for await (const event of client.chat({input:'hi'})) messages.push(event);
  assert.deepEqual(messages.map(e => e.type), ['delta','done']);
  assert.equal(messages[0].text, '안녕');
});

test('chat rejects an EOF before Rust reports completion', async () => {
  const client = new DominionClient({fetch:async () => sse(['data: {"type":"delta"}\n\n'])});
  await assert.rejects(async () => {
    for await (const _event of client.chat({input:'hi'})) {}
  }, /ended before done/);
});

test('event bus resumes after a sequence cursor and ignores duplicates', async () => {
  let target;
  const client = new DominionClient({
    fetch: async (url) => {
      target = url;
      return sse(['id: 4\ndata: {"seq":4,"type":"record"}\n\n','id: 5\ndata: {"seq":5,"type":"job_event"}\n\n']);
    },
  });
  const seen = [];
  for await (const event of client.events({after:4})) seen.push(event.seq);
  assert.deepEqual(seen,[5]);
  assert.equal(target,'http://127.0.0.1:4317/api/events?after=4');
});

test('rejects insecure remote admin tokens, HTTP errors and malformed endpoint responses', async () => {
  assert.throws(() => new DominionClient({baseUrl:'http://example.com',token:'secret'}),/plaintext/);
  const forbidden = new DominionClient({fetch:async () => response({error:'admin token required'},401)});
  await assert.rejects(forbidden.health(), error =>
    error instanceof DominionApiError && error.status === 401 && error.message === 'admin token required');
  const html = new DominionClient({fetch:async () => new Response('<html>wrong server</html>')});
  await assert.rejects(html.status(), /Expected JSON/);
});
