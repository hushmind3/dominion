import test from 'node:test';
import assert from 'node:assert/strict';
import {executeToolBatch,isReadOnlyCall} from '../server/tool-scheduler.mjs';

const call=(name,args={})=>({function:{name,arguments:JSON.stringify(args)}});
const deferred=()=>{let release;const promise=new Promise(resolve=>{release=resolve;});return {promise,release};};

test('read tools overlap but model-order outputs remain stable',async()=>{
  const gate=deferred(),started=[];
  const calls=[call('workspace_read'),call('web_search'),call('file_read')];
  const task=executeToolBatch(calls,async(_tool,index)=>{
    started.push(index);await gate.promise;return index*10;
  },{maxReadConcurrency:2});
  assert.deepEqual(started,[0,1]);
  gate.release();
  assert.deepEqual(await task,[0,10,20]);
  assert.deepEqual(started,[0,1,2]);
});

test('mutating tool is a barrier between parallel read batches',async()=>{
  const gate=deferred(),started=[],completed=[];
  const calls=[call('workspace_read'),call('web_search'),call('workspace_write'),call('file_read')];
  const task=executeToolBatch(calls,async(_tool,index)=>{
    started.push(index);
    if(index<2)await gate.promise;
    completed.push(index);return String(index);
  });
  assert.deepEqual(started,[0,1]);
  gate.release();
  assert.deepEqual(await task,['0','1','2','3']);
  assert.ok(completed.indexOf(2)>completed.indexOf(0));
  assert.ok(completed.indexOf(2)>completed.indexOf(1));
  assert.deepEqual(started,[0,1,2,3]);
});

test('unknown MCP tools and GitHub writes remain serial; GitHub GET can overlap',async()=>{
  assert.equal(isReadOnlyCall(call('github_api',{method:'GET',path:'/user'})),true);
  assert.equal(isReadOnlyCall(call('github_api',{method:'PATCH',path:'/user'})),false);
  assert.equal(isReadOnlyCall(call('mcp_calendar_list')),false);
  assert.equal(isReadOnlyCall(call('shell_exec')),false);
  assert.equal(isReadOnlyCall(call('github_api',{method:'POST'})),false);
  assert.equal(isReadOnlyCall({function:{name:'github_api',arguments:'{'}}),false);
});

test('read-worker failure drains started tasks and prevents later mutation',async()=>{
  const gate=deferred(),started=[];
  const task=executeToolBatch([call('workspace_read'),call('web_search'),call('workspace_write')],async(_tool,index)=>{
    started.push(index);
    if(index===0)throw Error('read failed');
    await gate.promise;return 'ok';
  },{maxReadConcurrency:2});
  await Promise.resolve();assert.deepEqual(started,[0,1]);
  gate.release();
  await assert.rejects(task,/read failed/);
  assert.equal(started.includes(2),false);
});

test('abort drains in-flight reads without launching later actions',async()=>{
  const gate=deferred(),controller=new AbortController(),started=[];
  const task=executeToolBatch([call('workspace_read'),call('file_read'),call('workspace_write')],async(_tool,index)=>{
    started.push(index);if(index<2)await gate.promise;return index;
  },{signal:controller.signal,maxReadConcurrency:2});
  assert.deepEqual(started,[0,1]);
  controller.abort(Error('cancelled'));
  gate.release();
  await assert.rejects(task,/cancelled/);
  assert.deepEqual(started,[0,1]);
});
