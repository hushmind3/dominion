import test from 'node:test';
import assert from 'node:assert/strict';
import {db} from '../server/store.mjs';
import {createGoal,runGoal,goalEvents} from '../server/harness.mjs';
test('a goal with an executable action never succeeds on model prose alone',async()=>{
 const run=createGoal({goal:'Build a website project and deploy it',budget:{maxRepairs:0,maxReplans:0}});
 try{
  const result=await runGoal(run.id,{
   plan:async()=>({steps:[{id:'fake',prompt:'Explain the launch',requiresTools:false}]}),
   execute:async()=>({content:'Already deployed.',toolCalls:0,evidence:[]}),
   review:async()=>({passed:true,reason:'LLM thinks it is done',mode:'independent-model'})
  });
  assert.equal(result.status,'needs_review');
  assert.match(result.reason,/no confirmed tool/);
  assert.ok(!goalEvents(run.id).some(e=>e.type==='workflow.completed'));
 }finally{
  db.prepare('DELETE FROM harness_events WHERE run_id=?').run(run.id);
  db.prepare('DELETE FROM harness_runs WHERE id=?').run(run.id);
 }
});
