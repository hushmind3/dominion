import { test } from 'node:test';
import assert from 'node:assert/strict';
import { Dominion, DominionError, StreamInterruptedError } from '../dist/index.js';

const json = (value, status = 200) =>
  new Response(JSON.stringify(value), {status, headers:{'content-type':'application/json'}});
const sse = (chunks) => new Response(new ReadableStream({start(c) {
  for (const chunk of chunks) c.enqueue(new TextEncoder().encode(chunk));
  c.close();
}}), {headers:{'content-type':'text/event-stream'}});

test('the SDK speaks actual /api/* Rust endpoints and sends the admin token header', async () => {
  const calls = [];
  const d = new Dominion({baseUrl:'http://127.0.0.1:4317/',token:'private',fetch:async (url,init) => {
    calls.push({url,init}); return json({id:'job-1',status:'queued',kind:'prompt'});
  }});
  await d.jobs.create({kind:'prompt',input:{prompt:'edit the repository',toolsEnabled:true}});
  await d.jobs.get('job/a');
  await d.rooms.send('my room',{input:'Ship it',clientMessageId:'stable-1'});
  assert.equal(calls[0].url,'http://127.0.0.1:4317/api/jobs');
  assert.equal(calls[1].url,'http://127.0.0.1:4317/api/jobs/job%2Fa');
  assert.equal(calls[2].url,'http://127.0.0.1:4317/api/rooms/my%20room/messages');
  assert.equal(calls[0].init.headers.get('X-Dominion-Token'),'private');
  assert.equal(calls[0].init.headers.get('Content-Type'),'application/json');
  assert.equal(calls[0].init.cache,'no-store');
  assert.deepEqual(JSON.parse(calls[2].init.body),{input:'Ship it',clientMessageId:'stable-1'});
});
test('external URL and traversal fail locally before credentials are sent', async () => {
  let calls=0;
  const d=new Dominion({token:'secret',fetch:async()=>{calls++;return json({});}});
  assert.throws(()=>d.raw('https://attacker.invalid/api/jobs'),TypeError);
  assert.throws(()=>d.raw('/api/../../evil'),TypeError);
  assert.equal(calls,0);
});
test('real Rust JSON error is preserved and HTML is not accidentally parsed',async()=>{
  const d=new Dominion({fetch:async()=>json({error:'승인 필요'},401)});
  await assert.rejects(d.jobs.list(),e=>e instanceof DominionError&&e.status===401&&e.message==='승인 필요');
  const html=new Dominion({fetch:async()=>new Response('<html>oops</html>',{status:502})});
  await assert.rejects(html.jobs.list(),e=>e instanceof DominionError&&e.status===502&&!e.message.includes('<html>'));
});
test('chat SSE handles fragmented UTF-8 / CRLF and stops at done without replay',async()=>{
  const seen=[];
  const payload='data: {"type":"delta","text":"안녕"}\r\n\r\n: keepalive\r\n\r\ndata: {"type":"done"}\r\n\r\n';
  const bytes=new TextEncoder().encode(payload);
  const stream=new Response(new ReadableStream({start(c){
    [bytes.slice(0,25),bytes.slice(25,34),bytes.slice(34,38),bytes.slice(38)].forEach(x=>c.enqueue(x));c.close();
  }}),{headers:{'content-type':'text/event-stream'}});
  let calls=0;
  const d=new Dominion({fetch:async()=>{calls++;return stream;}});
  await d.streamChat({sessionId:'s',agentId:'a',input:'hello'},event=>seen.push(event));
  assert.deepEqual(seen.map(x=>x.type),['delta','done']);
  assert.equal(seen[0].text,'안녕');
  assert.equal(calls,1);
});
test('unexpected chat stream close is explicit failure and never repeats the POST',async()=>{
  let calls=0;
  const d=new Dominion({fetch:async()=>{calls++;return sse(['data: {"type":"delta"}\n\n']);}});
  await assert.rejects(d.streamChat({sessionId:'s',agentId:'a',input:'x'},()=>{}),StreamInterruptedError);
  assert.equal(calls,1);
});
test('durable SSE reconnects with last sequence and ignores duplicate replay',async()=>{
  const urls=[];let calls=0;
  const d=new Dominion({fetch:async url=>{
    urls.push(url);calls++;
    return calls===1 ? sse(['id: 10\ndata: {"type":"job","seq":10}\n\n'])
      : sse(['id: 10\ndata: {"type":"job","seq":10}\n\n','id: 11\ndata: {"type":"job","seq":11}\n\n']);
  }});
  const received=[];
  for await(const event of d.events({after:9,reconnectDelayMs:1})){
    received.push(event.seq);
    if(received.length===2) break;
  }
  assert.deepEqual(received,[10,11]);
  assert.equal(urls[0],'http://127.0.0.1:4317/api/events?after=9');
  assert.equal(urls[1],'http://127.0.0.1:4317/api/events?after=10');
});
test('waitForJob polls persisted Rust state and reaches a terminal state',async()=>{
  let calls=0;
  const d=new Dominion({fetch:async()=>json({id:'j',kind:'prompt',input:{},status:++calls>=3?'completed':'running'})});
  const job=await d.waitForJob('j',{intervalMs:1,timeoutMs:2000});
  assert.equal(job.status,'completed');
  assert.equal(calls,3);
});
test('waitForJob returns when owner review is needed instead of polling forever',async()=>{
  const client=new Dominion({fetch:async()=>json({
    id:'j',kind:'prompt',input:{tokenBudget:1024},status:'needs_review'
  })});
  const job=await client.waitForJob('j',{timeoutMs:200});
  assert.equal(job.status,'needs_review');
});
test('abort ends durable polling before any network request',async()=>{
  const controller=new AbortController();controller.abort();
  let calls=0;
  const d=new Dominion({fetch:async()=>{calls++;return json({});}});
  await assert.rejects(d.waitForJob('j',{signal:controller.signal}),e=>e.name==='AbortError');
  assert.equal(calls,0);
});

