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
