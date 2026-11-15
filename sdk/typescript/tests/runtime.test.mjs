import {test} from 'node:test';
import assert from 'node:assert/strict';
import {DominionClient, DominionRuntime} from '../dist/index.js';

function json(value,status=200) {
  return new Response(JSON.stringify(value),{status,headers:{'content-type':'application/json'}});
}
function sse(events) {
  const frames=events.map(event=>`id: ${event.seq}\ndata: ${JSON.stringify(event)}\n\n`).join('');
  return new Response(new ReadableStream({
    start(c) { c.enqueue(new TextEncoder().encode(frames)); c.close(); }
  }),{headers:{'content-type':'text/event-stream'}});
}

test('native thread submits once, replays events and trusts persisted completion only',async()=>{
  const seen=[];
  const fetcher=async(url,init)=>{
    const route=new URL(url).pathname;
    seen.push({route,method:init.method,body:init.body ? JSON.parse(init.body) : null});
    if(route==='/api/tasks' && init.method==='POST') {
      return json({id:'task-1',status:'queued',requestId:'unique-2026',
        result:null},202);
    }
    if(route==='/api/tasks/task-1/events'){
      return sse([
        {seq:1,taskId:'task-1',type:'running',detail:{}},
        {seq:2,taskId:'task-1',type:'tool_result',detail:{name:'workspace_read'}},
        {seq:3,taskId:'task-1',type:'finished',detail:{status:'completed'}}
      ]);
    }
    if(route==='/api/tasks/task-1'){
      return json({id:'task-1',status:'completed',result:{
        answer:'Native engine result',verifiedWrites:0,verification:'workspace_readback_sha256'
      }});
    }
    throw Error('Unexpected native route: '+route);
  };
  const runtime=new DominionRuntime(new DominionClient({fetch:fetcher}));
  const thread=runtime.resumeThread('stable-session');
  const result=await thread.run('Read the project', {requestId:'unique-2026'});
  assert.equal(result.sessionId,'stable-session');
  assert.equal(result.finalResponse,'Native engine result');
  assert.equal(result.status,'completed');
  assert.deepEqual(result.events.map(e=>e.type),['running','tool_result','finished']);
  assert.equal(seen.filter(r=>r.method==='POST').length,1);
  assert.equal(seen[0].body.sessionId,'stable-session');
  assert.equal(seen[0].body.requestId,'unique-2026');
  assert.equal(seen[0].body.allowWrite,false);
  assert.equal(seen[0].body.allowShell,false);
  assert.deepEqual(seen.map(r=>r.route),[
    '/api/tasks','/api/tasks/task-1/events','/api/tasks/task-1'
  ]);
});

test('model failure is visible as terminal state, not a successful answer',async()=>{
  const runtime=new DominionRuntime({fetch:async(url,init)=>{
    const path=new URL(url).pathname;
    if(path==='/api/tasks' && init.method==='POST') return json({id:'x',status:'queued'},202);
    if(path.endsWith('/events')) return sse([
      {seq:1,taskId:'x',type:'finished',detail:{status:'failed',error:'provider timeout'}}
    ]);
    if(path==='/api/tasks/x') return json({id:'x',status:'failed',
      error:'provider timeout',result:{answer:'unfinished text'}});
    throw Error(path);
  }});
  const result=await runtime.startThread().run('Process these data');
  assert.equal(result.status,'failed');
  assert.equal(result.finalResponse,null);
});

test('steering and cancelling invoke the native Rust endpoints explicitly',async()=>{
  const paths=[];
  const runtime=new DominionRuntime({fetch:async(url,init)=>{
    paths.push([new URL(url).pathname,init.method]);
    if(paths.length===1) return json({id:'task-2',status:'queued',requestId:'key-2026'});
    return json({id:'task-2',status:'running'});
  }});
  const thread=runtime.resumeThread('persisted');
  await thread.runStreamed('Inspect source');
  await thread.steer('Focus on modules');
  await thread.cancel();
  assert.deepEqual(paths.map(([path])=>path),[
    '/api/tasks','/api/tasks/task-2/steer','/api/tasks/task-2/cancel'
  ]);
});

test('invalid thread identity does not touch a provider',()=>{
  const runtime=new DominionRuntime();
  assert.throws(()=>runtime.resumeThread(''),/valid native session ID/);
  assert.throws(()=>runtime.resumeThread('x\ny'),/valid native session ID/);
});
