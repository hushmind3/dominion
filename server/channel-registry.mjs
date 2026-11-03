import fs from 'node:fs';
import path from 'node:path';
import {pathToFileURL,fileURLToPath} from 'node:url';
import {DATA} from './config.mjs';

const here=path.dirname(fileURLToPath(import.meta.url));
const roots=[path.join(here,'channel-plugins'),path.join(DATA,'channel-plugins')];
const catalog=new Map(),drivers=new Map(),errors=new Map();
const valid=/^[a-z0-9][a-z0-9_-]{1,50}$/;
const fieldKey=/^[a-zA-Z][a-zA-Z0-9_]{0,63}$/;
const fieldTypes=new Set(['secret','text','url']);
const modes=new Set(['polling','gateway','socket','webhook','external']);

function validateManifest(raw,folder){
 if(!raw || typeof raw!=='object' || !valid.test(raw.id)||raw.id!==path.basename(folder))throw Error('Invalid channel ID or folder');
 if(typeof raw.name!=='string'||!raw.name.trim())throw Error('Channel name required');
 if(!modes.has(raw.mode))throw Error('Invalid channel transport');
 if(!Array.isArray(raw.fields)||!Array.isArray(raw.features))throw Error('Channel fields/features must be arrays');
 if(raw.fields.some(f=>!fieldKey.test(f.key)||!fieldTypes.has(f.type)||typeof f.label!=='string'))throw Error('Invalid configuration field');
 if(!/^https:\/\//.test(raw.docs||''))throw Error('Each channel manifest must reference HTTPS documentation');
 return {...raw,status:'planned',available:false};
}
export function registerChannel(manifest,driver){
 if(!catalog.has(manifest.id))throw Error('Channel manifest missing before driver registration: '+manifest.id);
 if(!driver||typeof driver.start!=='function')throw Error('Channel driver must provide start(ctx)');
 if(drivers.has(manifest.id))throw Error('Duplicate channel driver: '+manifest.id);
 drivers.set(manifest.id,driver);
}
export function registerChannelDelivery(id,fn){
 if(typeof fn!=='function')throw Error('Delivery handler must be a function');
 const d=drivers.get(id);if(!d)throw Error('Channel not registered: '+id);d.deliver=fn;
}
export function registerChannelMcpTools(id,tools){
 const d=drivers.get(id);if(!d)throw Error('Channel not registered: '+id);d.mcpTools=tools;
}
export function registerChannelRuntimeContext(id,context){
 const d=drivers.get(id);if(!d)throw Error('Channel not registered: '+id);d.context=context;
}
async function load(root){
 if(!fs.existsSync(root))return;
 for(const name of fs.readdirSync(root).sort()){
  const dir=path.join(root,name);
  if(!fs.statSync(dir).isDirectory())continue;
  const file=path.join(dir,'manifest.json');
  if(!fs.existsSync(file))continue;
  try{
   const manifest=validateManifest(JSON.parse(fs.readFileSync(file,'utf8')),dir);
   if(catalog.has(manifest.id))throw Error('Duplicate channel manifest');
   catalog.set(manifest.id,manifest);
   const entry=path.join(dir,'index.mjs');
   if(!fs.existsSync(entry))continue;
   const plugin=await import(pathToFileURL(entry).href);
   if(typeof plugin.default!=='function')throw Error('Channel plugin must export default register(api)');
   const api={
    registerChannel:driver=>registerChannel(manifest,driver),
    registerChannelDelivery:fn=>registerChannelDelivery(manifest.id,fn),
    registerChannelMcpTools:tools=>registerChannelMcpTools(manifest.id,tools),
    registerChannelRuntimeContext:context=>registerChannelRuntimeContext(manifest.id,context)
   };
   await plugin.default(api);
   if(!drivers.has(manifest.id))throw Error('Channel entry did not register a driver');
   catalog.set(manifest.id,{...manifest,status:'driver',available:true});
  }catch(e){
   drivers.delete(name);
   errors.set(name,String(e?.message||e).slice(0,300));
  }
 }
}
for(const root of roots)await load(root);
export const listChannels=()=>[...catalog.values()].map(x=>({...x,fields:x.fields.map(f=>({...f})),features:[...x.features]}));
export const channelManifest=id=>catalog.get(id)||null;
export const channelDriver=id=>drivers.get(id)||null;
export const registryErrors=()=>Object.fromEntries(errors);
export function validateChannel(kind,record,credentials){
 const manifest=channelManifest(kind);
 if(!manifest)throw Error('Unknown channel type: '+kind);
 if(!channelDriver(kind))throw Error('Driver not installed: '+kind);
 const missing=manifest.fields.filter(f=>f.required&&!(f.type==='secret'?credentials[f.key]:record[f.key])).map(f=>f.label);
 if(missing.length)throw Error('Missing configuration: '+missing.join(', '));
 for(const f of manifest.fields.filter(x=>x.type==='url'&&record[x.key])){
  const u=new URL(record[f.key]);if(!['http:','https:'].includes(u.protocol))throw Error('Invalid '+f.label+' URL');
 }
 return true;
}
