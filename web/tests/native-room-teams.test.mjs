import test from 'node:test'
import assert from 'node:assert/strict'
import {selectNativeRoomTeamRuns} from '../src/services/nativeRoomTeams.ts'
test('native room teams are joined only by persisted Lead session ownership',()=>{
 const jobs=[
  {id:'l1',kind:'prompt',status:'running',sessionId:'lead-a',input:{roomTeamLaunch:true,roomId:'r1',parentSessionId:'room:r1',goal:'Fix'}},
  {id:'l2',kind:'prompt',sessionId:'lead-b',input:{roomTeamLaunch:true,roomId:'r2',parentSessionId:'room:r2'}},
  {id:'legacy',kind:'team',sessionId:'lead-c',input:{roomId:'r1',parentSessionId:'room:r1'}}
 ]
 const teams=[
  {id:'t1',leadSessionId:'lead-a',tasks:[{id:'n1',verified:false}]},
  {id:'t2',leadSessionId:'lead-b'},
  {id:'t3',leadSessionId:'lead-c'}
 ]
 const rows=selectNativeRoomTeamRuns(jobs,teams,'r1','room:r1')
 assert.equal(rows.length,1)
 assert.equal(rows[0].lead.id,'l1')
 assert.deepEqual(rows[0].teams.map(t=>t.id),['t1'])
 assert.deepEqual(selectNativeRoomTeamRuns(jobs,teams,'r1','room:r2'),[])
})
