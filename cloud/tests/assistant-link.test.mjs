import {test} from 'node:test';
import assert from 'node:assert/strict';
import {DatabaseSync} from 'node:sqlite';
import {readFileSync} from 'node:fs';
import {assistantRoute} from '../src/assistant_link.js';
import {hash,randomToken} from '../src/token_crypto.js';

function fixture(){
 const sqlite=new DatabaseSync(':memory:');
 sqlite.exec(readFileSync(new URL('../schema.sql',import.meta.url),'utf8'));
 for(const id of ['google:alice','google:bob'])sqlite.prepare('INSERT INTO accounts(id,created_at) VALUES(?,0)').run(id);
 function prepare(sql){
  return {bind(...values){return {first:async()=>sqlite.prepare(sql).get(...values)||null,all:async()=>({results:sqlite.prepare(sql).all(...values)}),run:async()=>({meta:{changes:Number(sqlite.prepare(sql).run(...values).changes)}})}}}
 }
 const env={DB:{prepare,batch:async statements=>{sqlite.exec('BEGIN');try{const r=[];for(const s of statements)r.push(await s.run());sqlite.exec('COMMIT');return r}catch(e){sqlite.exec('ROLLBACK');throw e}}},ASSISTANT_LINK_ENABLED:'1',GOOGLE_CLIENT_ID:'test',GOOGLE_CLIENT_SECRET:'test'};
 const account=async req=>{const v=req.headers.get('authorization');return v==='Bearer alice'?{id:'google:alice'}:v==='Bearer bob'?{id:'google:bob'}:null};
 async function call(path,body,auth='Bearer alice'){
  const req=new Request('https://dominion-cloud.dominion-hushmind.workers.dev/v1/assistant/'+path,{method:body===undefined?'GET':'POST',headers:{authorization:auth},...(body===undefined?{}:{body:JSON.stringify(body)})});
  const r=await assistantRoute(req,env,{account,verifyGoogle:async()=>{throw Error('not invoked')}});
  return {status:r.status,data:await r.json()};
 }
 return {call,sqlite,env};
}
test('account isolation, one claim, idempotent reply, revocation',async()=>{
 const {call}=fixture(),token=randomToken();
 const device=(await call('register',{name:'PC',deviceToken:token})).data.id;
 assert.ok(device);
 assert.equal((await call('devices',undefined,'Bearer bob')).data.devices.length,0);
 const id=crypto.randomUUID(),data={id,deviceId:device,route:'send',body:{clientId:'one',text:'hello',kind:'chat'}};
 assert.equal((await call('rpc',data,'Bearer bob')).status,404);
 assert.equal((await call('rpc',{...data,route:'../../admin'})).status,400);
 assert.equal((await call('rpc',data)).status,202);
 assert.equal((await call('rpc',data)).status,202);
 assert.equal((await call('rpc',{...data,body:{text:'changed'}})).status,409);
 const auth='Device '+token;
 assert.equal((await call('device/pull',{},auth)).data.request.id,id);
 assert.equal((await call('device/pull',{},auth)).data.request,null);
 assert.equal((await call('rpc/status',{id},'Bearer bob')).status,404);
 const result={id,status:201,data:{requestId:'accepted'}};
 assert.equal((await call('device/reply',result,auth)).status,200);
 assert.equal((await call('device/reply',result,auth)).status,200);
 assert.equal((await call('rpc/status',{id})).data.data.requestId,'accepted');
 await call('revoke',{deviceId:device},'Bearer bob');
 assert.equal((await call('devices')).data.devices.length,1);
 await call('revoke',{deviceId:device});
 assert.equal((await call('device/pull',{},auth)).status,401);
 assert.equal((await call('rpc/status',{id})).status,404);
});
test('expired request is not claimed; device token cannot use account routes',async()=>{
 const {call,sqlite}=fixture(),token=randomToken();
 const device=(await call('register',{name:'PC',deviceToken:token})).data.id;
 const id=crypto.randomUUID();
 await call('rpc',{id,deviceId:device,route:'history',body:null});
 sqlite.prepare('UPDATE assistant_rpc SET expires_at=0 WHERE id=?').run(id);
 assert.equal((await call('device/pull',{},'Device '+token)).data.request,null);
 assert.equal((await call('rpc/status',{id})).status,409);
 assert.equal((await call('devices',undefined,'Device '+token)).status,401);
});
test('poll secret stays separate; revoked login cannot recreate its session',async()=>{
 const {call,sqlite}=fixture(),id=randomToken(),poll=randomToken(),expires=Math.floor(Date.now()/1000)+500;
 sqlite.prepare("INSERT INTO assistant_login(id,poll_hash,ticket_hash,nonce,ip_hash,status,account_id,session_until,created_at,expires_at) VALUES(?,?,?,?,?,'verified',?,?,0,?)").run(id,await hash(poll),'ticket','nonce','ip','google:alice',expires+1000,expires);
 assert.equal((await call('auth/status',{id,poll_token:randomToken()})).status,410);
 const a=await call('auth/status',{id,poll_token:poll}),b=await call('auth/status',{id,poll_token:poll});
 assert.equal(a.data.token,b.data.token);
 assert.equal(a.data.account_id,'google:alice');
 sqlite.prepare('DELETE FROM sessions WHERE token_hash=?').run(await hash(a.data.token));
 assert.equal((await call('auth/status',{id,poll_token:poll})).status,401);
});
