import test from 'node:test'
import assert from 'node:assert/strict'
import {refreshScopeForEvent,shouldRefreshEndpoint} from '../web/src/refreshScope.ts'

test('high-frequency receipt events cannot reload unrelated provider lists',()=>{
 assert.equal(refreshScopeForEvent({type:'record',kind:'execution_operation'}),
  'record:execution_operation')
 assert.equal(shouldRefreshEndpoint('/api/executions?limit=100',
  ['record:execution_operation']),true)
 assert.equal(shouldRefreshEndpoint('/api/jobs/one',
  ['record:execution_operation']),true)
 assert.equal(shouldRefreshEndpoint('/api/providers',
  ['record:execution_operation']),false)
 assert.equal(shouldRefreshEndpoint('/api/oauth/connections',
  ['record:execution_operation']),false)
})

test('message and audit events affect only their associated data families',()=>{
 assert.equal(shouldRefreshEndpoint('/api/sessions/abc/messages',['message']),true)
 assert.equal(shouldRefreshEndpoint('/api/rooms',['message']),true)
 assert.equal(shouldRefreshEndpoint('/api/providers',['message']),false)
 assert.equal(shouldRefreshEndpoint('/api/audit',['audit']),true)
 assert.equal(shouldRefreshEndpoint('/api/tools',['audit']),false)
 assert.equal(shouldRefreshEndpoint('/api/approvals',['approval']),true)
 assert.equal(shouldRefreshEndpoint('/api/providers',['approval']),false)
})

test('merged, unknown and manual refresh events cannot lose updates',()=>{
 assert.equal(shouldRefreshEndpoint('/api/approvals',
  ['record:execution_operation','approval']),true)
 assert.equal(shouldRefreshEndpoint('/api/providers',
  ['record:execution_operation','approval']),false)
 assert.equal(shouldRefreshEndpoint('/api/providers',
  ['record:provider']),true)
 assert.equal(shouldRefreshEndpoint('/api/providers',[]),true)
})
