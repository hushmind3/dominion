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

test('native run controls share the real Rust autonomous UI job routes',async()=>{
  const calls=[];
  const sdk=new Dominion({adminToken:'owner-token',fetch:async(url,init)=>{
    calls.push({url,init});
    return reply({id:'native/1',instruction:'Fix tests',status:'running'});
  }});
  await sdk.nativeRuns.list();
  await sdk.nativeRuns.start({instruction:'Fix tests',providerId:'p1',maxRounds:8,verifyCommand:'cargo test'});
  await sdk.nativeRuns.get('native/1');
  await sdk.nativeRuns.approve('native/1','approval/3');
  await sdk.nativeRuns.cancel('native/1');
  await sdk.nativeRuns.resume('native/1');
  await sdk.nativeRuns.reconcile('native/1',{applied:false,details:'Workspace checked'});
  assert.deepEqual(calls.map(call=>new URL(call.url).pathname),[
    '/api/sdk/runs','/api/sdk/runs','/api/sdk/runs/native%2F1',
    '/api/sdk/runs/native%2F1/approve','/api/sdk/runs/native%2F1/cancel',
    '/api/sdk/runs/native%2F1/resume','/api/sdk/runs/native%2F1/reconcile'
  ]);
  assert.deepEqual(JSON.parse(calls[1].init.body),{
    instruction:'Fix tests',providerId:'p1',maxRounds:8,verifyCommand:'cargo test'
  });
  assert.deepEqual(JSON.parse(calls[3].init.body),{callId:'approval/3'});
  assert.deepEqual(JSON.parse(calls[6].init.body),{applied:false,details:'Workspace checked'});
  assert.ok(calls.every(call=>call.init.headers['X-Dominion-Token']==='owner-token'));
});
test('offline replay and paired cost comparison use exact Rust read-only endpoints',async()=>{
  const calls=[];
  const sdk=new Dominion({adminToken:'owner',fetch:async(url,init)=>{
    calls.push({url,init});
    return reply({mode:'offline_decision_and_receipt_reconstruction',externalCalls:0});
  }});
  await sdk.jobs.replay('base/a');
  await sdk.jobs.compare('base/a','candidate/b');
  await sdk.nativeRuns.replay('run/1');
  await sdk.nativeRuns.compare('run/1','candidate/b');
  assert.deepEqual(calls.map(c=>new URL(c.url).pathname),[
    '/api/jobs/base%2Fa/replay','/api/jobs/base%2Fa/compare',
    '/api/sdk/runs/run%2F1/replay','/api/sdk/runs/run%2F1/compare'
  ]);
  assert.deepEqual(JSON.parse(calls[1].init.body),{candidateRunId:'candidate/b'});
  assert.deepEqual(JSON.parse(calls[3].init.body),{candidateRunId:'candidate/b'});
  assert.equal(calls[0].init.method,'GET');
  assert.equal(calls[1].init.method,'POST');
  assert.ok(calls.every(c=>c.init.headers['X-Dominion-Token']==='owner'));
});
test('native approval conflict is never turned into optimistic success',async()=>{
  const sdk=new Dominion({fetch:async()=>reply({error:'Approval expired'},409)});
  await assert.rejects(()=>sdk.nativeRuns.approve('n1','expired'),error=>
    error instanceof DominionApiError&&error.status===409);
});

test('SSE accepts many small events coalesced into a single >2 MiB transport chunk',async()=>{
  const payload=Array.from({length:400},(_,i)=>
    'data: '+JSON.stringify({seq:i+1,type:'record',detail:'x'.repeat(6500)})+'\n\n'
  ).join('');
  assert.ok(payload.length>2*1024*1024);
  const sdk=new Dominion({fetch:async()=>sse(payload)});
  let count=0;
  for await(const event of sdk.events()){
    count++;
    assert.equal(event.seq,count);
  }
  assert.equal(count,400);
});
test('SSE still rejects a single oversized frame',async()=>{
  const huge='data: '+JSON.stringify({seq:1,type:'record',detail:'x'.repeat(2*1024*1024)})+'\n\n';
  const sdk=new Dominion({fetch:async()=>sse(huge)});
  await assert.rejects(async()=>{
    for await(const event of sdk.events()){void event;}
  },error=>error instanceof DominionStreamError&&error.message.includes('larger than 2 MiB'));
});

test('native run wait returns at approval/recovery instead of hanging for completion',async()=>{
  let reads=0;
  const sdk=new Dominion({fetch:async()=>{
    reads++;
    return reply({
      id:'r',instruction:'Improve code',
      status:reads===1?'running':'awaiting_approval',
      checkpoint:{pendingCall:{id:'approval1',name:'workspace_write',arguments:{path:'x'}}}
    });
  }});
  const result=await sdk.nativeRuns.wait('r',{intervalMs:100,timeoutMs:1000});
  assert.equal(result.status,'awaiting_approval');
  assert.equal(result.checkpoint.pendingCall.id,'approval1');
  assert.equal(reads,2);
  const cancelled=new AbortController();cancelled.abort();
  await assert.rejects(()=>sdk.nativeRuns.wait('r',{signal:cancelled.signal}),error=>error.name==='AbortError');
  assert.equal(reads,2,'aborted waits must not send a request');
});
test('native wait exposes manual recovery and does not infer task success',async()=>{
  const sdk=new Dominion({fetch:async()=>reply({id:'r2',instruction:'Edit',status:'in_doubt'})});
  assert.equal((await sdk.nativeRuns.wait('r2',{timeoutMs:100})).status,'in_doubt');
});

test('SSE accepts mixed CRLF + LF, LF + CRLF, and CR + CRLF separators',async()=>{
  const sdk=new Dominion({fetch:async()=>sse(
    'data: {"seq":1,"type":"mixed-one"}\r\n\n',
    'data: {"seq":2,"type":"mixed-two"}\n\r\n',
    'data: {"seq":3,"type":"mixed-three"}\r\r\n'
  )});
  const found=[];
  for await(const event of sdk.events()){found.push(event.type);}
  assert.deepEqual(found,['mixed-one','mixed-two','mixed-three']);
});
