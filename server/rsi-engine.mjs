import fs from 'node:fs/promises';
import path from 'node:path';
import {spawn} from 'node:child_process';
import {randomUUID,createHash} from 'node:crypto';
import {DATA} from './config.mjs';
import {devConfig} from './dev-studio.mjs';
import {all,get,put,log} from './store.mjs';
import {resolveModel} from './model-router.mjs';
import {streamCompletion} from './providers.mjs';
import {experienceHints,recordExperience} from './experience.mjs';

// Non-weight RSI: generate in an isolated worktree, freeze evaluation, then
// require explicit adoption. The model never writes directly into sourceDir.
const K='rsi_experiment', active=new Map(), root=path.join(DATA,'rsi-worktrees');
const terminal=new Set(['accepted','rejected','failed','pending_approval','needs_review','cancelled']);
const short=(s,n=2000)=>String(s??'').slice(0,n);
const digest=s=>createHash('sha256').update(s).digest('hex');
const error=s=>new Error(s);
const redact=s=>short(s,5000).replace(/(?:sk-[\w-]{12,}|gh[pousr]_[\w-]{12,}|Bearer\s+\S+)/g,'[REDACTED]');
const getRow=id=>get(K,id);
function transition(id,status,extra={}){
 const before=getRow(id);if(!before)throw error('RSI experiment missing');
 return put(K,{...before,...extra,status,updatedAt:new Date().toISOString(),
  events:[...(before.events||[]),{status,at:new Date().toISOString()}].slice(-60)});
}
export const experiments=()=>all(K);
export const experiment=getRow;
function safePath(input){
 const p=String(input||'').replace(/\\/g,'/');
 if(!p||p.startsWith('/')||p.split('/').some(x=>!x||x==='.'||x==='..')||
    !/^[\p{L}\p{N}_./-]+$/u.test(p))throw error('Invalid candidate path');
 if(/^(tests|\.git|\.github|node_modules|runtime|workspace)\//.test(p)||
    /^(package(-lock)?\.json|\.env.*|.*\.test\.[cm]?[jt]sx?)$/.test(p))
   throw error('Frozen evaluation/dependency/secret file cannot be edited');
 return p;
}
export function createExperiment(input={}){
 const goal=short(input.goal,2400).trim();
 const paths=[...new Set((Array.isArray(input.paths)?input.paths:[]).map(safePath))];
 if(!goal||!paths.length||paths.length>8)throw error('Provide goal and 1–8 exact source file paths');
 const model={providerId:String(input.providerId||''),model:String(input.model||'')};
 const id=randomUUID();
 return put(K,{id,goal,paths,model,sourceDir:path.resolve(devConfig().sourceDir),
  test:input.test==='npm-test'?'npm-test':'node-tests',
  maxSeconds:Math.max(10,Math.min(300,Number(input.testTimeoutSeconds)||120)),
  ancestry:input.ancestry||null,createdAt:new Date().toISOString(),status:'queued',
  events:[{status:'queued',at:new Date().toISOString()}],usage:{modelCalls:0,tokens:0},
  baseCommit:'',candidateCommit:'',validation:null,reason:''});
}
function command(bin,args,cwd,{signal,timeoutMs=30000}={}){
 return new Promise((resolve,reject)=>{
  const child=spawn(bin,args,{cwd,windowsHide:true,env:{...process.env,GIT_TERMINAL_PROMPT:'0'},
   stdio:['ignore','pipe','pipe']});
  let stdout='',stderr='',done=false;
  const kill=()=>{try{child.kill('SIGKILL')}catch{}};
  const finish=(err,val)=>{
   if(done)return;done=true;clearTimeout(timer);
   signal?.removeEventListener('abort',abort);
   if(err)reject(err);else resolve(val);
  };
  const abort=()=>{kill();finish(error('RSI cancelled'))};
  const timer=setTimeout(()=>{kill();finish(error('Process timeout: '+bin))},timeoutMs);
  if(signal?.aborted)return abort();
  signal?.addEventListener('abort',abort,{once:true});
  child.on('error',e=>finish(e));
  child.stdout.on('data',b=>{stdout=short(stdout+b.toString(),12000)});
  child.stderr.on('data',b=>{stderr=short(stderr+b.toString(),12000)});
  child.on('close',code=>finish(null,{code,stdout,stderr}));
 });
}
async function git(cwd,args,signal){
 const r=await command('git',args,cwd,{signal,timeoutMs:60000});
 if(r.code!==0)throw error('Git '+args[0]+' failed: '+redact(r.stderr||r.stdout));
 return r.stdout.trim();
}
async function requireClean(dir,signal){
 if(await git(dir,['status','--porcelain','--untracked-files=normal'],signal))
  throw error('Source checkout is dirty; commit or stash changes first');
}
async function attachModules(source,dir){
 try{
  const from=path.join(source,'node_modules');
  if((await fs.stat(from)).isDirectory())await fs.symlink(from,path.join(dir,'node_modules'),'junction');
 }catch(e){if(!['ENOENT','EEXIST'].includes(e.code))throw e}
}
async function allowedFile(dir,rel){
 const base=path.resolve(dir),file=path.resolve(base,rel);
 if(!file.startsWith(base+path.sep))throw error('Worktree escape');
 let pos=base;
 for(const part of rel.split('/')){
  pos=path.join(pos,part);
  const st=await fs.lstat(pos).catch(e=>{if(e.code==='ENOENT')return null;throw e});
  if(st?.isSymbolicLink())throw error('Symlink in candidate path');
 }
 return file;
}
async function frozenDigest(dir){
 const files=[];
 async function visit(rel){
  const p=path.join(dir,rel);
  const st=await fs.lstat(p).catch(e=>{if(e.code==='ENOENT')return null;throw e});
  if(!st)return;
  if(st.isSymbolicLink())throw error('Frozen test path cannot be a symlink');
  if(st.isDirectory())for(const name of (await fs.readdir(p)).sort())await visit(path.posix.join(rel,name));
  else if(st.isFile())files.push(rel+':'+digest(await fs.readFile(p)));
 }
 await visit('tests');await visit('package.json');await visit('scripts/engine-gate.mjs');
 return digest(files.join('\n'));
}
async function evaluate(dir,mode,seconds,signal){
 const npm=path.join(dir,'node_modules','npm','bin','npm-cli.js');
 if(mode==='npm-test')await fs.access(npm);
 const testFiles=(await fs.readdir(path.join(dir,'tests'))).filter(name=>name.endsWith('.test.mjs')).sort().map(name=>'tests/'+name);
 if(mode==='node-tests'&&!testFiles.length)throw error('No frozen Node test files found');
 const args=mode==='npm-test'?[npm,'test']:['--test','--test-concurrency=1',...testFiles];
 const out=await command(process.execPath,args,dir,{signal,timeoutMs:seconds*1000});
 return {passed:out.code===0,exitCode:out.code,output:redact(out.stdout+'\n'+out.stderr)};
}
function parseEdits(raw){
 const str=String(raw||'').trim();
 let data;
 try{data=JSON.parse(str)}catch{
  const start=str.indexOf('{'),end=str.lastIndexOf('}');
  if(start<0||end<start)throw error('Generator returned no JSON object');
  try{data=JSON.parse(str.slice(start,end+1))}catch{throw error('Generator JSON invalid')}
 }
 if(!Array.isArray(data.edits)||!data.edits.length||data.edits.length>8)throw error('Expected 1–8 edits');
 return {edits:data.edits,strategy:short(data.strategy,500)};
}
async function generate(row,baseline,signal){
 const model=await resolveModel(row.model);
 const files=[];
 for(const p of row.paths){
  const file=await allowedFile(baseline,p),st=await fs.stat(file);
  if(!st.isFile()||st.size>20000)throw error('Source file missing/too large: '+p);
  files.push({path:p,content:await fs.readFile(file,'utf8')});
 }
 const prompt='Goal: '+row.goal+'\nSource file contents:\n'+JSON.stringify(files).slice(0,52000)+
  '\nPrior verifiable outcome hints:\n'+experienceHints(row.goal,{limit:3})+
  (row.ancestry?'\nAccepted ancestor: '+JSON.stringify(row.ancestry).slice(0,1200):'')+
  '\nReturn JSON {"edits":[{"path":"allowed path","old":"exact unique substring","new":"replacement"}],"strategy":"why"}. '+
  'Maximum 8 edits and 14000 replacement characters. No test, dependency, secret or evaluation changes.';
 const res=await streamCompletion({providerId:model.providerId,model:model.model,signal,
  messages:[{role:'system',content:'Generate a code improvement proposal, only valid JSON. You cannot evaluate or approve your proposal.'},
   {role:'user',content:prompt}],tools:[]});
 const tokens=Number(res.usage?.total_tokens||((res.usage?.prompt_tokens||0)+(res.usage?.completion_tokens||0)))||0;
 transition(row.id,'generating',{usage:{modelCalls:1,tokens},
  generator:{providerId:model.providerId,model:model.model}});
 return parseEdits(res.content);
}
async function applyEdits(dir,paths,edits){
 if(edits.reduce((n,e)=>n+String(e.new??'').length,0)>14000)throw error('Replacement size budget exceeded');
 const changed=new Map();
 for(const edit of edits){
  const p=safePath(edit.path);
  if(!paths.includes(p))throw error('Unauthorized candidate edit: '+p);
  const file=await allowedFile(dir,p);
  const content=changed.has(p)?changed.get(p):await fs.readFile(file,'utf8');
  const old=String(edit.old??''),replacement=String(edit.new??'');
  if(!old||old===replacement)throw error('Empty or unchanged replacement');
  const i=content.indexOf(old);
  if(i<0||content.indexOf(old,i+old.length)!==-1)throw error('Substring must appear once: '+p);
  changed.set(p,content.slice(0,i)+replacement+content.slice(i+old.length));
 }
 for(const [p,text] of changed)await fs.writeFile(await allowedFile(dir,p),text,'utf8');
 return [...changed.keys()];
}
export async function runExperiment(id){
 const row=getRow(id);
 if(!row||row.status!=='queued'||active.has(id))throw error('Experiment is not queued');
 const controller=new AbortController(),signal=controller.signal;
 active.set(id,controller);
 try{
  await fs.mkdir(root,{recursive:true});
  await requireClean(row.sourceDir,signal);
  const base=await git(row.sourceDir,['rev-parse','HEAD'],signal);
  const home=path.join(root,id);
  await fs.mkdir(home,{recursive:true});
  const baseline=path.join(home,'baseline'),candidate=path.join(home,'candidate');
  transition(id,'isolating',{baseCommit:base,baselineDir:baseline,candidateDir:candidate});
  await git(row.sourceDir,['worktree','add','--detach',baseline,base],signal);
  await git(row.sourceDir,['worktree','add','--detach',candidate,base],signal);
  await attachModules(row.sourceDir,baseline);await attachModules(row.sourceDir,candidate);
  const frozen=await frozenDigest(baseline);
  transition(id,'baseline_testing',{frozenDigest:frozen});
  const initial=await evaluate(baseline,row.test,row.maxSeconds,signal);
  transition(id,'generating',{baseline:{passed:initial.passed,exitCode:initial.exitCode,output:initial.output}});
  const proposed=await generate(row,baseline,signal);
  const touched=await applyEdits(candidate,row.paths,proposed.edits);
  if(await frozenDigest(candidate)!==frozen)throw error('Frozen evaluation files changed');
  transition(id,'candidate_testing',{pathsChanged:touched,strategy:proposed.strategy});
  const final=await evaluate(candidate,row.test,row.maxSeconds,signal);
  if(await frozenDigest(candidate)!==frozen)throw error('Candidate test changed frozen evaluation files');
  const validation={mode:'frozen-deterministic',baselinePassed:initial.passed,
   candidatePassed:final.passed,candidateExitCode:final.exitCode,
   improvedFailingBaseline:!initial.passed&&final.passed,
   frozenDigest:frozen,candidateOutput:final.output};
  if(!final.passed){
   transition(id,'needs_review',{validation,reason:'Candidate failed frozen tests'});
   recordExperience({task:row.goal,source:'rsi',status:'failed',strategy:proposed.strategy,
    reason:short(final.output,400)});
   return getRow(id);
  }
  await git(candidate,['add','--',...touched],signal);
  await git(candidate,['-c','user.name=DOMINION RSI','-c','user.email=rsi@localhost','commit',
   '-m','RSI candidate: '+short(row.goal,70)],signal);
  const candidateCommit=await git(candidate,['rev-parse','HEAD'],signal);
  transition(id,'pending_approval',{validation,candidateCommit,reason:'Frozen tests passed; adoption awaits approval'});
  recordExperience({task:row.goal,source:'rsi',status:'completed',
   reason:'Tests passed in isolated candidate; not yet adopted',
   strategy:proposed.strategy,verified:true,evidence:[{name:'frozen-tests',status:'completed'}]});
  return getRow(id);
 }catch(e){
  if(getRow(id)?.status!=='cancelled')transition(id,'failed',{reason:redact(e.message||e)});
  return getRow(id);
 }finally{active.delete(id);}
}
export async function acceptExperiment(id){
 const row=getRow(id);
 if(!row||row.status!=='pending_approval'||!row.candidateCommit)throw error('Validated candidate required');
 await requireClean(row.sourceDir);
 if(await git(row.sourceDir,['rev-parse','HEAD'])!==row.baseCommit)throw error('Source HEAD changed; adoption refused');
 if(await frozenDigest(row.candidateDir)!==row.frozenDigest)throw error('Frozen evaluation changed');
 if(await git(row.candidateDir,['rev-parse','HEAD'])!==row.candidateCommit)throw error('Candidate commit changed');
 await git(row.sourceDir,['merge','--ff-only',row.candidateCommit]);
 if(await git(row.sourceDir,['rev-parse','HEAD'])!==row.candidateCommit)throw error('Adopted HEAD mismatch');
 log('rsi.accepted',{id,commit:row.candidateCommit});
 return transition(id,'accepted',{adoptedCommit:row.candidateCommit,acceptedAt:new Date().toISOString(),reason:'Fast-forward adopted'});
}
export function cancelExperiment(id){
 const row=getRow(id);
 if(!row||terminal.has(row.status))throw error('Experiment cannot be cancelled');
 active.get(id)?.abort();
 return transition(id,'cancelled',{reason:'Cancelled by user'});
}
export function nextExperiment(id,input={}){
 const row=getRow(id);
 if(!row||row.status!=='accepted')throw error('Accepted ancestor required');
 return createExperiment({...input,goal:input.goal||row.goal,paths:input.paths||row.paths,
  test:input.test||row.test,providerId:input.providerId||row.model.providerId,
  model:input.model||row.model.model,
  ancestry:{id:row.id,commit:row.adoptedCommit,strategy:row.strategy}});
}
export function recoverExperiments(){
 for(const row of experiments())if(!terminal.has(row.status)&&row.status!=='queued')
  transition(row.id,'needs_review',{reason:'Interrupted; inspect worktrees before manual restart'});
}
