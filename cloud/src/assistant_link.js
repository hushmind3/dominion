import {hash} from './token_crypto.js';
import {messages,publish} from './assistant_messages.js';
const now=()=>Math.floor(Date.now()/1000);
const token=v=>typeof v==='string'&&/^[A-Za-z0-9_-]{43}$/.test(v);
const uuid=v=>typeof v==='string'&&/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(v);
const reply=(x,status=200)=>new Response(JSON.stringify(x),{status,headers:{'content-type':'application/json','cache-control':'no-store'}});
const fail=(error,status=400)=>reply({error},status);
const stub=(env,id)=>env.ASSISTANT_SOCKET.get(env.ASSISTANT_SOCKET.idFromName(id));
const call=(env,id,path,h={})=>stub(env,id).fetch(new Request('https://internal'+path,{headers:{'x-account':id,...h}}));
export const ready=env=>env.ASSISTANT_LINK_ENABLED==='1'&&Boolean(env.DB&&env.ASSISTANT_SOCKET);
export async function cleanupAssistant(env){if(ready(env)){await env.DB.prepare('DELETE FROM assistant_ws_requests WHERE expires_at<?').bind(now()-3600).run();await env.DB.prepare('DELETE FROM assistant_messages WHERE created_at<?').bind(now()-2592000).run();await env.DB.prepare("DELETE FROM assistant_push WHERE app_session_id IN (SELECT id FROM app_sessions WHERE status='revoked' OR expires_at<=?)").bind(now()).run()}}
export async function revokeAppSocket(env,accountId,key){if(ready(env))await call(env,accountId,'/revoke',{'x-key':key})}
export async function assistantRoute(req,env,{account}){
 const p=new URL(req.url).pathname.slice('/v1/assistant/'.length),auth=req.headers.get('authorization')||'';
 if(!ready(env))return fail('Assistant relay not configured',503);
 if(['device/pull','device/reply','rpc','rpc/status'].includes(p))return fail('Use WebSocket relay',410);
 if(auth.startsWith('Device ')){
  const raw=auth.slice(7);if(!token(raw))return fail('Unauthorized device',401);
  const key=await hash(raw);
  const d=await env.DB.prepare("SELECT d.id,d.account_id FROM assistant_devices d LEFT JOIN app_sessions s ON s.id=d.app_session_id WHERE d.token_hash=? AND d.revoked=0 AND (d.app_session_id IS NULL OR (s.status='approved' AND s.expires_at>? AND s.capability='assistant_pc'))").bind(key,now()).first();
  if(!d)return fail('Unauthorized device',401);
  if(p==='socket'&&req.method==='GET'&&req.headers.get('upgrade')?.toLowerCase()==='websocket')return call(env,d.account_id,'/connect',{'x-role':'pc','x-device':d.id,'x-key':key,upgrade:'websocket'});
  if(p==='device/message'&&req.method==='POST'){
   const raw=await req.text();if(raw.length>12000)return fail('Message too large',413);let b;try{b=JSON.parse(raw)}catch{return fail('Invalid JSON')}
   const response=await publish(env,d,b);if(response.ok)await call(env,d.account_id,'/message');return response;
  }
  if(p==='device/revoke'&&req.method==='POST'){await env.DB.prepare('UPDATE assistant_devices SET revoked=1 WHERE id=?').bind(d.id).run();await call(env,d.account_id,'/revoke',{'x-device':d.id});return reply({ok:true})}
  return fail('Not found',404);
 }
 const user=await account(req,env);if(!user)return fail('Unauthorized',401);
 if(['messages','push/register','push/unregister','push/status'].includes(p))return messages(req,env,user,p);
 const remote=user.scope!=='app'||user.capability==='assistant_remote',pc=user.scope!=='app'||user.capability==='assistant_pc';
 if(p==='socket'&&req.method==='GET'){
  if(!remote)return fail('Assistant remote permission required',403);
  if(req.headers.get('upgrade')?.toLowerCase()!=='websocket')return fail('WebSocket required',426);
  return call(env,user.id,'/connect',{'x-role':'mobile','x-key':await hash(auth.slice(7)),'x-session-kind':user.scope==='app'?'app':'owner',upgrade:'websocket'});
 }
 if(p==='devices'&&req.method==='GET')return remote?call(env,user.id,'/presence'):fail('Assistant remote permission required',403);
 if(req.method!=='POST')return fail('Not found',404);
 const raw=await req.text();if(raw.length>2048)return fail('Request too large',413);
 let b;try{b=JSON.parse(raw)}catch{return fail('Invalid JSON')}
 if(p==='register'){
  if(!pc)return fail('PC registration permission required',403);
  if(typeof b.name!=='string'||!b.name.trim()||b.name.length>80||!token(b.deviceToken))return fail('Invalid device');
  const th=await hash(b.deviceToken),sid=user.scope==='app'?user.app_session_id:null;
  const old=await env.DB.prepare('SELECT id,account_id,revoked,app_session_id FROM assistant_devices WHERE token_hash=?').bind(th).first();
  if(old)return old.account_id===user.id&&!old.revoked&&old.app_session_id===sid?reply({id:old.id,account_id:user.id}):fail('Device key already used',409);
  const id=crypto.randomUUID();
  const inserted=await env.DB.prepare("INSERT OR IGNORE INTO assistant_devices(id,account_id,name,app_session_id,token_hash,last_seen,revoked) SELECT ?,?,?,?,?,0,0 WHERE (SELECT COUNT(*) FROM assistant_devices WHERE account_id=? AND revoked=0)<10").bind(id,user.id,b.name.trim(),sid,th,user.id).run();
  return inserted.meta.changes===1?reply({id,account_id:user.id},201):fail('Device limit',409);
 }
 if(p==='revoke'&&remote&&uuid(b.deviceId)){
  await env.DB.prepare('UPDATE assistant_devices SET revoked=1 WHERE id=? AND account_id=?').bind(b.deviceId,user.id).run();
  await call(env,user.id,'/revoke',{'x-device':b.deviceId});return reply({ok:true});
 }
 return fail('Permission denied or unknown route',403);
}
