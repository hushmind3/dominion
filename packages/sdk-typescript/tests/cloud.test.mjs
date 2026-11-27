import test from 'node:test';
import assert from 'node:assert/strict';
import {DominionCloudClient} from '../dist/cloud.js';

const session=(status,expiresAt=Math.floor(Date.now()/1000)+600)=>({
  id:'pair-id',status,expires_at:expiresAt
});

test('cloud approval waits for owner approval without auto-approving',async()=>{
  const observed=[];
  const responses=[session('pending'),session('approved')];
  const client=new DominionCloudClient('dapp_test',async(url,options)=>{
    observed.push({url:String(url),options});
    return new Response(JSON.stringify(responses.shift()),{status:200});
  });
  const approved=await client.waitForApproval({intervalMs:50,timeoutMs:1000});
  assert.equal(approved.status,'approved');
  assert.equal(observed.length,2);
  assert.ok(observed.every(x=>x.url.endsWith('/v1/apps/status')));
  assert.ok(observed.every(x=>x.options.headers.Authorization==='Bearer dapp_test'));
  assert.ok(observed.every(x=>!x.options.method||x.options.method==='GET'));
});

test('cloud approval surfaces revocation without retrying',async()=>{
  let calls=0;
  const client=new DominionCloudClient('token',async()=>{
    calls++;
    return new Response(JSON.stringify(session('revoked')),{status:200});
  });
  await assert.rejects(client.waitForApproval(),/Cloud pairing revoked/);
  assert.equal(calls,1);
});

test('cloud approval times out without changing the server pairing',async()=>{
  let calls=0;
  const client=new DominionCloudClient('token',async()=>{
    calls++;
    return new Response(JSON.stringify(session('pending')),{status:200});
  });
  await assert.rejects(client.waitForApproval({timeoutMs:0}),/expired or timed out/);
  assert.equal(calls,1);
});

test('cloud approval respects server-side expiration',async()=>{
  const client=new DominionCloudClient('token',async()=>
    new Response(JSON.stringify(session('pending',1)),{status:200}));
  await assert.rejects(client.waitForApproval(),/expired or timed out/);
});

test('cloud approval cancellation aborts the wait',async()=>{
  const controller=new AbortController();
  const client=new DominionCloudClient('token',async()=>
    new Response(JSON.stringify(session('pending')),{status:200}));
  const pending=client.waitForApproval({intervalMs:500,signal:controller.signal});
  setTimeout(()=>controller.abort(),30);
  await assert.rejects(pending,error=>error.name==='AbortError');
});

test('cloud completion forwards abort signal and does not leak token into the URL',async()=>{
  const controller=new AbortController();
  let observed;
  const client=new DominionCloudClient('private-token',async(url,options)=>{
    observed={url:String(url),options};
    return new Response(JSON.stringify({choices:[]}),{status:200});
  });
  await client.complete({model:'model',messages:[{role:'user',content:'hello'}]},controller.signal);
  assert.ok(observed.url.endsWith('/v1/chat/completions'));
  assert.equal(observed.url.includes('private-token'),false);
  assert.equal(observed.options.signal,controller.signal);
  assert.equal(observed.options.headers.Authorization,'Bearer private-token');
  assert.equal(JSON.parse(observed.options.body).stream,false);
});

test('cloud gateway malformed and non-JSON failures produce readable status',async()=>{
  for(const [status,body,expected] of [
    [502,'<html>bad gateway</html>',/Cloud HTTP 502: non-JSON response/],
    [503,'',/Cloud HTTP 503: non-JSON response/],
    [429,'{"error":"Quota exhausted"}',/Quota exhausted/],
  ]){
    const client=new DominionCloudClient('token',async()=>new Response(body,{status}));
    await assert.rejects(client.models(),expected);
  }
});
