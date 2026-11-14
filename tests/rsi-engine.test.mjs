import test from 'node:test';
import assert from 'node:assert/strict';
import {get,put,remove} from '../server/store.mjs';
import {createExperiment,experiment,cancelExperiment,nextExperiment,acceptExperiment,recoverExperiments} from '../server/rsi-engine.mjs';

const cleanup=row=>remove('rsi_experiment',row.id);

test('RSI requires a bounded goal and a precise source allowlist',()=>{
 assert.throws(()=>createExperiment({goal:'Improve routing'}),/source file paths/);
 assert.throws(()=>createExperiment({goal:'Improve routing',paths:['../server/index.mjs']}),/Invalid candidate path/);
 assert.throws(()=>createExperiment({goal:'Improve routing',paths:['tests/harness.test.mjs']}),/Frozen evaluation/);
 assert.throws(()=>createExperiment({goal:'Improve routing',paths:['package.json']}),/Frozen evaluation/);
 const row=createExperiment({goal:'Improve routing',paths:['server/model-router.mjs','server/model-router.mjs']});
 try{
  assert.equal(row.status,'queued');
  assert.deepEqual(row.paths,['server/model-router.mjs']);
  assert.equal(row.usage.modelCalls,0);
  assert.equal(experiment(row.id).goal,'Improve routing');
 }finally{cleanup(row)}
});
test('RSI cannot adopt an unevaluated candidate or generate a successor',async()=>{
 const row=createExperiment({goal:'Improve routing',paths:['server/model-router.mjs']});
 try{
  await assert.rejects(acceptExperiment(row.id),/Validated candidate required/);
  assert.throws(()=>nextExperiment(row.id),/Accepted ancestor required/);
  assert.equal(experiment(row.id).baseCommit,'');
 }finally{cleanup(row)}
});
test('RSI cancellation is durable and terminal',()=>{
 const row=createExperiment({goal:'Improve routing',paths:['server/model-router.mjs']});
 try{
  assert.equal(cancelExperiment(row.id).status,'cancelled');
  assert.throws(()=>cancelExperiment(row.id),/cannot be cancelled/);
  recoverExperiments();
  assert.equal(experiment(row.id).status,'cancelled');
 }finally{cleanup(row)}
});
test('RSI crash recovery holds in-flight changes for review',()=>{
 const row=createExperiment({goal:'Improve routing',paths:['server/model-router.mjs']});
 try{
  put('rsi_experiment',{...row,status:'candidate_testing'});
  recoverExperiments();
  assert.equal(experiment(row.id).status,'needs_review');
  assert.match(experiment(row.id).reason,/Interrupted/);
 }finally{cleanup(row)}
});
