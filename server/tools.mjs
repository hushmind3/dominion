import fs from 'node:fs/promises';
import path from 'node:path';
import {spawn} from 'node:child_process';
import {WORKSPACE} from './config.mjs';
import {all,get,put,log,bus} from './store.mjs';
import {discoverMcp,callMcp} from './mcp.mjs';
const params=(properties,required=[])=>({type:'object',properties,required,additionalProperties:false});
export const builtin=[
 {name:'workspace_list',description:'List files and folders in the workspace',parameters:params({path:{type:'string',description:'Relative folder path'}})},
 {name:'workspace_read',description:'Read a UTF-8 file from the workspace',parameters:params({path:{type:'string'}},['path'])},
 {name:'workspace_write',description:'Write a UTF-8 file into the workspace',parameters:params({path:{type:'string'},content:{type:'string'}},['path','content']),approval:true},
 {name:'terminal_exec',description:'Run an executable with argument array in the workspace, without a shell',parameters:params({command:{type:'string'},args:{type:'array',items:{type:'string'}}},['command']),approval:true},
 {name:'web_search',description:'Search the web using your configured SearXNG instance',parameters:params({query:{type:'string'}},['query'])},
 {name:'memory_store',description:'Save a concise durable fact for future sessions',parameters:params({text:{type:'string'},scope:{type:'string'}},['text'])},
 {name:'memory_search',description:'Find saved knowledge relevant to a topic',parameters:params({query:{type:'string'}},['query'])},
 {name:'skill_propose',description:'Draft a reusable skill for user review and installation',parameters:params({name:{type:'string'},instructions:{type:'string'}},['name','instructions'])}
];
const pending=new Map();
export const approvals=()=>[...pending.values()].map(x=>({id:x.id,tool:x.tool,args:x.args,at:x.at}));
export function resolveApproval(id,allow){const p=pending.get(id);if(!p)return false;pending.delete(id);p.resolve(Boolean(allow));bus.emit('approval',{id,allowed:Boolean(allow)});return true;}
async function allowed(tool,args,onEvent){
 const policy=get('policy','default')||{};
 if((policy.autoApprove||[]).includes(tool.name)||policy.autoApproveAll===true)return true;
 const id=crypto.randomUUID();
 const accepted=await new Promise(resolve=>{
  const timeout=setTimeout(()=>{pending.delete(id);resolve(false);},120000);timeout.unref?.();
  pending.set(id,{id,tool:tool.name,args,at:Date.now(),resolve:v=>{clearTimeout(timeout);resolve(v);}});
  onEvent?.({type:'approval',id,tool:tool.name,args});
  bus.emit('approval',{id,tool:tool.name,args});
 });
 return accepted;
}
async function workspacePath(p='',existing=false){
 const dest=path.resolve(WORKSPACE,String(p));
 if(dest!==WORKSPACE&&!dest.startsWith(WORKSPACE+path.sep))throw Error('Outside workspace');
 // Reject symlink escapes for existing ancestors as well as the destination.
 let current=dest;
 while(current!==WORKSPACE&&current!==path.dirname(current)){
  try{const stat=await fs.lstat(current);if(stat.isSymbolicLink())throw Error('Symlinks prohibited in workspace tools');}catch(e){if(e.code!=='ENOENT')throw e;}
  current=path.dirname(current);
 }
 if(existing)await fs.access(dest);return dest;
}
async function exec(command,args=[]){
 if(typeof command!=='string'||!/^[\w./:-]+$/.test(command))throw Error('Invalid executable');
 if(!Array.isArray(args)||args.length>40||args.some(x=>typeof x!=='string'))throw Error('Invalid arguments');
 return new Promise((resolve,reject)=>{
  const child=spawn(command,args,{cwd:WORKSPACE,shell:false,windowsHide:true,env:process.env});
  let stdout='',stderr='';const timer=setTimeout(()=>child.kill(),60000);
  child.stdout.on('data',x=>stdout=(stdout+x).slice(-30000));
  child.stderr.on('data',x=>stderr=(stderr+x).slice(-10000));
  child.on('error',e=>{clearTimeout(timer);reject(e)});
  child.on('close',(code)=>{clearTimeout(timer);resolve({exitCode:code,stdout,stderr});});
 });
}
export async function toolsForModel(enabled=true){
 if(!enabled)return[];
 const external=await discoverMcp();
 return [...builtin,...external.filter(x=>x.parameters&&!x.error)];
}
export async function executeTool(tool,args,onEvent=()=>{}){
 if(tool.approval&&!(await allowed(tool,args,onEvent)))throw Error('Tool execution denied or expired');
 if(tool.mcpId){
  // External MCP calls may cause side effects; first-time use requires permission.
  const policy=get('policy','default')||{};
  if(!policy.autoApproveMcp && !(await allowed({...tool,approval:true},args,onEvent)))throw Error('MCP tool denied');
  return callMcp(tool,args);
 }
 let result;
 switch(tool.name){
  case 'workspace_list':{
   const folder=await workspacePath(args.path||'',true);
   result=(await fs.readdir(folder,{withFileTypes:true})).slice(0,300).map(x=>({name:x.name,type:x.isDirectory()?'directory':x.isFile()?'file':'other'}));break;
  }
  case 'workspace_read':{
   const file=await workspacePath(args.path,true);
   if((await fs.stat(file)).size>1048576)throw Error('File exceeds 1 MB read limit');
   result=await fs.readFile(file,'utf8');break;
  }
  case 'workspace_write':{
   const file=await workspacePath(args.path);if(file===WORKSPACE)throw Error('Choose a file');
   await fs.mkdir(path.dirname(file),{recursive:true});await fs.writeFile(file,String(args.content||''),'utf8');
   result={written:path.relative(WORKSPACE,file)};break;
  }
  case 'terminal_exec':result=await exec(args.command,args.args||[]);break;
  case 'web_search':{
   const endpoint=process.env.SEARXNG_URL;if(!endpoint)throw Error('Set SEARXNG_URL to enable web search');
   const u=new URL('/search',endpoint);u.searchParams.set('q',String(args.query));u.searchParams.set('format','json');
   const r=await fetch(u,{signal:AbortSignal.timeout(20000)});if(!r.ok)throw Error('Search HTTP '+r.status);
   result=(await r.json()).results?.slice(0,10).map(x=>({title:x.title,url:x.url,content:x.content}));break;
  }
  case 'memory_store':result=put('memory',{text:String(args.text).slice(0,3000),scope:String(args.scope||'shared')});break;
  case 'memory_search':{
   const terms=String(args.query).toLowerCase().split(/\s+/).filter(Boolean);
   result=all('memory').map(x=>({...x,score:terms.filter(t=>x.text.toLowerCase().includes(t)).length})).filter(x=>x.score).sort((a,b)=>b.score-a.score).slice(0,8);break;
  }
  case 'skill_propose':result=put('skill',{name:String(args.name).slice(0,80),instructions:String(args.instructions).slice(0,12000),enabled:false,status:'proposed'});break;
  default:throw Error('Tool not found: '+tool.name);
 }
 log('tool.executed',{name:tool.name,args:tool.name==='memory_store'?{scope:args.scope}:args,resultPreview:JSON.stringify(result).slice(0,250)});
 return result;
}
