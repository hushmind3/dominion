import test from 'node:test'
import assert from 'node:assert/strict'
import {DominionAgentClient, DominionSdkError, isRunSettled} from '../dist/index.js'

const mockClient = snapshots => {
  let calls = 0
  const client = new DominionAgentClient({
    fetch: async (url, request) => {
      assert.match(String(url), /\/api\/sdk\/runs\/run%20id$/)
      assert.equal(request.method, 'GET')
      const run = snapshots[Math.min(calls++, snapshots.length - 1)]
      return new Response(JSON.stringify(run), {status: 200})
    },
  })
  return {client, calls: () => calls}
}

test('waitForRun uses durable progress and stops at approval without auto-granting', async () => {
  const {client, calls} = mockClient([
    {id:'run id',status:'running'},
    {id:'run id',status:'awaiting_approval',checkpoint:{pendingCall:{id:'a'}}},
  ])
  const statuses = []
  const run = await client.waitForRun('run id', {
    intervalMs: 50,timeoutMs: 1_000,onUpdate: item => statuses.push(item.status),
  })
  assert.equal(run.status, 'awaiting_approval')
  assert.deepEqual(statuses, ['running','awaiting_approval'])
  assert.equal(calls(), 2)
  assert.equal(isRunSettled(run), true)
})

test('waitForVerifiedRun rejects a model answer without outcome evidence', async () => {
  const {client} = mockClient([{id:'run id',status:'completed',answer:'done',
    verified:false,taskOutcome:'answered_unverified'}])
  await assert.rejects(client.waitForVerifiedRun('run id'), error =>
    error instanceof DominionSdkError && error.status === 422)
})

test('waitForVerifiedRun accepts independent success', async () => {
  const {client} = mockClient([{id:'run id',status:'completed',
    verified:true,taskOutcome:'succeeded'}])
  const run = await client.waitForVerifiedRun('run id')
  assert.equal(run.verified, true)
})

test('waitForRun times out without cancelling remote work', async () => {
  const {client} = mockClient([{id:'run id',status:'running'}])
  await assert.rejects(client.waitForRun('run id',{intervalMs:50,timeoutMs:0}),
    error => error instanceof DominionSdkError && error.status === 408)
})

test('run timeline keeps native execution identity and cursor across SDK facade', async () => {
  const paths = [];
  const client = new DominionAgentClient({
    fetch: async url => {
      paths.push(String(url));
      if (String(url).includes('/api/sdk/runs/')) {
        return new Response(JSON.stringify({
          id:'sdk id', status:'running',
          runRef:{schemaVersion:1,executionId:'native/execution',sessionId:'s'},
        }), {status:200});
      }
      return new Response(JSON.stringify({
        schemaVersion:1,executionId:'native/execution',
        after:5,nextAfter:8,hasMore:false,earliestAvailableSeq:1,
        historyTruncated:false,
        events:[{seq:8,type:'record',kind:'execution',
          runRef:{schemaVersion:1,executionId:'native/execution',lifecycle:'completed'}}],
      }), {status:200});
    },
  });
  const page = await client.getRunEvents('sdk id', {after:5,limit:10});
  assert.equal(page.nextAfter,8);
  assert.equal(page.events[0].runRef.lifecycle,'completed');
  assert.match(paths[0],/\/api\/sdk\/runs\/sdk%20id$/);
  assert.match(paths[1],/\/api\/executions\/native%2Fexecution\/events\?after=5&limit=10$/);
});

test('unadmitted SDK run does not invent an execution timeline', async () => {
  const client = new DominionAgentClient({
    fetch: async () => new Response(JSON.stringify({id:'sdk',status:'queued'}),{status:200}),
  });
  await assert.rejects(client.getRunEvents('sdk'),error=>
    error instanceof DominionSdkError && error.status===409);
});

