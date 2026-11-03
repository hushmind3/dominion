import {all,get,put,remove,log} from './store.mjs';
import {secretSet,secretGet,hasSecret} from './vault.mjs';
// The native engine is the only automatically configured model source.
// Imported external endpoints remain available through explicit user setup.
const legacyDefaults=[
 {id:'lmstudio',name:'LM Studio',baseUrl:'http://127.0.0.1:1234/v1'},
 {id:'ollama',name:'Ollama',baseUrl:'http://127.0.0.1:11434/v1'}
];
for(const preset of legacyDefaults){
 const existing=get('provider',preset.id);
 if(existing&&existing.name===preset.name&&existing.baseUrl===preset.baseUrl&&!existing.description&&!hasSecret('provider:'+preset.id))remove('provider',preset.id);
}
if(!get('provider','dominion-native'))put('provider',{
 id:'dominion-native',name:'DOMINION Local',kind:'openai',
 baseUrl:'http://127.0.0.1:11491/v1',local:true,enabled:false
});
export const providers=()=>all('provider').map(p=>({...p,hasKey:hasSecret('provider:'+p.id)}));
export function saveProvider(data){
 if(!data.name||!data.baseUrl)throw Error('Provider name and base URL required');
 const url=new URL(data.baseUrl);if(!['http:','https:'].includes(url.protocol))throw Error('HTTP(S) required');
 const id=String(data.id||crypto.randomUUID());
 const old=get('provider',id);
 const value=put('provider',{id,name:String(data.name),baseUrl:url.toString().replace(/\/$/,''),kind:data.kind==='ollama'?'ollama':'openai',local:Boolean(data.local),enabled:data.enabled!==false,description:String(data.description||''),createdAt:old?.createdAt});
 if(typeof data.apiKey==='string')secretSet('provider:'+id,data.apiKey);
 log('provider.saved',{providerId:id});return {...value,hasKey:hasSecret('provider:'+id)};
}
export function providerById(id){const p=get('provider',id);if(!p||!p.enabled)throw Error('Provider unavailable: '+id);return p;}
export async function availableModels(id){
 const p=providerById(id), key=secretGet('provider:'+id);
 const base=p.kind==='ollama'?new URL('/api/tags',p.baseUrl).toString():p.baseUrl+'/models';
 const response=await fetch(base,{headers:key?{Authorization:'Bearer '+key}: {},signal:AbortSignal.timeout(6500)});
 if(!response.ok)throw Error(p.name+' models HTTP '+response.status);
 const json=await response.json();
 return (p.kind==='ollama'?(json.models||[]).map(m=>({id:m.name,name:m.name,size:m.size})):
 (json.data||json.models||[]).map(m=>({id:m.id||m.name,name:m.id||m.name}))).filter(m=>m.id);
}
export async function streamCompletion({providerId,model,messages,tools=[],onEvent,signal}){
 const p=providerById(providerId),key=secretGet('provider:'+providerId);
 if(!model)throw Error('Choose a model first');
 const body={model,messages,stream:true,stream_options:{include_usage:true}};
 if(tools.length)body.tools=tools.map(t=>({type:'function',function:{name:t.name,description:t.description,parameters:t.parameters}}));
 const response=await fetch(p.baseUrl+'/chat/completions',{method:'POST',headers:{'Content-Type':'application/json',...(key?{Authorization:'Bearer '+key}:{})},body:JSON.stringify(body),signal:signal||AbortSignal.timeout(300000)});
 if(!response.ok)throw Error(p.name+' '+response.status+': '+(await response.text()).slice(0,900));
 if(!response.body)throw Error('Provider returned an empty stream');
 const out={role:'assistant',content:'',reasoning_content:'',tool_calls:[],usage:null};
 const calls=new Map();
 const reader=response.body.getReader();const decoder=new TextDecoder();let buffer='';
 try{
  while(true){
   const {done,value}=await reader.read();if(done)break;
   buffer+=decoder.decode(value,{stream:true}).replace(/\r\n/g,'\n');
   let sep;
   while((sep=buffer.indexOf('\n\n'))!==-1){
    const block=buffer.slice(0,sep);buffer=buffer.slice(sep+2);
    const payload=block.split('\n').filter(x=>x.startsWith('data:')).map(x=>x.slice(5).trimStart()).join('\n');
    if(!payload||payload==='[DONE]')continue;
    let piece;try{piece=JSON.parse(payload);}catch{continue;}
    if(piece.error)throw Error(JSON.stringify(piece.error));
    if(piece.usage)out.usage=piece.usage;
    const d=piece.choices?.[0]?.delta||{};
    if(d.content){out.content+=d.content;onEvent?.({type:'text',text:d.content});}
    const reason=d.reasoning_content||d.reasoning||'';
    if(reason){out.reasoning_content+=reason;onEvent?.({type:'reasoning',text:reason});}
    for(const tc of d.tool_calls||[]){
      const i=tc.index??0;const t=calls.get(i)||{id:'',type:'function',function:{name:'',arguments:''}};
      if(tc.id)t.id=tc.id;
      if(tc.function?.name)t.function.name+=tc.function.name;
      if(tc.function?.arguments)t.function.arguments+=tc.function.arguments;
      calls.set(i,t);
    }
   }
  }
 }finally{reader.releaseLock();}
 out.tool_calls=[...calls.entries()].sort((a,b)=>a[0]-b[0]).map(x=>x[1]);
 return out;
}
