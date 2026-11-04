// First-party dynamic feeds into the generic DOMINION capability kernel.
// These feeds expose resources already owned by the engine, not copied placeholders.
import {kernel} from './capability-kernel.mjs';
import './channel-registry.mjs'; // Load on-disk channel capabilities before building the aggregate inventory.
import {providers,availableModels} from './providers.mjs';
import {mcpServers,discoverMcp} from './mcp.mjs';
import {all,put,get} from './store.mjs';
import {builtin} from './tools.mjs';
import {oauthProviders,oauthCallbackBase,connections as oauthConnections} from './oauth.mjs';

const cleanId=v=>String(v).replace(/[^a-zA-Z0-9_-]/g,'_').toLowerCase().slice(0,65);
kernel.feed('model',{
 list:()=>providers().map(x=>({
  id:cleanId(x.id),sourceId:x.id,name:x.name,description:x.description||x.baseUrl,
  status:x.enabled?'configured':'disabled',available:Boolean(x.enabled),
  features:[x.local?'local':'cloud',x.kind],fields:[],origin:'provider'
 })),
 execute:(id,op)=>{
  if(op!=='models')throw Error('Unsupported model provider operation');
  const original=providers().find(x=>cleanId(x.id)===id);if(!original)throw Error('Provider not found');
  return availableModels(original.id);
 }
});
kernel.feed('mcp',{
 list:()=>mcpServers().map(x=>({
  id:cleanId(x.id),sourceId:x.id,name:x.name,description:x.transport==='http'?x.url:x.command,
  status:x.enabled?'configured':'disabled',available:Boolean(x.enabled),
  features:[x.transport],fields:[],origin:'mcp'
 })),
 execute:async(id,op)=>{
  if(op!=='tools')throw Error('Unsupported MCP operation');
  const actual=mcpServers().find(x=>cleanId(x.id)===id);if(!actual)throw Error('MCP not found');
  return (await discoverMcp()).filter(x=>x.mcpId===actual.id);
 }
});
kernel.feed('skill',{
 list:()=>all('skill').map(x=>({
  id:cleanId(x.id),sourceId:x.id,name:x.name,description:String(x.instructions||'').slice(0,120),
  status:x.enabled?'enabled':'disabled',available:Boolean(x.enabled),features:['knowledge'],fields:[],origin:'skill'
 })),
 execute:(id,op,input)=>{
  if(op!=='activate'&&op!=='deactivate')throw Error('Unsupported skill operation');
  const entry=all('skill').find(x=>cleanId(x.id)===id);if(!entry)throw Error('Skill not found');
  return put('skill',{...entry,enabled:op==='activate'});
 }
});
kernel.feed('account',{
 list:()=>{const accounts=oauthConnections();return oauthProviders().map(x=>({
  id:x.id,name:x.id==='github'?'GitHub':'Google',description:'OAuth connector',
  status:x.configured?'configured':'requires_setup',available:Boolean(x.configured),connected:accounts.some(account=>account.provider===x.id),
  features:['oauth'],fields:[],origin:'oauth',callback:oauthCallbackBase()+'/api/oauth/'+x.id+'/callback'
 }))}
});
kernel.feed('tool',{
 list:()=>builtin.map(x=>({
  id:cleanId(x.name),name:x.name,description:x.description||'',status:'ready',available:true,
  features:['agent_tool'],fields:[],origin:'builtin'
 }))
});
export const capabilityInventory=(family)=>{
 const rows=kernel.list(family);
 // Channel drivers report a verification stamp only after a real successful connection.
 for(const row of rows)if(row.namespace==='channel')row.verifiedAt=get('driver_health',row.id)?.lastConnectedAt||'';
 return rows;
};
export const capabilityExecute=(family,id,operation,input)=>kernel.invoke(family,id,operation,input);