test('remote SDK jobs stay unverified after owner review and use URL-safe identity', async () => {
  const observed=[];
  const client = new DominionAgentClient({
    fetch: async (url, request) => {
      observed.push({url: String(url), method: request.method,
        body: request.body ? JSON.parse(request.body) : null});
      return new Response(JSON.stringify({
        id:'remote:job 1',kind:'remote_sdk',status:'owner_accepted',
        instruction:'inspect',nativeVerified:false
      }),{status:200});
    }
  });
  await client.createRemoteJob({instruction:'inspect',maxRounds:4});
  await client.reviewRemoteJob('remote:job 1',true);
  assert.match(observed[0].url,/\/api\/remote\/jobs$/);
  assert.equal(observed[0].body.instruction,'inspect');
  assert.match(observed[1].url,/\/api\/remote\/jobs\/remote%3Ajob%201\/review$/);
  assert.deepEqual(observed[1].body,{accept:true});
  const job=await client.getRemoteJob('remote:job 1');
  assert.equal(job.nativeVerified,false);
});

test('remote lease reconciliation is explicit and never reports verified work', async () => {
  const seen=[];
  const client=new DominionAgentClient({fetch: async (url, options)=>{
    seen.push({url:String(url),method:options.method});
    return new Response(JSON.stringify({count:1,needsReview:['remote:unknown']}),{status:200});
  }});
  const result=await client.reconcileRemoteLeases();
  assert.equal(result.count,1);
  assert.deepEqual(result.needsReview,['remote:unknown']);
  assert.match(seen[0].url,/\/api\/remote\/reconcile$/);
  assert.equal(seen[0].method,'POST');
});


test('P3 owner worker enrollment and pinned dispatch have stable SDK contracts',async()=>{
  const seen=[];
  const client=new DominionAgentClient({fetch:async(url,request)=>{
    const path=String(url),body=request.body?JSON.parse(request.body):null;
    seen.push({path,body,method:request.method});
    let result;
    if(path.endsWith('/api/remote/workers')&&request.method==='GET'){
      result=[{id:'node-1',kind:'remote_worker',status:'enabled',
        protocolVersion:1,capabilities:['sdk']}];
    }else if(path.endsWith('/api/remote/workers')||path.endsWith('/rotate')){
      result={worker:{id:'node-1',status:'enabled',capabilities:['sdk']},token:'once'};
    }else if(path.endsWith('/state')){
      result={id:'node-1',status:'disabled',capabilities:['sdk']};
    }else{
      result={id:'remote:job',kind:'remote_sdk',status:'queued',
        nativeVerified:false,instruction:'inspect',
        targetWorkerId:'node-1',requiredCapabilities:['sdk']};
    }
    return new Response(JSON.stringify(result),{status:200});
  }});
  const enrollment=await client.enrollRemoteWorker('node-1',['sdk']);
  assert.equal(enrollment.token,'once');
  const workers=await client.listRemoteWorkers();
  assert.equal(workers[0].token,undefined);
  await client.createRemoteJob({instruction:'inspect',
    targetWorkerId:'node-1',requiredCapabilities:['sdk']});
  await client.rotateRemoteWorkerToken('node-1');
  await client.setRemoteWorkerEnabled('node-1',false);
  assert.deepEqual(seen[0].body,{workerId:'node-1',capabilities:['sdk']});
  assert.equal(seen[2].body.targetWorkerId,'node-1');
  assert.deepEqual(seen[2].body.requiredCapabilities,['sdk']);
  assert.match(seen[3].path,/\/remote\/workers\/node-1\/rotate$/);
  assert.deepEqual(seen[4].body,{enabled:false});
});

test('remote Git artifact requires separate owner review and never native-verifies',async()=>{
  const observed=[];
  const client=new DominionAgentClient({fetch:async(url,options)=>{
    observed.push({url:String(url),method:options.method,
      body:options.body?JSON.parse(options.body):undefined});
    const result=options.method==='GET'
      ?{manifest:{id:'remote-artifact:job',jobId:'remote:job',workerId:'node',
          patchSha256:'a'.repeat(64),patchBytes:42,baseCommit:'b'.repeat(40),
          status:'quarantined',nativeVerified:false},patch:'diff --git a/a b/a'}
      :{id:'remote-artifact:job',jobId:'remote:job',
          status:'owner_accepted',nativeVerified:false};
    return new Response(JSON.stringify(result),{status:200});
  }});
  const stored=await client.getRemoteArtifact('remote:job');
  assert.equal(stored.manifest.nativeVerified,false);
  assert.match(stored.patch,/^diff --git/);
  const reviewed=await client.reviewRemoteArtifact('remote:job',true);
  assert.equal(reviewed.nativeVerified,false);
  assert.match(observed[0].url,/\/api\/remote\/jobs\/remote%3Ajob\/artifact$/);
  assert.match(observed[1].url,/\/api\/remote\/jobs\/remote%3Ajob\/artifact\/review$/);
  assert.deepEqual(observed[1].body,{accept:true});
});

