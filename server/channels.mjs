import {all,put,get,log} from './store.mjs';
import {secretSet,secretGet} from './vault.mjs';
import {newSession,runTurn} from './engine.mjs';
const workers=new Map();
export const channels=()=>all('channel').map(x=>({...x,hasToken:Boolean(secretGet('channel:'+x.id))}));
export async function saveChannel(data){
 if(!['telegram','discord'].includes(data.kind))throw Error('Unsupported channel');
 const id=data.id||crypto.randomUUID();
 const record=put('channel',{id,name:String(data.name||data.kind),kind:data.kind,enabled:data.enabled!==false,agentId:data.agentId||'general',providerId:data.providerId||'',model:data.model||'',mentionOnly:data.mentionOnly!==false});
 if(typeof data.token==='string')secretSet('channel:'+id,data.token);
 await stopChannel(id);
 if(record.enabled)startChannel(id);
 return record;
}
export async function stopChannel(id){
 const w=workers.get(id);if(w){workers.delete(id);w.stop();}return true;
}
async function dispatch(record,conversationId,text,reply){
 const key=record.id+':'+conversationId;
 let session=all('session').find(x=>x.channelKey===key);
 if(!session){session=newSession({title:record.name+' '+conversationId,agentId:record.agentId,providerId:record.providerId,model:record.model});session=put('session',{...session,channelKey:key});}
 try{
  let response='';
  await runTurn({sessionId:session.id,agentId:record.agentId,providerId:record.providerId,model:record.model,input:text,onEvent:x=>{if(x.type==='text')response+=x.text;}});
  if(response.trim())await reply(response.slice(0,1800));
 }catch(e){log('channel.error',{channelId:record.id,error:String(e)});}
}
function telegram(record,token){
 let active=true,offset=0;
 const url='https://api.telegram.org/bot'+token+'/';
 async function api(method,body){
  const r=await fetch(url+method,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(body),signal:AbortSignal.timeout(38000)});
  if(!r.ok)throw Error('Telegram '+r.status);const d=await r.json();if(!d.ok)throw Error(d.description||'Telegram API error');return d.result;
 }
 (async()=>{
  while(active){
   try{
    const rows=await api('getUpdates',{offset,timeout:25,allowed_updates:['message']});
    for(const e of rows){offset=e.update_id+1;const m=e.message;if(!m?.text||m.from?.is_bot)continue;
     const chatId=m.chat?.id, text=m.text.trim();
     if(record.mentionOnly&&m.chat?.type!=='private'&&!text.includes('@'))continue;
     await dispatch(record,String(chatId),text,answer=>api('sendMessage',{chat_id:chatId,text:answer}));
    }
   }catch(e){if(active){log('channel.telegram.retry',{id:record.id,error:String(e)});await new Promise(r=>setTimeout(r,5000));}}
  }
 })();
 return {stop:()=>{active=false;}};
}
function discord(record,token){
 let active=true,socket=null,heartbeat=null,lastSeq=null;
 const intents=(1<<9)|(1<<12)|(1<<15);
 const connect=()=>{
  if(!active)return;
  socket=new WebSocket('wss://gateway.discord.gg/?v=10&encoding=json');
  socket.addEventListener('message',async e=>{
   let d;try{d=JSON.parse(e.data);}catch{return;}
   if(d.s!==null&&d.s!==undefined)lastSeq=d.s;
   if(d.op===10){
    heartbeat=setInterval(()=>{if(socket?.readyState===1)socket.send(JSON.stringify({op:1,d:lastSeq}));},d.d.heartbeat_interval);
    socket.send(JSON.stringify({op:2,d:{token,intents,properties:{os:process.platform,browser:'dominion',device:'dominion'}}}));
   }
   if(d.op===0&&d.t==='MESSAGE_CREATE'){
    const m=d.d;if(m.author?.bot||!m.content)return;
    if(record.mentionOnly&&m.guild_id&&!(m.mentions||[]).some(x=>x.bot))return;
    const reply=async text=>{
     const r=await fetch('https://discord.com/api/v10/channels/'+m.channel_id+'/messages',{method:'POST',headers:{Authorization:'Bot '+token,'Content-Type':'application/json'},body:JSON.stringify({content:text.slice(0,1900)})});
     if(!r.ok)throw Error('Discord send '+r.status);
    };
    await dispatch(record,String(m.channel_id),m.content,reply);
   }
  });
  socket.addEventListener('close',()=>{clearInterval(heartbeat);if(active)setTimeout(connect,5000);});
  socket.addEventListener('error',e=>log('channel.discord.error',{id:record.id,message:String(e.message||'socket error')}));
 };
 connect();
 return {stop:()=>{active=false;clearInterval(heartbeat);socket?.close();}};
}
export function startChannel(id){
 const record=get('channel',id);if(!record?.enabled)return;
 const token=secretGet('channel:'+id);if(!token){log('channel.missing_token',{id});return;}
 try{workers.set(id,record.kind==='telegram'?telegram(record,token):discord(record,token));log('channel.started',{id,kind:record.kind});}
 catch(e){log('channel.start_error',{id,error:String(e)});}
}
export function startChannels(){for(const c of all('channel').filter(x=>x.enabled))startChannel(c.id);}
