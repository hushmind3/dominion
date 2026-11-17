import test from 'node:test'
import assert from 'node:assert/strict'
import {refreshScopesForEvent,endpointAffected} from '../web/src/refreshScope.ts'

test('one operation never triggers a settings or provider list refetch',()=>{
 const roots=refreshScopesForEvent({type:'record',kind:'execution_operation'})
 assert.deepEqual(roots,['executions','jobs','rooms'])
 for(const url of ['/api/providers','/api/oauth/connections','/api/agents','/api/channels/catalog','/api/model-library/runtime']){
  assert.equal(endpointAffected(url,roots),false,url)
 }
 for(const url of ['/api/executions?limit=100','/api/jobs','/api/rooms/abc/state']){
  assert.equal(endpointAffected(url,roots),true,url)
 }
})
test('message, provider, policy and OAuth changes invalidate separate resources',()=>{
 const cases=[
  [{type:'message'},'/api/sessions/one/messages','/api/providers'],
  [{type:'record',kind:'provider'},'/api/providers','/api/sessions'],
  [{type:'record',kind:'policy'},'/api/policy','/api/agents'],
  [{type:'record',kind:'oauth_config'},'/api/oauth/providers','/api/jobs'],
 ]
 for(const [event,yes,no] of cases){
  const roots=refreshScopesForEvent(event)
  assert.ok(roots,event.type)
  assert.equal(endpointAffected(yes,roots),true,yes)
  assert.equal(endpointAffected(no,roots),false,no)
 }
})
test('unknown event shapes preserve legacy global refresh semantics',()=>{
 for(const event of [{type:'record'}, {type:'record',kind:'future_version'}, {type:'future_event'}]){
  assert.equal(refreshScopesForEvent(event),null)
 }
 assert.equal(endpointAffected('external-url',[]),true)
 assert.equal(endpointAffected('/api/models',['*']),true)
})
