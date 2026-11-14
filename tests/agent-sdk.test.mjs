import test from 'node:test';
import assert from 'node:assert/strict';
import {DominionSDK} from '../server/agent-sdk.mjs';

function fixture(){
 let counter=0;
 const sessions=new Map(),history=new Map(),calls=[],agents=new Map([
  ['manager',{id:'manager',providerId:'mock',model:'main',systemPrompt:'Lead'}],
  ['researcher',{id:'researcher',providerId:'mock',model:'fast',systemPrompt:'Investigate'}]
 ]);
 const host={
  createSession:options=>{
   const row={...options,id:'thread-'+(++counter)};
   sessions.set(row.id,row);history.set(row.id,[]);return row;
  },
  getSession:id=>sessions.get(id),
  getAgent:id=>agents.get(id),
  readMessages:id=>history.get(id)||[],
  getExecution:()=>({tokens:19,modelCalls:2,toolCalls:1}),
  cancel:()=>true,
  runTurn:async opts=>{
   calls.push(opts);
   const rows=history.get(opts.sessionId);
   rows.push({role:'user',content:opts.input});
   opts.onEvent({type:'runtime.started',runId:'run-'+opts.sessionId});
   if(opts.input.startsWith('delegate')){
    const tool=opts.sdkTools.find(x=>x.name==='agent_delegate');
    assert.ok(tool,'Delegation must be a runtime tool');
    const outcome=await opts.sdkInvoke(tool,{
     agentId:'researcher',task:'Find references',context:'Architecture'
    },()=>{},{runId:'parent-run'});
    rows.push({role:'tool',content:JSON.stringify(outcome)});
   }
   const message={role:'assistant',content:'completed '+opts.sessionId};
   rows.push(message);
   opts.onEvent({type:'done',runId:'run-'+opts.sessionId,usage:{total_tokens:19}});
   return message;
  }
 };
 return {sdk:new DominionSDK(host),sessions,history,calls};
}

test('start, execute and resume durable SDK threads without an external CLI',async()=>{
 const f=fixture(),thread=f.sdk.startThread({agentId:'manager',providerId:'mock',model:'main'});
 const a=await thread.run('implement task');
 assert.equal(a.threadId,thread.id);
 assert.equal(a.finalResponse,'completed '+thread.id);
 assert.equal(a.usage.tokens,19);
 assert.deepEqual(a.items.map(x=>x.role),['user','assistant']);
 const resumed=f.sdk.resumeThread(thread.id);
 const b=await resumed.run('continue work');
 assert.equal(b.items.length,2,'A resumed run returns only its new items');
 assert.equal(f.history.get(thread.id).length,4);
 assert.equal(f.calls.length,2);
});

test('stream emits runtime events and a terminal result',async()=>{
 const f=fixture(),thread=f.sdk.startThread({agentId:'manager'});
 const {events}=await thread.runStreamed('investigate');
 const collected=[];
 for await(const event of events)collected.push(event);
 assert.deepEqual(collected.map(x=>x.type),
  ['runtime.started','done','sdk.turn.completed']);
 assert.equal(collected.at(-1).result.threadId,thread.id);
});

test('custom handlers are injected into the same engine and approval defaults on',async()=>{
 const f=fixture();
 f.sdk.registerTool({name:'sdk_probe',description:'Probe',
  parameters:{type:'object',properties:{value:{type:'string'}},required:['value']},
  handler:async({value})=>'reply '+value});
 const thread=f.sdk.startThread({agentId:'manager'});
 await thread.run('call tool');
 const args=f.calls[0];
 const probe=args.sdkTools.find(x=>x.name==='sdk_probe');
 assert.ok(probe);
 assert.equal(probe.approval,true);
 const output=await args.sdkInvoke(probe,{value:'ok'},()=>{},{signal:undefined});
 assert.equal(output,'reply ok');
 assert.throws(()=>f.sdk.registerTool({name:'sdk_probe',
  parameters:{type:'object'},handler:()=>{}}),/Duplicate/);
});

test('agent delegation creates linked child sessions with target model',async()=>{
 const f=fixture(),root=f.sdk.startThread({agentId:'manager',providerId:'mock',model:'main',
  delegates:['researcher'],maxDelegateDepth:2});
 const events=[];
 await root.run('delegate the research',{onEvent:e=>events.push(e)});
 assert.equal(f.calls.length,2);
 const child=[...f.sessions.values()].find(x=>x.parentSessionId===root.id);
 assert.ok(child);
 assert.equal(child.agentId,'researcher');
 assert.equal(child.model,'fast');
 assert.equal(child.parentRunId,'parent-run');
 assert.equal(f.calls[1].sessionId,child.id);
 assert.ok(events.some(x=>x.type==='sdk.delegate.completed'));
});

test('delegate whitelist and depth limit prevent unbounded recursive spawning',async()=>{
 const f=fixture(),thread=f.sdk.startThread({agentId:'manager',
  delegates:['researcher'],depth:2,maxDelegateDepth:2});
 await thread.run('ordinary request');
 assert.equal(f.calls[0].sdkTools.some(x=>x.name==='agent_delegate'),false);
 const other=f.sdk.startThread({agentId:'manager',delegates:['unknown']});
 await other.run('ordinary request');
 assert.equal(f.calls[1].sdkTools.some(x=>x.name==='agent_delegate'),false);
});

test('run rejects missing prompts and unknown sessions',async()=>{
 const f=fixture(),thread=f.sdk.startThread({});
 await assert.rejects(thread.run(''),/nonempty/);
 assert.throws(()=>f.sdk.resumeThread('missing'),/not found/);
});
