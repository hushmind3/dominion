import {useEffect,useMemo,useState} from 'react'
import {Check,ChevronDown,ChevronRight,GitBranch,Layers3,MessageSquare,RefreshCw,ShieldCheck} from 'lucide-react'
import {get,post} from '../api'
import {useLocale} from '../i18n'
import {Panel,useRemote} from './UI'
import {classifyTeamReview} from './teamOutcome'

type Job={
 id:string;kind?:string;status?:string;error?:string;result?:Record<string,unknown>|null;
 createdAt?:string;queuedAt?:string;
 delegationPatchSha256?:string;integrationStatus?:string;integratedAt?:string;
 input?:{
  goal?:string;prompt?:string;sessionId?:string;parentSessionId?:string;agentId?:string;
  teamJobId?:string;teamStepId?:string;delegation?:boolean;delegationProfile?:string;externalRuntime?:string|null;
  dependsOnJobIds?:string[];resumeFromJobId?:string|null;remoteWorker?:boolean;
  maxParallel?:number;plan?:Step[];childJobIds?:string[];
 }
}
type Step={id:string;jobId:string;agentId?:string;profile?:string;dependsOn?:string[];remoteWorker?:boolean}
type TeamSteps={teamId:string;jobs:Job[];missing:string[]}
const compact=(value:unknown)=>typeof value==='string'?value:'—'
const humanStatus=(status:string|undefined,en:boolean)=>({queued:en?'Waiting':'대기 중',running:en?'Working':'작업 중',completed:en?'Finished':'완료',failed:en?'Failed':'실패',cancelled:en?'Stopped':'중단됨',needs_review:en?'Review needed':'검토 필요',interrupted:en?'Interrupted':'중단됨'} as Record<string,string>)[status||'']||(en?'Checking…':'확인 중…')
const humanRole=(role:string|undefined,en:boolean)=>({general:en?'General':'일반',inspection:en?'Research':'조사',implementation:en?'Code changes':'코드 수정',verification:en?'Verification':'검증'} as Record<string,string>)[role||'']||role||''
function workerTask(job:Job|undefined,step:Step){
 const raw=job?.input?.prompt
 const marker='\nTask '+step.id+': '
 const start=raw?.indexOf(marker)??-1
 return start>=0?raw!.slice(start+marker.length).split('\nReturn actual evidence')[0].trim():step.id
}
const stateClass=(state?:string)=>state==='completed'?'border-emerald-200 bg-emerald-50 text-emerald-700':
 state==='failed'||state==='cancelled'?'border-rose-200 bg-rose-50 text-rose-700':
 state==='running'?'border-[var(--color-brand)] bg-[var(--color-brand-soft)] text-[var(--color-brand-hover)]':
 state==='needs_review'?'border-amber-200 bg-amber-50 text-amber-800':
 'border-divider bg-[var(--color-canvas)] text-muted'
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
  <div className="flex flex-wrap items-center gap-2 text-[10px] text-muted">
   <span className="rounded-md bg-[var(--color-canvas)] px-2 py-0.5">{runtime}</span>
   <span>{en?'External receipt':'외부 실행 증거'}: {compact(job.result?.externalRunId)}</span>
   {job.input?.resumeFromJobId&&<span>{en?'Continued from':'이어받은 작업'}: {job.input.resumeFromJobId}</span>}
   {parent&&<a href={'#/chat?session='+encodeURIComponent(parent)} className="underline">{en?'Parent conversation':'원본 대화'}</a>}
  </div>
  {finished&&<button type="button" aria-expanded={open} onClick={()=>setOpen(v=>!v)}
   className="mt-2 inline-flex items-center gap-1 rounded-lg border border-divider px-2.5 py-1.5 text-[11px] text-[var(--color-muted)] hover:bg-[var(--color-canvas)]">
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
   <input className="min-w-40 flex-1 rounded-lg border border-divider px-3 py-2 text-[11px]"
    value={prompt} onChange={e=>setPrompt(e.target.value)} maxLength={3000}
    aria-label={en?'Follow-up inspection instructions':'이어갈 조사 지시'}
    placeholder={en?'What should the same inspector check next?':'같은 에이전트가 이어서 조사할 내용'}/>
   <button type="submit" disabled={busy||!prompt.trim()} className="rounded-lg border border-[var(--color-brand)] px-3 py-2 text-[11px] font-semibold text-[var(--color-brand-hover)] disabled:opacity-50">{busy?(en?'Submitting':'전달 중'):(en?'Resume':'재개')}</button>
  </form>}
  {error&&<p role="alert" className="mt-2 break-words text-[11px] text-rose-700">{error}</p>}
  {result&&<p role="status" className="mt-2 flex flex-wrap items-center gap-1 text-[11px] text-emerald-700"><Check size={12}/>{en?'Child inspection queued':'후속 조사 예약 완료'}: {result.jobId}{result.sessionId&&<a href={'#/chat?session='+encodeURIComponent(result.sessionId)} className="underline">{en?'Open session':'대화 열기'}</a>}</p>}
 </div>
}
function WorkerStep({step,job,ids,onCreated,index=0,agentNames}:{step:Step;job?:Job;ids:Record<string,string>;onCreated:()=>void;index?:number;agentNames?:Record<string,string>}){
 const en=useLocale()==='en'
 const [showResult,setShowResult]=useState(false)
 const done=job?.status==='completed'
 const fileHash=job?.delegationPatchSha256
 const deps=(step.dependsOn||[])
 const receipt=job?.result
 return <div className="relative rounded-xl border border-divider bg-white p-3">
  <div className="flex min-w-0 flex-wrap items-center justify-between gap-2">
   <div className="flex min-w-0 items-center gap-2">
    <span className="grid size-8 shrink-0 place-items-center rounded-full bg-[var(--color-brand-soft)] text-[12px] font-bold text-[var(--color-brand)]">{index+1}</span>
    <div className="min-w-0">
     <strong className="block break-words text-[13px] leading-5">{workerTask(job,step)}</strong>
     <small className="text-[12px] text-[var(--color-muted)]">{humanRole(step.profile,en)}</small>
    </div>
   </div>
   <span className={'shrink-0 rounded-md border px-2.5 py-1 text-[12px] font-semibold '+stateClass(job?.status)}>{humanStatus(job?.status,en)}</span>
  </div>
  <p className="mt-2 break-words text-[13px] font-medium text-[var(--color-ink)]">
   {en?'Assigned AI':'담당 AI'}: {agentNames?.[step.agentId||'']||step.agentId||job?.input?.agentId||'—'}
   {step.remoteWorker&&<span className="ml-2 rounded bg-[var(--color-canvas)] px-2 py-0.5 text-[11px] font-normal">{en?'Remote':'원격'}</span>}
  </p>
  <div className="mt-2 flex flex-wrap items-center gap-1.5 text-[12px] text-muted">
   {deps.length?<>{en?'After':'선행 단계'}: {deps.map((dep,index)=><span key={dep} className="inline-flex items-center gap-1 rounded bg-[var(--color-canvas)] px-1.5 py-1">
     {index>0?', ':''}{dep}<span className={'size-1.5 rounded-full '+(Object.keys(ids).includes(dep)&&ids[dep]==='completed'?'bg-emerald-500':'bg-amber-400')}/>
    </span>)}</>:<span>{en?'No prerequisites':'선행 조건 없음'}</span>}
  </div>
  {job?.error&&<p role="alert" className="mt-2 break-words text-[11px] text-rose-700">{job.error}</p>}
  {fileHash&&<p className="mt-2 break-all text-[10px] text-[var(--color-muted)]">{en?'Frozen patch SHA256':'고정된 패치 SHA256'}: {fileHash}</p>}
  {job?.input?.externalRuntime&&<ExternalContinuation job={job} onCreated={onCreated}/>}
  {job?.result&&<button type="button" aria-expanded={showResult} onClick={()=>setShowResult(v=>!v)}
   className="mt-2 inline-flex min-h-9 items-center gap-1.5 rounded-lg px-2 text-[12px] font-semibold text-[var(--color-brand)] hover:bg-[var(--color-brand-soft)] focus-visible:outline-2 focus-visible:outline-offset-2">
   <ShieldCheck size={16}/>{en?'Technical receipt':'작업 영수증(기술 정보)'} {showResult?'▴':'▾'}
  </button>}
  {job?.input?.sessionId&&<a href={'#/chat?session='+encodeURIComponent(job.input.sessionId)}
   className="ml-2 mt-2 inline-flex min-h-9 items-center gap-1.5 rounded-lg border border-divider px-3 py-1 text-[12px] font-medium text-[var(--color-muted)] hover:bg-[var(--color-canvas)] focus-visible:outline-2 focus-visible:outline-offset-2">
   <MessageSquare size={16}/>{en?'Open agent conversation':'담당 AI 대화 보기'}
  </a>}
  {showResult&&job?.result&&<pre aria-label={en?'Raw native job receipt':'원본 작업 영수증'} className="mt-2 max-h-60 overflow-auto whitespace-pre-wrap break-all rounded-lg bg-[var(--color-canvas)] p-3 text-[12px] leading-5">{JSON.stringify(job.result,null,2)}</pre>}
  {job&&<details className="mt-2 text-[11px] text-[var(--color-muted)]"><summary className="cursor-pointer py-1 focus-visible:outline-2">{en?'Technical job ID':'기술 정보'}</summary><code className="break-all text-[11px]">{job.id}</code></details>}
  {done&&<span className="sr-only">{en?'Completed':'완료'}</span>}
 </div>
}
function TeamCard({team,summary,onCreated,initiallyOpen=false,agentNames}:{team:Job;summary:Map<string,Job>;onCreated:()=>void;initiallyOpen?:boolean;agentNames?:Record<string,string>}){
 const en=useLocale()==='en'
 const [open,setOpen]=useState(initiallyOpen)
 const [showAllReports,setShowAllReports]=useState(false)
 const details=useRemote<TeamSteps>(open?'/api/jobs/'+encodeURIComponent(team.id)+'/team-steps':null,
  {teamId:team.id,jobs:[],missing:[]})
 const plan=team.input?.plan||[]
 const fresh=open&&!details.loading&&!details.error&&details.value.teamId===team.id
 const records=fresh?new Map(details.value.jobs.map(j=>[j.id,j])):summary
 const statuses=Object.fromEntries(plan.map(step=>[step.id,records.get(step.jobId)?.status||'unknown']))
 const complete=plan.filter(step=>records.get(step.jobId)?.status==='completed').length
 const unknown=plan.filter(step=>!records.has(step.jobId)).length
 const review=classifyTeamReview(team)
 const reports=Array.isArray(team.result?.nodes)?team.result.nodes.filter(
  (node):node is Record<string,unknown>=>Boolean(node)&&typeof node==='object'&&!Array.isArray(node)
 ).filter(node=>typeof node.answer==='string'&&node.answer.length>0):[]
 useEffect(()=>{
  if(!open||!['queued','running'].includes(team.status||''))return
  const timer=setInterval(()=>{if(!document.hidden)void details.reload()},5000)
  return()=>clearInterval(timer)
 },[open,team.status,details.reload])
 return <div className="rounded-xl border border-divider bg-[var(--color-surface)] p-4">
  <button type="button" aria-expanded={open} onClick={()=>setOpen(v=>!v)} className="flex w-full min-w-0 justify-between gap-2 text-left">
   <span className="min-w-0"><strong className="block break-words text-[14px] leading-6">{team.input?.goal||team.id}</strong>
    <span className="mt-1 block text-[12px] text-muted">{en?'Steps finished':'완료된 단계'} {complete}/{plan.length}
     {unknown>0&&<> · {en?'Checking':'확인 중'} {unknown}</>}</span></span>
   <span className={'shrink-0 self-start rounded-lg border px-3 py-1.5 text-[12px] font-semibold '+stateClass(team.status)}>{humanStatus(team.status,en)}</span>
  </button>
  <progress className="mt-3 block h-2 w-full accent-[var(--color-brand)]" value={complete} max={Math.max(1,plan.length)}
   aria-label={en?'Team steps completed':'완료된 협업 작업 단계'}/>
  {reports.length>0&&<section className="mt-4 rounded-xl border border-[#DFD5C6] bg-white p-4" aria-label={en?'Verified worker answers':'실제 AI 작업 결과'}>
   <div className="flex flex-wrap items-center justify-between gap-2">
    <h4 className="text-[14px] font-bold text-[var(--color-ink)]">{en?'What the agents found':'AI 작업 결과'}</h4>
    <span className="text-[12px] text-muted">{reports.length}{en?' reports':'개 결과'}</span>
   </div>
   <p className="mt-1 text-[12px] leading-5 text-[var(--color-muted)]">{en
    ?'These are saved worker answers; the code changes may still require your approval.'
    :'저장된 AI 답변입니다. 코드 수정 사항은 별도 승인·통합이 필요할 수 있습니다.'}</p>
   <div className="mt-3 space-y-3">
    {(showAllReports?reports:reports.slice(0,2)).map((report,index)=><article key={String(report.jobId||index)} className="rounded-lg border border-[#EEE8DE] bg-[var(--color-surface)] p-3">
     <strong className="block text-[12px] text-[#7A624B]">{agentNames?.[String(report.agentId||'')]||String(report.agentId|| (en?'Agent':'AI'))}
      {report.stepId?' · '+String(report.stepId):''}</strong>
     <p className="mt-2 whitespace-pre-wrap break-words text-[13px] leading-6 text-[var(--color-ink)]">{String(report.answer)}</p>
    </article>)}
   </div>
   {reports.length>2&&<button type="button" onClick={()=>setShowAllReports(v=>!v)}
    className="mt-3 inline-flex min-h-9 items-center rounded-lg px-3 text-[12px] font-semibold text-[var(--color-brand)] hover:bg-[var(--color-brand-soft)] focus-visible:outline-2 focus-visible:outline-offset-2">
    {showAllReports?(en?'Show fewer':'간략히 보기'):(en?'Show all '+reports.length+' reports':'전체 '+reports.length+'개 결과 보기')}
   </button>}
  </section>}
  {review.kind==='integrated'&&<p role="status" className="mt-2 flex items-center gap-1 text-[11px] text-emerald-700">
   <Check size={12}/>{en?'Team patches integrated':'팀 패치 통합 완료'}
  </p>}
  {review.kind==='pending'&&<p role="status" className="mt-2 text-[11px] text-amber-800">
   {en?'Integration still requires owner review for':'검토·통합이 아직 끝나지 않은 패치 대상'} {review.count}{en?' patch jobs':'개'}
   {team.input?.sessionId&&<a href={'#/chat?session='+encodeURIComponent(team.input.sessionId)}
    className="ml-2 underline">{en?'Open owner conversation':'원본 대화 열기'}</a>}
  </p>}
  {review.kind==='gaps'&&<p role="alert" className="mt-2 text-[11px] text-rose-700">
   {en?'Worker verification gaps block integration':'검증 공백으로 통합 불가'}: {review.count}
  </p>}
  {review.kind==='review'&&<p role="status" className="mt-2 text-[11px] text-amber-800">
   {en?'Team needs owner review':'팀 실행 결과 확인 필요'}
   {typeof team.result?.reason==='string'&&<>: {team.result.reason}</>}
  </p>}
  {open&&<div className="mt-3 space-y-2">
   {details.loading&&<p role="status" className="text-[11px] text-muted">{en?'Loading durable Worker records…':'저장된 Worker 기록 조회 중…'}</p>}
   {details.error&&<p role="alert" className="text-[11px] text-rose-700">{en?'Detailed Worker status could not be loaded':'Worker 상세 기록 조회 실패'}: {details.error}
    <button type="button" onClick={()=>void details.reload()} className="ml-2 underline">{en?'Retry':'다시 조회'}</button></p>}
   {fresh&&details.value.missing.length>0&&<p role="alert" className="text-[11px] text-rose-700">{en?'Missing or mismatched Worker records':'누락되거나 소유권이 일치하지 않는 Worker 기록'}: {details.value.missing.length}</p>}
   {plan.map((step,index)=><WorkerStep key={step.id} index={index} agentNames={agentNames} step={step} job={records.get(step.jobId)} ids={statuses} onCreated={onCreated}/>)}
   <p className="mt-2 text-[10px] text-[var(--color-muted)]">{en?'States come from persisted Worker jobs; a completed step does not mean its patch was integrated.':'단계 상태는 저장된 Worker 기록 기준입니다. 완료된 단계가 패치 병합까지 뜻하지는 않습니다.'}</p>
   {team.error&&<p role="alert" className="text-[11px] text-rose-700">{team.error}</p>}
   <span className="block break-all font-mono text-[10px] text-[var(--color-muted)]">{team.id}</span>
  </div>}
 </div>
}
/** Reads existing team job plan and the child job journal. No new planner,
 * no second DAG state and no mutation of the engine's dependency graph. */
