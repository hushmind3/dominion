import {dominionSdk} from '../native-sdk.mjs';
import {all,get} from '../store.mjs';

// All SDK endpoints are reached only after the gateway authorization check.
export async function handleSdkRoutes({url,method,req,res,json,body}){
 if(url==='/api/sdk/status'&&method==='GET'){
  json(res,{name:'DOMINION Agent SDK',engine:'native',persistentThreads:true,
    runStreamed:true,delegation:true,customTools:true,externalSdkRequired:false});
  return true;
 }
 if(url==='/api/sdk/threads'){
  if(method==='GET'){
   json(res,all('session').filter(x=>x.sdkOptions).map(x=>({
     id:x.id,title:x.title,agentId:x.agentId,providerId:x.providerId,
     model:x.model,lastActivity:x.lastActivity,parentSessionId:x.parentSessionId
   })));return true;
  }
  if(method==='POST'){
   const thread=dominionSdk.startThread(await body(req));
   json(res,{id:thread.id,options:thread.options},201);return true;
  }
 }
 const match=url.match(/^\/api\/sdk\/threads\/([^/]+)(?:\/(history|run|stream|cancel))?$/);
 if(!match)return false;
 const [,id,action]=match;
 const row=get('session',id);
 if(!row||!row.sdkOptions){json(res,{error:'SDK thread not found'},404);return true;}
 if((!action||action==='history')&&method==='GET'){
  json(res,action==='history'?dominionSdk.resumeThread(id).history():{
   id:row.id,options:row.sdkOptions,title:row.title
  });return true;
 }
 if(action==='cancel'&&method==='POST'){
  json(res,{cancelled:dominionSdk.resumeThread(id).cancel()});return true;
 }
 if((action==='run'||action==='stream')&&method==='POST'){
  const data=await body(req),input=data.input;
  const options={limits:data.limits,reasoningEffort:data.reasoningEffort,thinking:data.thinking};
  const thread=dominionSdk.resumeThread(id);
  if(action==='run'){
   json(res,await thread.run(input,options));return true;
  }
  res.writeHead(200,{'Content-Type':'text/event-stream; charset=utf-8',
    'Cache-Control':'no-cache, no-transform',Connection:'keep-alive',
    'X-Accel-Buffering':'no'});
  const controller=new AbortController();
  const onClose=()=>{if(!res.writableEnded)controller.abort(Error('Client disconnected'));};
  res.on('close',onClose);
  try{
   const {events}=await thread.runStreamed(input,{...options,signal:controller.signal});
   for await(const event of events){
    if(res.writableEnded||controller.signal.aborted)break;
    res.write('data: '+JSON.stringify(event)+'\n\n');
   }
  }catch(error){
   if(!res.writableEnded&&!controller.signal.aborted)
    res.write('data: '+JSON.stringify({type:'sdk.error',error:String(error?.message||error)})+'\n\n');
  }finally{res.off('close',onClose);if(!res.writableEnded)res.end();}
  return true;
 }
 return false;
}
