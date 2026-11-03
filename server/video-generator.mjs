import fs from 'node:fs';
import fsp from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import crypto from 'node:crypto';
import {spawn,execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {Readable} from 'node:stream';
import {pipeline} from 'node:stream/promises';
import {WORKSPACE} from './config.mjs';
import {all,get,put,log} from './store.mjs';

const jobs=new Map();
const execFileAsync=promisify(execFile);
const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
let managedComfyPid=0;

export function videoConfig(){
 const stored=get('video_config','default')||{};
 return {
  comfyUrl:String(stored.comfyUrl||'http://127.0.0.1:8188').replace(/\/$/,''),
  comfyDir:path.resolve(String(stored.comfyDir||'C:\\ComfyUI')),
  workflowDir:path.resolve(String(stored.workflowDir||path.join(WORKSPACE,'video-workflows'))),
  outputDir:path.resolve(String(stored.outputDir||path.join(os.homedir(),'Videos','DOMINION')))
 };
}
function ensureDirs(config){
 fs.mkdirSync(config.workflowDir,{recursive:true});
 fs.mkdirSync(config.outputDir,{recursive:true});
}
function workflowModel(file){
 try{
  const data=JSON.parse(fs.readFileSync(file,'utf8'));
  for(const node of Object.values(data)){
   if(!node||typeof node!=='object')continue;
   if(node.class_type==='UNETLoader'&&node.inputs?.unet_name)return node.inputs.unet_name;
   if(node.class_type==='CheckpointLoaderSimple'&&node.inputs?.ckpt_name)return node.inputs.ckpt_name;
  }
 }catch{}
 return '';
}
export function videoWorkflows(){
 const config=videoConfig();ensureDirs(config);
 return fs.readdirSync(config.workflowDir,{withFileTypes:true}).filter(entry=>entry.isFile()&&/\.json$/i.test(entry.name)).map(entry=>{
  const file=path.join(config.workflowDir,entry.name);
  return {id:entry.name,name:entry.name.replace(/\.json$/i,''),bytes:fs.statSync(file).size,path:file,model:workflowModel(file)};
 }).sort((a,b)=>a.name.localeCompare(b.name));
}
function requiredModels(config,workflows){
 const base='https://huggingface.co/Comfy-Org/Wan_2.1_ComfyUI_repackaged/resolve/main/split_files';
 const known={
  'wan2.1_t2v_1.3B_fp16.safetensors':base+'/diffusion_models/wan2.1_t2v_1.3B_fp16.safetensors',
  'umt5_xxl_fp8_e4m3fn_scaled.safetensors':base+'/text_encoders/umt5_xxl_fp8_e4m3fn_scaled.safetensors',
  'wan_2.1_vae.safetensors':base+'/vae/wan_2.1_vae.safetensors'
 };
 const found=new Map();
 for(const workflow of workflows){
  try{
   const data=JSON.parse(fs.readFileSync(workflow.path,'utf8'));
   for(const node of Object.values(data)){
    if(!node||typeof node!=='object')continue;
    if(node.class_type==='UNETLoader'&&node.inputs?.unet_name)found.set('diffusion:'+node.inputs.unet_name,{type:'diffusion',name:node.inputs.unet_name});
    if(node.class_type==='CLIPLoader'&&node.inputs?.clip_name)found.set('text:'+node.inputs.clip_name,{type:'text_encoder',name:node.inputs.clip_name});
    if(node.class_type==='VAELoader'&&node.inputs?.vae_name)found.set('vae:'+node.inputs.vae_name,{type:'vae',name:node.inputs.vae_name});
   }
  }catch{}
 }
 return [...found.values()].map(item=>{
  const folder=item.type==='diffusion'?'diffusion_models':item.type==='text_encoder'?'text_encoders':'vae';
  const file=path.join(config.comfyDir,'models',folder,item.name);
  const present=fs.existsSync(file);
  let sizeBytes=0;
  if(present){try{sizeBytes=fs.statSync(file).size}catch{}}
  return {...item,path:file,present,sizeBytes,download:known[item.name]||''};
 });
}
function installedVideoModels(config,workflows){
 const folders=[['diffusion_models','diffusion'],['checkpoints','checkpoint'],['unet','unet']];
 const items=[];
 for(const [folder,kind] of folders){
  const dir=path.join(config.comfyDir,'models',folder);
  if(!fs.existsSync(dir))continue;
  for(const entry of fs.readdirSync(dir,{withFileTypes:true})){
   if(!entry.isFile()||!/\.(safetensors|ckpt|gguf|pt|pth)$/i.test(entry.name))continue;
   const file=path.join(dir,entry.name);
   let sizeBytes=0;
   try{sizeBytes=fs.statSync(file).size}catch{}
   items.push({name:entry.name,kind,folder,path:file,sizeBytes,usedBy:workflows.filter(workflow=>workflow.model===entry.name).map(workflow=>workflow.name)});
  }
 }
 return items.sort((a,b)=>a.name.localeCompare(b.name));
}
export async function videoStatus(){
 const config=videoConfig();ensureDirs(config);
 let comfy={reachable:false,error:''};
 try{
  const response=await fetch(config.comfyUrl+'/system_stats',{signal:AbortSignal.timeout(3000)});
  if(response.ok){comfy={reachable:true,data:await response.json(),error:''}}
  else comfy.error='HTTP '+response.status;
 }catch(error){comfy.error=String(error?.message||error)}
 const workflows=videoWorkflows();
 const models=requiredModels(config,workflows);
 const installedModels=installedVideoModels(config,workflows);
 return {config,comfy,installed:fs.existsSync(path.join(config.comfyDir,'main.py'))&&fs.existsSync(path.join(config.comfyDir,'.venv','Scripts','python.exe')),workflows,models,modelsReady:models.length>0&&models.every(model=>model.present),installedModels,jobs:videoJobs().slice(-12)};
}
function revealInShell(args){
 const explorer=path.join(process.env.WINDIR||'C:\\Windows','explorer.exe');
 const child=spawn(explorer,args,{detached:true,windowsHide:true,stdio:'ignore'});
 child.on('error',()=>{});
 child.unref();
}
export function openModelFolder(){
 const models=path.join(videoConfig().comfyDir,'models');
 if(!fs.existsSync(models))throw Error('ComfyUI 모델 폴더가 없습니다: '+models);
 if(process.platform==='win32')revealInShell([models]);
 else if(process.platform==='darwin')spawn('open',[models],{detached:true,stdio:'ignore'}).unref();
 else spawn('xdg-open',[models],{detached:true,stdio:'ignore'}).unref();
 return {ok:true,path:models};
}
export async function startComfyUI(){
 const config=videoConfig();
 try{const response=await fetch(config.comfyUrl+'/system_stats',{signal:AbortSignal.timeout(1500)});if(response.ok)return {started:false,already:true}}catch{}
 const python=path.join(config.comfyDir,'.venv','Scripts','python.exe');
 const main=path.join(config.comfyDir,'main.py');
 if(!fs.existsSync(python)||!fs.existsSync(main))throw Error('ComfyUI가 '+config.comfyDir+'에 설치되어 있지 않습니다.');
 const port=new URL(config.comfyUrl).port||'8188';
 const logFile=path.join(config.comfyDir,'comfyui.log');
 const logFd=fs.openSync(logFile,'a');
 const child=spawn(python,[main,'--listen','127.0.0.1','--port',port,'--lowvram','--preview-method','none'],{cwd:config.comfyDir,detached:true,windowsHide:true,stdio:['ignore',logFd,logFd]});
 fs.closeSync(logFd);
 child.unref();
 managedComfyPid=child.pid||0;
 put('video_config',{id:'default',...config,managedByApp:true,managedPid:managedComfyPid});
 log('video.comfy_started',{pid:child.pid,port});
 return {started:true,pid:child.pid,port};
}
function activeVideoJobs(){return videoJobs().filter(job=>['queued','running','downloading'].includes(job.status))}
async function comfyListenerPids(port){
 const pids=new Set();
 if(process.platform==='win32'){
  const {stdout}=await execFileAsync('netstat',['-ano','-p','tcp'],{windowsHide:true,maxBuffer:4*1024*1024});
  for(const line of stdout.split(/\r?\n/)){
   const cols=line.trim().split(/\s+/);
   if(String(cols[0]).toUpperCase()!=='TCP'||String(cols[3]).toUpperCase()!=='LISTENING')continue;
   const match=String(cols[1]||'').match(/:(\d+)$/);
   if(match&&Number(match[1])===Number(port)&&Number(cols[4]))pids.add(Number(cols[4]));
  }
 }else{
  try{const {stdout}=await execFileAsync('lsof',['-ti',`tcp:${port}`,'-sTCP:LISTEN']);for(const line of stdout.split(/\r?\n/))if(line.trim())pids.add(Number(line.trim()))}catch{}
 }
 return [...pids];
}
export async function stopComfyUI(options={}){
 if(!options.force&&activeVideoJobs().length)throw Error('생성 중인 작업이 있습니다. 작업이 끝난 뒤 종료하세요.');
 const config=videoConfig();
 const stored=get('video_config','default')||{};
 if(options.onlyManaged&&!stored.managedByApp)return {stopped:false,skipped:true,message:'앱에서 시작한 ComfyUI가 아닙니다.'};
 const port=Number(new URL(config.comfyUrl).port||8188);
 const pids=await comfyListenerPids(port);
 for(const pid of pids){
  try{
   if(process.platform==='win32')await execFileAsync('taskkill',['/PID',String(pid),'/T','/F'],{windowsHide:true});
   else process.kill(pid,'SIGTERM');
  }catch{}
 }
 if(managedComfyPid&&!pids.includes(managedComfyPid)){try{process.kill(managedComfyPid)}catch{}}
 for(let i=0;i<24;i++){
  try{const response=await fetch(config.comfyUrl+'/system_stats',{signal:AbortSignal.timeout(700)});if(!response.ok)break}catch{return finishStop(stored,pids.length)}
  await sleep(250);
 }
 return finishStop(stored,pids.length);
}
function finishStop(stored,count){
 managedComfyPid=0;
 try{put('video_config',{...stored,id:'default',managedByApp:false,managedPid:0})}catch{}
 log('video.comfy_stopped',{pids:count});
 return {stopped:true,pids:count};
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
function slugify(value){
 return String(value||'').normalize('NFKC').replace(/[^\p{L}\p{N}]+/gu,'-').replace(/^-+|-+$/g,'').slice(0,48)||'video';
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
 const seconds=Math.max(1,Math.min(9,Number(input.seconds)||4));
 const requestedFrames=Number(input.frames)||Math.round(fps*seconds);
 const frames=Math.max(1,Math.min(600,input.frames?requestedFrames:1+Math.max(0,Math.round((requestedFrames-1)/4))*4));
 const seed=Number.isFinite(Number(input.seed))&&Number(input.seed)>=0?Math.floor(Number(input.seed)):Math.floor(Math.random()*1e9);
 const workflow=replaceTokens(raw,{prompt:String(input.prompt||'').trim(),negative:String(input.negative||'').trim(),width,height,frames,fps,seed});
 const now=new Date(),pad=value=>String(value).padStart(2,'0');
 const stamp=now.getFullYear()+pad(now.getMonth()+1)+pad(now.getDate())+'-'+pad(now.getHours())+pad(now.getMinutes());
 const title=String(input.title||input.prompt||'영상').trim().slice(0,100);
 const job={id:crypto.randomUUID(),status:'queued',progress:0,workflow:path.basename(file),title,prompt:String(input.prompt||'').slice(0,300),width,height,frames,fps,seconds,seed,filenameBase:stamp+'-'+slugify(title),createdAt:new Date().toISOString(),error:'',result:null};
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
    const filename=(job.filenameBase||job.id)+'-'+job.id.slice(0,6)+path.extname(output.filename);
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
export function openVideoFile(file){
 const config=videoConfig();
 const target=path.resolve(String(file||''));
 if(!target.startsWith(config.outputDir+path.sep)||!fs.existsSync(target))throw Error('Video file not found');
 if(process.platform==='win32')revealInShell(['/select,'+target]);
 else if(process.platform==='darwin')spawn('open',['-R',target],{detached:true,stdio:'ignore'}).unref();
 else spawn('xdg-open',[path.dirname(target)],{detached:true,stdio:'ignore'}).unref();
 return {ok:true,path:target};
}
export function videoJob(id){return jobs.get(String(id||''))||get('video_job',String(id||''))||null}
export function videoJobs(){return all('video_job').sort((a,b)=>String(a.createdAt||'').localeCompare(String(b.createdAt||''))).slice(-20)}
