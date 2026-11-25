import {googleRoute,googleReady} from './assistant_google.js';
import {browserRoute,browserReady} from './assistant_browser.js';

// Thin server-side dispatcher around the existing durable executors.
// No replacement queues, cloned workers, or automatic replay of side effects.
const reply=(data,status=200)=>Response.json(data,{status,headers:{'cache-control':'no-store'}});
const fail=(message,status=400)=>reply({error:message},status);
const idOK=x=>typeof x==='string'&&/^[a-zA-Z0-9_-]{8,80}$/.test(x);
const GOOGLE=new Set(['gmail_recent','calendar_upcoming','drive_recent']);
const BROWSER_ACTIONS=new Set(['view','resume','approve','cancel']);
function forwarded(req,path,payload){
 const url=new URL(req.url);url.pathname=path;url.search='';
 const headers=new Headers(req.headers);headers.delete('content-length');headers.delete('host');
 return new Request(url,{method:payload===undefined?'GET':'POST',headers,body:payload===undefined?undefined:JSON.stringify(payload)});
}
async function parse(req){
 if(Number(req.headers.get('content-length')||0)>12000)throw Error('Too large');
 const raw=await req.text();if(raw.length>12000)throw Error('Too large');
 const v=JSON.parse(raw);if(!v||typeof v!=='object'||Array.isArray(v))throw Error('Invalid body');return v;
}
export function publicWork(kind,job){
 if(!job||!idOK(job.id)||typeof job.status!=='string'||!Number.isFinite(job.updatedAt))return null;
 return {kind,id:job.id,status:job.status,updatedAt:job.updatedAt,job};
}
export async function workRoute(req,env,{account,ctx,adapters={google:googleRoute,browser:browserRoute},readiness={google:googleReady,browser:browserReady}}){
 const user=await account(req,env);
 if(!user||user.scope!=='app'||user.capability!=='assistant_remote')return fail('Assistant remote permission required',403);
 const path=new URL(req.url).pathname,base='/v1/assistant/work/';
 const enabled={google:readiness.google(env),browser:readiness.browser(env)};
 if(path===base+'jobs'&&req.method==='GET'){
  const jobs=[];
  for(const kind of ['google','browser']){
   if(!enabled[kind])continue;
   const upstream=await adapters[kind](forwarded(req,'/v1/assistant/'+kind+'/jobs'),env,{account,ctx});
   if(!upstream.ok)return upstream;
   const data=await upstream.json();if(!Array.isArray(data.jobs))return fail('Job list unavailable',502);
   for(const j of data.jobs.slice(0,100)){const item=publicWork(kind,j);if(item)jobs.push(item)}
  }
  jobs.sort((a,b)=>b.updatedAt-a.updatedAt);
  return reply({jobs:jobs.slice(0,150),available:enabled});
 }
 if(req.method!=='POST'||![base+'start',base+'action'].includes(path))return fail('Work route not found',404);
 let data;try{data=await parse(req)}catch{return fail('Invalid request body')}
 const kind=data.kind;
 if(!['google','browser'].includes(kind)||!idOK(data.id))return fail('Invalid work kind or id');
 if(!enabled[kind])return fail('Requested work executor not configured',503);
 let target,payload;
 if(path===base+'start'){
  if(kind==='google'){
   if(!GOOGLE.has(data.operation))return fail('Unsupported Google operation');
   target='/v1/assistant/google/jobs';payload={id:data.id,operation:data.operation};
  }else{
   if(data.confirmed!==true||typeof data.url!=='string'||typeof data.goal!=='string')return fail('Browser start requires explicit confirmation');
   target='/v1/assistant/browser/start';
   payload={id:data.id,url:data.url,goal:data.goal,confirmed:true,remember:data.remember===true};
  }
 }else{
  const action=data.action;
  if(kind==='google'){
   if(action!=='cancel')return fail('Unsupported Google action');
   target='/v1/assistant/google/jobs/cancel';payload={id:data.id};
  }else{
   if(!BROWSER_ACTIONS.has(action))return fail('Unsupported browser action');
   target='/v1/assistant/browser/'+action;
   payload={id:data.id,...(typeof data.approvalId==='string'?{approvalId:data.approvalId}:{})};
  }
 }
 return adapters[kind](forwarded(req,target,payload),env,{account,ctx});
}
