import {hash,randomToken} from './token_crypto.js';
const now=()=>Math.floor(Date.now()/1000);
const secret=x=>typeof x==='string'&&/^[A-Za-z0-9_-]{43}$/.test(x);
const uuid=x=>typeof x==='string'&&/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(x);
const size=x=>new TextEncoder().encode(x).length;
const out=(data,status=200)=>new Response(JSON.stringify(data),{status,headers:{'content-type':'application/json; charset=utf-8','cache-control':'no-store'}});
const err=(error,status=400)=>out({error},status);
const origin='https://dominion-cloud.dominion-hushmind.workers.dev';
const callback=origin+'/v1/assistant/auth/callback';
const routes=new Set(['history','phone','send','cancel','profile','attachment','phone/start','phone/stop','voice/start','voice/stop']);
const reads=new Set(['history','phone']);
const readRoute=r=>reads.has(r)||typeof r==='string'&&/^attachment\/[a-f0-9]{64}$/.test(r);
const validRoute=r=>routes.has(r)||readRoute(r);
const page=(ok)=>new Response('<!doctype html><meta charset="utf-8"><meta name="viewport" content="width=device-width"><title>DOMINION</title><h1>'+(ok?'로그인 완료':'로그인 실패')+'</h1><p>DOMINION 앱으로 돌아가세요.</p>',{status:ok?200:400,headers:{'content-type':'text/html; charset=utf-8','cache-control':'no-store','referrer-policy':'no-referrer','content-security-policy':"default-src 'none'; frame-ancestors 'none'"}});
const cookie=(v,age=600)=>'dominion_assistant_oauth='+v+'; Secure; HttpOnly; SameSite=Lax; Path=/v1/assistant/auth/; Max-Age='+age;
async function body(req,limit=950000){
 const n=Number(req.headers.get('content-length')||0);if(n>limit)throw Error('Request too large');
 const reader=req.body?.getReader();if(!reader)throw Error('JSON required');let total=0,parts=[];
 try{while(true){const {done,value}=await reader.read();if(done)break;total+=value.byteLength;if(total>limit){await reader.cancel();throw Error('Request too large')}parts.push(value)}}finally{reader.releaseLock()}
 const buf=new Uint8Array(total);let offset=0;for(const part of parts){buf.set(part,offset);offset+=part.length}const v=JSON.parse(new TextDecoder().decode(buf));if(!v||Array.isArray(v)||typeof v!=='object')throw Error('JSON object required');return v;
}
export const ready=env=>env.ASSISTANT_LINK_ENABLED==='1'&&Boolean(env.DB&&env.GOOGLE_CLIENT_ID&&env.GOOGLE_CLIENT_SECRET);
export async function cleanupAssistant(env){
 if(!ready(env))return;
 await env.DB.batch([
  env.DB.prepare('DELETE FROM assistant_login WHERE expires_at<?').bind(now()-60),
  env.DB.prepare('DELETE FROM assistant_rpc WHERE expires_at<?').bind(now()-600),
  env.DB.prepare('DELETE FROM assistant_ws_requests WHERE expires_at<?').bind(now()-3600)
 ]);
}
async function auth(req,env,path,verifyGoogle){
 if(!ready(env))return err('Assistant cloud not configured',503);
 const u=new URL(req.url);
 if(path==='auth/start'&&req.method==='POST'){
  const ip=await hash((env.EMAIL_RATE_PEPPER||env.GOOGLE_CLIENT_SECRET)+':'+(req.headers.get('cf-connecting-ip')||'unknown'));
  const count=await env.DB.prepare('SELECT COUNT(*) AS n FROM assistant_login WHERE ip_hash=? AND created_at>?').bind(ip,now()-600).first();
  if(Number(count?.n||0)>=12)return err('Too many login attempts',429);
  const id=randomToken(),poll=randomToken(),ticket=randomToken();
  await env.DB.prepare('INSERT INTO assistant_login(id,poll_hash,ticket_hash,nonce,ip_hash,status,created_at,expires_at) VALUES(?,?,?,?,?,?,?,?)').bind(id,await hash(poll),await hash(ticket),randomToken(),ip,'pending',now(),now()+600).run();
  return out({id,poll_token:poll,url:origin+'/v1/assistant/auth/open?id='+id+'&ticket='+ticket,expires_in:600});
 }
 if(path==='auth/open'&&req.method==='GET'){
  const id=u.searchParams.get('id'),ticket=u.searchParams.get('ticket');
  if(!secret(id)||!secret(ticket))return page(false);
  const row=await env.DB.prepare("SELECT nonce FROM assistant_login WHERE id=? AND ticket_hash=? AND status='pending' AND expires_at>?").bind(id,await hash(ticket),now()).first();
  if(!row)return page(false);
  const browser=randomToken();
  const change=await env.DB.prepare("UPDATE assistant_login SET browser_hash=?,status='opened' WHERE id=? AND status='pending' AND expires_at>?").bind(await hash(browser),id,now()).run();
  if(change.meta.changes!==1)return page(false);
  const url=new URL('https://accounts.google.com/o/oauth2/v2/auth');
  for(const [k,v] of Object.entries({client_id:env.GOOGLE_CLIENT_ID,redirect_uri:callback,response_type:'code',scope:'openid email profile',state:id,nonce:row.nonce,prompt:'select_account'}))url.searchParams.set(k,v);
  return new Response(null,{status:302,headers:{location:url.href,'set-cookie':cookie(browser),'cache-control':'no-store','referrer-policy':'no-referrer'}});
 }
 if(path==='auth/callback'&&req.method==='GET'){
  const id=u.searchParams.get('state'),code=u.searchParams.get('code'),browser=(req.headers.get('cookie')||'').split(';').map(s=>s.trim()).find(s=>s.startsWith('dominion_assistant_oauth='))?.split('=')[1];
  if(!secret(id)||!secret(browser)||!code||code.length>2048)return page(false);
  const bh=await hash(browser),row=await env.DB.prepare("SELECT nonce FROM assistant_login WHERE id=? AND browser_hash=? AND status='opened' AND expires_at>?").bind(id,bh,now()).first();
  if(!row)return page(false);
  const claim=await env.DB.prepare("UPDATE assistant_login SET status='exchanging' WHERE id=? AND browser_hash=? AND status='opened' AND expires_at>?").bind(id,bh,now()).run();
  if(claim.meta.changes!==1)return page(false);
  try{
   const res=await fetch('https://oauth2.googleapis.com/token',{method:'POST',headers:{'content-type':'application/x-www-form-urlencoded'},body:new URLSearchParams({client_id:env.GOOGLE_CLIENT_ID,client_secret:env.GOOGLE_CLIENT_SECRET,grant_type:'authorization_code',redirect_uri:callback,code}),signal:AbortSignal.timeout(12000)});
   if(!res.ok)throw Error('exchange');
   const token=(await res.json()).id_token;if(typeof token!=='string'||token.length>10000)throw Error('token');
   const claims=await verifyGoogle(token,env);if(claims.aud!==env.GOOGLE_CLIENT_ID||claims.nonce!==row.nonce)throw Error('nonce');
   const idAccount='google:'+claims.sub;
   const result=await env.DB.batch([
    env.DB.prepare('INSERT INTO accounts(id,email,credits,created_at) VALUES(?,?,0,?) ON CONFLICT(id) DO UPDATE SET email=excluded.email').bind(idAccount,String(claims.email||'').slice(0,320),now()),
    env.DB.prepare("UPDATE assistant_login SET status='verified',account_id=?,session_until=? WHERE id=? AND status='exchanging' AND expires_at>?").bind(idAccount,now()+7*86400,id,now())
   ]);
   return page(result[1].meta.changes===1);
  }catch{await env.DB.prepare("UPDATE assistant_login SET status='failed' WHERE id=? AND status='exchanging'").bind(id).run();return page(false)}
 }
 if(path==='auth/status'&&req.method==='POST'){
  const b=await body(req,2048);if(!secret(b.id)||!secret(b.poll_token))return err('Invalid login');
  const ph=await hash(b.poll_token),row=await env.DB.prepare('SELECT status,account_id,session_until,expires_at FROM assistant_login WHERE id=? AND poll_hash=?').bind(b.id,ph).first();
  if(!row||row.expires_at<=now())return err('Login expired',410);
  if(row.status==='failed')return err('Google login failed',400);
  if(!['verified','issued'].includes(row.status))return out({status:'waiting'});
  const token=await hash('dominion-assistant-session:'+b.id+':'+b.poll_token);
  await env.DB.batch([
   env.DB.prepare("INSERT OR IGNORE INTO sessions(token_hash,account_id,expires_at) SELECT ?,account_id,session_until FROM assistant_login WHERE id=? AND poll_hash=? AND status='verified' AND expires_at>?").bind(await hash(token),b.id,ph,now()),
   env.DB.prepare("UPDATE assistant_login SET status='issued' WHERE id=? AND poll_hash=? AND status='verified' AND expires_at>?").bind(b.id,ph,now())
  ]);
  const session=await env.DB.prepare('SELECT account_id FROM sessions WHERE token_hash=? AND expires_at>?').bind(await hash(token),now()).first();
  if(!session||session.account_id!==row.account_id)return err('Login session revoked',401);
  return out({status:'done',token,account_id:row.account_id,expires_in:row.session_until-now()});
 }
 return err('Not found',404);
}
async function device(req,env){
 const token=/^Device ([A-Za-z0-9_-]{43})$/.exec(req.headers.get('authorization')||'')?.[1];
 if(!token)return null;
 return env.DB.prepare('SELECT id,account_id FROM assistant_devices WHERE token_hash=? AND revoked=0').bind(await hash(token)).first();
}
function socketStub(env,account){return env.ASSISTANT_SOCKET.get(env.ASSISTANT_SOCKET.idFromName(account))}
async function socketCall(env,account,path,headers={}){
 return socketStub(env,account).fetch(new Request('https://internal'+path,{headers:{'x-account':account,...headers}}));
}
export async function assistantRoute(req,env,{account,verifyGoogle}){
 const path=new URL(req.url).pathname.slice('/v1/assistant/'.length);
 if(path.startsWith('auth/'))return auth(req,env,path,verifyGoogle);
 if(!ready(env))return err('Assistant cloud not configured',503);
 if(path==='socket'&&req.method==='GET'){
  if(!env.ASSISTANT_SOCKET)return err('Socket relay not configured',503);
  if(req.headers.get('upgrade')?.toLowerCase()!=='websocket')return err('WebSocket required',426);
  const authorization=req.headers.get('authorization')||'';
  if(authorization.startsWith('Device ')){
   const d=await device(req,env);if(!d)return err('Unauthorized device',401);
   return socketCall(env,d.account_id,'/connect',{'x-role':'pc','x-device':d.id,'x-key':await hash(authorization.slice(7)),upgrade:'websocket'});
  }
  const user=await account(req,env);if(!user)return err('Unauthorized',401);
  return socketCall(env,user.id,'/connect',{'x-role':'mobile','x-key':await hash(authorization.slice(7)),upgrade:'websocket'});
 }
 if(env.ASSISTANT_SOCKET&&['device/pull','device/reply','rpc','rpc/status'].includes(path))return err('Use WebSocket relay',410);
 if(path.startsWith('device/')){
  const d=await device(req,env);if(!d)return err('Unauthorized device',401);
  if(req.method!=='POST')return err('Method not allowed',405);
  const b=await body(req);
  if(path==='device/pull'){
   await env.DB.prepare('UPDATE assistant_devices SET last_seen=? WHERE id=? AND revoked=0 AND last_seen<?').bind(now(),d.id,now()-25).run();
   // One atomic claim. A lost delivery is not retried, since it may be a paid call or side effect.
   const row=await env.DB.prepare("UPDATE assistant_rpc SET state='running' WHERE id=(SELECT id FROM assistant_rpc WHERE device_id=? AND state='queued' AND expires_at>? ORDER BY created_at,id LIMIT 1) AND state='queued' AND EXISTS(SELECT 1 FROM assistant_devices WHERE id=? AND revoked=0) RETURNING id,route,payload").bind(d.id,now(),d.id).first();
   return out({request:row?{id:row.id,route:row.route,body:JSON.parse(row.payload)}:null});
  }
  if(path==='device/reply'){
   if(!uuid(b.id)||!Number.isInteger(b.status)||b.status<200||b.status>599)return err('Invalid reply');
   const result=JSON.stringify(b.data??null);if(size(result)>850000)return err('Reply too large',413);
   const r=await env.DB.prepare("UPDATE assistant_rpc SET state='done',status=?,result=?,payload='null' WHERE id=? AND device_id=? AND state='running' AND expires_at>? AND EXISTS(SELECT 1 FROM assistant_devices WHERE id=? AND revoked=0)").bind(b.status,result,b.id,d.id,now(),d.id).run();
   if(r.meta.changes===1)return out({ok:true});
   const old=await env.DB.prepare("SELECT status,result FROM assistant_rpc WHERE id=? AND device_id=? AND state='done'").bind(b.id,d.id).first();
   return old&&old.status===b.status&&old.result===result?out({ok:true}):err('Request expired or changed',409);
  }
  if(path==='device/revoke'){
   await env.DB.prepare('UPDATE assistant_devices SET revoked=1 WHERE id=?').bind(d.id).run();if(env.ASSISTANT_SOCKET)await socketCall(env,d.account_id,'/revoke',{'x-device':d.id});return out({ok:true});
  }
  return err('Not found',404);
 }
 const user=await account(req,env);if(!user)return err('Unauthorized',401);
 if(path==='devices'&&req.method==='GET'){
  if(env.ASSISTANT_SOCKET)return socketCall(env,user.id,'/presence');
  const rows=await env.DB.prepare('SELECT id,name,last_seen FROM assistant_devices WHERE account_id=? AND revoked=0 ORDER BY last_seen DESC LIMIT 20').bind(user.id).all();
  return out({devices:rows.results.map(r=>({...r,online:r.last_seen>now()-70}))});
 }
 if(req.method!=='POST')return err('Method not allowed',405);
 const b=await body(req);
 if(path==='register'){
  if(typeof b.name!=='string'||!b.name.trim()||b.name.length>80||!secret(b.deviceToken))return err('Invalid device');
  const th=await hash(b.deviceToken);
  const old=await env.DB.prepare('SELECT id,account_id,revoked FROM assistant_devices WHERE token_hash=?').bind(th).first();
  if(old)return old.account_id===user.id&&!old.revoked?out({id:old.id,account_id:user.id}):err('Device key already used',409);
  const id=crypto.randomUUID();
  const inserted=await env.DB.prepare("INSERT OR IGNORE INTO assistant_devices(id,account_id,name,token_hash,last_seen,revoked) SELECT ?,?,?,?,?,0 WHERE (SELECT COUNT(*) FROM assistant_devices WHERE account_id=? AND revoked=0)<10").bind(id,user.id,b.name.trim(),th,now(),user.id).run();
  return inserted.meta.changes===1?out({id,account_id:user.id},201):err('Device limit or duplicate',409);
 }
 if(path==='logout'){
  const token=/^Bearer ([A-Za-z0-9_-]{32,})$/.exec(req.headers.get('authorization')||'')?.[1];
  if(token){const key=await hash(token);await env.DB.prepare('DELETE FROM sessions WHERE token_hash=? AND account_id=?').bind(key,user.id).run();if(env.ASSISTANT_SOCKET)await socketCall(env,user.id,'/revoke',{'x-key':key})}
  return out({ok:true});
 }
 if(path==='revoke'){
  if(!uuid(b.deviceId))return err('Invalid device');await env.DB.prepare('UPDATE assistant_devices SET revoked=1 WHERE id=? AND account_id=?').bind(b.deviceId,user.id).run();if(env.ASSISTANT_SOCKET)await socketCall(env,user.id,'/revoke',{'x-device':b.deviceId});return out({ok:true});
 }
 if(path==='rpc'){
  if(!uuid(b.deviceId)||!uuid(b.id)||!validRoute(b.route)||(readRoute(b.route)?b.body!==null:!b.body||typeof b.body!=='object'||Array.isArray(b.body)))return err('Invalid operation');
  const d=await env.DB.prepare('SELECT id,last_seen FROM assistant_devices WHERE id=? AND account_id=? AND revoked=0').bind(b.deviceId,user.id).first();
  if(!d)return err('Device not found',404);
  const payload=JSON.stringify(b.body);if(size(payload)>850000)return err('Attachment or request too large',413);
  const fp=await hash(b.route+'\n'+payload);
  const existing=await env.DB.prepare('SELECT fingerprint,device_id FROM assistant_rpc WHERE id=? AND account_id=?').bind(b.id,user.id).first();
  if(existing)return existing.fingerprint===fp&&existing.device_id===d.id?out({id:b.id},202):err('Request conflict',409);
  if(d.last_seen<now()-70)return err('PC offline',503);
  const r=await env.DB.prepare("INSERT OR IGNORE INTO assistant_rpc(id,account_id,device_id,route,payload,fingerprint,state,created_at,expires_at) SELECT ?,?,?,?,?,?,'queued',?,? WHERE (SELECT COUNT(*) FROM assistant_rpc WHERE account_id=? AND created_at>?)<400 AND (SELECT COUNT(*) FROM assistant_rpc WHERE device_id=? AND state IN ('queued','running') AND expires_at>?)<8 AND EXISTS(SELECT 1 FROM assistant_devices WHERE id=? AND account_id=? AND revoked=0)").bind(b.id,user.id,d.id,b.route,payload,fp,now(),now()+180,user.id,now()-3600,d.id,now(),d.id,user.id).run();
  return r.meta.changes===1?out({id:b.id},202):err('Queue busy or duplicate',429);
 }
 if(path==='rpc/status'){
  if(!uuid(b.id))return err('Invalid request');
  const r=await env.DB.prepare('SELECT r.state,r.status,r.result,r.expires_at FROM assistant_rpc r JOIN assistant_devices d ON d.id=r.device_id WHERE r.id=? AND r.account_id=? AND d.revoked=0').bind(b.id,user.id).first();
  if(!r)return err('Request not found',404);
  if(r.expires_at<=now())return err(r.state==='queued'?'Request expired before delivery':'Request outcome uncertain or expired; check PC before retrying',409);
  return r.state==='done'?out({state:'done',status:r.status,data:JSON.parse(r.result)}):out({state:r.state});
 }
 return err('Not found',404);
}
