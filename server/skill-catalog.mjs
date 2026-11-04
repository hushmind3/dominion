import fs from 'node:fs';
import fsp from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import crypto from 'node:crypto';
import {spawnSync} from 'node:child_process';
import {all,put} from './store.mjs';
import {DATA,WORKSPACE} from './config.mjs';

const cache={at:0,items:[],paths:new Map()};
const binCache=new Map();
const safeReaddir=dir=>{try{return fs.readdirSync(dir,{withFileTypes:true})}catch{return[]}};
const exists=file=>{try{return fs.existsSync(file)}catch{return false}};
const idFor=file=>crypto.createHash('sha1').update(file).digest('hex').slice(0,16);

function walkSkillFiles(dir,depth=0,found=[]){
 if(depth>5)return found;
 for(const entry of safeReaddir(dir)){
  if(entry.name==='node_modules'||entry.name==='.git')continue;
  const full=path.join(dir,entry.name);
  if(entry.isSymbolicLink()){
   try{
    const real=fs.realpathSync(full);
    if(fs.statSync(real).isDirectory())walkSkillFiles(real,depth+1,found);
    else if(entry.name==='SKILL.md')found.push(real);
   }catch{}
   continue;
  }
  if(entry.isDirectory())walkSkillFiles(full,depth+1,found);
  else if(entry.name==='SKILL.md')found.push(full);
 }
 return found;
}

function sourceRoots(){
 const local=process.env.LOCALAPPDATA||path.join(os.homedir(),'AppData','Local');
 const roots=[];
 for(const [dir,label] of [[path.join(DATA,'skills'),'DOMINION'], [path.join(WORKSPACE,'skills'),'워크스페이스'], [path.join(os.homedir(),'.dominion','skills'),'DOMINION 사용자']]){
  if(exists(dir))roots.push({dir,source:'DOMINION',label});
 }
 const openclawBase=path.join(local,'OpenClawRuntime');
 for(const entry of safeReaddir(openclawBase)){
  if(!/^node-v/i.test(entry.name))continue;
  for(const folder of ['skills','custodian-skills']){
   const dir=path.join(openclawBase,entry.name,'node_modules','openclaw',folder);
   if(exists(dir))roots.push({dir,source:'OpenClaw',label:folder});
  }
 }
 const pluginSkills=path.join(os.homedir(),'.openclaw','plugin-skills');
 if(exists(pluginSkills))roots.push({dir:pluginSkills,source:'OpenClaw plugin',label:'plugin-skills'});
 for(const [folder,label] of [['skills','Hermes'],['optional-skills','Hermes optional']]){
  const dir=path.join(local,'hermes','hermes-agent',folder);
  if(exists(dir))roots.push({dir,source:label,label:folder});
 }
 return roots;
}

