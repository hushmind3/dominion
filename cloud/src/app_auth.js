import {hash,randomToken} from './token_crypto.js';
const now=()=>Math.floor(Date.now()/1000);
const valid=v=>typeof v==='string'&&/^[A-Za-z0-9_-]{43}$/.test(v);
const reply=(body,status=200)=>new Response(JSON.stringify(body),{status,headers:{'content-type':'application/json; charset=utf-8','cache-control':'no-store'}});
const fail=(error,status=400)=>reply({error},status);
const bearer=r=>/^Bearer (dapp_[A-Za-z0-9_-]{43})$/.exec(r.headers.get('authorization')||'')?.[1];
export const isAppRequest=r=>Boolean(bearer(r));
async function read(r){
 const text=await r.text();
 if(text.length>2048)return null;
 try{return JSON.parse(text)}catch{return null}
}
export async function appAccount(request,env){
 const token=bearer(request);if(!token)return null;
 return env.DB.prepare("SELECT a.id,a.credits,'app' AS scope,s.id AS app_session_id,s.capability FROM app_sessions s JOIN accounts a ON a.id=s.account_id WHERE s.token_hash=? AND s.status='approved' AND s.expires_at>?").bind(await hash(token),now()).first();
}
export async function startApp(request,env){
 const body=await read(request),name=typeof body?.app_name==='string'?body.app_name.trim():'';
 if(!name||name.length>64||/[\x00-\x1f\x7f]/.test(name)||!valid(body?.token_hash))return fail('Invalid app pairing request');
 const capability=body.capability??'inference';
 if(!['inference','assistant_remote','assistant_pc'].includes(capability))return fail('Invalid app capability');
 const issued=now(),id=randomToken();
 const ip=await hash('dominion-app-pair:'+String(request.headers.get('cf-connecting-ip')||'unknown'));
 // A single SQL statement enforces the bound even for simultaneous starts.
 const inserted=await env.DB.prepare("INSERT INTO app_sessions(id,token_hash,app_name,capability,ip_hash,status,created_at,expires_at) SELECT ?,?,?,?,?,'pending',?,? WHERE (SELECT COUNT(*) FROM app_sessions WHERE ip_hash=? AND created_at>?)<20 ON CONFLICT(token_hash) DO NOTHING RETURNING id").bind(id,body.token_hash,name,capability,ip,issued,issued+600,ip,issued-600).first();
 if(!inserted)return fail('Pairing limit or duplicate token',429);
 return reply({id,expires_in:600,status:'pending',verification_uri:new URL('/connect?code='+id,request.url).href});
}
export async function appStatus(request,env){
 const token=bearer(request);if(!token)return fail('Unauthorized',401);
 const row=await env.DB.prepare('SELECT id,status,expires_at FROM app_sessions WHERE token_hash=?').bind(await hash(token)).first();
 if(!row)return fail('Unknown app session',404);
 return reply({id:row.id,status:row.expires_at<=now()?'expired':row.status,expires_at:row.expires_at});
}
export async function disconnectApp(request,env){
 const token=bearer(request);if(!token)return fail('Unauthorized',401);
 await env.DB.prepare("UPDATE app_sessions SET status='revoked' WHERE token_hash=?").bind(await hash(token)).run();
 return reply({status:'revoked'});
}
export async function pendingApp(env,id){
 if(!valid(id))return null;
 return env.DB.prepare("SELECT id,app_name,capability,expires_at FROM app_sessions WHERE id=? AND status='pending' AND expires_at>?").bind(id,now()).first();
}
export async function approveApp(env,id,userId){
 if(!valid(id))return fail('Invalid pairing code');
 const issued=now();
 const row=await env.DB.prepare("UPDATE app_sessions SET account_id=?,status='approved',approved_at=?,expires_at=? WHERE id=? AND status='pending' AND expires_at>? AND (SELECT COUNT(*) FROM app_sessions WHERE account_id=? AND status='approved' AND expires_at>?)<20 RETURNING id,app_name,capability,status,expires_at").bind(userId,issued,issued+7*86400,id,issued,userId,issued).first();
 return row?reply(row):fail('Pairing unavailable or app limit reached',409);
}
export async function manageApps(request,env,user,path){
 if(!user||user.scope==='app')return fail('Owner login required',401);
 if(path==='/v1/apps'&&request.method==='GET'){
  const result=await env.DB.prepare('SELECT id,app_name,capability,status,created_at,expires_at,approved_at FROM app_sessions WHERE account_id=? ORDER BY created_at DESC LIMIT 100').bind(user.id).all();
  return reply({items:result.results||[]});
 }
 const body=await read(request);
 if(!valid(body?.id))return fail('Invalid pairing code');
 if(path==='/v1/apps/inspect'){
  const row=await pendingApp(env,body.id);
  return row?reply(row):fail('Pairing unavailable or expired',404);
 }
 if(path==='/v1/apps/approve'){
  return approveApp(env,body.id,user.id);
 }
 if(path==='/v1/apps/revoke'){
  const row=await env.DB.prepare("UPDATE app_sessions SET status='revoked' WHERE id=? AND account_id=? RETURNING id,status").bind(body.id,user.id).first();
  return row?reply(row):fail('App session not found',404);
 }
 return fail('Not found',404);
}
