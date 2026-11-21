import {useMemo,useState} from 'react'
import {Check,ChevronDown,ChevronRight,GitBranch,Layers3,RefreshCw,ShieldCheck} from 'lucide-react'
import {get,post} from '../api'
import {useLocale} from '../i18n'
import {Panel,useRemote} from './UI'

type Job={
 id:string;kind?:string;status?:string;error?:string;result?:Record<string,unknown>|null;
 createdAt?:string;queuedAt?:string;
 delegationPatchSha256?:string;integrationStatus?:string;
 input?:{
  goal?:string;prompt?:string;sessionId?:string;parentSessionId?:string;agentId?:string;
  teamJobId?:string;teamStepId?:string;delegation?:boolean;delegationProfile?:string;externalRuntime?:string|null;
  dependsOnJobIds?:string[];resumeFromJobId?:string|null;remoteWorker?:boolean;
  maxParallel?:number;plan?:Step[];childJobIds?:string[];
 }
}
type Step={id:string;jobId:string;agentId?:string;profile?:string;dependsOn?:string[];remoteWorker?:boolean}
const compact=(value:unknown)=>typeof value==='string'?value:'—'
const stateClass=(state?:string)=>state==='completed'?'border-emerald-200 bg-emerald-50 text-emerald-700':
 state==='failed'||state==='cancelled'?'border-rose-200 bg-rose-50 text-rose-700':
 state==='running'?'border-[#D9BAA6] bg-[#F7E8E1] text-[#9D5039]':
 'border-[#E7E0D5] bg-[#FAF7F1] text-[#7C7469]'
