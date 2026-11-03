import test from 'node:test';
import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import net from 'node:net';
const sleep=ms=>new Promise(r=>setTimeout(r,ms));
async function port(){return new Promise((resolve,reject)=>{const server=net.createServer();server.listen(0,'127.0.0.1',()=>{const p=server.address().port;server.close(()=>resolve(p))});server.on('error',reject);});}
test('real gateway creates and retrieves agents, rooms, sessions and jobs', {timeout:40000}, async ()=>{
 const base=fs.mkdtempSync(path.join(os.tmpdir(),'dominion-smoke-'));
 const p=await port(), url='http://127.0.0.1:'+p;
 const child=spawn(process.execPath,['server/index.mjs'],{cwd:process.cwd(),env:{...process.env,DOMINION_DATA:path.join(base,'state'),DOMINION_WORKSPACE:path.join(base,'workspace'),DOMINION_PORT:String(p),DOMINION_HOST:'127.0.0.1'},stdio:'pipe'});
 let err='';child.stderr.on('data',d=>err+=d.toString());
 try{
  let ready=false;
  for(let i=0;i<90;i++){
   if(child.exitCode!==null)throw Error('Server exited: '+err);
   try{const r=await fetch(url+'/api/health');if(r.ok){ready=true;break}}catch{}
   await sleep(150);
  }
  assert.ok(ready,'Gateway did not start: '+err);
  let response=await fetch(url+'/api/status');assert.equal(response.status,200);const status=await response.json();assert.ok(status.agents>=1);
  response=await fetch(url+'/api/providers');assert.equal(response.status,200);const providers=await response.json();assert.ok(providers.some(x=>x.id==='lmstudio'));
  response=await fetch(url+'/api/agents',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({name:'QA Agent',systemPrompt:'Independent testing'})});assert.equal(response.status,200);const agent=await response.json();assert.equal(agent.name,'QA Agent');
  response=await fetch(url+'/api/rooms',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({name:'Test Room',agentIds:['general',agent.id],rounds:2})});assert.equal(response.status,200);const room=await response.json();assert.equal(room.agentIds.length,2);
  response=await fetch(url+'/api/sessions',{method:'POST',headers:{'Content-Type':'application/json'},body:'{}'});assert.equal(response.status,200);const session=await response.json();assert.ok(session.id);
  response=await fetch(url+'/api/sessions/'+session.id+'/messages');assert.deepEqual(await response.json(),[]);
  response=await fetch(url+'/api/chat',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({sessionId:session.id,input:'Hello',providerId:'',model:''})});assert.equal(response.status,200);const stream=await response.text();assert.match(stream,/Select a provider and model/);
  response=await fetch(url+'/api/capabilities');
  assert.equal(response.status,200);
  const capabilities=await response.json();
  assert.ok(capabilities.items.some(x=>x.namespace==='channel'&&x.id==='telegram'&&x.available));
  assert.ok(capabilities.items.some(x=>x.namespace==='channel'&&x.id==='nostr'&&!x.available));
  assert.ok(capabilities.items.some(x=>x.namespace==='model'&&x.id==='lmstudio'));
  response=await fetch(url+'/api/capabilities?family=channel');
  const channelCapabilities=await response.json();
  assert.ok(channelCapabilities.items.length>=26);
  assert.ok(channelCapabilities.items.every(x=>x.namespace==='channel'));
  response=await fetch(url+'/api/channels/catalog');
  assert.equal(response.status,200);
  assert.ok((await response.json()).length>=26);
  response=await fetch(url+'/api/approvals');assert.deepEqual(await response.json(),[]);
  response=await fetch(url+'/api/mcp');assert.deepEqual(await response.json(),[]);
  response=await fetch(url+'/api/policy');assert.equal((await response.json()).autoApproveAll,false);
  response=await fetch(url+'/api/audit');assert.equal(response.status,200);
 }finally{child.kill('SIGTERM');fs.rmSync(base,{recursive:true,force:true});}
});
