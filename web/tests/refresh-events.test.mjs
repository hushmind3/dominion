import test from 'node:test'
import assert from 'node:assert/strict'
import {classifyRefreshEvent,matchesRefreshTarget,matchesRoomRefresh,mergeRefreshScopes,emptyRefreshScope} from '../src/refreshEvents.ts'

test('chat messages only refresh the matching transcript and sessions list',()=>{
 const change=classifyRefreshEvent({type:'message',message:{id:'m',session_id:'chat-1'}})
 assert.ok(matchesRefreshTarget('/api/sessions/chat-1/messages',change.targets))
 assert.ok(matchesRefreshTarget('/api/sessions',change.targets))
 assert.equal(matchesRefreshTarget('/api/sessions/chat-2/messages',change.targets),false)
 assert.equal(matchesRefreshTarget('/api/providers',change.targets),false)
 assert.equal(matchesRefreshTarget('/api/agents',change.targets),false)
})
test('room events reach only the active room state, not foreign sessions',()=>{
 const change=classifyRefreshEvent({type:'record',kind:'job',record:{id:'job',input:{sessionId:'s1',roomId:'r1'}}})
 assert.ok(matchesRoomRefresh(change,'r1','s1'))
 assert.equal(matchesRoomRefresh(change,'r2','s2'),false)
 assert.equal(matchesRefreshTarget('/api/providers',change.targets),false)
})
test('room settings and provider edits do not trigger unrelated data loads',()=>{
 const room=classifyRefreshEvent({type:'record',kind:'room',record:{id:'r1'}})
 assert.ok(matchesRefreshTarget('/api/rooms',room.targets))
 assert.ok(matchesRefreshTarget('/api/rooms/r1/state',room.targets))
 assert.equal(matchesRoomRefresh(room,'r2','s2'),false)
 const provider=classifyRefreshEvent({type:'record',kind:'provider',record:{id:'p'}})
 assert.ok(matchesRefreshTarget('/api/providers',provider.targets))
 assert.equal(matchesRefreshTarget('/api/rooms',provider.targets),false)
})
test('approval and model events target only their consumers',()=>{
 const approval=classifyRefreshEvent({type:'approval',approval:{sessionId:'s2',id:'a'}})
 assert.ok(matchesRefreshTarget('/api/approvals',approval.targets))
 assert.equal(matchesRefreshTarget('/api/agents',approval.targets),false)
 const model=classifyRefreshEvent({type:'record',kind:'model_download',record:{id:'download'}})
 assert.ok(matchesRefreshTarget('/api/model-library/runtime',model.targets))
 assert.equal(matchesRefreshTarget('/api/providers',model.targets),false)
})
test('merge collapses rapid SSE bursts while unknown events do not cause full reloads',()=>{
 const a=classifyRefreshEvent({type:'message',message:{session_id:'same'}})
 const merged=mergeRefreshScopes(mergeRefreshScopes(emptyRefreshScope(),a),a)
 assert.deepEqual(merged.targets,a.targets)
 assert.deepEqual(merged.sessions,['same'])
 assert.deepEqual(classifyRefreshEvent({type:'audit'}).targets,['/api/audit/*','/api/audit'])
 assert.deepEqual(classifyRefreshEvent({type:'unrecognized'}).targets,[])
})

test('permission and extension policy events refresh only their owner controls',()=>{
 const permission=classifyRefreshEvent({type:'record',kind:'policy',record:{id:'default'}})
 assert.deepEqual(permission.targets,['/api/policy'])
 assert.equal(matchesRefreshTarget('/api/policy',permission.targets),true)
 assert.equal(matchesRefreshTarget('/api/extensions/runtime',permission.targets),false)
 const extension=classifyRefreshEvent({type:'record',kind:'extension_policy',record:{id:'default'}})
 assert.ok(matchesRefreshTarget('/api/extensions/runtime',extension.targets))
 assert.ok(matchesRefreshTarget('/api/tools',extension.targets))
 assert.equal(matchesRefreshTarget('/api/policy',extension.targets),false)
})

test('worker changes refresh only their own Team detail snapshot',()=>{
 const changed=classifyRefreshEvent({type:'record',kind:'job',record:{
  id:'worker-1',kind:'prompt',input:{parentSessionId:'owner',teamJobId:'team:alpha'}
 }})
 assert.ok(matchesRefreshTarget('/api/jobs/team%3Aalpha/team-steps',changed.targets))
 assert.equal(matchesRefreshTarget('/api/jobs/team%3Abeta/team-steps',changed.targets),false)
 assert.ok(matchesRefreshTarget('/api/jobs?sessionId=owner',changed.targets))
 const root=classifyRefreshEvent({type:'record',kind:'job',record:{
  id:'team:alpha',kind:'team',input:{sessionId:'owner'}
 }})
 assert.ok(matchesRefreshTarget('/api/jobs/team%3Aalpha/team-steps',root.targets))
})

test('memory changes refresh actual collection endpoints and provider changes refresh route controls',()=>{
 const memory=classifyRefreshEvent({type:'record',kind:'memory',record:{id:'m'}})
 for(const path of ['/api/memories','/api/experience','/api/improvements','/api/algorithms','/api/synergy','/api/transfer'])assert.equal(matchesRefreshTarget(path,memory.targets),true)
 assert.equal(matchesRefreshTarget('/api/jobs',memory.targets),false)
 const provider=classifyRefreshEvent({type:'record',kind:'provider',record:{id:'p'}})
 assert.equal(matchesRefreshTarget('/api/chat/inference-support?providerId=p',provider.targets),true)
 const mcp=classifyRefreshEvent({type:'record',kind:'mcp',record:{id:'m'}})
 assert.equal(matchesRefreshTarget('/api/tools',mcp.targets),true)
})
