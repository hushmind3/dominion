import fs from 'node:fs';
import fsp from 'node:fs/promises';
import path from 'node:path';
import crypto from 'node:crypto';
import {spawn} from 'node:child_process';
import {Readable} from 'node:stream';
import {pipeline} from 'node:stream/promises';
import {WORKSPACE} from './config.mjs';
import {all,get,put,log} from './store.mjs';

const jobs=new Map();
const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));

export function videoConfig(){
 const stored=get('video_config','default')||{};
 return {
  comfyUrl:String(stored.comfyUrl||'http://127.0.0.1:8188').replace(/\/$/,''),
  comfyDir:path.resolve(String(stored.comfyDir||'C:\\ComfyUI')),
  workflowDir:path.resolve(String(stored.workflowDir||path.join(WORKSPACE,'video-workflows'))),
  outputDir:path.resolve(String(stored.outputDir||path.join(WORKSPACE,'videos')))
 };
}
function ensureDirs(config){
 fs.mkdirSync(config.workflowDir,{recursive:true});
 fs.mkdirSync(config.outputDir,{recursive:true});
}
export function videoWorkflows(){
 const config=videoConfig();ensureDirs(config);
 return fs.readdirSync(config.workflowDir,{withFileTypes:true}).filter(entry=>entry.isFile()&&/\.json$/i.test(entry.name)).map(entry=>{
  const file=path.join(config.workflowDir,entry.name);
  return {id:entry.name,name:entry.name.replace(/\.json$/i,''),bytes:fs.statSync(file).size,path:file};
 }).sort((a,b)=>a.name.localeCompare(b.name));
}
export async function videoStatus(){
 const config=videoConfig();ensureDirs(config);
 let comfy={reachable:false,error:''};
 try{
  const response=await fetch(config.comfyUrl+'/system_stats',{signal:AbortSignal.timeout(3000)});
  if(response.ok){comfy={reachable:true,data:await response.json(),error:''}}
  else comfy.error='HTTP '+response.status;
 }catch(error){comfy.error=String(error?.message||error)}
 return {config,comfy,installed:fs.existsSync(path.join(config.comfyDir,'main.py'))&&fs.existsSync(path.join(config.comfyDir,'.venv','Scripts','python.exe')),workflows:videoWorkflows(),jobs:videoJobs().slice(-12)};
}
export async function startComfyUI(){
 const config=videoConfig();
 try{const response=await fetch(config.comfyUrl+'/system_stats',{signal:AbortSignal.timeout(1500)});if(response.ok)return {started:false,already:true}}catch{}
 const python=path.join(config.comfyDir,'.venv','Scripts','python.exe');
 const main=path.join(config.comfyDir,'main.py');
 if(!fs.existsSync(python)||!fs.existsSync(main))throw Error('ComfyUI가 '+config.comfyDir+'에 설치되어 있지 않습니다.');
 const port=new URL(config.comfyUrl).port||'8188';
 const logFile=path.join(config.comfyDir,'comfyui.log');
 const log=fs.openSync(logFile,'a');
 const child=spawn(python,[main,'--listen','127.0.0.1','--port',port,'--lowvram','--preview-method','none'],{cwd:config.comfyDir,detached:true,windowsHide:true,stdio:['ignore',log,log]});
 child.unref();
 log('video.comfy_started',{pid:child.pid,port});
 return {started:true,pid:child.pid,port};
}
export function saveVideoConfig(input={}){
 const current=videoConfig();
 const next={...current,...input};
 if(input.comfyDir)next.comfyDir=path.resolve(String(input.comfyDir));
 if(input.workflowDir)next.workflowDir=path.resolve(String(input.workflowDir));
 if(input.outputDir)next.outputDir=path.resolve(String(input.outputDir));
 if(input.comfyUrl){
  const parsed=new URL(String(input.comfyUrl));
  if(!['http:','https:'].includes(parsed.protocol))throw Error('ComfyUI URL must be HTTP(S)');
  next.comfyUrl=parsed.toString().replace(/\/$/,'');
 }
 ensureDirs(next);
 put('video_config',{id:'default',...next});
 log('video.config_saved',{comfyUrl:next.comfyUrl,workflowDir:next.workflowDir});
 return videoStatus();
}
function replaceTokens(value,tokens){
 if(typeof value==='string'){
  const exact=value.match(/^\{\{\s*(width|height|frames|fps|seed)\s*\}\}$/i);
  if(exact)return tokens[exact[1].toLowerCase()];
  return value.replace(/\{\{\s*(prompt|negative|width|height|frames|fps|seed)\s*\}\}/gi,(_match,key)=>String(tokens[key.toLowerCase()]??''));
 }
 if(Array.isArray(value))return value.map(item=>replaceTokens(item,tokens));
 if(value&&typeof value==='object')return Object.fromEntries(Object.entries(value).map(([key,item])=>[key,replaceTokens(item,tokens)]));
 return value;
}
function hasPromptToken(value){
 if(typeof value==='string')return /\{\{\s*prompt\s*\}\}/i.test(value);
 if(Array.isArray(value))return value.some(hasPromptToken);
 if(value&&typeof value==='object')return Object.values(value).some(hasPromptToken);
 return false;
}
function findOutput(history){
 const outputs=history?.outputs||{};
 for(const node of Object.values(outputs)){
  for(const key of ['videos','gifs','images']){
   const file=(node?.[key]||[]).find(item=>/\.(mp4|webm|mov|gif|webp)$/i.test(item.filename||''));
   if(file)return {filename:file.filename,subfolder:file.subfolder||'',type:file.type||'output'};
  }
 }
 return null;
}
function workflowFile(name){
 const config=videoConfig();ensureDirs(config);
 const file=path.resolve(config.workflowDir,String(name||''));
 if(!file.startsWith(config.workflowDir+path.sep)||!fs.existsSync(file))throw Error('Workflow file not found');
 return file;
}
export async function startVideoJob(input={}){
 const config=videoConfig();ensureDirs(config);
 const file=workflowFile(input.workflow);
 const raw=JSON.parse(await fsp.readFile(file,'utf8'));
 if(!hasPromptToken(raw))throw Error('이 워크플로에는 {{prompt}} 자리표시자가 없습니다. ComfyUI에서 API 형식으로 저장한 JSON에 {{prompt}}, {{negative}}, {{width}}, {{height}}, {{frames}}, {{fps}}, {{seed}}를 넣어주세요.');
 const width=Number(input.width)||720,height=Number(input.height)||1280;
 const fps=Math.max(1,Math.min(60,Number(input.fps)||16));
 const seconds=Math.max(1,Math.min(30,Number(input.seconds)||4));
 const requestedFrames=Number(input.frames)||Math.round(fps*seconds);
 const frames=Math.max(1,Math.min(600,input.frames?requestedFrames:1+Math.max(0,Math.round((requestedFrames-1)/4))*4));
 const seed=Number.isFinite(Number(input.seed))&&Number(input.seed)>=0?Math.floor(Number(input.seed)):Math.floor(Math.random()*1e9);
 const workflow=replaceTokens(raw,{prompt:String(input.prompt||'').trim(),negative:String(input.negative||'').trim(),width,height,frames,fps,seed});
 const job={id:crypto.randomUUID(),status:'queued',progress:0,workflow:path.basename(file),prompt:String(input.prompt||'').slice(0,300),width,height,frames,fps,seconds,seed,createdAt:new Date().toISOString(),error:'',result:null};
 jobs.set(job.id,job);put('video_job',job);
 const clientId=crypto.randomUUID();
 try{
  const response=await fetch(config.comfyUrl+'/prompt',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({prompt:workflow,client_id:clientId}),signal:AbortSignal.timeout(30000)});
  const data=await response.json();
  if(!response.ok)throw Error('ComfyUI '+response.status+': '+JSON.stringify(data).slice(0,700));
  job.status='running';job.promptId=data.prompt_id;put('video_job',job);
  void pollJob(job,config);
  return job;
 }catch(error){
  job.status='failed';job.error=String(error?.message||error);put('video_job',job);throw error;
 }
}
async function pollJob(job,config){
 const deadline=Date.now()+20*60*1000;
 while(Date.now()<deadline){
  await sleep(2500);
  if(job.status==='cancelled')return;
  try{
   const response=await fetch(config.comfyUrl+'/history/'+encodeURIComponent(job.promptId),{signal:AbortSignal.timeout(10000)});
   if(!response.ok)continue;
   const data=await response.json();
   const entry=data[job.promptId];
   if(!entry)continue;
   const output=findOutput(entry);
   if(output){
    job.status='downloading';put('video_job',job);
    const remote=config.comfyUrl+'/view?'+new URLSearchParams({filename:output.filename,subfolder:output.subfolder,type:output.type}).toString();
    const filename=job.id+path.extname(output.filename);
    const local=path.join(config.outputDir,filename);
    const download=await fetch(remote,{signal:AbortSignal.timeout(120000)});
    if(!download.ok||!download.body)throw Error('결과 다운로드 실패: HTTP '+download.status);
    await pipeline(Readable.fromWeb(download.body),fs.createWriteStream(local));
    job.status='completed';job.progress=100;
    job.result={file:local,filename,url:'/api/video/files/'+encodeURIComponent(filename),remote};
    put('video_job',job);
    log('video.completed',{id:job.id,file:local});
    return;
   }
   if(entry.status?.status_str==='error'||entry.status?.completed===false&&entry.status?.messages?.some(message=>String(message?.[0]).includes('error')))throw Error('ComfyUI 실행 오류');
  }catch(error){
   if(job.status==='downloading'){job.status='failed';job.error=String(error?.message||error);put('video_job',job);return}
  }
 }
 job.status='failed';job.error='영상 생성 시간이 초과되었습니다 (20분)';put('video_job',job);
}
export function videoJob(id){return jobs.get(String(id||''))||get('video_job',String(id||''))||null}
export function videoJobs(){return all('video_job').sort((a,b)=>String(a.createdAt||'').localeCompare(String(b.createdAt||''))).slice(-20)}
