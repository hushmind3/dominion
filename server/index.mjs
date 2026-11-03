import http from 'node:http';
import fs from 'node:fs/promises';
import path from 'node:path';
import {timingSafeEqual} from 'node:crypto';
import {fileURLToPath} from 'node:url';
import {HOST,PORT,ROOT,IS_LOCAL,ADMIN_TOKEN} from './config.mjs';
import {capabilityInventory,capabilityExecute} from './capability-sources.mjs';
import {all,get,put,remove,messages,recentAudit,bus,pruneInbound} from './store.mjs';
import {providers,saveProvider,availableModels,searchHuggingFace,modelFiles,downloadIntoLMStudio,lmDownloadStatus} from './providers.mjs';
import {agents,sessions,newSession,runTurn,saveRoom} from './engine.mjs';
import {enqueue,startJobs} from './jobs.mjs';
import {workflows,saveWorkflow} from './workflows.mjs';
import {mcpServers,saveMcp,deleteMcp,discoverMcp} from './mcp.mjs';
import {builtin,toolsForModel,approvals,resolveApproval} from './tools.mjs';
import {oauthProviders,oauthStart,oauthCallback,connections,connectorCall} from './oauth.mjs';
import {channels,catalog,channelById,saveChannel,deleteChannel,diagnoseChannel,handleChannelWebhook,startChannels} from './channels.mjs';
const json=(res,data,status=200)=>{res.writeHead(status,{'Content-Type':'application/json; charset=utf-8','Cache-Control':'no-store'});res.end(JSON.stringify(data));};
const fail=(res,e)=>json(res,{error:String(e.message||e)},e.status||400);
async function body(req){
 const parts=[];let size=0;for await(const chunk of req){size+=chunk.length;if(size>1024*1024)throw Error('Body limit: 1 MB');parts.push(chunk);}
 try{return parts.length?JSON.parse(Buffer.concat(parts).toString('utf8')):{};}catch{throw Error('Invalid JSON body');}
}
const methods=new Set(['POST','PUT','PATCH','DELETE']);
function authorized(req){
 if(ADMIN_TOKEN){
  const raw=req.headers['x-dominion-token']||'';
  const a=Buffer.from(String(raw)),b=Buffer.from(ADMIN_TOKEN);
  if(a.length!==b.length||!timingSafeEqual(a,b))return false;
 }
 if(methods.has(req.method)){
  const origin=req.headers.origin;
  if(origin){
   try{const host=new URL(origin).hostname;if(!['localhost','127.0.0.1','[::1]'].includes(host)&&IS_LOCAL)return false;}catch{return false;}
  }
 }
 return true;
}
const send=(res,event)=>{if(!res.writableEnded)res.write('data: '+JSON.stringify(event)+'\n\n');};
async function stream(res,run){
 res.writeHead(200,{'Content-Type':'text/event-stream; charset=utf-8','Cache-Control':'no-cache, no-transform','Connection':'keep-alive','X-Accel-Buffering':'no'});
 try{await run(x=>send(res,x));}
 catch(e){send(res,{type:'error',message:String(e.message||e)});}
 finally{res.end();}
}
const mime={'.html':'text/html; charset=utf-8','.js':'text/javascript; charset=utf-8','.css':'text/css; charset=utf-8','.svg':'image/svg+xml','.png':'image/png','.ico':'image/x-icon','.woff2':'font/woff2','.json':'application/json'};
async function staticFile(req,res,u){
 const dist=path.join(ROOT,'dist');
 let requestPath=decodeURIComponent(u.pathname);
 if(requestPath.includes('\0')||requestPath.includes('..'))return json(res,{error:'Invalid path'},400);
 if(requestPath==='/'||!path.extname(requestPath))requestPath='/index.html';
 const target=path.resolve(dist,'.'+requestPath);
 if(!target.startsWith(dist+path.sep))return json(res,{error:'Invalid path'},403);
 try{
  const content=await fs.readFile(target);
  res.writeHead(200,{'Content-Type':mime[path.extname(target)]||'application/octet-stream','Cache-Control':target.endsWith('index.html')?'no-store':'public,max-age=31536000,immutable'});
  res.end(content);
 }catch{json(res,{error:'Web app not built. Run npm install && npm run build, or npm run dev'},404);}
}
async function api(req,res,u){
 const url=u.pathname,method=req.method;
 if(url==='/api/health')return json(res,{ok:true,service:'dominion',version:'0.1.0',date:new Date().toISOString()});
 if(url.match(/^\/api\/oauth\/(github|google)\/callback$/)&&method==='GET'){
  try{
   const provider=url.split('/')[3];await oauthCallback(provider,u.searchParams.get('code'),u.searchParams.get('state'));
   res.writeHead(200,{'Content-Type':'text/html; charset=utf-8'});return res.end('<!doctype html><meta charset="utf-8"><title>Connected</title><body style="font:16px system-ui;padding:40px">계정 연결이 완료되었습니다. 창을 닫아도 됩니다.<script>window.opener?.postMessage("dominion-oauth-complete","*");window.close()</script></body>');
  }catch(e){return fail(res,e);}
 }
 if(!authorized(req))return json(res,{error:'Unauthorized. Set X-Dominion-Token or use local origin.'},401);
 if(url==='/api/capabilities'&&method==='GET'){
  const family=u.searchParams.get('family')||undefined;
  return json(res,{items:capabilityInventory(family),updatedAt:new Date().toISOString()});
 }
 const capabilityOp=url.match(/^\/api\/capabilities\/([a-z0-9_-]+)\/([a-zA-Z0-9_-]+)\/([a-zA-Z0-9_]+)$/);
 if(capabilityOp&&method==='POST'){
  const [,family,id,operation]=capabilityOp;
  if(!['models','tools','activate','deactivate'].includes(operation))throw Error('Capability action not exposed via HTTP');
  return json(res,await capabilityExecute(family,id,operation,await body(req)));
 }
 if(url==='/api/status')return json(res,{version:'0.1.0',providers:providers().length,agents:agents().length,sessions:sessions().length,rooms:all('room').length,mcp:mcpServers().length,jobs:all('job').filter(x=>x.status==='running').length});
 if(url==='/api/events'&&method==='GET'){
  res.writeHead(200,{'Content-Type':'text/event-stream','Cache-Control':'no-cache','Connection':'keep-alive'});
  send(res,{type:'connected'});
  const listeners=['record','message','audit','job_event','approval','channel_status'].map(n=>{const fn=x=>send(res,{type:n,data:x});bus.on(n,fn);return [n,fn];});
  const ping=setInterval(()=>send(res,{type:'ping'}),25000);
  req.on('close',()=>{clearInterval(ping);listeners.forEach(([n,fn])=>bus.off(n,fn));});
  return;
 }
 if(url==='/api/providers'){
  if(method==='GET')return json(res,providers());
  if(method==='POST')return json(res,saveProvider(await body(req)));
 }
 const modelMatch=url.match(/^\/api\/providers\/([^/]+)\/models$/);
 if(modelMatch&&method==='GET')return json(res,await availableModels(modelMatch[1]));
 if(url==='/api/hf/search'&&method==='GET')return json(res,await searchHuggingFace(u.searchParams.get('q')||''));
 if(url==='/api/hf/files'&&method==='GET')return json(res,await modelFiles(u.searchParams.get('id')||''));
 if(url==='/api/hf/download'&&method==='POST')return json(res,await downloadIntoLMStudio(await body(req)));
 if(url.startsWith('/api/hf/download/')&&method==='GET')return json(res,await lmDownloadStatus(url.split('/').pop()));
 if(url==='/api/sessions'){
  if(method==='GET')return json(res,sessions());
  if(method==='POST')return json(res,newSession(await body(req)));
 }
 const msgs=url.match(/^\/api\/sessions\/([^/]+)\/messages$/);
 if(msgs&&method==='GET')return json(res,messages(msgs[1]));
 if(url==='/api/chat'&&method==='POST'){
  const input=await body(req);
  const sessionId=input.sessionId||newSession({agentId:input.agentId,providerId:input.providerId,model:input.model}).id;
  return stream(res,onEvent=>runTurn({...input,sessionId,onEvent}));
 }
 if(url==='/api/agents'){
  if(method==='GET')return json(res,agents());
  if(method==='POST'){
   const x=await body(req);return json(res,put('agent',{...x,id:x.id||crypto.randomUUID(),name:String(x.name||'Agent'),systemPrompt:String(x.systemPrompt||''),providerId:x.providerId||'',model:x.model||''}));
  }
 }
 if(url==='/api/rooms'){
  if(method==='GET')return json(res,all('room'));
  if(method==='POST')return json(res,saveRoom(await body(req)));
 }
 const roomRun=url.match(/^\/api\/rooms\/([^/]+)\/run$/);
 if(roomRun&&method==='POST')return json(res,enqueue('meeting',{roomId:roomRun[1],goal:(await body(req)).goal}));
 if(url==='/api/workflows'){
  if(method==='GET')return json(res,workflows());
  if(method==='POST')return json(res,saveWorkflow(await body(req)));
 }
 const flowRun=url.match(/^\/api\/workflows\/([^/]+)\/run$/);
 if(flowRun&&method==='POST'){
  if(!get('workflow',flowRun[1]))throw Error('Workflow not found');
  return json(res,enqueue('workflow',{workflowId:flowRun[1],input:String((await body(req)).input||'')}));
 }
 if(url==='/api/jobs'){
  if(method==='GET')return json(res,all('job'));
  if(method==='POST'){const x=await body(req);return json(res,enqueue(x.kind,x.input));}
 }
 if(url==='/api/automations'){
  if(method==='GET')return json(res,all('automation'));
  if(method==='POST'){
   const x=await body(req);if(!['prompt','meeting','workflow'].includes(x.kind))throw Error('Invalid task kind');
   return json(res,put('automation',{...x,id:x.id||crypto.randomUUID(),enabled:x.enabled!==false,nextAt:x.nextAt||new Date(Date.now()+3600000).toISOString()}));
  }
 }
 if(url==='/api/mcp/catalog'&&method==='GET'){
  const q=(u.searchParams.get('q')||'').trim();
  if(q.length<3)throw Error('Search term must be at least 3 characters');
  const endpoint=new URL('https://registry.modelcontextprotocol.io/v0.1/servers');
  endpoint.searchParams.set('search',q);endpoint.searchParams.set('version','latest');endpoint.searchParams.set('limit','25');
  const response=await fetch(endpoint,{signal:AbortSignal.timeout(12000)});
  if(!response.ok)throw Error('MCP registry HTTP '+response.status);
  const raw=await response.json();
  return json(res,(raw.servers||[]).map(x=>{
   const s=x.server||x;const pkg=(s.packages||[]).find(p=>(p.registryType||p.registry_type)==='npm');
   const remote=(s.remotes||[]).find(r=>/^https:\/\//.test(r.url||''));
   return {name:s.name,description:s.description||'',packageName:pkg?.identifier||'',url:remote?.url||''};
  }));
 }
 if(url==='/api/mcp'){
  if(method==='GET')return json(res,mcpServers());
  if(method==='POST')return json(res,saveMcp(await body(req)));
 }
 if(url==='/api/mcp/discover'&&method==='GET')return json(res,await discoverMcp());
 if(url==='/api/tools'&&method==='GET')return json(res,await toolsForModel());
 if(url==='/api/approvals'&&method==='GET')return json(res,approvals());
 if(url==='/api/approvals'&&method==='POST'){const x=await body(req);return json(res,{ok:resolveApproval(x.id,x.allow)});}
 if(url==='/api/skills'){
  if(method==='GET')return json(res,all('skill'));
  if(method==='POST'){const x=await body(req);return json(res,put('skill',{...x,id:x.id||crypto.randomUUID(),name:String(x.name||'Skill'),enabled:Boolean(x.enabled)}));}
 }
 if(url==='/api/memories'){
  if(method==='GET')return json(res,all('memory'));
  if(method==='POST'){const x=await body(req);return json(res,put('memory',{...x,id:x.id||crypto.randomUUID(),text:String(x.text||'')}));}
 }
 if(url==='/api/audit'&&method==='GET')return json(res,recentAudit(120));
 if(url==='/api/policy'){
  if(method==='GET')return json(res,get('policy','default')||{id:'default',autoApprove:[],autoApproveMcp:false,autoApproveAll:false});
  if(method==='POST'){const x=await body(req);return json(res,put('policy',{id:'default',autoApprove:Array.isArray(x.autoApprove)?x.autoApprove:[],autoApproveMcp:Boolean(x.autoApproveMcp),autoApproveAll:Boolean(x.autoApproveAll)}));}
 }
 if(url==='/api/channels/catalog'&&method==='GET')return json(res,catalog());
 if(url==='/api/channels'){
  if(method==='GET')return json(res,channels());
  if(method==='POST')return json(res,await saveChannel(await body(req)));
 }
 const channelId=url.match(/^\/api\/channels\/([^/]+)$/);
 if(channelId&&method==='GET')return json(res,channelById(channelId[1])||{error:'Channel not found'});
 const diagnostic=url.match(/^\/api\/channels\/([^/]+)\/diagnose$/);
 if(diagnostic&&method==='POST')return json(res,await diagnoseChannel(diagnostic[1]));
 if(url==='/api/oauth/providers'&&method==='GET')return json(res,oauthProviders());
 if(url==='/api/oauth/connections'&&method==='GET')return json(res,connections());
 if(url.match(/^\/api\/oauth\/(github|google)\/start$/)&&method==='GET'){
  const provider=url.split('/')[3],origin='http://127.0.0.1:'+PORT;
  return json(res,{url:oauthStart(provider,origin)});
 }
 if(url.match(/^\/api\/connector\/(github|google)\/(repos|me|youtube_channels|youtube_search)$/)&&method==='GET'){
  const [,provider,action]=url.match(/^\/api\/connector\/(github|google)\/([^/]+)$/);
  return json(res,await connectorCall(provider,action));
 }
 const deleted=url.match(/^\/api\/(agents|rooms|skills|memories|automations|workflows|channels|mcp|providers)\/([^/]+)$/);
 if(deleted&&method==='DELETE'){
  const kind=deleted[1],id=deleted[2];
  if(kind==='mcp')await deleteMcp(id);
  else if(kind==='channels')await deleteChannel(id);
  else remove(({agents:'agent',rooms:'room',skills:'skill',memories:'memory',automations:'automation',workflows:'workflow',providers:'provider'})[kind],id);
  return json(res,{ok:true});
 }
 return json(res,{error:'Not found',path:url},404);
}
const server=http.createServer(async(req,res)=>{
 try{
  const u=new URL(req.url,'http://127.0.0.1');
  if(u.pathname.startsWith('/api/webhooks/') && await handleChannelWebhook(req,res,u))return;
  if(u.pathname.startsWith('/api/'))await api(req,res,u);
  else await staticFile(req,res,u);
 }catch(e){if(!res.headersSent)fail(res,e);else{if(res.getHeader('Content-Type')?.includes('text/event-stream'))send(res,{type:'error',message:String(e)});res.end();}}
});
pruneInbound();agents();startJobs();startChannels();
server.listen(PORT,HOST,()=>console.log('DOMINION running at http://'+HOST+':'+PORT));
process.on('SIGTERM',()=>server.close());
