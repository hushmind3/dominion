import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdtempSync,writeFileSync,rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {spawnSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';

const scorecard=fileURLToPath(new URL('./coding-scorecard.mjs',import.meta.url));
const example=(scenario,overrides={})=>({
  schemaVersion:1,scenario,
  provider:'deterministic_scripted_no_paid_api',
  evidenceLevel:'real_filesystem_real_shell_scripted_model',
  completed:true,verified:true,sourceFiles:1,modelCalls:4,
  chargedTokens:480,toolReceipts:4,failedTests:1,
  successfulTests:1,replayedEffects:0,wallMs:110,...overrides,
});
function run(...records){
  const dir=mkdtempSync(join(tmpdir(),'dominion-scorecard-'));
  const paths=records.map((r,i)=>{
    const path=join(dir,'fixture-'+i+'.log');
    writeFileSync(path,'cargo test output\nDOMINION_CODING_BENCHMARK '+JSON.stringify(r)+'\n');
    return path;
  });
  try{return spawnSync(process.execPath,[scorecard,...paths],{encoding:'utf8'});}
  finally{rmSync(dir,{recursive:true,force:true});}
}

test('real fixture results retain exact counts, costs and trust classification',()=>{
  const runResult=run(example('shell-repair'),example('rustc-crash-restore',{
    sourceFiles:2,modelCalls:5,chargedTokens:800,toolReceipts:5,wallMs:210
  }));
  assert.equal(runResult.status,0,runResult.stderr);
  const result=JSON.parse(runResult.stdout);
  assert.equal(result.totals.scenarios,2);
  assert.equal(result.totals.verifiedCompletionRatePct,100);
  assert.equal(result.totals.modelCalls,9);
  assert.equal(result.totals.scriptedProviderTokens,1280);
  assert.equal(result.totals.toolReceipts,9);
  assert.equal(result.totals.replayedEffects,0);
  assert.equal(result.limits.actualModelCodingSuccessMeasured,false);
  assert.equal(result.limits.measuredBilledCostKrw,null);
});

test('failed acceptance is reported as a failure rather than dropped',()=>{
  const out=run(example('shell-repair'),example('rustc-crash-restore',{
    completed:false,verified:false,successfulTests:0
  }));
  assert.equal(out.status,1);
  const metrics=JSON.parse(out.stdout);
  assert.equal(metrics.totals.verifiedCompletions,1);
  assert.equal(metrics.totals.verifiedCompletionRatePct,50);
});

test('duplicate scenario and missing evidence are rejected',()=>{
  const dup=run(example('same'),example('same'));
  assert.equal(dup.status,1);
  assert.match(dup.stderr,/duplicate scenario/);
  const missing=run(example('one'));
  assert.equal(missing.status,1);
  assert.match(missing.stderr,/two distinct/);
});

test('mixing paid provider evidence or impossible counters is rejected',()=>{
  const paid=run(example('scripted'),example('paid',{provider:'real-provider'}));
  assert.equal(paid.status,1);
  assert.match(paid.stderr,/mixed providers/);
  const malformed=run(example('ok'),example('negative',{chargedTokens:-1}));
  assert.equal(malformed.status,1);
  assert.match(malformed.stderr,/chargedTokens/);
});