test('job workspace travels through the native Rust API contract',async()=>{
  let payload;
  const sdk=new Dominion({fetch:async(_url,init)=>{
    payload=JSON.parse(init.body);
    return json({id:'scoped-job',status:'queued',kind:'prompt',input:payload.input});
  }});
  const result=await sdk.jobs.create({kind:'prompt',input:{
    prompt:'Fix the project',workspaceDir:'/home/you/code/project',
    tokenBudget:24000,autonomous:true
  }});
  assert.equal(payload.input.workspaceDir,'/home/you/code/project');
  assert.equal(payload.input.tokenBudget,24000);
  assert.equal(result.input.prompt,'Fix the project');
});
test('owner replacement uses the existing job and explicit replace mode',async()=>{
  let last;
  const client=new Dominion({fetch:async(url,init)=>{
    last={url,body:JSON.parse(init.body)};
    return json({jobId:'same',steeringRevision:2,mode:'replace',appliesAt:'next_model_turn'});
  }});
  const result=await client.jobs.replaceGoal('same','Make a presentation instead');
  assert.equal(result.mode,'replace');
  assert.equal(result.steeringRevision,2);
  assert.equal(last.url,'http://127.0.0.1:4317/api/jobs/same/steer');
  assert.deepEqual(last.body,{instruction:'Make a presentation instead',mode:'replace'});
});
test('owner steering uses the same native job and revision contract',async()=>{
  let call;
  const d=new Dominion({token:'token',fetch:async(url,init)=>{
    call={url,init};
    return json({jobId:'abc',steeringRevision:1,appliesAt:'next_model_turn'});
  }});
  const applied=await d.jobs.steer('abc','Check the actual tests');
  assert.equal(applied.steeringRevision,1);
  assert.equal(call.url,'http://127.0.0.1:4317/api/jobs/abc/steer');
  assert.equal(call.init.headers.get('X-Dominion-Token'),'token');
  assert.deepEqual(JSON.parse(call.init.body),{instruction:'Check the actual tests'});
});

test('evaluation summary SDK accesses the same Rust persisted scorecard as GUI',async()=>{
  const calls=[];
  const client=new Dominion({fetch:async(url,init)=>{
    calls.push({url,method:init.method});
    return json({rootTasks:2,verified:{passed:1,failed:1,successRate:0.5},
      pricingCoverage:{pricedCalls:0,unpricedCalls:2}});
  }});
  const result=await client.evaluation.summary();
  assert.equal(calls[0].url,'http://127.0.0.1:4317/api/evaluation/summary');
  assert.equal(calls[0].method,'GET');
  assert.equal(result.verified.successRate,0.5);
});