function parseFrontmatter(text){
 const normalized=String(text||'').replace(/^\uFEFF/,'').replace(/\r\n/g,'\n');
 if(!normalized.startsWith('---\n'))return {meta:{},body:normalized};
 const end=normalized.indexOf('\n---',4);
 if(end<0)return {meta:{},body:normalized};
 const raw=normalized.slice(4,end);
 const meta={};
 for(const line of raw.split('\n')){
  const match=line.match(/^([a-zA-Z0-9_-]+):\s*(.*)$/);
  if(match)meta[match[1]]=match[2].replace(/^['"]|['"]$/g,'');
 }
 const body=normalized.slice(end+4).replace(/^\n+/,'');
 return {meta,body,raw};
}

function extractArray(raw,key){
 const match=String(raw||'').match(new RegExp('[\\x22\\x27]?'+key+'[\\x22\\x27]?\\s*:\\s*\\[([^\\]]*)\\]','i'));
 if(!match)return [];
 return match[1].split(',').map(value=>value.trim().replace(/^['"]|['"]$/g,'')).filter(Boolean);
}

function commandAvailable(bin){
 if(binCache.has(bin))return binCache.get(bin);
 const command=process.platform==='win32'?'where':'which';
 const check=spawnSync(command,[bin],{stdio:'ignore',windowsHide:true});
 const ok=check.status===0;
 binCache.set(bin,ok);
 return ok;
}

function summarize(text,max=240){
 const clean=String(text||'').replace(/```[\s\S]*?```/g,' ').replace(/[#>*_`]/g,' ').replace(/\s+/g,' ').trim();
 return clean.slice(0,max);
}

function skillFromFile(file,source,folder){
 const text=fs.readFileSync(file,'utf8');
 const {meta,body,raw}=parseFrontmatter(text);
 const relative=path.relative(folder,file);
 const parentParts=path.dirname(relative).split(path.sep).filter(part=>part&&part!=='.');
 const parent=parentParts[0]||source;
 const category=(raw||'').match(/category:\s*([^\n,]+)/i)?.[1]?.trim().replace(/^['"]|['"]$/g,'')||parent;
 const bins=extractArray(raw,'bins');
 const tags=extractArray(raw,'tags');
 const available=bins.every(commandAvailable);
 const name=(meta.name||path.basename(path.dirname(file))).trim();
 const description=(meta.description||summarize(body)||name).trim();
 return {
  id:idFor(file),
  name,
  description:description.slice(0,300),
  category:category.slice(0,80),
  source,
  tags:tags.slice(0,8),
  requires:bins,
  available,
  path:file,
  relative
 };
}

export function skillCatalog({q='',category='',source='',limit=500}={}){
 const now=Date.now();
 if(now-cache.at>5000){
  const items=[],paths=new Map(),seen=new Set();
  for(const root of sourceRoots()){
   for(const file of walkSkillFiles(root.dir)){
    let real=file;
    try{real=fs.realpathSync(file)}catch{}
    if(seen.has(real))continue;
    seen.add(real);
    try{
     const item=skillFromFile(real,root.source,root.dir);
     items.push(item);paths.set(item.id,real);
    }catch{}
   }
  }
  items.sort((a,b)=>a.source.localeCompare(b.source)||a.category.localeCompare(b.category)||a.name.localeCompare(b.name));
  cache.items=items;cache.paths=paths;cache.at=now;
 }
 const query=String(q||'').trim().toLowerCase();
 let items=cache.items;
 if(query)items=items.filter(item=>[item.name,item.description,item.category,item.source,...(item.tags||[])].join(' ').toLowerCase().includes(query));
 if(category)items=items.filter(item=>item.category===category);
 if(source)items=items.filter(item=>item.source===source);
 return {items:items.slice(0,Math.max(1,Math.min(Number(limit)||500,1000))),total:items.length,updatedAt:new Date(cache.at).toISOString(),roots:sourceRoots().map(root=>({source:root.source,path:root.dir}))};
}

export function skillCatalogMeta(){
 const catalog=skillCatalog({limit:1000});
 return {total:catalog.total,categories:[...new Set(catalog.items.map(item=>item.category))].sort(),sources:[...new Set(catalog.items.map(item=>item.source))].sort(),unavailable:catalog.items.filter(item=>!item.available).length,updatedAt:catalog.updatedAt};
}

export async function importCatalogSkill(id){
 let file=cache.paths.get(String(id||''));
 if(!file){
  skillCatalog({limit:1000});
  file=cache.paths.get(String(id||''));
 }
 if(!file||!exists(file))throw Error('Skill catalog entry not found');
 const text=await fsp.readFile(file,'utf8');
 const {meta,body}=parseFrontmatter(text);
 const name=String(meta.name||path.basename(path.dirname(file))).trim().slice(0,80);
 const instructions=String(body||'').trim().slice(0,50000);
 if(!name||!instructions)throw Error('Skill file has no usable instructions');
 const existing=all('skill').find(skill=>skill.name===name);
 const record=put('skill',{...(existing||{}),id:existing?.id||crypto.randomUUID(),name,instructions,enabled:true,status:'installed',source:'catalog',sourcePath:file,importedAt:new Date().toISOString()});
 return {skill:record,file};
}
