import test from 'node:test';
import assert from 'node:assert/strict';
import {Dominion,DominionApiError,DominionStreamError} from '../dist/index.js';
const reply=(body,status=200)=>new Response(JSON.stringify(body),{status,headers:{'content-type':'application/json'}});
const sse=(...chunks)=>new Response(new ReadableStream({start(controller){for(const chunk of chunks)controller.enqueue(new TextEncoder().encode(chunk));controller.close();}}),{headers:{'content-type':'text/event-stream'}});

test('Rust routes, agent job envelopes, approval decision and X-Dominion-Token',async()=>{
  const requests=[];
  const sdk=new Dominion({adminToken:'secret',fetch:async(url,init)=>{requests.push({url,init});return reply({id:'j',kind:'prompt',status:'queued'});}});
  await sdk.jobs.create({kind:'prompt',input:{prompt:'inspect'}});
  await sdk.jobs.cancel('run/a');await sdk.approvals.resolve('approval',false);
  assert.equal(requests[0].url,'http://127.0.0.1:4317/api/jobs');
  assert.equal(requests[1].url,'http://127.0.0.1:4317/api/jobs/run%2Fa/cancel');
  assert.equal(requests[2].init.headers['X-Dominion-Token'],'secret');
  assert.equal(requests[2].init.headers.Authorization,undefined);
  assert.deepEqual(JSON.parse(requests[2].init.body),{id:'approval',allow:false});
});
test('preserves structured HTTP failure instead of claiming success',async()=>{
  const sdk=new Dominion({fetch:async()=>reply({error:'admin token required'},401)});
  await assert.rejects(()=>sdk.jobs.list(),err=>err instanceof DominionApiError&&err.status===401&&err.message==='admin token required');
});
test('SSE handles split CRLF, comments, replay and duplicates',async()=>{
  const sdk=new Dominion({fetch:async()=>sse(': keepalive\r\n\r\nid: 3\r\ndata: {"seq":3,"type":"record"}\r\n','\r\nid: 4\r\ndata: {"seq":4,"type":"approval"}\r\n\r\nid: 4\r\ndata: {"seq":4,"type":"duplicate"}\r\n\r\n')});
  const values=[];for await(const event of sdk.events({after:2}))values.push(event.type);
  assert.deepEqual(values,['record','approval']);
});
test('reconnect carries forward the last sequence number',async()=>{
  let calls=0;const urls=[],controller=new AbortController();
  const sdk=new Dominion({fetch:async url=>{urls.push(url);calls++;
    return calls===1?sse('data: {"seq":5,"type":"job"}\n\n'):
      sse('data: {"seq":5,"type":"duplicate"}\n\n','data: {"seq":6,"type":"approval"}\n\n');}});
  const values=[];
  for await(const event of sdk.watchEvents({after:4,retryDelayMs:1,signal:controller.signal})){
    values.push(event.seq);if(values.length===2)controller.abort();
  }
  assert.deepEqual(values,[5,6]);assert.match(urls[1],/after=5$/);
});
test('chat interruption is surfaced; done terminates properly',async()=>{
  let sdk=new Dominion({fetch:async()=>sse('data: {"type":"text","content":"partial"}\n\n')});
  await assert.rejects(async()=>{for await(const x of sdk.chat({sessionId:'s',agentId:'a',input:'hello'})){}},DominionStreamError);
  sdk=new Dominion({fetch:async()=>sse('data: {"type":"text"}\n\n','data: {"type":"done"}\n\n')});
  const events=[];for await(const value of sdk.chat({sessionId:'s',agentId:'a',input:'hello'}))events.push(value.type);
  assert.deepEqual(events,['text','done']);
});
test('waitJob polls durable status, not UI-local promises',async()=>{
  let count=0;const sdk=new Dominion({fetch:async()=>reply({id:'j',kind:'prompt',status:++count>1?'completed':'running'})});
  assert.equal((await sdk.waitJob('j',{intervalMs:100,timeoutMs:1000})).status,'completed');
  assert.equal(count,2);
});

test('native SDK steering and session binding share real Rust API contracts',async()=>{
  const observed=[];
  const sdk=new Dominion({adminToken:'owner',fetch:async(url,init)=>{
    observed.push({url,init});return reply({ok:true,jobId:'job-1',directive:'Change summary'});
  }});
  await sdk.jobs.steer('job-1','Change summary');
  await sdk.jobs.create({kind:'prompt',input:{
    prompt:'Build the report',sessionId:'session-1',maxSteps:60,
    strategy:'verify_first',requireEvidence:true,allowFallback:false
  }});
  assert.equal(observed[0].url,'http://127.0.0.1:4317/api/jobs/job-1/directive');
  assert.equal(observed[0].init.headers['X-Dominion-Token'],'owner');
  assert.equal(JSON.parse(observed[0].init.body).instruction,'Change summary');
  assert.deepEqual(JSON.parse(observed[1].init.body),{
    kind:'prompt',input:{prompt:'Build the report',sessionId:'session-1',
      maxSteps:60,strategy:'verify_first',requireEvidence:true,allowFallback:false}
  });
});
