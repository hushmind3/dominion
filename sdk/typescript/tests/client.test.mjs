import {test} from "node:test";
import assert from "node:assert/strict";
import {DominionClient,DominionError} from "../dist/index.js";

const json = (payload,status=200) => new Response(JSON.stringify(payload),{
  status,headers:{"content-type":"application/json"}
});
const sse = (...chunks) => new Response(new ReadableStream({
  start(controller) {
    const encoder = new TextEncoder();
    for (const c of chunks) controller.enqueue(encoder.encode(c));
    controller.close();
  }
}),{headers:{"content-type":"text/event-stream"}});

test("a caller-owned retry key is passed through unchanged without duplicate POSTs",async()=>{
  const requests=[];
  const client=new DominionClient({fetch:async(url,init)=>{
    requests.push({url:String(url),method:init.method,body:JSON.parse(init.body)});
    return json({id:"task-1",status:"queued"});
  }});
  await client.submit({requestId:"stable-2026",prompt:"read project",allowWrite:false});
  assert.equal(requests.length,1);
  assert.equal(requests[0].body.requestId,"stable-2026");
  assert.equal(requests[0].body.allowWrite,false);
  assert.equal(requests[0].method,"POST");
});

test("model errors do not trigger an automatic paid retry",async()=>{
  let calls=0;
  const client=new DominionClient({fetch:async()=>{
    calls++;
    return json({error:"provider unavailable"},503);
  }});
  await assert.rejects(client.submit({prompt:"work",requestId:"failed-2026"}), error =>
    error instanceof DominionError && error.status===503);
  assert.equal(calls,1);
});

test("event watcher resumes from last durable cursor, not another model submission",async()=>{
  const urls=[];
  const client=new DominionClient({fetch:async url=>{
    urls.push(String(url));
    if(urls.length===1) return sse(
      'id: 1\ndata: {"seq":1,"taskId":"task-1","type":"running","detail":{}}\n\n',
      'id: 2\ndata: {"seq":2,"taskId":"task-1","type":"model_step","detail":{}}\n\n'
    );
    return sse(
      'id: 2\ndata: {"seq":2,"taskId":"task-1","type":"model_step","detail":{}}\n\n',
      'id: 3\ndata: {"seq":3,"taskId":"task-1","type":"finished","detail":{}}\n\n'
    );
  }});
  const observed=[];
  for await(const event of client.watch("task-1")) {
    observed.push(event.seq);
    if (observed.length===3) break;
  }
  assert.deepEqual(observed,[1,2,3]);
  assert.deepEqual(urls,[
    "http://127.0.0.1:4317/api/tasks/task-1/events?after=0",
    "http://127.0.0.1:4317/api/tasks/task-1/events?after=2"
  ]);
});

test("cancel and correction remain separate explicit native actions",async()=>{
  const calls=[];
  const client=new DominionClient({fetch:async(url,init)=>{
    calls.push({path:new URL(url).pathname,body:JSON.parse(init.body)});
    return json({id:"task-1",status:"running"});
  }});
  await client.steer("task-1","read the latest file first");
  await client.cancel("task-1");
  assert.deepEqual(calls.map(c=>c.path),[
    "/api/tasks/task-1/steer","/api/tasks/task-1/cancel"
  ]);
  assert.equal(calls[0].body.text,"read the latest file first");
});

test("typed client uses the native A-B-A evolution routes and exact budget",async()=>{
  const calls=[];
  const evolution={id:"run-2026",starting_trial:"seed-2026",status:"running",
    max_generations:2,target_files:["src/a.rs","src/b.rs"],
    descendants:[],completed_generations:0,cost_budget_krw:900,
    spent_krw:0,stop_reason:null};
  const client=new DominionClient({fetch:async(url,init)=>{
    calls.push({method:init.method,path:new URL(url).pathname,
      body:init.body?JSON.parse(init.body):undefined});
    return json(calls.length===2?[evolution]:evolution);
  }});
  const started=await client.improvementEvolutionStart("seed-2026",{
    max_generations:2,cost_budget_krw:900,
    target_files:["src/a.rs","src/b.rs"]
  });
  const list=await client.evolutionList();
  const single=await client.evolutionGet("run-2026");
  assert.equal(started.status,"running");
  assert.equal(list[0].starting_trial,"seed-2026");
  assert.equal(single.max_generations,2);
  assert.deepEqual(calls.map(c=>[c.method,c.path]),[
    ["POST","/api/improvements/seed-2026/evolution"],
    ["GET","/api/evolutions"],
    ["GET","/api/evolutions/run-2026"]
  ]);
  assert.deepEqual(calls[0].body,{
    max_generations:2,cost_budget_krw:900,
    target_files:["src/a.rs","src/b.rs"]
  });
});

test("stop requests use the native persisted evolution action, never client cancellation",async()=>{
 const calls=[];
 const client=new DominionClient({fetch:async(url,init)=>{
   calls.push([new URL(url).pathname,init.method,JSON.parse(init.body)]);
   return json({id:"run-2026",status:"running",stop_requested:true});
 }});
 const run=await client.evolutionStop("run-2026");
 assert.equal(run.stop_requested,true);
 assert.deepEqual(calls,[["/api/evolutions/run-2026/stop","POST",{}]]);
});