function ExternalContinuation({job,onCreated}:{job:Job;onCreated:()=>void}){
 const en=useLocale()==='en'
 const [open,setOpen]=useState(false)
 const [prompt,setPrompt]=useState('')
 const [busy,setBusy]=useState(false)
 const [error,setError]=useState('')
 const [result,setResult]=useState<{jobId?:string;sessionId?:string}|null>(null)
 const runtime=job.input?.externalRuntime
 const finished=job.status==='completed'&&typeof job.result?.externalRunId==='string'
 const parent=job.input?.parentSessionId
 return <div className="mt-2 min-w-0">
  <div className="flex flex-wrap items-center gap-2 text-[10px] text-[#7C7469]">
   <span className="rounded-md bg-[#F3EEE5] px-2 py-0.5">{runtime}</span>
   <span>{en?'External receipt':'외부 실행 증거'}: {compact(job.result?.externalRunId)}</span>
   {job.input?.resumeFromJobId&&<span>{en?'Continued from':'이어받은 작업'}: {job.input.resumeFromJobId}</span>}
   {parent&&<a href={'#/chat?session='+encodeURIComponent(parent)} className="underline">{en?'Parent conversation':'원본 대화'}</a>}
  </div>
  {finished&&<button type="button" aria-expanded={open} onClick={()=>setOpen(v=>!v)}
   className="mt-2 inline-flex items-center gap-1 rounded-lg border border-[#E7E0D5] px-2.5 py-1.5 text-[11px] text-[#5C544A] hover:bg-[#FAF7F1]">
   <RefreshCw size={12}/>{en?'Continue verified inspection session':'검증된 조사 세션 이어하기'} {open?'▴':'▾'}
  </button>}
  {open&&finished&&<form className="mt-2 flex flex-wrap gap-2" onSubmit={event=>{
   event.preventDefault();if(busy||!prompt.trim())return
   setBusy(true);setError('');setResult(null)
   void post<{jobId?:string;sessionId?:string}>('/api/jobs/'+encodeURIComponent(job.id)+'/resume-external',{prompt:prompt.trim()})
    .then(value=>{setResult(value);setPrompt('');onCreated()})
    .catch(e=>setError((e as Error).message))
    .finally(()=>setBusy(false))
  }}>
   <input className="min-w-40 flex-1 rounded-lg border border-[#E7E0D5] px-3 py-2 text-[11px]"
    value={prompt} onChange={e=>setPrompt(e.target.value)} maxLength={3000}
    aria-label={en?'Follow-up inspection instructions':'이어갈 조사 지시'}
    placeholder={en?'What should the same inspector check next?':'같은 에이전트가 이어서 조사할 내용'}/>
   <button type="submit" disabled={busy||!prompt.trim()} className="rounded-lg border border-[#D9BAA6] px-3 py-2 text-[11px] font-semibold text-[#9D5039] disabled:opacity-50">{busy?(en?'Submitting':'전달 중'):(en?'Resume':'재개')}</button>
  </form>}
  {error&&<p role="alert" className="mt-2 break-words text-[11px] text-rose-700">{error}</p>}
  {result&&<p role="status" className="mt-2 flex flex-wrap items-center gap-1 text-[11px] text-emerald-700"><Check size={12}/>{en?'Child inspection queued':'후속 조사 예약 완료'}: {result.jobId}{result.sessionId&&<a href={'#/chat?session='+encodeURIComponent(result.sessionId)} className="underline">{en?'Open session':'대화 열기'}</a>}</p>}
 </div>
}
function WorkerStep({step,job,ids,onCreated}:{step:Step;job?:Job;ids:Record<string,string>;onCreated:()=>void}){
 const en=useLocale()==='en'
 const [showResult,setShowResult]=useState(false)
 const done=job?.status==='completed'
 const fileHash=job?.delegationPatchSha256
 const deps=(step.dependsOn||[])
 const receipt=job?.result
 return <div className="relative rounded-xl border border-[#E7E0D5] bg-white p-3">
  <div className="flex min-w-0 flex-wrap items-center justify-between gap-2">
   <div className="flex min-w-0 items-center gap-2">
    <span className="grid size-6 shrink-0 place-items-center rounded-full bg-[#F7E8E1] text-[10px] font-bold text-[#A4583E]">{step.id.slice(0,3)}</span>
    <strong className="text-[12px]">{step.id}</strong>
    <small className="text-[10px] text-[#9C9388]">{step.profile||'general'}</small>
   </div>
   <span className={'rounded border px-2 py-0.5 text-[10px] font-semibold '+stateClass(job?.status)}>{job?.status||(en?'Not scheduled':'대기')}</span>
  </div>
  <p className="mt-2 break-words text-[11px] text-[#514940]">
   {en?'Agent':'담당'}: {step.agentId||job?.input?.agentId||'—'}
   {step.remoteWorker&&<span className="ml-2 rounded bg-[#F3EEE5] px-1.5 py-0.5">remote</span>}
  </p>
  <div className="mt-2 flex flex-wrap items-center gap-1 text-[10px] text-[#8A8176]">
   {deps.length?<>{en?'After':'선행 단계'}: {deps.map((dep,index)=><span key={dep} className="inline-flex items-center gap-1 rounded bg-[#FAF7F1] px-1.5 py-1">
     {index>0?', ':''}{dep}<span className={'size-1.5 rounded-full '+(Object.keys(ids).includes(dep)&&ids[dep]==='completed'?'bg-emerald-500':'bg-amber-400')}/>
    </span>)}</>:<span>{en?'No prerequisites':'선행 조건 없음'}</span>}
  </div>
  {job?.error&&<p role="alert" className="mt-2 break-words text-[11px] text-rose-700">{job.error}</p>}
  {fileHash&&<p className="mt-2 break-all text-[10px] text-[#8A8176]">{en?'Frozen patch SHA256':'고정된 패치 SHA256'}: {fileHash}</p>}
  {job?.input?.externalRuntime&&<ExternalContinuation job={job} onCreated={onCreated}/>}
  {job?.result&&<button type="button" aria-expanded={showResult} onClick={()=>setShowResult(v=>!v)}
   className="mt-2 inline-flex items-center gap-1 text-[11px] text-[#A4583E]">
   <ShieldCheck size={12}/>{en?'Evidence and output':'증거·결과 확인'} {showResult?'▴':'▾'}
  </button>}
  {showResult&&job?.result&&<pre className="mt-2 max-h-44 overflow-auto whitespace-pre-wrap break-all rounded-lg bg-[#FAF7F1] p-2 text-[10px]">{JSON.stringify(job.result,null,2)}</pre>}
  {job&&<span className="mt-1 block break-all font-mono text-[9px] text-[#A79C8E]">{job.id}</span>}
  {done&&<span className="sr-only">{en?'Completed':'완료'}</span>}
 </div>
}
/** Reads existing team job plan and the child job journal. No new planner,
 * no second DAG state and no mutation of the engine's dependency graph. */
