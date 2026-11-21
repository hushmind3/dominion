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
