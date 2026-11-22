import test from 'node:test'
import assert from 'node:assert/strict'
import {classifyTeamReview} from '../src/components/teamOutcome.ts'

test('a completed integrated Team is not relabeled pending by its historical manifest',()=>{
 const persisted={status:'completed',integratedAt:'2026-11-21T12:00:00Z',
  result:{gaps:[],pendingIntegrationJobs:['worker-1','worker-2']}}
 assert.deepEqual(classifyTeamReview(persisted),{kind:'integrated',count:2})
})
test('unintegrated frozen patches require owner review, never fake success',()=>{
 assert.deepEqual(classifyTeamReview({status:'needs_review',
  result:{gaps:[],pendingIntegrationJobs:['worker-1']}}),{kind:'pending',count:1})
 assert.deepEqual(classifyTeamReview({status:'needs_review',
  result:{gaps:[{error:'verification failed'}],pendingIntegrationJobs:['worker-1']}}),
  {kind:'gaps',count:1})
})
test('non-patch Team completion and unknown review remain distinct',()=>{
 assert.deepEqual(classifyTeamReview({status:'completed',
  result:{pendingIntegrationJobs:[]}}),{kind:'none',count:0})
 assert.deepEqual(classifyTeamReview({status:'needs_review',result:{reason:'deadline'}}),
  {kind:'review',count:0})
 assert.deepEqual(classifyTeamReview({status:'running',
  result:{pendingIntegrationJobs:['w']}}),{kind:'none',count:0})
})