export function TeamWorkflowPanel({jobs,filterSessionId}:{jobs?:Job[];filterSessionId?:string}){
 const en=useLocale()==='en'
 const [expanded,setExpanded]=useState(false)
 // Room panels own the job subscription when no parent supplies one.
 // Native job record events refresh worker status without a manual reload.
 const endpoint=jobs===undefined?(filterSessionId
  ?'/api/jobs?sessionId='+encodeURIComponent(filterSessionId)+'&limit=128'
  :'/api/jobs'):null
 const remote=useRemote<Job[]>(endpoint,[])
 const rows=jobs??remote.value
 const parentTeams=useMemo(()=>rows.filter(j=>j.kind==='team'&&Array.isArray(j.input?.plan)
  &&(!filterSessionId||j.input?.sessionId===filterSessionId)).sort((a,b)=>(b.queuedAt||'').localeCompare(a.queuedAt||'')).slice(0,8),[rows,filterSessionId])
 const standaloneExternal=useMemo(()=>rows.filter(j=>Boolean(j.input?.externalRuntime)&&!j.input?.teamJobId
  &&(!filterSessionId||j.input?.parentSessionId===filterSessionId)).slice(0,10),[rows,filterSessionId])
 const dependentJobs=useMemo(()=>rows.filter(j=>j.input?.delegation===true&&!j.input?.teamJobId
  &&(j.input?.dependsOnJobIds?.length||0)>0
  &&(!filterSessionId||j.input?.parentSessionId===filterSessionId)).slice(0,18),[rows,filterSessionId])
 const independentStates=useMemo(()=>Object.fromEntries(rows.map(j=>[j.id,j.status||'queued'])),[rows])
 const jobMap=useMemo(()=>new Map(rows.map(j=>[j.id,j])),[rows])
 const [opened,setOpened]=useState<string[]>([])
 const toggle=(id:string)=>setOpened(old=>old.includes(id)?old.filter(x=>x!==id):[...old,id])
 const refresh=()=>{if(jobs===undefined)void remote.reload()}
 if(!parentTeams.length&&!standaloneExternal.length&&!dependentJobs.length&&!remote.loading)return null
 return <Panel className="mt-4 min-w-0 overflow-hidden">
  <button type="button" aria-expanded={expanded} onClick={()=>setExpanded(x=>!x)}
   className="flex w-full flex-wrap items-center justify-between gap-2 p-4 text-left hover:bg-[#FAF7F1]">
   <span className="flex items-center gap-2"><GitBranch size={17} className="text-[#A4583E]"/><span><strong className="block text-[13px]">{en?'Team dependency graph & external agents':'팀 작업 의존성·외부 에이전트'}</strong>
     <small className="text-[11px] text-[#8A8176]">{en?'Recorded native steps, dependencies, actual worker outcomes.':'네이티브 작업 단계·선행 조건·실제 워커 결과를 연결합니다.'}</small></span></span>
   {expanded?<ChevronDown size={15}/>:<ChevronRight size={15}/>}
  </button>
  {expanded&&<div className="space-y-3 border-t border-[#E7E0D5] p-4">
   {jobs===undefined&&<button type="button" onClick={refresh} className="text-[11px] text-[#7C7469] underline">{en?'Refresh native jobs':'작업 기록 새로고침'}</button>}
   {parentTeams.map(team=>{
    const plan=team.input?.plan||[]
    const statuses=Object.fromEntries(plan.map(step=>[step.id,jobMap.get(step.jobId)?.status||'queued']))
    const done=plan.filter(step=>jobMap.get(step.jobId)?.status==='completed').length
    const open=opened.includes(team.id)
    return <div key={team.id} className="rounded-xl border border-[#E7E0D5] bg-[#FCFAF6] p-3">
     <button type="button" aria-expanded={open} onClick={()=>toggle(team.id)} className="flex w-full min-w-0 justify-between gap-2 text-left">
      <span className="min-w-0"><strong className="block text-[12px]">{team.input?.goal||team.id}</strong>
       <span className="mt-1 block text-[10px] text-[#7C7469]">{en?'Completed':'완료'} {done}/{plan.length} · {en?'Parallel capacity':'동시 실행 한도'} {team.input?.maxParallel||'—'}</span></span>
      <span className={'shrink-0 self-start rounded border px-2 py-1 text-[10px] '+stateClass(team.status)}>{team.status}</span>
     </button>
     {open&&<div className="mt-3 space-y-2">
      {plan.map(step=><WorkerStep key={step.id} step={step} job={jobMap.get(step.jobId)} ids={statuses} onCreated={refresh}/>)}
      <p className="mt-2 text-[10px] text-[#8A8176]">{en?'Edges show persisted prerequisites. A completed step is not proof that an unreviewed patch was integrated.':'선행 조건은 실제 저장된 의존성입니다. 단계 완료만으로 미검토 패치가 병합된 것은 아닙니다.'}</p>
      {team.error&&<p role="alert" className="text-[11px] text-rose-700">{team.error}</p>}
      <span className="block break-all font-mono text-[10px] text-[#A79C8E]">{team.id}</span>
     </div>}
    </div>
   })}
   {dependentJobs.length>0&&<section className="rounded-xl border border-[#E7E0D5] bg-[#FCFAF6] p-3">
    <h3 className="mb-2 flex items-center gap-2 text-[12px] font-bold"><GitBranch size={14}/>{en?'Dependent delegated jobs':'의존성 있는 개별 위임 작업'}</h3>
    <p className="mb-2 text-[10px] text-[#8A8176]">{en?'The arrows below represent durable sibling job dependencies, not inferred order.':'개별 위임 작업의 저장된 선행 작업 ID를 그대로 표시합니다. 화면 순서로 의존성을 추정하지 않습니다.'}</p>
    <div className="space-y-2">{dependentJobs.map(job=><WorkerStep key={job.id} job={job} ids={independentStates} onCreated={refresh}
      step={{id:job.input?.teamStepId||job.id.slice(0,8),jobId:job.id,agentId:job.input?.agentId,
       profile:job.input?.delegationProfile,dependsOn:job.input?.dependsOnJobIds}}/>)}</div>
   </section>}
   {standaloneExternal.length>0&&<section className="rounded-xl border border-[#E7E0D5] bg-[#FCFAF6] p-3">
    <h3 className="mb-2 flex items-center gap-2 text-[12px] font-bold"><Layers3 size={14}/>{en?'Standalone external inspections':'독립 외부 조사 기록'}</h3>
    <div className="space-y-2">{standaloneExternal.map(job=><div key={job.id} className="rounded-lg border border-[#EFE9DF] bg-white p-3">
     <div className="flex flex-wrap justify-between gap-2 text-[11px]"><strong className="min-w-0 break-words">{job.input?.prompt||job.id}</strong>
      <span className={'shrink-0 rounded border px-2 py-0.5 '+stateClass(job.status)}>{job.status}</span></div>
     <ExternalContinuation job={job} onCreated={refresh}/>
     {job.error&&<p className="mt-1 text-[11px] text-rose-700">{job.error}</p>}
    </div>)}</div>
   </section>}
  </div>}
 </Panel>
}
