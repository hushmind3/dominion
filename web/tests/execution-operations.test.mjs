import test from 'node:test'
import assert from 'node:assert/strict'
import {projectExecutionOperations} from '../src/services/executionOperations.ts'

test('sub2 operation projection preserves owner run scope and real effect states',()=>{
 const items=projectExecutionOperations([
  {id:'e-1',runId:'run-1',effectState:'verified',toolReceipt:{tool:'file_read',requestDigest:'SECRET'},startedAt:'2026-11-22'},
  {id:'e-2',runId:'run-2',effectState:'effect_unknown',toolReceipt:{tool:'file_write',token:'SECRET'}},
  {id:'e-3',runId:'run-1',effectState:'effect_unknown',toolReceipt:{tool:'file_write',secret:'PRIVATE'},input:{prompt:'PRIVATE'}}
 ],'run-1')
 assert.deepEqual(items,[
  {id:'e-1',tool:'file_read',kind:undefined,state:'verified',startedAt:'2026-11-22',finishedAt:undefined},
  {id:'e-3',tool:'file_write',kind:undefined,state:'effect_unknown',startedAt:undefined,finishedAt:undefined}
 ])
 assert.ok(!JSON.stringify(items).includes('SECRET'))
 assert.ok(!JSON.stringify(items).includes('PRIVATE'))
})
test('malformed backend operation response is not silently treated as empty history',()=>{
 assert.throws(()=>projectExecutionOperations({events:[]},'run-1'),/Invalid execution operations response/)
 assert.deepEqual(projectExecutionOperations([null,[],{runId:'another'},{runId:'run-1',id:'r'}],'run-1'),[
  {id:'r',tool:undefined,kind:undefined,state:undefined,startedAt:undefined,finishedAt:undefined}
 ])
})
