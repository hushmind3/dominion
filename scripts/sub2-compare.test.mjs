import test from 'node:test';
import assert from 'node:assert/strict';
import {compare} from './sub2-compare.mjs';
const meta={toolSchemaHash:'x',maxRounds:8,maxTools:20,providerPolicy:'same'};
const pair=(outcome,taskId)=>({taskId,providerId:'p',model:'m',verified:true,
  taskOutcome:outcome,modelCalls:2,tokens:100,durationMs:500});
test('paired verification produces measurable pass/time/token deltas',()=>{
 const baseline={comparisonMeta:meta,tasks:[pair('failed','one'),pair('succeeded','two')]};
 const candidate={comparisonMeta:meta,tasks:[pair('succeeded','one'),pair('succeeded','two')]};
 const result=compare(baseline,candidate);
 assert.equal(result.comparable,true);
 assert.equal(result.pairedVerified,2);
 assert.equal(result.delta.passed,1);
 assert.equal(result.baseline.successRate,0.5);
 assert.equal(result.candidate.successRate,1);
 assert.equal(result.baseline.costUsd,null);
});
test('unverified self-grades and budget drift invalidate comparison',()=>{
 const old={comparisonMeta:meta,tasks:[pair('failed','one')]};
 const drift={comparisonMeta:{...meta,maxTools:50},tasks:[{...pair('succeeded','one'),verified:false}]};
 const result=compare(old,drift);
 assert.equal(result.comparable,false);
 assert.equal(result.delta,null);
 assert.ok(result.conditionsMissing.some(s=>s.includes('maxTools')));
 assert.ok(result.conditionsMissing.some(s=>s.includes('verifier')));
});
test('changed task IDs or duplicated cases are not comparable',()=>{
 assert.throws(()=>compare({tasks:[pair('failed','one')]},{tasks:[pair('failed','two')]}),/task IDs/);
 assert.throws(()=>compare({tasks:[pair('failed','one'),pair('failed','one')]},{tasks:[pair('failed','one')]}),/Duplicate/);
});
