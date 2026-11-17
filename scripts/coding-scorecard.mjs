#!/usr/bin/env node
// Comparable harness metrics from real filesystem/shell test fixtures.
// A deterministic scripted model is NOT a model-capability or invoice benchmark.
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';

const PREFIX='DOMINION_CODING_BENCHMARK ';
const required=['scenario','provider','evidenceLevel','completed','verified',
  'sourceFiles','modelCalls','chargedTokens','toolReceipts','failedTests',
  'successfulTests','replayedEffects','wallMs'];
const counters=['sourceFiles','modelCalls','chargedTokens','toolReceipts',
  'failedTests','successfulTests','replayedEffects','wallMs'];

function fail(message) {
  process.stderr.write('Coding scorecard rejected: '+message+'\n');
  process.exitCode=1;
}
try {
  const paths=process.argv.slice(2);
  assert.ok(paths.length>0,'pass the actual cargo test log file(s)');
  const records=[];
  for(const path of paths){
    const log=readFileSync(path,'utf8');
    for(const line of log.split(/\r?\n/)){
      const offset=line.indexOf(PREFIX);
      if(offset<0)continue;
      const record=JSON.parse(line.slice(offset+PREFIX.length).trim());
      for(const field of required){
        assert.ok(Object.hasOwn(record,field),field+' missing from '+path);
      }
      assert.equal(record.schemaVersion,1,'unsupported evidence schema');
      assert.equal(record.provider,'deterministic_scripted_no_paid_api',
        'mixed providers require separate cost and quality cohorts');
      assert.equal(typeof record.scenario,'string');
      assert.ok(record.scenario.length>0);
      assert.equal(typeof record.evidenceLevel,'string');
      assert.equal(typeof record.completed,'boolean');
      assert.equal(typeof record.verified,'boolean');
      for(const key of counters){
        assert.ok(Number.isSafeInteger(record[key])&&record[key]>=0,
          key+' is not a nonnegative safe integer');
      }
      assert.ok(record.sourceFiles>0,'scenario must touch a real source artifact');
      records.push(record);
    }
  }
  const ids=records.map(r=>r.scenario);
  assert.equal(new Set(ids).size,ids.length,'duplicate scenario; no double counting');
  assert.ok(records.length>=2,'at least two distinct real shell coding tasks required');
  records.sort((a,b)=>a.scenario.localeCompare(b.scenario));
  const sum=(field)=>records.reduce((total,row)=>total+row[field],0);
  const verified=records.filter(r=>r.completed&&r.verified&&r.replayedEffects===0).length;
  const summary={
    schemaVersion:1,
    suite:'dominion-scripted-coding-harness-v1',
    evidenceClass:'real_filesystem_and_verification_with_scripted_model',
    limits:{
      actualModelCodingSuccessMeasured:false,
      actualProviderBillingMeasured:false,
      crossBranchIdenticalWorkloadMeasured:false,
      measuredCostsCurrency:'KRW',
      measuredBilledCostKrw:null
    },
    totals:{
      scenarios:records.length,
      verifiedCompletions:verified,
      verifiedCompletionRatePct:Math.round(10000*verified/records.length)/100,
      modelCalls:sum('modelCalls'),
      scriptedProviderTokens:sum('chargedTokens'),
      toolReceipts:sum('toolReceipts'),
      failingVerificationRuns:sum('failedTests'),
      passingVerificationRuns:sum('successfulTests'),
      replayedEffects:sum('replayedEffects'),
      wallMsSum:sum('wallMs')
    },
    scenarios:records
  };
  process.stdout.write(JSON.stringify(summary,null,2)+'\n');
  if(verified!==records.length)process.exitCode=1;
}catch(error){
  fail(error instanceof Error?error.message:String(error));
}