test('room cursor is scoped to the room and never aliases global SSE position', async () => {
  const visited=[];
  const client=new DominionAgentClient({fetch:async url=>{
    visited.push(String(url));
    return new Response(JSON.stringify({schemaVersion:1,roomId:'room/a',
      after:120,nextAfter:122,hasMore:false,
      events:[{schemaVersion:1,roomId:'room/a',seq:122,eventId:'agent-reply',
        author:'agent-a',replyTo:'owner',originRun:'native-exec',
        kind:'agent_reply',createdAt:'2026-11-21'}]}),{status:200});
  }});
  const page=await client.getRoomEvents('room/a',{after:120,limit:50});
  assert.equal(page.nextAfter,122);
  assert.equal(page.events[0].originRun,'native-exec');
  assert.match(visited[0],/\/api\/rooms\/room%2Fa\/events\?after=120&limit=50$/);
});


test('needs_review stops polling instead of replaying completed work',async()=>{
  const {client,calls}=mockClient([{id:'run id',status:'needs_review'}]);
  assert.equal((await client.waitForRun('run id')).status,'needs_review');
  assert.equal(calls(),1);
  await assert.rejects(client.waitForVerifiedRun('run id'), e=>e.status===409);
});
test('start forwards caller retry identity and provenance',async()=>{
  const client=new DominionAgentClient({fetch:async(url,request)=>{
    assert.match(String(url),/\/api\/sdk\/runs$/);
    assert.equal(request.method,'POST');
    assert.deepEqual(JSON.parse(request.body),{instruction:'goal',idempotencyKey:'retry-1',agentId:'a',source:{kind:'issue',id:'1'}});
    return new Response(JSON.stringify({id:'r',status:'queued'}));
  }});
  await client.startRun({instruction:'goal',idempotencyKey:'retry-1',agentId:'a',source:{kind:'issue',id:'1'}});
});

test('wait deadline aborts an in-flight HTTP read without cancelling the run', async()=>{
  let called=0;
  const client=new DominionAgentClient({fetch:async(_url,request)=>{
    called++;
    assert.equal(request.method,'GET');
    return new Promise((_resolve,reject)=>{
      request.signal.addEventListener('abort',()=>reject(request.signal.reason),{once:true});
    });
  }});
  await assert.rejects(client.waitForRun('r',{timeoutMs:25}), e=>e instanceof DominionSdkError&&e.status===408);
  assert.equal(called,1);
});
test('caller abort reaches the HTTP read and retains its reason', async()=>{
  const controller=new AbortController();
  const reason=new Error('caller stopped waiting');
  const client=new DominionAgentClient({fetch:async(_url,request)=>{
    controller.abort(reason);
    assert.equal(request.signal.aborted,true);
    throw request.signal.reason;
  }});
  await assert.rejects(client.waitForRun('r',{signal:controller.signal}), e=>e===reason);
});
test('source history and route metadata use the existing server routes', async()=>{
  const calls=[];
  const client=new DominionAgentClient({fetch:async(url,request)=>{
    calls.push({url:String(url),method:request.method,body:request.body});
    return new Response(JSON.stringify(request.method==='POST'?[]:{}));
  }});
  await client.runsBySource('issue');
  await client.inferenceSupport({providerId:'p/1',model:'m x'});
  await client.inferenceSupportBatch([{providerId:'p/1',model:'m x'}]);
  assert.match(calls[0].url,/\/api\/sdk\/runs\/source\/issue$/);
  assert.match(calls[1].url,/\/api\/chat\/inference-support\?providerId=p%2F1&model=m\+x$/);
  assert.deepEqual(JSON.parse(calls[2].body),[{providerId:'p/1',model:'m x'}]);
});

