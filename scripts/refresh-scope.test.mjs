import test from 'node:test'
import assert from 'node:assert/strict'
import {classifyRefreshEvent,matchesRefreshTarget,mergeRefreshScopes,emptyRefreshScope} from '../web/src/refreshEvents.ts'

test('high-frequency receipt events cannot reload unrelated provider lists',()=>{
 const operation=classifyRefreshEvent({type:'record',kind:'execution_operation',
  record:{id:'op-1',runId:'run-1',sessionId:'chat-1'}})
 assert.ok(matchesRefreshTarget('/api/executions?limit=100',operation.targets))
 assert.equal(matchesRefreshTarget('/api/providers',operation.targets),false)
 assert.equal(matchesRefreshTarget('/api/oauth/connections',operation.targets),false)
})

test('message and audit events affect only their associated data families',()=>{
 const message=classifyRefreshEvent({type:'message',message:{id:'m',session_id:'abc'}})
 assert.ok(matchesRefreshTarget('/api/sessions/abc/messages',message.targets))
 assert.equal(matchesRefreshTarget('/api/rooms',message.targets),false)
 assert.equal(matchesRefreshTarget('/api/providers',message.targets),false)
 const audit=classifyRefreshEvent({type:'audit'})
 assert.ok(matchesRefreshTarget('/api/audit',audit.targets))
 assert.equal(matchesRefreshTarget('/api/tools',audit.targets),false)
 const approval=classifyRefreshEvent({type:'approval'})
 assert.ok(matchesRefreshTarget('/api/approvals',approval.targets))
 assert.equal(matchesRefreshTarget('/api/providers',approval.targets),false)
})

test('merged, unknown and manual refresh events cannot lose updates',()=>{
 const operation=classifyRefreshEvent({type:'record',kind:'execution_operation',
  record:{id:'op-1',runId:'run-1',sessionId:'chat-1'}})
 const approval=classifyRefreshEvent({type:'approval'})
 const merged=mergeRefreshScopes(operation,approval)
 assert.ok(matchesRefreshTarget('/api/approvals',merged.targets))
 assert.equal(matchesRefreshTarget('/api/providers',merged.targets),false)
 assert.ok(matchesRefreshTarget('/api/providers',classifyRefreshEvent({
  type:'record',kind:'provider',record:{id:'provider-1'}}).targets))
 assert.deepEqual(emptyRefreshScope(),{targets:[],sessions:[],rooms:[]})
})
