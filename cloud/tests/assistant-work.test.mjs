import test from 'node:test';
import assert from 'node:assert/strict';
import {workRoute,publicWork} from '../src/assistant_work.js';
const origin='https://example.com',valid={id:'abcdefgh-1',operation:'gmail_recent'};
const account=async()=>({id:'user-A',scope:'app',capability:'assistant_remote'});
const called=[];
const adapters={
 google:async(req)=>{called.push({kind:'google',path:new URL(req.url).pathname,body:req.method==='POST'?await req.json():null});return new URL(req.url).pathname.endsWith('jobs')&&req.method==='GET'?Response.json({jobs:[{id:valid.id,status:'queued',updatedAt:20,operation:valid.operation}]}):Response.json({id:valid.id,status:'queued',updatedAt:20})},
 browser:async(req)=>{called.push({kind:'browser',path:new URL(req.url).pathname,body:req.method==='POST'?await req.json():null});return new URL(req.url).pathname.endsWith('jobs')&&req.method==='GET'?Response.json({jobs:[{id:'12345678-b',status:'completed',updatedAt:30,goal:'조회'}]}):Response.json({id:'12345678-b',status:'queued',updatedAt:22})}
};
const config={account,adapters,readiness:{google:()=>true,browser:()=>true}};
const request=(route,data)=>new Request(origin+'/v1/assistant/work/'+route,data===undefined?{}:{method:'POST',headers:{Authorization:'Bearer dummy','Content-Type':'application/json'},body:JSON.stringify(data)});
test('shared work jobs combine durable Google and browser status in server order',async()=>{
 called.length=0;
 const r=await workRoute(request('jobs'),{},config),body=await r.json();
 assert.equal(r.status,200);
 assert.deepEqual(body.jobs.map(j=>j.kind),['browser','google']);
 assert.deepEqual(called.map(x=>x.path),['/v1/assistant/google/jobs','/v1/assistant/browser/jobs']);
 assert.equal(body.jobs[0].job.goal,'조회');
});
test('Google and browser start invoke the original authorization/execution routes',async()=>{
 called.length=0;
 let r=await workRoute(request('start',{kind:'google',...valid}),{},config);
 assert.equal(r.status,200);
 r=await workRoute(request('start',{kind:'browser',id:'12345678-b',url:'https://example.com',goal:'조회',confirmed:true}),{},config);
 assert.equal(r.status,200);
 assert.equal(called[0].path,'/v1/assistant/google/jobs');
 assert.equal(called[1].path,'/v1/assistant/browser/start');
 assert.equal(called[1].body.confirmed,true);
});
test('non-approved browser starts and forbidden side effects never reach executors',async()=>{
 called.length=0;
 for(const payload of [{kind:'browser',id:'12345678-b',url:'https://example.com',goal:'조회'},{kind:'google',...valid,operation:'gmail_send'},{kind:'google',id:'bad',operation:'gmail_recent'}]){
  const r=await workRoute(request('start',payload),{},config);assert.equal(r.status,400);
 }
 assert.equal((await workRoute(request('action',{kind:'browser',id:'12345678-b',action:'upload'}),{},config)).status,400);
 assert.equal((await workRoute(request('action',{kind:'google',id:valid.id,action:'approve'}),{},config)).status,400);
 assert.equal(called.length,0);
});
test('per-account assistant capability enforced before forwarding any action',async()=>{
 called.length=0;
 const denied={...config,account:async()=>({id:'user-A',scope:'app',capability:'assistant_pc'})};
 assert.equal((await workRoute(request('jobs'),{},denied)).status,403);
 assert.equal(called.length,0);
 assert.equal(publicWork('browser',{id:'bad',updatedAt:1,status:'running'}),null);
});
test('disabled connectors remain absent from combined job list',async()=>{
 called.length=0;
 const r=await workRoute(request('jobs'),{}, {...config,readiness:{google:()=>false,browser:()=>true}});
 const body=await r.json();
 assert.deepEqual(body.jobs.map(j=>j.kind),['browser']);
 assert.equal(body.available.google,false);
});

test('server work UI exposes only scoped, known action definitions',async()=>{
 const result=await workRoute(request('ui'),{},config);
 assert.equal(result.status,200);
 const payload=await result.json();
 assert.equal(payload.version,1);
 assert.deepEqual(payload.sections.map(x=>x.type),['actions','form']);
 assert.deepEqual(payload.sections[0].actions.map(x=>x.operation),
   ['gmail_recent','calendar_upcoming','drive_recent']);
 assert.deepEqual(payload.sections[1].fields.map(x=>x.kind),['https_url','multiline']);
 assert.equal(JSON.stringify(payload).includes('javascript:'),false);
});
