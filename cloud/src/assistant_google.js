// PC-independent Google connector. Provider credentials stay in the Worker.
const te=new TextEncoder(),td=new TextDecoder(),now=()=>Math.floor(Date.now()/1000);
const b64=b=>btoa(String.fromCharCode(...new Uint8Array(b))).replace(/\+/g,'-').replace(/\//g,'_').replace(/=+$/,'');
const un64=s=>Uint8Array.from(atob(s.replace(/-/g,'+').replace(/_/g,'/')+'='.repeat((4-s.length%4)%4)),c=>c.charCodeAt(0));
const random=()=>b64(crypto.getRandomValues(new Uint8Array(32)));
const digest=async s=>b64(await crypto.subtle.digest('SHA-256',te.encode(s)));
const reply=(x,status=200)=>Response.json(x,{status,headers:{'cache-control':'no-store'}});
const fail=(error,status=400)=>reply({error},status);
const scopes={gmail:'https://www.googleapis.com/auth/gmail.readonly',calendar:'https://www.googleapis.com/auth/calendar.events.readonly',drive:'https://www.googleapis.com/auth/drive.metadata.readonly'};
const operations={gmail:'gmail_recent',calendar:'calendar_upcoming',drive:'drive_recent'};
const cb='/v1/assistant/google/callback';
export const googleReady=e=>e.ASSISTANT_GOOGLE_ENABLED==='1'&&Boolean(e.DB&&e.CONNECTOR_GOOGLE_CLIENT_ID&&e.CONNECTOR_GOOGLE_CLIENT_SECRET&&typeof e.CONNECTOR_TOKEN_SECRET==='string'&&e.CONNECTOR_TOKEN_SECRET.length>=32);
const mobile=u=>u?.scope==='app'&&u.capability==='assistant_remote'&&u.id.startsWith('google:');
async function key(env){return crypto.subtle.importKey('raw',await crypto.subtle.digest('SHA-256',te.encode(env.CONNECTOR_TOKEN_SECRET)),{name:'AES-GCM'},false,['encrypt','decrypt'])}
export async function seal(env,account,value){const iv=crypto.getRandomValues(new Uint8Array(12));return b64(iv)+'.'+b64(await crypto.subtle.encrypt({name:'AES-GCM',iv,additionalData:te.encode(account)},await key(env),te.encode(value)))}
export async function unseal(env,account,cipher){const [iv,body]=cipher.split('.');return td.decode(await crypto.subtle.decrypt({name:'AES-GCM',iv:un64(iv),additionalData:te.encode(account)},await key(env),un64(body)))}
const callbackUrl=req=>new URL(cb,req.url).href;
const authPage=(message,status=200)=>new Response('<!doctype html><html lang="ko"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>DOMINION 연결</title><body><h2>'+message+'</h2><p>이 창을 닫고 DOMINION 앱으로 돌아가세요.</p></body></html>',{status,headers:{'content-type':'text/html; charset=utf-8','cache-control':'no-store','content-security-policy':"default-src 'none'; frame-ancestors 'none'; base-uri 'none'","referrer-policy":"no-referrer"}});
async function body(req){const text=await req.text();if(text.length>4096)throw Error('invalid body');return JSON.parse(text)}
async function token(env,params,send=fetch){const r=await send('https://oauth2.googleapis.com/token',{method:'POST',redirect:'error',headers:{'content-type':'application/x-www-form-urlencoded'},body:new URLSearchParams({client_id:env.CONNECTOR_GOOGLE_CLIENT_ID,client_secret:env.CONNECTOR_GOOGLE_CLIENT_SECRET,...params}),signal:AbortSignal.timeout(15000)});if(!r.ok)return {error:r.status===400?'reauthorize':'provider_unavailable'};return r.json()}
async function api(url,access,send=fetch){const r=await send(url,{redirect:'error',headers:{authorization:'Bearer '+access},signal:AbortSignal.timeout(15000)});if(!r.ok)throw new Error(r.status===401||r.status===403?'google_permission':'google_unavailable');return r.json()}
const clip=(v,n=300)=>String(v??'').slice(0,n);
export async function readGoogle(operation,access,send=fetch){
 if(operation==='gmail_recent'){
  const list=await api('https://gmail.googleapis.com/gmail/v1/users/me/messages?labelIds=INBOX&maxResults=5',access,send);
  const items=await Promise.all((list.messages||[]).slice(0,5).filter(m=>typeof m.id==='string').map(async m=>{
   const u=new URL('https://gmail.googleapis.com/gmail/v1/users/me/messages/'+encodeURIComponent(m.id));u.searchParams.set('format','metadata');for(const h of ['Subject','From','Date'])u.searchParams.append('metadataHeaders',h);
   const x=await api(u.href,access,send),header=k=>x.payload?.headers?.find(h=>h.name?.toLowerCase()===k)?.value;
   return {subject:clip(header('subject')),from:clip(header('from')),date:clip(header('date'),100)};
  }));return items;
 }
 if(operation==='calendar_upcoming'){
  const u=new URL('https://www.googleapis.com/calendar/v3/calendars/primary/events');for(const [k,v] of Object.entries({timeMin:new Date().toISOString(),singleEvents:'true',orderBy:'startTime',maxResults:'10',fields:'items(summary,start,end,location)'}))u.searchParams.set(k,v);
  const r=await api(u.href,access,send);return (r.items||[]).slice(0,10).map(x=>({title:clip(x.summary),start:clip(x.start?.dateTime||x.start?.date,80),end:clip(x.end?.dateTime||x.end?.date,80),location:clip(x.location)}));
 }
 if(operation==='drive_recent'){
  const u=new URL('https://www.googleapis.com/drive/v3/files');for(const [k,v] of Object.entries({q:'trashed = false',pageSize:'10',orderBy:'modifiedTime desc',fields:'files(name,mimeType,modifiedTime)'}))u.searchParams.set(k,v);
  const r=await api(u.href,access,send);return (r.files||[]).slice(0,10).map(x=>({name:clip(x.name),type:clip(x.mimeType,100),modified:clip(x.modifiedTime,80)}));
 }
 throw Error('unknown_operation');
}
export async function cleanupGoogle(env){if(googleReady(env))await env.DB.prepare('DELETE FROM assistant_google_pending WHERE expires_at<=?').bind(now()).run()}
export async function googleRoute(req,env,{account,send=fetch}){
 const p=new URL(req.url).pathname;
 if(!googleReady(env))return fail('Google connector not configured',503);
 if(p===cb&&req.method==='GET'){
  const u=new URL(req.url),state=u.searchParams.get('state')||'',code=u.searchParams.get('code')||'';
  if(!/^[A-Za-z0-9_-]{43}$/.test(state)||!code||code.length>2048)return authPage('Google 연결이 취소되었거나 만료되었습니다.',400);
  // Consume exactly once. The live mobile session must still own this attempt.
  const row=await env.DB.prepare("DELETE FROM assistant_google_pending WHERE state_hash=? AND expires_at>? AND EXISTS(SELECT 1 FROM app_sessions s WHERE s.id=assistant_google_pending.app_session_id AND s.account_id=assistant_google_pending.account_id AND s.status='approved' AND s.expires_at>? AND s.capability='assistant_remote') RETURNING account_id,app_session_id,verifier").bind(await digest(state),now(),now()).first();
  if(!row)return authPage('연결 요청이 만료되었습니다.',403);
  try{
   const t=await token(env,{grant_type:'authorization_code',code,redirect_uri:callbackUrl(req),code_verifier:await unseal(env,row.account_id,row.verifier)},send);
   if(typeof t.access_token!=='string'||typeof t.refresh_token!=='string')return authPage('Google 연결 권한을 다시 승인해 주세요.',403);
   const info=await api('https://openidconnect.googleapis.com/v1/userinfo',t.access_token,send);
   if('google:'+info.sub!==row.account_id)return authPage('DOMINION에 로그인한 Google 계정과 같은 계정을 선택해 주세요.',403);
   const granted=String(t.scope||'').split(/\s+/),allowed=Object.keys(scopes).filter(k=>granted.includes(scopes[k]));
   if(!allowed.length)return authPage('서비스 조회 권한을 선택해 주세요.',403);
   const cipher=await seal(env,row.account_id,t.refresh_token);
   const result=await env.DB.prepare("INSERT INTO assistant_google_grants(account_id,refresh_cipher,scopes,updated_at) SELECT ?,?,?,? WHERE EXISTS(SELECT 1 FROM app_sessions WHERE id=? AND account_id=? AND status='approved' AND expires_at>? AND capability='assistant_remote') ON CONFLICT(account_id) DO UPDATE SET refresh_cipher=excluded.refresh_cipher,scopes=excluded.scopes,updated_at=excluded.updated_at").bind(row.account_id,cipher,JSON.stringify(allowed),now(),row.app_session_id,row.account_id,now()).run();
   return result.meta.changes?authPage('Google 서비스 연결 완료'):authPage('연결 세션이 만료되었습니다.',403);
  }catch{return authPage('Google 연결에 실패했습니다. 다시 시도해 주세요.',502)}
 }
 const user=await account(req,env);if(!mobile(user))return fail('Assistant Google permission required',403);
 if(p==='/v1/assistant/google/status'&&req.method==='GET'){
  const row=await env.DB.prepare('SELECT scopes FROM assistant_google_grants WHERE account_id=?').bind(user.id).first();
  return reply({connected:Boolean(row),services:row?JSON.parse(row.scopes):[]});
 }
 if(req.method!=='POST')return fail('Unknown connector route',404);
 if(p==='/v1/assistant/google/start'){
  const state=random(),verifier=random();
  const inserted=await env.DB.prepare('INSERT INTO assistant_google_pending(state_hash,account_id,app_session_id,verifier,expires_at) SELECT ?,?,?,?,? WHERE (SELECT COUNT(*) FROM assistant_google_pending WHERE account_id=? AND expires_at>?)<5').bind(await digest(state),user.id,user.app_session_id,await seal(env,user.id,verifier),now()+600,user.id,now()).run();
  if(!inserted.meta.changes)return fail('Too many pending connection attempts',429);
  const url=new URL('https://accounts.google.com/o/oauth2/v2/auth');for(const [k,v] of Object.entries({client_id:env.CONNECTOR_GOOGLE_CLIENT_ID,redirect_uri:callbackUrl(req),response_type:'code',scope:'openid '+Object.values(scopes).join(' '),access_type:'offline',prompt:'consent select_account',state,code_challenge:await digest(verifier),code_challenge_method:'S256'}))url.searchParams.set(k,v);
  return reply({url:url.href});
 }
 if(p==='/v1/assistant/google/disconnect'){
  await env.DB.prepare('DELETE FROM assistant_google_pending WHERE account_id=?').bind(user.id).run();
  const row=await env.DB.prepare('DELETE FROM assistant_google_grants WHERE account_id=? RETURNING refresh_cipher').bind(user.id).first();
  let revoked=false;if(row)try{const refresh=await unseal(env,user.id,row.refresh_cipher);const r=await send('https://oauth2.googleapis.com/revoke',{method:'POST',redirect:'error',headers:{'content-type':'application/x-www-form-urlencoded'},body:new URLSearchParams({token:refresh}),signal:AbortSignal.timeout(15000)});revoked=r.ok}catch{}
  return reply({ok:true,revoked});
 }
 if(p!=='/v1/assistant/google/run')return fail('Unknown connector route',404);
 let data;try{data=await body(req)}catch{return fail('Invalid request')}
 const service=Object.keys(operations).find(k=>operations[k]===data?.operation);if(!service)return fail('Unknown operation');
 const row=await env.DB.prepare('SELECT refresh_cipher,scopes FROM assistant_google_grants WHERE account_id=?').bind(user.id).first();
 if(!row||!JSON.parse(row.scopes).includes(service))return fail('Connect Google service first',409);
 try{
  const refresh=await unseal(env,user.id,row.refresh_cipher),t=await token(env,{grant_type:'refresh_token',refresh_token:refresh},send);
  if(t.error==='reauthorize'){await env.DB.prepare('DELETE FROM assistant_google_grants WHERE account_id=? AND refresh_cipher=?').bind(user.id,row.refresh_cipher).run();return fail('Google connection expired. Reconnect.',409)}
  if(typeof t.access_token!=='string')return fail('Google provider unavailable',502);
  const current=await env.DB.prepare('SELECT refresh_cipher FROM assistant_google_grants WHERE account_id=?').bind(user.id).first();
  if(current?.refresh_cipher!==row.refresh_cipher)return fail('Connection changed',409);
  const items=await readGoogle(data.operation,t.access_token,send);return reply({operation:data.operation,items});
 }catch{return fail('Google API unavailable or permission denied',502)}
}
