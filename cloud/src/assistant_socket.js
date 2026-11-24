// Account-scoped hibernating WebSocket relay. No intervals, no idle D1 reads.
import {hash} from './token_crypto.js';
const now=()=>Math.floor(Date.now()/1000);
const uuid=x=>typeof x==='string'&&/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(x);
const read=r=>r==='history'||r==='phone'||typeof r==='string'&&/^attachment\/[a-f0-9]{64}$/.test(r);
const write=new Set(['send','cancel','profile','attachment','phone/start','phone/stop','voice/start','voice/stop']);
const valid=(r,b)=>read(r)?b===null:write.has(r)&&b!==null&&typeof b==='object'&&!Array.isArray(b);
const size=s=>new TextEncoder().encode(s).byteLength;
const out=(x,status=200)=>new Response(JSON.stringify(x),{status,headers:{'content-type':'application/json','cache-control':'no-store'}});
const safeSend=(ws,x)=>{try{ws.send(JSON.stringify(x));return true}catch{return false}};
const close=(ws,code=1008,reason='Not authorized')=>{try{ws.close(code,reason)}catch{}};
export class AssistantSocket {
 constructor(ctx,env){
  this.ctx=ctx;this.env=env;
  if(typeof WebSocketRequestResponsePair!=='undefined')ctx.setWebSocketAutoResponse(new WebSocketRequestResponsePair('ping','pong'));
 }
 meta(ws){return ws.deserializeAttachment()}
 sockets(role){return this.ctx.getWebSockets(role).filter(w=>w.readyState===1)}
 pc(id){return this.sockets('pc').find(w=>this.meta(w)?.device===id)}
 async authorized(m){
  if(!m||!m.account||!m.key)return false;
  const q=m.role==='pc'
   ?this.env.DB.prepare("SELECT d.id FROM assistant_devices d LEFT JOIN app_sessions s ON s.id=d.app_session_id WHERE d.id=? AND d.account_id=? AND d.token_hash=? AND d.revoked=0 AND (d.app_session_id IS NULL OR (s.status='approved' AND s.expires_at>? AND s.capability='assistant_pc'))").bind(m.device,m.account,m.key,now())
   :m.sessionKind==='app'?this.env.DB.prepare("SELECT account_id FROM app_sessions WHERE account_id=? AND token_hash=? AND expires_at>? AND status='approved' AND capability='assistant_remote'").bind(m.account,m.key,now()):this.env.DB.prepare('SELECT account_id FROM sessions WHERE account_id=? AND token_hash=? AND expires_at>?').bind(m.account,m.key,now());
  return Boolean(await q.first());
 }
 async sendMember(ws,data){
  if(!await this.authorized(this.meta(ws))){close(ws);return false}
  return safeSend(ws,data);
 }
 async presence(account){
  const rows=await this.env.DB.prepare('SELECT id,name,last_seen FROM assistant_devices WHERE account_id=? AND revoked=0 ORDER BY last_seen DESC LIMIT 20').bind(account).all();
  return Promise.all(rows.results.map(async r=>{const ws=this.pc(r.id);return {...r,online:Boolean(ws&&await this.authorized(this.meta(ws)))}}));
 }
 async broadcast(account,event){
  await Promise.all(this.sockets('mobile').filter(w=>this.meta(w)?.account===account).map(w=>this.sendMember(w,event)));
 }
 async broadcastPresence(account){if(this.sockets('mobile').length)await this.broadcast(account,{type:'devices',devices:await this.presence(account)})}
 async fetch(req){
  // Only the public Worker constructs these internal requests after authenticating.
  const url=new URL(req.url),account=req.headers.get('x-account');
  if(!account)return out({error:'Missing account'},403);
  if(url.pathname==='/message'){await this.broadcast(account,{type:'message'});return out({ok:true})}
  if(url.pathname==='/presence')return out({devices:await this.presence(account)});
  if(url.pathname==='/revoke'){
   const device=req.headers.get('x-device'),key=req.headers.get('x-key');
   for(const ws of this.ctx.getWebSockets()){
    const m=this.meta(ws);
    if(m?.account===account&&((device&&m.device===device)||(key&&m.key===key)||!await this.authorized(m)))close(ws);
   }
   await this.broadcastPresence(account);return out({ok:true});
  }
  if(url.pathname!=='/connect')return out({error:'Not found'},404);
  const role=req.headers.get('x-role'),device=req.headers.get('x-device'),key=req.headers.get('x-key');
  if(!['pc','mobile'].includes(role))return out({error:'Invalid role'},403);
  const m={role,account,device,key,sessionKind:req.headers.get('x-session-kind')||'owner',cid:crypto.randomUUID()};
  if(!await this.authorized(m))return out({error:'Unauthorized'},401);
  if(role==='mobile'&&this.sockets('mobile').length>=10)return out({error:'Too many connections'},429);
  if(role==='pc'){
   for(const ws of this.sockets('pc').filter(w=>this.meta(w)?.device===device))close(ws,4000,'Replaced');
   await this.env.DB.prepare('UPDATE assistant_devices SET last_seen=? WHERE id=? AND revoked=0').bind(now(),device).run();
  }
  const [client,server]=Object.values(new WebSocketPair());
  this.ctx.acceptWebSocket(server,[role]);server.serializeAttachment(m);
  safeSend(server,{type:'ready'});
  if(role==='mobile')safeSend(server,{type:'devices',devices:await this.presence(account)});
  else await this.broadcastPresence(account);
  return new Response(null,{status:101,webSocket:client});
 }
 async webSocketMessage(ws,message){
  const m=this.meta(ws);
  if(message==='ping'){safeSend(ws,{type:'pong'});return}
  if(typeof message!=='string'||size(message)>900000){close(ws,1009,'Message too large');return}
  if(!await this.authorized(m)){close(ws);return}
  let x;try{x=JSON.parse(message)}catch{close(ws,1007,'Invalid JSON');return}
  if(!x||typeof x!=='object')return;
  try{
   if(m.role==='mobile'&&x.type==='rpc')await this.submit(ws,m,x);
   else if(m.role==='pc'&&x.type==='reply')await this.reply(m,x);
   else if(m.role==='pc'&&x.type==='changed')await this.broadcast(m.account,{type:'changed',deviceId:m.device});
   else if(m.role==='mobile'&&x.type==='devices')safeSend(ws,{type:'devices',devices:await this.presence(m.account)});
  }catch{if(m.role==='mobile'&&uuid(x.id))safeSend(ws,{type:'result',id:x.id,status:503,data:{error:'Relay interrupted; check task history before retrying'}});else close(ws,1011,'Relay error')}
 }
 async submit(ws,m,x){
  const fail=(status,error)=>safeSend(ws,{type:'result',id:x.id,status,data:{error}});
  if(!uuid(x.id)||!uuid(x.deviceId)||!valid(x.route,x.body))return fail(400,'Invalid operation');
  const pc=this.pc(x.deviceId);if(!pc)return fail(503,'PC offline');
  const target=this.meta(pc);
  if(target.account!==m.account||!await this.authorized(target))return fail(503,'PC offline');
  const payload=JSON.stringify(x.body);if(size(payload)>850000)return fail(413,'Request too large');
  const fp=await hash(x.route+'\n'+payload);
  const t=now();
  // The durable admission receipt precedes delivery. Never replay uncertain side effects.
  const inserted=await this.env.DB.prepare("INSERT OR IGNORE INTO assistant_ws_requests(id,account_id,device_id,client_id,device_connection,fingerprint,state,created_at,expires_at) SELECT ?,?,?,?,?,?,'running',?,? WHERE (SELECT COUNT(*) FROM assistant_ws_requests WHERE account_id=? AND created_at>?)<400 AND (SELECT COUNT(*) FROM assistant_ws_requests WHERE device_id=? AND state='running' AND expires_at>?)<8 AND EXISTS(SELECT 1 FROM assistant_devices WHERE id=? AND account_id=? AND revoked=0) AND (EXISTS(SELECT 1 FROM sessions WHERE account_id=? AND token_hash=? AND expires_at>?) OR EXISTS(SELECT 1 FROM app_sessions WHERE account_id=? AND token_hash=? AND expires_at>? AND status='approved' AND capability='assistant_remote'))").bind(x.id,m.account,x.deviceId,m.cid,target.cid,fp,t,t+180,m.account,t-3600,x.deviceId,t,x.deviceId,m.account,m.account,m.key,t,m.account,m.key,t).run();
  if(inserted.meta.changes!==1)return fail(409,'Duplicate request or queue full; check history');
  if(!safeSend(pc,{type:'request',id:x.id,route:x.route,body:x.body})){
   await this.env.DB.prepare("UPDATE assistant_ws_requests SET state='uncertain' WHERE id=? AND state='running'").bind(x.id).run();
   return fail(503,'PC connection lost; check history');
  }
 }
 async reply(m,x){
  if(!uuid(x.id)||!Number.isInteger(x.status)||x.status<200||x.status>599||size(JSON.stringify(x.data??null))>850000)return;
  const row=await this.env.DB.prepare("UPDATE assistant_ws_requests SET state='done' WHERE id=? AND account_id=? AND device_id=? AND device_connection=? AND state='running' AND expires_at>? RETURNING client_id").bind(x.id,m.account,m.device,m.cid,now()).first();
  if(!row)return;
  const ws=this.sockets('mobile').find(w=>this.meta(w)?.cid===row.client_id);
  if(ws)await this.sendMember(ws,{type:'result',id:x.id,status:x.status,data:x.data??null});
 }
 async webSocketClose(ws){
  const m=this.meta(ws);if(m?.role==='pc'){
   await this.env.DB.prepare("UPDATE assistant_ws_requests SET state='uncertain' WHERE device_connection=? AND state='running'").bind(m.cid).run();
   await this.broadcastPresence(m.account);
  }
 }
 async webSocketError(ws){close(ws,1011,'Connection error')}
}
