import {test} from 'node:test';
import assert from 'node:assert/strict';
import {DatabaseSync} from 'node:sqlite';
import {readFileSync} from 'node:fs';
import {AssistantSocket} from '../src/assistant_socket.js';
import {assistantRoute} from '../src/assistant_link.js';
import {hash,randomToken} from '../src/token_crypto.js';
async function fixture(){
 const db=new DatabaseSync(':memory:');db.exec(readFileSync(new URL('../schema.sql',import.meta.url),'utf8'));
 const account='google:test',device=crypto.randomUUID(),dk=await hash(randomToken()),mk=await hash(randomToken());
 db.prepare('INSERT INTO accounts(id,created_at) VALUES(?,0)').run(account);
 db.prepare('INSERT INTO sessions(token_hash,account_id,expires_at) VALUES(?,?,?)').run(mk,account,Math.floor(Date.now()/1000)+3600);
 db.prepare('INSERT INTO assistant_devices(id,account_id,name,token_hash,last_seen) VALUES(?,?,?,?,0)').run(device,account,'PC',dk);
 const DB={prepare(sql){return{bind(...args){return{first:async()=>db.prepare(sql).get(...args)||null,all:async()=>({results:db.prepare(sql).all(...args)}),run:async()=>({meta:{changes:Number(db.prepare(sql).run(...args).changes)}})}}}}};
 const sockets=[];const sock=(role,key,deviceId)=>{const m={role,key,account,device:deviceId,cid:crypto.randomUUID()},messages=[];const ws={readyState:1,messages,deserializeAttachment:()=>m,send:s=>messages.push(JSON.parse(s)),close:()=>{ws.readyState=3}};sockets.push(ws);return ws};
 const pc=sock('pc',dk,device),mobile=sock('mobile',mk),other=sock('mobile',mk);
 const ctx={getWebSockets:role=>sockets.filter(w=>!role||w.deserializeAttachment().role===role),setWebSocketAutoResponse(){}};
 const relay=new AssistantSocket(ctx,{DB});
 return {db,device,dk,mk,pc,mobile,other,relay,ctx,DB,account};
}
test('event delivery, duplicate rejection, only initiating socket receives result, hibernation reconstruction',async()=>{
 const {db,device,pc,mobile,other,relay,ctx,DB}=await fixture();
 const id=crypto.randomUUID(),x={type:'rpc',id,deviceId:device,route:'send',body:{text:'hello'}};
 await relay.webSocketMessage(mobile,JSON.stringify(x));await relay.webSocketMessage(mobile,JSON.stringify(x));
 assert.equal(pc.messages.length,1);assert.equal(mobile.messages[0].status,409);
 assert.equal(db.prepare('SELECT COUNT(*) AS n FROM assistant_ws_requests').get().n,1);
 const resumed=new AssistantSocket(ctx,{DB});
 await resumed.webSocketMessage(pc,JSON.stringify({type:'reply',id,status:201,data:{requestId:'accepted'}}));
 assert.equal(mobile.messages.at(-1).data.requestId,'accepted');assert.equal(other.messages.length,0);
 await resumed.webSocketMessage(pc,JSON.stringify({type:'changed'}));
 assert.equal(other.messages[0].type,'changed');
});
test('offline or revoked PC, revoked session, restricted route',async()=>{
 const {db,device,pc,mobile,relay,mk}=await fixture();
 const send=async route=>relay.webSocketMessage(mobile,JSON.stringify({type:'rpc',id:crypto.randomUUID(),deviceId:device,route,body:{}}));
 await send('../../admin');assert.equal(mobile.messages.at(-1).status,400);
 db.prepare('UPDATE assistant_devices SET revoked=1').run();await send('send');assert.equal(mobile.messages.at(-1).status,503);assert.equal(pc.messages.length,0);
 db.prepare('DELETE FROM sessions WHERE token_hash=?').run(mk);await send('send');assert.equal(mobile.readyState,3);
});
test('production binding disables legacy polling',async()=>{
 const env={ASSISTANT_LINK_ENABLED:'1',DB:{},GOOGLE_CLIENT_ID:'test',GOOGLE_CLIENT_SECRET:'test',ASSISTANT_SOCKET:{}};
 for(const path of ['device/pull','device/reply','rpc','rpc/status']){
  const r=await assistantRoute(new Request('https://test/v1/assistant/'+path,{method:'POST',body:'{}'}),env,{account:async()=>null});
  assert.equal(r.status,410);
 }
});
