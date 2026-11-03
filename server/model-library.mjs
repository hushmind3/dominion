import fs from 'node:fs';
import fsp from 'node:fs/promises';
import path from 'node:path';
import {createHash} from 'node:crypto';
import {once} from 'node:events';
import {DATA} from './config.mjs';

const REPO=/^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/;
const HEX=/^[0-9a-f]{24}$/;
const format=['gguf','mlx'];
const MAX_TREE_PAGES=12;
const quantFrom=name=>{
 const matches=name.toUpperCase().match(/(?:^|[-_.])(IQ[1-4](?:_[A-Z0-9]+)*|Q[1-8](?:_[A-Z0-9]+)*|TQ[12]|F16|BF16|FP16|FP8)(?=[-_.]|$)/g);
 return matches?.at(-1)?.replace(/^[-_.]/,'')||'';
};
const safely=(s)=>typeof s==='string'&&s.length<400&&!s.includes('\\')&&!s.includes('\0')&&!s.startsWith('/')&&s.split('/').every(x=>x&&x!=='.'&&x!=='..');
function groupsFor(files,tags=[],repo=''){
 const ggufs=files.filter(x=>/\.gguf$/i.test(x.path));
 const groups=new Map();
 for(const f of ggufs){
  const stem=f.path.replace(/-\d{5}-of-\d{5}\.gguf$/i,'.gguf');
  const arr=groups.get(stem)||[];arr.push(f);groups.set(stem,arr);
 }
 const options=[...groups].map(([key,list])=>{
  const size=list.reduce((n,x)=>n+x.size,0), quant=quantFrom(key);
  return {id:'gguf:'+key,format:'gguf',label:quant||path.basename(key),quantization:quant,bytes:size,files:list.map(x=>x.path),shards:list.length};
 }).filter(x=>x.bytes>0);
 const isMlx=/mlx/i.test(repo)||tags.some(t=>/^(mlx|library_name:mlx|mlx-lm)$|mlx/i.test(t));
 const weights=files.filter(f=>/\.safetensors$/i.test(f.path)&&!/(optimizer|training|adapter)/i.test(f.path));
 if(isMlx&&weights.length){
  const extras=files.filter(f=>/^(config\.json|generation_config\.json|tokenizer[^/]*|special_tokens_map\.json|vocab\.[^/]+|merges\.txt|preprocessor_config\.json|processor_config\.json|chat_template\.jinja|model\.safetensors\.index\.json|*.ignored)$/i.test(f.path));
  const chosen=[...weights,...extras];
  options.push({id:'mlx:all',format:'mlx',label:'MLX · model snapshot',quantization:quantFrom(repo)||'Repository',bytes:chosen.reduce((n,x)=>n+x.size,0),files:chosen.map(x=>x.path),shards:weights.length});
 }
 return options.sort((a,b)=>a.bytes-b.bytes);
}
const jsonResponse=async(res,source)=>{
 if(!res.ok)throw Error(source+' HTTP '+res.status);
 return res.json();
};
export class ModelLibrary{
 constructor({root=path.join(DATA,'models'),hub='https://huggingface.co',fetcher=globalThis.fetch}={}){
  this.root=root;this.hub=hub.replace(/\/$/,'');this.fetcher=fetcher;this.jobs=new Map();
  fs.mkdirSync(root,{recursive:true,mode:0o700});
 }
 url(repo,route=''){
  if(!REPO.test(repo))throw Error('Invalid Hugging Face repository');
  return this.hub+'/'+route+repo.split('/').map(encodeURIComponent).join('/');
 }
 async catalog({q='',format='',sort='downloads'}={}){
  const u=new URL(this.hub+'/api/models');
  u.searchParams.set('limit','40');
  u.searchParams.set('sort',['downloads','likes','lastModified'].includes(sort)?sort:'downloads');
  u.searchParams.set('direction','-1');
  let search=String(q).trim().slice(0,100);
  if(!search&&format==='gguf')search='GGUF';
  if(!search&&format==='mlx')search='mlx';
  if(search)u.searchParams.set('search',search);
  const rows=await jsonResponse(await this.fetcher(u,{signal:AbortSignal.timeout(15000)}),'Hugging Face');
  return rows.filter(x=>x.id&&REPO.test(x.id)).filter(x=>!format||format==='all'||(format==='gguf' ? /gguf/i.test(x.id+' '+(x.tags||[]).join(' ')) : /mlx/i.test(x.id+' '+(x.tags||[]).join(' '))))
   .map(x=>this._summary(x));
 }
 _summary(x){
  const tags=x.tags||[];
  return {id:x.id,downloads:x.downloads||0,likes:x.likes||0,updatedAt:x.lastModified||'',task:x.pipeline_tag||'',tags:tags.slice(0,22),
   gated:Boolean(x.gated),description:String(x.cardData?.description||'').slice(0,500),
   capabilities:{vision:tags.some(t=>/image-text-to-text|vision|visual-question-answering/i.test(t)),tools:tags.some(t=>/tool-use|function-calling|tool-calling/i.test(t)),reasoning:tags.some(t=>/reasoning|thinking/i.test(t))}};
 }
 async _tree(repo,revision){
  const base=this.url(repo,'api/models/')+'/tree/'+encodeURIComponent(revision)+'?recursive=true&expand=false';
  let current=base,files=[],pages=0;
  while(current&&pages++<MAX_TREE_PAGES){
   const r=await this.fetcher(current,{signal:AbortSignal.timeout(25000)});
   const rows=await jsonResponse(r,'Model files');
   files.push(...rows.filter(x=>x.type==='file'&&safely(x.path)).map(x=>({path:x.path,size:Number(x.size||x.lfs?.size||0)})));
   const link=r.headers?.get('link')||'';
   const next=link.match(/<([^>]+)>;\s*rel="next"/);
   if(!next)break;
   const u=new URL(next[1],this.hub);
   if(u.origin!==new URL(this.hub).origin)break;
   current=u.toString();
  }
  return files;
 }
 async detail(repo){
  if(!REPO.test(repo))throw Error('Invalid Hugging Face repository');
  const r=await this.fetcher(this.url(repo,'api/models/'),{signal:AbortSignal.timeout(15000)});
  const item=await jsonResponse(r,'Model metadata');
  const revision=/^[0-9a-f]{40}$/.test(item.sha||'')?item.sha:'main';
  const files=await this._tree(repo,revision);
  let readme='';
  try{
   const res=await this.fetcher(this.url(repo,'')+'/raw/'+revision+'/README.md',{signal:AbortSignal.timeout(7000)});
   if(res.ok)readme=(await res.text()).slice(0,14000);
  }catch{}
  return {...this._summary(item),revision,architecture:item.config?.model_type||item.cardData?.base_model||'',license:item.cardData?.license||'',readme,options:groupsFor(files,item.tags||[],repo)};
 }
 _key(repo,revision,optionId){return createHash('sha256').update(repo+'|'+revision+'|'+optionId).digest('hex').slice(0,24);}
 _view(job){const {controller,promise,...data}=job;return {...data};}
 jobsList(){return [...this.jobs.values()].map(x=>this._view(x));}
 job(id){const x=this.jobs.get(id);if(!x)throw Error('Download job not found');return this._view(x);}
 _dir(id){if(!HEX.test(id))throw Error('Invalid model ID');return path.join(this.root,id);}
 async installed(){
  const entries=await fsp.readdir(this.root,{withFileTypes:true});
  const list=[];
  for(const entry of entries.filter(x=>x.isDirectory()&&HEX.test(x.name))){
   try{const manifest=JSON.parse(await fsp.readFile(path.join(this._dir(entry.name),'manifest.json'),'utf8'));if(manifest.status==='installed')list.push({...manifest,id:entry.name});}catch{}
  }
  return list.sort((a,b)=>String(b.installedAt).localeCompare(String(a.installedAt)));
 }
 async download({repo,optionId}){
  const detail=await this.detail(repo);
  const option=detail.options.find(x=>x.id===optionId);
  if(!option)throw Error('Select an available GGUF or MLX variant');
  if(detail.gated)throw Error('This repository requires Hugging Face access approval');
  if(!option.files.length||!option.bytes)throw Error('No downloadable weight files');
  const id=this._key(repo,detail.revision,option.id);
  const target=this._dir(id);
  try{
   const manifest=JSON.parse(await fsp.readFile(path.join(target,'manifest.json'),'utf8'));
   if(manifest.status==='installed')return {id,status:'installed',repo,optionId,downloaded:option.bytes,total:option.bytes};
  }catch{}
  let job=this.jobs.get(id);
  if(job&&['downloading','resolving'].includes(job.status))return this._view(job);
  if(job?.promise)await job.promise.catch(()=>{});
  job={id,repo,revision:detail.revision,optionId,option,format:option.format,label:option.label,status:'resolving',downloaded:0,total:option.bytes,current:'',error:'',createdAt:new Date().toISOString()};
  this.jobs.set(id,job);
  job.controller=new AbortController();
  job.promise=this._run(job);
  return this._view(job);
 }
 async _run(job){
  try{
   const target=this._dir(job.id);
   await fsp.mkdir(target,{recursive:true,mode:0o700});
   const files=job.option.files;
   const sizes=await this._tree(job.repo,job.revision);
   const known=new Map(sizes.map(x=>[x.path,x.size]));
   job.total=files.reduce((sum,x)=>sum+(known.get(x)||0),0);
   job.downloaded=0;
   for(const name of files){
    if(!safely(name))throw Error('Invalid repository file path');
    const targetFile=path.resolve(target,name);
    if(!targetFile.startsWith(target+path.sep))throw Error('Model path escaped the library');
    const expected=known.get(name)||0;
    let complete=0;
    try{const stat=await fsp.stat(targetFile);if(expected&&stat.size===expected)complete=expected;}catch{}
    job.downloaded+=complete;
   }
   job.status='downloading';
   for(const name of files){
    if(job.controller.signal.aborted)throw Error('Download interrupted');
    job.current=name;
    const expected=known.get(name)||0;
    const dest=path.resolve(target,name);
    await fsp.mkdir(path.dirname(dest),{recursive:true,mode:0o700});
    try{const stat=await fsp.stat(dest);if(expected&&stat.size===expected)continue;}catch{}
    await this._downloadFile(job,name,dest,expected);
   }
   const manifest={status:'installed',repo:job.repo,revision:job.revision,optionId:job.optionId,format:job.format,label:job.label,files,bytes:job.total,installedAt:new Date().toISOString()};
   await fsp.writeFile(path.join(target,'manifest.json'),JSON.stringify(manifest,null,2));
   job.status='completed';job.downloaded=job.total;job.current='';
  }catch(e){
   if(!['paused','cancelled'].includes(job.status)){job.status='failed';job.error=String(e.message||e);}
  }
 }
 async _downloadFile(job,name,dest,expected){
  const partial=dest+'.part';
  let offset=0;try{offset=(await fsp.stat(partial)).size;}catch{}
  if(expected&&offset>expected){await fsp.rm(partial,{force:true});offset=0;}
  const headers=offset?{Range:'bytes='+offset+'-'}:{};
  const u=this.url(job.repo,'')+'/resolve/'+job.revision+'/'+name.split('/').map(encodeURIComponent).join('/');
  const response=await this.fetcher(u,{headers,signal:job.controller.signal,redirect:'follow'});
  if(!response.ok||(![200,206].includes(response.status)))throw Error('Download HTTP '+response.status+' ('+name+')');
  if(offset&&response.status!==206){offset=0;await fsp.rm(partial,{force:true});}
  const source=response.body;
  if(!source)throw Error('Empty download stream');
  const writer=fs.createWriteStream(partial,{flags:offset?'a':'w'});
  let written=offset;
  try{
   for await(const chunk of source){
    if(job.controller.signal.aborted)throw Error('Download interrupted');
    const buf=Buffer.from(chunk);
    if(!writer.write(buf))await once(writer,'drain');
    written+=buf.length;
    job.downloaded+=buf.length;
   }
   await new Promise((resolve,reject)=>writer.end(err=>err?reject(err):resolve()));
  }catch(e){writer.destroy();throw e;}
  if(expected&&written!==expected)throw Error('Incomplete file '+name+': '+written+'/'+expected);
  await fsp.rename(partial,dest);
 }
 async control(id,action){
  const job=this.jobs.get(id);
  if(!job)throw Error('Download job not found');
  if(action==='pause'||action==='cancel'){
   if(['downloading','resolving'].includes(job.status)){
    job.status=action==='pause'?'paused':'cancelled';
    job.controller.abort();
   }
   return this._view(job);
  }
  if(action==='resume'){
   if(!['paused','failed','cancelled'].includes(job.status))return this._view(job);
   if(job.promise)await job.promise.catch(()=>{});
   job.status='resolving';job.error='';job.controller=new AbortController();job.promise=this._run(job);
   return this._view(job);
  }
  throw Error('Unknown download action');
 }
 async remove(id){
  if(!HEX.test(id))throw Error('Invalid model ID');
  const dir=this._dir(id);
  try{await fsp.access(path.join(dir,'manifest.json'));}catch{throw Error('Model not installed');}
  await fsp.rm(dir,{recursive:true,force:true});return {ok:true};
 }
 async find(id){
  const found=(await this.installed()).find(x=>x.id===id);
  if(!found)throw Error('Installed model not found');
  return {...found,folder:this._dir(id)};
 }
}
export const modelLibrary=new ModelLibrary();
