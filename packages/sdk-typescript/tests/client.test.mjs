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
