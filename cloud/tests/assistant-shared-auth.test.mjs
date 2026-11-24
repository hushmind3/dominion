import {test} from 'node:test';
import assert from 'node:assert/strict';
import {DatabaseSync} from 'node:sqlite';
import {readFileSync} from 'node:fs';
import worker from '../src/index.js';
import {hash,randomToken} from '../src/token_crypto.js';
test('shared Google account app grants: free inference unchanged, remote explicit, PC registration distinct',async()=>{
 const db=new DatabaseSync(':memory:');db.exec(readFileSync(new URL('../schema.sql',import.meta.url),'utf8'));
 const DB={prepare(sql){return{bind(...a){return{first:async()=>db.prepare(sql).get(...a)||null,all:async()=>({results:db.prepare(sql).all(...a)}),run:async()=>({meta:{changes:Number(db.prepare(sql).run(...a).changes)}})}}}}};
 const now=Math.floor(Date.now()/1000),owner=randomToken();
 db.prepare('INSERT INTO accounts(id,created_at) VALUES(?,?)').run('google:owner',now);
 db.prepare('INSERT INTO sessions VALUES(?,?,?)').run(await hash(owner),'google:owner',now+600);
 const env={DB,ALLOWED_ORIGIN:'http://localhost:4317',ASSISTANT_LINK_ENABLED:'1',ASSISTANT_SOCKET:{idFromName:id=>id,get:()=>({fetch:async()=>new Response(JSON.stringify({devices:[]}),{headers:{'content-type':'application/json'}})})},AI:{run:async()=>({response:'answer'})}};
 const call=(path,t,b)=>worker.fetch(new Request('https://dominion.example'+path,{headers:{...(t?{authorization:'Bearer '+t}:{}),...(b===undefined?{}:{'content-type':'application/json'})},...(b===undefined?{}:{method:'POST',body:JSON.stringify(b)})}),env);
 async function grant(capability){
  const t='dapp_'+randomToken();
  const start=await call('/v1/apps/start',null,{app_name:'DOMINION Assistant',token_hash:await hash(t),...(capability?{capability}:{})});
  assert.equal(start.status,200);const {id}=await start.json();
  assert.equal((await call('/v1/apps/approve',owner,{id})).status,200);
  assert.equal((await (await call('/v1/me',t)).json()).account_id,'google:owner');
  return t;
 }
 const inference=await grant(),remote=await grant('assistant_remote'),pc=await grant('assistant_pc');
 assert.equal((await call('/v1/assistant/devices',inference)).status,403);
 assert.equal((await call('/v1/assistant/devices',remote)).status,200);
 assert.equal((await call('/v1/assistant/register',remote,{name:'PC',deviceToken:randomToken()})).status,403);
 assert.equal((await call('/v1/assistant/register',pc,{name:'PC',deviceToken:randomToken()})).status,201);
 assert.equal((await call('/v1/billing/checkout',remote,{plan:'pro'})).status,403);
 assert.equal(db.prepare('SELECT COUNT(*) AS n FROM accounts').get().n,1);
});