export function TeamWorkflowPanel({jobs,filterSessionId,agentNames}:{jobs?:Job[];filterSessionId?:string;agentNames?:Record<string,string>}){
 const en=useLocale()==='en'
 const [expanded,setExpanded]=useState(Boolean(filterSessionId))
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
 const refresh=()=>{if(jobs===undefined)void remote.reload()}
 const hasActiveTeam=parentTeams.length===0||parentTeams.some(team=>['queued','running'].includes(team.status||''))
 useEffect(()=>{
  if(!filterSessionId||jobs!==undefined||!hasActiveTeam)return
  const timer=setInterval(()=>{if(!document.hidden)void remote.reload()},5000)
  return()=>clearInterval(timer)
 },[filterSessionId,jobs,hasActiveTeam,remote.reload])
 if(!parentTeams.length&&!standaloneExternal.length&&!dependentJobs.length&&!remote.loading){
  return filterSessionId?<div className="mt-4 rounded-xl border border-dashed border-[#D7CCBD] bg-[var(--color-canvas)] p-5 text-center">
   <p className="text-[13px] font-semibold text-[var(--color-muted)]">{en?'No collaboration runs yet':'아직 실행한 협업 작업이 없습니다'}</p>
   <p className="mt-1 text-[12px] text-[var(--color-muted)]">{en?'Start a task above; its agent assignments and results will appear here.':'위에서 작업을 시작하면 AI별 진행 단계와 결과가 이곳에 나타납니다.'}</p>
  </div>:null
 }
 return <Panel className="mt-4 min-w-0 overflow-hidden">
  <button type="button" aria-expanded={expanded} onClick={()=>setExpanded(x=>!x)}
   className="flex w-full flex-wrap items-center justify-between gap-2 p-4 text-left hover:bg-[var(--color-canvas)]">
   <span className="flex items-center gap-2"><GitBranch size={17} className="text-[var(--color-brand)]"/><span><strong className="block text-[13px]">{filterSessionId?(en?'Collaboration history':'협업 작업 기록'):(en?'Team dependency graph & external agents':'팀 작업 의존성·외부 에이전트')}</strong>
     <small className="text-[11px] text-[var(--color-muted)]">{filterSessionId?(en?'Progress, responsible agents and verified results':'진행 단계 · 담당 AI · 실제 작업 결과'):(en?'Recorded native steps, dependencies, actual worker outcomes.':'네이티브 작업 단계·선행 조건·실제 워커 결과를 연결합니다.')}</small></span></span>
   {expanded?<ChevronDown size={15}/>:<ChevronRight size={15}/>}
  </button>
  {expanded&&<div className="space-y-3 border-t border-divider p-4">
   {jobs===undefined&&<button type="button" onClick={refresh} className="text-[11px] text-muted underline">{en?'Refresh native jobs':'작업 기록 새로고침'}</button>}
   {parentTeams.map((team,index)=><TeamCard key={team.id} team={team} summary={jobMap} onCreated={refresh} agentNames={agentNames} initiallyOpen={Boolean(filterSessionId)&&index===0}/>)}
   {dependentJobs.length>0&&<section className="rounded-xl border border-divider bg-[var(--color-surface)] p-3">
    <h3 className="mb-2 flex items-center gap-2 text-[12px] font-bold"><GitBranch size={14}/>{en?'Dependent delegated jobs':'의존성 있는 개별 위임 작업'}</h3>
    <p className="mb-2 text-[10px] text-[var(--color-muted)]">{en?'The arrows below represent durable sibling job dependencies, not inferred order.':'개별 위임 작업의 저장된 선행 작업 ID를 그대로 표시합니다. 화면 순서로 의존성을 추정하지 않습니다.'}</p>
    <div className="space-y-2">{dependentJobs.map(job=><WorkerStep key={job.id} job={job} ids={independentStates} onCreated={refresh}
      step={{id:job.input?.teamStepId||job.id.slice(0,8),jobId:job.id,agentId:job.input?.agentId,
       profile:job.input?.delegationProfile,dependsOn:job.input?.dependsOnJobIds}}/>)}</div>
   </section>}
   {standaloneExternal.length>0&&<section className="rounded-xl border border-divider bg-[var(--color-surface)] p-3">
    <h3 className="mb-2 flex items-center gap-2 text-[12px] font-bold"><Layers3 size={14}/>{en?'Standalone external inspections':'독립 외부 조사 기록'}</h3>
    <div className="space-y-2">{standaloneExternal.map(job=><div key={job.id} className="rounded-lg border border-[var(--color-divider)] bg-white p-3">
     <div className="flex flex-wrap justify-between gap-2 text-[11px]"><strong className="min-w-0 break-words">{job.input?.prompt||job.id}</strong>
      <span className={'shrink-0 rounded border px-2 py-0.5 '+stateClass(job.status)}>{job.status}</span></div>
     <ExternalContinuation job={job} onCreated={refresh}/>
     {job.error&&<p className="mt-1 text-[11px] text-rose-700">{job.error}</p>}
    </div>)}</div>
   </section>}
  </div>}
 </Panel>
}
