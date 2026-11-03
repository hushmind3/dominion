import {all,get,put,log} from './store.mjs';
import {secretSet,secretGet,hasSecret} from './vault.mjs';
const defaults=[
 {id:'lmstudio',name:'LM Studio',kind:'openai',baseUrl:'http://127.0.0.1:1234/v1',local:true,enabled:true},
 {id:'ollama',name:'Ollama',kind:'ollama',baseUrl:'http://127.0.0.1:11434/v1',local:true,enabled:true},
 {id:'deepseek',name:'DeepSeek',kind:'openai',baseUrl:'https://api.deepseek.com/v1',local:false,enabled:false}
];
for(const x of defaults)if(!get('provider',x.id))put('provider',x);
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
export async function searchHuggingFace(query='',limit=24){
 const url=new URL('https://huggingface.co/api/models');
 url.searchParams.set('search',query);url.searchParams.set('limit',String(Math.max(1,Math.min(40,limit))));
 url.searchParams.set('sort','downloads');url.searchParams.set('direction','-1');url.searchParams.set('full','false');
 const res=await fetch(url,{signal:AbortSignal.timeout(12000)});
 if(!res.ok)throw Error('Hugging Face HTTP '+res.status);
 const rows=await res.json();
 return rows.map(x=>({id:x.id,downloads:x.downloads||0,likes:x.likes||0,task:x.pipeline_tag||'',tags:(x.tags||[]).filter(t=>/gguf|mlx|text-generation|quantized/i.test(t)).slice(0,8)}));
}
export async function modelFiles(id){
 if(!/^[\w.-]+\/[\w.-]+$/.test(id))throw Error('Invalid model identifier');
 const r=await fetch('https://huggingface.co/api/models/'+id+'/tree/main?recursive=true&expand=false',{signal:AbortSignal.timeout(12000)});
 if(!r.ok)throw Error('Hugging Face files HTTP '+r.status);
 return (await r.json()).filter(f=>f.type==='file'&&/\.(gguf|safetensors)$/i.test(f.path)).map(f=>({path:f.path,size:f.size||0})).slice(0,120);
}
export async function downloadIntoLMStudio({modelId,quantization}){
 if(!/^[\w.-]+\/[\w.-]+$/.test(modelId))throw Error('Invalid model identifier');
 const endpoint='http://127.0.0.1:1234/api/v1/models/download';
 const payload={model:'https://huggingface.co/'+modelId};
 if(quantization)payload.quantization=quantization;
 const key=secretGet('provider:lmstudio');
 const r=await fetch(endpoint,{method:'POST',headers:{'Content-Type':'application/json',...(key?{Authorization:'Bearer '+key}:{})},body:JSON.stringify(payload),signal:AbortSignal.timeout(20000)});
 if(!r.ok)throw Error('LM Studio download HTTP '+r.status+': '+(await r.text()).slice(0,300));
 return r.json();
}
export async function lmDownloadStatus(jobId){
 if(!/^[\w-]+$/.test(jobId))throw Error('Invalid job id');
 const key=secretGet('provider:lmstudio');
 const r=await fetch('http://127.0.0.1:1234/api/v1/models/download/status/'+jobId,{headers:key?{Authorization:'Bearer '+key}:{},signal:AbortSignal.timeout(6500)});
 if(!r.ok)throw Error('LM Studio HTTP '+r.status);return r.json();
}