test('external app lifecycle uses the same SDK and reserved source',async()=>{
 const calls=[]
 const manifest={schemaVersion:1,id:'example.assistant',name:'Assistant',version:'1.0.0'}
 const client=new DominionAgentClient({fetch:async(url,request)=>{
  calls.push([new URL(url).pathname,request.method,request.body?JSON.parse(request.body):null])
  return new Response(JSON.stringify({id:'r',status:'queued'}))
 }})
 await client.registerExtension(manifest)
 await client.setExtensionEnabled(manifest.id,true)
 await client.startExtensionRun(manifest.id,{instruction:'test',idempotencyKey:'retry'})
 await client.setExtensionEnabled(manifest.id,false)
 await client.unregisterExtension(manifest.id)
 assert.deepEqual(calls,[
  ['/api/extensions/apps','POST',manifest],
  ['/api/extensions/apps/example.assistant/state','POST',{enabled:true}],
  ['/api/sdk/runs','POST',{instruction:'test',idempotencyKey:'retry',source:{kind:'extension',id:'example.assistant'}}],
  ['/api/extensions/apps/example.assistant/state','POST',{enabled:false}],
  ['/api/extensions/apps/example.assistant','DELETE',null]
 ])
})

test('scoped extension sends no admin credential or owner options',async()=>{
 const {DominionExtensionClient}=await import('../dist/index.js')
 const token='a'.repeat(64),run='12345678-1234-1234-1234-123456789012'
 const calls=[]
 const client=new DominionExtensionClient({id:'example.assistant',token,fetch:async(url,request)=>{
  calls.push([new URL(url).pathname,request.method,request.body?JSON.parse(request.body):null])
  assert.equal(request.headers['x-dominion-extension-token'],token)
  assert.equal(request.headers['x-dominion-token'],undefined)
  assert.equal(request.redirect,'error')
  return new Response(JSON.stringify({id:run,status:'completed',answer:'OK'}))
 }})
 await client.start('hello','retry')
 await client.get(run)
 assert.deepEqual(calls,[['/api/extensions/apps/example.assistant/runs','POST',{instruction:'hello',idempotencyKey:'retry'}],['/api/extensions/apps/example.assistant/runs/'+run,'GET',null]])
 assert.throws(()=>client.get('../settings'))
})

test('scoped task workflow uses explicit follow-up and immutable artifact routes',async()=>{
 const {DominionExtensionClient}=await import('../dist/index.js')
 const id='a'.repeat(64),artifact='12345678-1234-1234-1234-123456789012',token='b'.repeat(64),calls=[]
 const client=new DominionExtensionClient({id:'example.assistant',token,fetch:async(url,request)=>{
  assert.equal(request.headers['x-dominion-extension-token'],token)
  assert.equal(request.headers['x-dominion-token'],undefined)
  calls.push([new URL(url).pathname,request.method,request.body?JSON.parse(request.body):null])
  return new Response(JSON.stringify({id,status:'approved',runStatus:'completed',message:'released'}))
 }})
 await client.proposeTask('first','key')
 await client.proposeFollowUp(id,'second','next')
 await client.artifact(id,artifact)
 await client.waitTaskRequest(id,{requireMessage:true,timeoutMs:1000})
 assert.deepEqual(calls,[
 ['/api/extensions/apps/example.assistant/requests','POST',{instruction:'first',idempotencyKey:'key'}],
 ['/api/extensions/apps/example.assistant/requests','POST',{instruction:'second',idempotencyKey:'next',parentRequestId:id}],
 ['/api/extensions/apps/example.assistant/requests/'+id+'/artifacts/'+artifact,'GET',null],
 ['/api/extensions/apps/example.assistant/requests/'+id,'GET',null]
 ])
 assert.throws(()=>client.artifact(id,'../private'))
})
test('scoped publication wait does not mistake completion for released data',async()=>{
 const {DominionExtensionClient}=await import('../dist/index.js')
 let reads=0
 const id='a'.repeat(64)
 const client=new DominionExtensionClient({id:'example.assistant',token:'b'.repeat(64),fetch:async(_url,request)=>{
  assert.equal(request.method,'GET');reads++
  return new Response(JSON.stringify({id,status:'approved',runStatus:'completed',message:reads===1?null:'released'}))
 }})
 const receipt=await client.waitTaskRequest(id,{requireMessage:true,intervalMs:50,timeoutMs:1000})
 assert.equal(reads,2)
 assert.equal(receipt.message,'released')
})
