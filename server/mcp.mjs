import {all,get,put,remove,log} from './store.mjs';
import {secretGet,secretSet} from './vault.mjs';
const sessions=new Map();
const label=x=>String(x).replace(/[^a-zA-Z0-9_]/g,'_').slice(0,35);
export const mcpServers=()=>all('mcp').map(x=>({...x,hasToken:Boolean(secretGet('mcp:'+x.id))}));
export function saveMcp(input){
 if(!input.name)throw Error('Server name required');
 if(!['http','stdio'].includes(input.transport))throw Error('Invalid MCP transport');
 if(input.transport==='http'&&!/^https?:\/\//.test(input.url||''))throw Error('MCP URL required');
 if(input.transport==='stdio'&&!input.command)throw Error('MCP executable required');
 const id=input.id||crypto.randomUUID();
 const v=put('mcp',{id,name:input.name,transport:input.transport,url:input.url||'',command:input.command||'',args:Array.isArray(input.args)?input.args:[],env:input.env||{},enabled:input.enabled!==false});
 if(typeof input.token==='string')secretSet('mcp:'+id,input.token);
 sessions.delete(id);log('mcp.saved',{id});return v;
}
export async function disconnectMcp(id){const old=sessions.get(id);sessions.delete(id);if(old)await old.close().catch(()=>{});}
export async function deleteMcp(id){await disconnectMcp(id);remove('mcp',id);}
async function connect(record){
 const cached=sessions.get(record.id);if(cached)return cached;
 const {Client,StreamableHTTPClientTransport}=await import('@modelcontextprotocol/client');
 const client=new Client({name:'dominion',version:'0.1.0'});
 if(record.transport==='http'){
   const token=secretGet('mcp:'+record.id);
   await client.connect(new StreamableHTTPClientTransport(new URL(record.url),token?{requestInit:{headers:{Authorization:'Bearer '+token}}}:{}));
 }else{
   const {StdioClientTransport}=await import('@modelcontextprotocol/client/stdio');
   await client.connect(new StdioClientTransport({command:record.command,args:record.args||[],env:{...process.env,...record.env}}));
 }
 sessions.set(record.id,client);return client;
}
export async function discoverMcp(){
 const result=[];
 for(const r of all('mcp').filter(x=>x.enabled)){
  try{const client=await connect(r);const found=await client.listTools();for(const t of found.tools||[])result.push({name:'mcp__'+label(r.id)+'__'+label(t.name),description:'['+r.name+'] '+(t.description||t.name),parameters:t.inputSchema||{type:'object',properties:{}},mcpId:r.id,originalName:t.name});}
  catch(e){result.push({error:String(e),mcpId:r.id,name:r.name});await disconnectMcp(r.id);}
 }
 return result;
}
export async function callMcp(tool,args){
 const r=get('mcp',tool.mcpId);if(!r||!r.enabled)throw Error('MCP unavailable');
 const client=await connect(r);const result=await client.callTool({name:tool.originalName,arguments:args});
 return result;
}
