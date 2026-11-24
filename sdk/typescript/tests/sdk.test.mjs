import { test } from 'node:test';
import assert from 'node:assert/strict';
import { Dominion, DominionError, StreamInterruptedError, ChatTurnFailedError } from '../dist/index.js';

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
test('direct Chat and immutable attachments share typed Rust API and auth',async()=>{
  const calls=[];
  const client=new Dominion({token:'private',fetch:async(url,init)=>{
    calls.push({url,init});
    if(url.endsWith('/content'))
      return new Response(new Uint8Array([65,66]),{headers:{'content-type':'text/plain'}});
    if(url.endsWith('/api/chat'))
      return sse(['data: {"type":"done"}\n\n']);
    return json({id:'attachment-id',name:'report.md',sessionId:'s',roomId:null,
      mimeType:'text/plain; charset=utf-8',size:6,isImage:false,
      supportedForModel:'text',url:'/api/attachments/attachment-id/content'});
  }});
  await client.sessions.create({title:'Normal chat',conversationMode:'chat',providerId:'provider'});
  const uploaded=await client.attachments.upload({
    sessionId:'s',name:'report.md',mimeType:'text/plain',data:Buffer.from('hello!').toString('base64')
  });
  assert.equal(uploaded.id,'attachment-id');
  const bytes=await client.attachments.content('attachment-id');
  assert.deepEqual([...new Uint8Array(bytes)],[65,66]);
  await client.streamChat({sessionId:'s',input:'summarize',conversationMode:'chat',
    webSearch:true,attachmentIds:[uploaded.id],toolsEnabled:false,toolsExecutionEnabled:false},()=>{});
  assert.equal(JSON.parse(calls[0].init.body).conversationMode,'chat');
  assert.equal(calls[1].url,'http://127.0.0.1:4321/api/attachments');
  assert.equal(JSON.parse(calls[1].init.body).sessionId,'s');
  assert.equal(calls[2].url,'http://127.0.0.1:4321/api/attachments/attachment-id/content');
  assert.equal(calls[3].url,'http://127.0.0.1:4321/api/chat');
  const payload=JSON.parse(calls[3].init.body);
  assert.equal(payload.agentId,undefined);
  assert.equal(payload.conversationMode,'chat');
  assert.equal(payload.webSearch,true);
  assert.deepEqual(payload.attachmentIds,['attachment-id']);
  assert.equal(payload.toolsExecutionEnabled,false);
  assert.ok(calls.every(call=>call.init.headers.get('X-Dominion-Token')==='private'));
});
test('external URL and traversal fail locally before credentials are sent', async () => {
  let calls=0;
  const d=new Dominion({token:'secret',fetch:async()=>{calls++;return json({});}});
  assert.throws(()=>d.raw('https://attacker.invalid/api/jobs'),TypeError);
  assert.throws(()=>d.raw('/api/../../evil'),TypeError);
  assert.equal(calls,0);
});
test('private Chat deletion reaches Rust DELETE without retry and honors active-run conflict',async()=>{
  const calls=[];
  const client=new Dominion({token:'private',fetch:async(url,init)=>{
    calls.push({url,init});
    if(url.endsWith('/busy'))return json({error:'실행 중인 대화입니다.'},409);
    return json({ok:true});
  }});
  const result=await client.sessions.delete('private/one');
  assert.equal(result.ok,true);
  assert.equal(calls[0].url,'http://127.0.0.1:4321/api/sessions/private%2Fone');
  assert.equal(calls[0].init.method,'DELETE');
  assert.equal(calls[0].init.headers.get('X-Dominion-Token'),'private');
  await assert.rejects(client.sessions.delete('busy'),
    error=>error instanceof DominionError && error.status===409);
  assert.equal(calls.length,2,'DELETE must never auto-retry an uncertain mutation');
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
test('Rust in-band chat failure rejects even when transport follows with done',async()=>{
  const seen=[];let posts=0;
  const client=new Dominion({fetch:async(url,init)=>{
    assert.ok(url.endsWith('/api/chat'));
    assert.equal(init.method,'POST');
    posts++;
    return sse([
      'data: {"type":"text","text":"partial model output"}\n\n',
      'data: {"type":"error","message":"모델 전송 실패","executionId":"failed-run"}\n\n',
      'data: {"type":"done","sessionId":"s"}\n\n',
    ]);
  }});
  await assert.rejects(
    client.streamChat({sessionId:'s',conversationMode:'chat',input:'work'},event=>seen.push(event.type)),
    error=>error instanceof ChatTurnFailedError
      && error.message==='모델 전송 실패'
      && error.event.executionId==='failed-run'
  );
  assert.deepEqual(seen,['text','error']);
  assert.equal(posts,1,'failed paid Chat POST must never be retried');
});
test('missing in-band failure message still rejects with a typed error',async()=>{
  const client=new Dominion({fetch:async()=>sse([
    'data: {"type":"error"}\n\n','data: {"type":"done"}\n\n'
  ])});
  await assert.rejects(client.streamChat({sessionId:'s',input:'work'},()=>{}),
    error=>error instanceof ChatTurnFailedError && error.message==='Chat turn failed');
});
test('SDK cancels exact delayed Chat request and waits for durable server release',async()=>{
  const requestId='123e4567-e89b-42d3-a456-426614174000';
  const calls=[];
  const client=new Dominion({token:'private',fetch:async(url,init)=>{
    calls.push({url,init});
    return json(url.endsWith('/cancel-status')
      ?{sessionId:'session-1',stopped:true,active:false,running:false}
      :{cancelled:true,requestRecorded:true,running:true});
  }});
  const ack=await client.sessions.cancelTurn('session-1',requestId);
  const state=await client.sessions.cancelStatus('session-1');
  assert.equal(ack.requestRecorded,true);
  assert.equal(state.stopped,true);
  assert.deepEqual(JSON.parse(calls[0].init.body),{requestId});
  assert.equal(calls[0].url,'http://127.0.0.1:4321/api/sessions/session-1/cancel');
  assert.equal(calls[1].url,'http://127.0.0.1:4321/api/sessions/session-1/cancel-status');
  assert.equal(calls[1].init.method,'GET');
  assert.ok(calls.every(call=>call.init.headers.get('X-Dominion-Token')==='private'));
});
test('SDK rejects malformed Chat cancellation UUID before contacting Rust',()=>{
  let calls=0;
  const client=new Dominion({fetch:async()=>{calls++;return json({});}});
  assert.throws(()=>client.sessions.cancelTurn('session-1','not-a-request-uuid'),TypeError);
  assert.equal(calls,0);
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
  assert.equal(urls[0],'http://127.0.0.1:4321/api/events?after=9');
  assert.equal(urls[1],'http://127.0.0.1:4321/api/events?after=10');
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
  assert.equal(last.url,'http://127.0.0.1:4321/api/jobs/same/steer');
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
  assert.equal(call.url,'http://127.0.0.1:4321/api/jobs/abc/steer');
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
  assert.equal(calls[0].url,'http://127.0.0.1:4321/api/evaluation/summary');
  assert.equal(calls[0].method,'GET');
  assert.equal(result.verified.successRate,0.5);
});


test('typed runEvents includes persisted Chat Room and SDK states on one ordered cursor',async()=>{
  const reply=[
    {seq:1,type:'record',kind:'job',record:{id:'job-1',status:'queued'}},
    {seq:2,eventSeq:2,type:'run_state',schemaVersion:1,runKind:'job',runId:'job-1',
      sourceMessageId:null,agentId:null,state:'queued'},
    {seq:3,eventSeq:999,type:'run_state',schemaVersion:1,runKind:'execution',
      runId:'forgery',sourceMessageId:null,agentId:null,state:'completed'},
    {seq:4,eventSeq:4,type:'run_state',schemaVersion:1,runKind:'room_work_assignment',
      runId:'room-1',sourceMessageId:'owner-1',agentId:'worker',state:'completed'}
  ];
  const d=new Dominion({fetch:async()=>sse(
    reply.map(x=>'id: '+x.seq+'\ndata: '+JSON.stringify(x)+'\n\n')
  )});
  const received=[];
  for await(const event of d.runEvents({reconnectDelayMs:1})){
    received.push(event);
    if(received.length===2) break;
  }
  assert.deepEqual(received.map(x=>[x.runKind,x.runId,x.eventSeq]),[
    ['job','job-1',2],['room_work_assignment','room-1',4]
  ]);
  assert.equal(received[0].sourceMessageId,null);
  assert.equal(received[1].sourceMessageId,'owner-1');
});

test('managed Lead and Worker DAG events are typed on existing SSE cursor',async()=>{
  const events=[
    {seq:1,eventSeq:1,type:'run_state',schemaVersion:1,
      runKind:'managed_team',runId:'team-1',
      sourceMessageId:null,agentId:'lead',state:'running',parentRunId:'chat-1'},
    {seq:2,eventSeq:2,type:'run_state',schemaVersion:1,
      runKind:'managed_team_task',runId:'worker-1',
      sourceMessageId:null,agentId:'coder',state:'queued',
      parentRunId:'team-1',nodeId:'implement',dependsOn:['inspect']},
    {seq:3,eventSeq:3,type:'run_state',schemaVersion:1,
      runKind:'managed_team_task',runId:'worker-1',
      sourceMessageId:null,agentId:'coder',state:'completed',
      parentRunId:'team-1',nodeId:'implement',verified:true,workerJobId:'job-1'}
  ];
  const client=new Dominion({fetch:async()=>sse(
    events.map(event=>'id: '+event.seq+'\ndata: '+JSON.stringify(event)+'\n\n')
  )});
  const observed=[];
  for await(const event of client.runEvents({reconnectDelayMs:1})){
    observed.push(event);
    if(observed.length===3)break;
  }
  assert.deepEqual(observed.map(x=>[x.runKind,x.state]),[
    ['managed_team','running'],['managed_team_task','queued'],
    ['managed_team_task','completed']
  ]);
  assert.equal(observed[2].workerJobId,'job-1');
  assert.equal(observed[2].verified,true);
});

test('distributed Rust Worker leases and receipts use the same typed durable SSE cursor',async()=>{
  const events=[
    {seq:1,eventSeq:1,type:'run_state',schemaVersion:1,
      runKind:'remote_task',runId:'remote-task-1',state:'queued',
      sourceMessageId:null,agentId:'worker-agent',
      parentRunId:'team-1',nodeId:'research',remoteWorkerId:'node-b'},
    {seq:2,eventSeq:2,type:'run_state',schemaVersion:1,
      runKind:'remote_task',runId:'remote-task-1',state:'running',
      sourceMessageId:null,agentId:'worker-agent',remoteWorkerId:'node-b',
      leaseUntilMs:123456},
    {seq:3,eventSeq:3,type:'run_state',schemaVersion:1,
      runKind:'remote_task',runId:'remote-task-1',state:'completed',
      sourceMessageId:null,agentId:'worker-agent',remoteWorkerId:'node-b',
      workerJobId:'local-run-1',verified:true,
      evidenceOrigin:'remote_worker_reported',
      remoteUsage:{calls:2,promptTokens:1200,completionTokens:300,
        totalTokens:1500,cachedPromptTokens:200,
        cacheObservedCalls:1,estimatedCalls:0}}
  ];
  const client=new Dominion({fetch:async()=>sse(
    events.map(event=>'id: '+event.seq+'\ndata: '+JSON.stringify(event)+'\n\n')
  )});
  const received=[];
  for await(const event of client.runEvents({reconnectDelayMs:1})){
    received.push(event);
    if(received.length===3)break;
  }
  assert.deepEqual(received.map(x=>[x.runKind,x.state]),[
    ['remote_task','queued'],['remote_task','running'],['remote_task','completed']
  ]);
  assert.equal(received[2].evidenceOrigin,'remote_worker_reported');
  assert.equal(received[2].workerJobId,'local-run-1');
  assert.equal(received[2].remoteUsage.cachedPromptTokens,200);
  assert.equal(received[2].remoteUsage.totalTokens,1500);
  assert.equal(received[0].remoteWorkerId,'node-b');
});

test('extension lifecycle uses its own authenticated endpoints and explicit owner confirmation', async()=>{
  const calls=[];
  const d=new Dominion({token:'owner',fetch:async(url,init)=>{calls.push({url,init});return json({id:'fixture'});}});
  const manifest={sdkVersion:1,id:'fixture',name:'Fixture',version:'1.0.0',description:'Test',runtime:'node',artifact:{url:'https://example.invalid/entry.mjs',sha256:'0'.repeat(64)},tools:[{name:'echo',description:'Echo',parameters:{type:'object'}}]};
  const settings={urls:['https://example.invalid/catalog.json'],githubDiscovery:false};
  await d.extensions.registries();
  await d.extensions.saveRegistries(settings);
  await d.extensions.search('한글 & echo');
  await d.extensions.install({manifest,source:settings.urls[0]},true);
  await d.extensions.importBundle({bundleVersion:1,manifest,artifactBase64:'YQ=='},true);
  await d.extensions.enable('fixture',true);
  await d.extensions.status('fixture');
  await d.extensions.disable('fixture');
  await d.extensions.list();
  await d.extensions.remove('fixture');
  await d.extensions.versions('fixture');
  assert.deepEqual(calls.map(c=>new URL(c.url).pathname),[
    '/api/extensions/registries','/api/extensions/registries','/api/extensions/catalog',
    '/api/extensions','/api/extensions/import','/api/extensions/fixture/enable',
    '/api/extensions/fixture/status','/api/extensions/fixture/enable','/api/extensions','/api/extensions/fixture','/api/extensions/fixture/versions']);
  assert.equal(new URL(calls[2].url).searchParams.get('q'),'한글 & echo');
  assert.deepEqual(JSON.parse(calls[1].init.body),settings);
  assert.equal(JSON.parse(calls[3].init.body).confirmed,true);
  assert.equal(JSON.parse(calls[4].init.body).artifactBase64,'YQ==');
  assert.deepEqual(JSON.parse(calls[5].init.body),{enabled:true,confirmed:true});
  assert.deepEqual(JSON.parse(calls[7].init.body),{enabled:false});
  assert.equal(calls[9].init.method,'DELETE');
  assert.ok(calls.every(c=>c.init.headers.get('X-Dominion-Token')==='owner'));
});
