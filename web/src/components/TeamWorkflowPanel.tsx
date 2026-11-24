import {useEffect,useMemo,useRef,useState} from 'react'
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
  teamJobId?:string;teamStepId?:string;parentRunId?:string;teamTaskId?:string;delegation?:boolean;delegationProfile?:string;externalRuntime?:string|null;bridgeRuntime?:string;
  dependsOnJobIds?:string[];resumeFromJobId?:string|null;remoteWorker?:boolean;
  maxParallel?:number;plan?:Step[];tasks?:NativeTask[];childJobIds?:string[];
 }
}
type NativeTask={id:string;prompt:string;agentId?:string;dependsOn?:string[];mode?:string;runtime?:string}
type Worktree={id:string;status:string;mergeStatus:string;path:string;baseCommit:string}
type Step={id:string;worktree?:Worktree|null;jobId:string;agentId?:string;profile?:string;dependsOn?:string[];remoteWorker?:boolean;prompt?:string;runtime?:string;recordState?:string;status?:string;dependencySatisfied?:boolean;evidenceGrade?:string;verified?:boolean}
type ProjectedTask={taskId:string;worktree?:Worktree|null;childJobId:string;prompt?:string;agentId?:string;dependsOn?:string[];mode?:string;runtime?:string;status?:string|null;recordState:string;dependencySatisfied:boolean;evidenceGrade?:string|null;verified:boolean;answerExcerpt?:string|null}
type TeamSteps={teamId:string;jobs:Job[];missing:string[];tasks?:ProjectedTask[];status?:string}
const nativeStep=(task:NativeTask,teamId:string):Step=>({id:task.id,jobId:'team-child:'+teamId+':'+task.id,prompt:task.prompt,agentId:task.agentId,dependsOn:task.dependsOn,profile:task.mode,runtime:task.runtime,remoteWorker:task.runtime==='remote'})
const projectedStep=(task:ProjectedTask):Step=>({id:task.taskId,worktree:task.worktree,jobId:task.childJobId,prompt:task.prompt,agentId:task.agentId,dependsOn:task.dependsOn,profile:task.mode,runtime:task.runtime,remoteWorker:task.runtime==='remote',recordState:task.recordState,status:task.status||undefined,dependencySatisfied:task.dependencySatisfied,evidenceGrade:task.evidenceGrade||undefined,verified:task.verified})
const externalJob=(job?:Job)=>Boolean(job?.input?.externalRuntime||(job?.input?.bridgeRuntime&&job.input.bridgeRuntime!=='native'))
const compact=(value:unknown)=>typeof value==='string'?value:'—'
const humanStatus=(status:string|undefined,en:boolean)=>({not_started:en?'Not started':'실행 대기',queued:en?'Waiting':'대기 중',running:en?'Working':'작업 중',completed:en?'Finished':'완료',failed:en?'Failed':'실패',cancelled:en?'Stopped':'중단됨',needs_review:en?'Review needed':'검토 필요',interrupted:en?'Interrupted':'중단됨'} as Record<string,string>)[status||'']||(en?'Checking…':'확인 중…')
const humanRole=(role:string|undefined,en:boolean)=>({read:en?'Read-only':'읽기 전용',write:en?'Write':'수정 작업',general:en?'General':'일반',inspection:en?'Research':'조사',implementation:en?'Code changes':'코드 수정',verification:en?'Verification':'검증'} as Record<string,string>)[role||'']||role||''
function workerTask(job:Job|undefined,step:Step){
 if(step.prompt)return step.prompt
 const raw=job?.input?.prompt
 const marker='\nTask '+step.id+': '
 const start=raw?.indexOf(marker)??-1
 return start>=0?raw!.slice(start+marker.length).split('\nReturn actual evidence')[0].trim():step.id
}
const stateClass=(state?:string)=>state==='completed'?'border-success/20 bg-success-soft text-success':
 state==='failed'||state==='cancelled'?'border-danger/20 bg-danger-soft text-danger':
 state==='running'?'border-brand bg-brand-soft text-brand-hover':
 state==='needs_review'?'border-warning/20 bg-warning-soft text-warning':
 'border-divider bg-canvas text-muted'
function ExternalReceipt({job}:{job:Job}){
 const en=useLocale()==='en'
 const runtime=job.input?.bridgeRuntime||job.input?.externalRuntime
 const parent=job.input?.parentSessionId
 return <div className="mt-2 min-w-0">
  <div className="flex flex-wrap items-center gap-2 text-[10px] text-muted">
   <span className="rounded-md bg-canvas px-2 py-0.5">{runtime}</span>
   <span>{en?'External receipt (not host-verified)':'외부 실행 증거 (호스트 미검증)'}: {compact(job.result?.externalRunId)}</span>
   {job.input?.resumeFromJobId&&<span>{en?'Continued from':'이어받은 작업'}: {job.input.resumeFromJobId}</span>}
   {parent&&<a href={'#/chat?session='+encodeURIComponent(parent)} className="underline">{en?'Parent conversation':'원본 대화'}</a>}
  </div>
 </div>
}
type WorktreePreview={teamId:string;taskId:string;worktreeId:string;baseCommit:string;diffSha256:string;patchBytes:number;files:string[];fileCount:number}
function WorktreeReview({teamId,step,teamStatus,onChanged}:{teamId:string;step:Step;teamStatus?:string;onChanged:()=>void}){
 const en=useLocale()==='en'
 const tree=step.worktree!
 const [preview,setPreview]=useState<WorktreePreview|null>(null)
 const [reviewed,setReviewed]=useState(false),[busy,setBusy]=useState(false),[error,setError]=useState(''),[applied,setApplied]=useState(false)
 const inFlight=useRef(false)
 const eligible=teamStatus==='completed'&&step.dependencySatisfied===true&&step.verified===true&&tree.status==='ready'&&tree.mergeStatus==='pending'&&!applied
 const worktreeEndpoint='/api/task-teams/'+encodeURIComponent(teamId)+'/worktrees/'+encodeURIComponent(step.id)
 const read=async()=>{
  if(inFlight.current||!eligible)return
  inFlight.current=true;setBusy(true);setError('');setPreview(null);setReviewed(false)
  try{
   const value=await get<WorktreePreview>(worktreeEndpoint+'/preview')
   if(value.teamId!==teamId||value.taskId!==step.id||value.worktreeId!==tree.id||value.baseCommit!==tree.baseCommit||!/^[a-f0-9]{64}$/i.test(value.diffSha256)||!Array.isArray(value.files))throw Error(en?'Worktree review identity mismatch':'작업 사본의 검토 정보가 일치하지 않습니다.')
   setPreview(value)
  }catch(e){setError((e as Error).message)}
  finally{inFlight.current=false;setBusy(false)}
 }
 const apply=async()=>{
  if(inFlight.current||!eligible||!preview||!reviewed||!window.confirm(en?'Apply the reviewed patch to the source index? This does not commit or push.':'검토한 패치를 원본 작업 폴더와 Git 스테이징 영역에 적용할까요? 커밋이나 푸시는 하지 않습니다.'))return
  inFlight.current=true;setBusy(true);setError('')
  try{
   const result=await post<{status:string;diffSha256:string;requiresOwnerCommit:boolean}>(worktreeEndpoint+'/promote',{confirm:true,expectedDiffSha256:preview.diffSha256})
   if(result.status!=='applied'||result.diffSha256!==preview.diffSha256)throw Error(en?'Check the source before retrying':'재시도하기 전에 원본 상태를 확인하세요.')
   setApplied(true);setPreview(null);onChanged()
  }catch(e){setError((e as Error).message);setPreview(null);setReviewed(false);onChanged()}
  finally{inFlight.current=false;setBusy(false)}
 }
 return <div className="mt-3 rounded-lg border border-divider p-3 text-[12px]">
  <strong>{en?'Isolated worktree':'격리된 작업 사본'}</strong>
  <p className="mt-1 break-all text-muted">{tree.path}</p>
  <p className="mt-1 text-muted">{en?'Apply status':'반영 상태'}: {applied||tree.mergeStatus==='applied'?(en?'Staged; owner commit required':'스테이징 완료 · 직접 커밋 필요'):tree.mergeStatus}</p>
  {eligible&&<button type="button" disabled={busy} onClick={()=>void read()} className="mt-2 rounded-lg border border-divider px-3 py-2 disabled:opacity-50">{en?'Review changed files':'변경 파일 확인'}</button>}
  {eligible&&preview&&<div className="mt-2 space-y-2">
   <p>{preview.fileCount}{en?' changed files':'개 변경 파일'} · {preview.patchBytes.toLocaleString()} bytes</p>
   <ul className="max-h-36 overflow-auto break-all text-muted">{preview.files.map((file,i)=><li key={i}>{file}</li>)}</ul>
   <p className="break-all text-muted">SHA-256: {preview.diffSha256}</p>
   <p className="text-muted">{en?'This is a file list, not the full diff. Inspect the actual diff in the worktree path above before approving.':'파일 목록이며 전체 변경 내용은 아닙니다. 위 작업 사본 경로에서 실제 변경 내용을 검토한 뒤 승인하세요.'}</p>
   <label className="flex items-start gap-2"><input type="checkbox" checked={reviewed} disabled={busy} onChange={e=>setReviewed(e.target.checked)}/>{en?'I reviewed the actual changes.':'실제 변경 내용을 검토했습니다.'}</label>
   <button type="button" disabled={busy||!reviewed} onClick={()=>void apply()} className="rounded-lg border border-brand px-3 py-2 font-semibold text-brand-hover disabled:opacity-50">{en?'Apply reviewed patch':'검토한 변경 반영'}</button>
  </div>}
  {error&&<p role="alert" className="mt-2 break-words text-danger">{error}</p>}
 </div>
}
function WorkerStep({step,job,ids,onCreated,index=0,agentNames,teamId,teamStatus}:{step:Step;job?:Job;ids:Record<string,string>;onCreated:()=>void;index?:number;agentNames?:Record<string,string>;teamId?:string;teamStatus?:string}){
 const en=useLocale()==='en'
 const [showResult,setShowResult]=useState(false)
 const done=step.dependencySatisfied===true
 const displayedStatus=step.recordState==='foreign_binding'?'needs_review':step.recordState==='not_started'?'not_started':(step.status||job?.status)==='completed'&&step.dependencySatisfied===false?'needs_review':step.status||job?.status
 const fileHash=job?.delegationPatchSha256
 const deps=(step.dependsOn||[])
 const receipt=job?.result
 return <div className="relative rounded-xl border border-divider bg-surface p-3">
  <div className="flex min-w-0 flex-wrap items-center justify-between gap-2">
   <div className="flex min-w-0 items-center gap-2">
    <span className="grid size-8 shrink-0 place-items-center rounded-full bg-brand-soft text-[12px] font-bold text-brand">{index+1}</span>
    <div className="min-w-0">
     <strong className="block break-words text-[13px] leading-5">{workerTask(job,step)}</strong>
     <small className="text-[12px] text-muted">{humanRole(step.profile,en)}</small>
    </div>
   </div>
   <span className={'shrink-0 rounded-md border px-2.5 py-1 text-[12px] font-semibold '+stateClass(displayedStatus)}>{humanStatus(displayedStatus,en)}</span>
  </div>
  <p className="mt-2 break-words text-[13px] font-medium text-ink">
   {en?'Assigned AI':'담당 AI'}: {agentNames?.[step.agentId||'']||step.agentId||job?.input?.agentId||'—'}
   {step.remoteWorker&&<span className="ml-2 rounded bg-canvas px-2 py-0.5 text-[11px] font-normal">{en?'Remote':'원격'}</span>}
  </p>
  <div className="mt-2 flex flex-wrap items-center gap-1.5 text-[12px] text-muted">
   {deps.length?<>{en?'After':'선행 단계'}: {deps.map((dep,index)=><span key={dep} className="inline-flex items-center gap-1 rounded bg-canvas px-1.5 py-1">
     {index>0?', ':''}{dep}<span className={'size-1.5 rounded-full '+(Object.keys(ids).includes(dep)&&ids[dep]==='completed'?'bg-success':'bg-warning')}/>
    </span>)}</>:<span>{en?'No prerequisites':'선행 조건 없음'}</span>}
  </div>
  {job?.error&&<p role="alert" className="mt-2 break-words text-[11px] text-danger">{job.error}</p>}
  {fileHash&&<p className="mt-2 break-all text-[10px] text-muted">{en?'Frozen patch SHA256':'고정된 패치 SHA256'}: {fileHash}</p>}
  {step.evidenceGrade&&<p className="mt-2 text-[11px] text-muted">{en?'Evidence':'검증 근거'}: {step.evidenceGrade==='host_verified'?(en?'Host verified':'호스트 검증 완료'):step.evidenceGrade==='native_observed'?(en?'Native observation (not independently verified)':'네이티브 관측 · 독립 검증 아님'):step.evidenceGrade==='external_unverified'?(en?'External report (unverified)':'외부 보고 · 미검증'):(en?'Requires review':'확인 필요')}</p>}
  {externalJob(job)&&job&&<ExternalReceipt job={job}/>}
  {job?.result&&<button type="button" aria-expanded={showResult} onClick={()=>setShowResult(v=>!v)}
   className="mt-2 inline-flex min-h-9 items-center gap-1.5 rounded-lg px-2 text-[12px] font-semibold text-brand hover:bg-brand-soft focus-visible:outline-2 focus-visible:outline-offset-2">
   <ShieldCheck size={16}/>{en?'Technical receipt':'작업 영수증(기술 정보)'} {showResult?'▴':'▾'}
  </button>}
  {job?.input?.sessionId&&<a href={'#/chat?session='+encodeURIComponent(job.input.sessionId)}
   className="ml-2 mt-2 inline-flex min-h-9 items-center gap-1.5 rounded-lg border border-divider px-3 py-1 text-[12px] font-medium text-muted hover:bg-canvas focus-visible:outline-2 focus-visible:outline-offset-2">
   <MessageSquare size={16}/>{en?'Open agent conversation':'담당 AI 대화 보기'}
  </a>}
  {showResult&&job?.result&&<pre aria-label={en?'Raw native job receipt':'원본 작업 영수증'} className="mt-2 max-h-60 overflow-auto whitespace-pre-wrap break-all rounded-lg bg-canvas p-3 text-[12px] leading-5">{JSON.stringify(job.result,null,2)}</pre>}
  {job&&<details className="mt-2 text-[11px] text-muted"><summary className="cursor-pointer py-1 focus-visible:outline-2">{en?'Technical job ID':'기술 정보'}</summary><code className="break-all text-[11px]">{job.id}</code></details>}
  {teamId&&step.worktree&&<WorktreeReview key={step.worktree.id+':'+step.worktree.mergeStatus} teamId={teamId} step={step} teamStatus={teamStatus} onChanged={onCreated}/>}
  {done&&<span className="sr-only">{en?'Completed':'완료'}</span>}
 </div>
}
function TeamCard({team,summary,onCreated,initiallyOpen=false,agentNames}:{team:Job;summary:Map<string,Job>;onCreated:()=>void;initiallyOpen?:boolean;agentNames?:Record<string,string>}){
 const en=useLocale()==='en'
 const [open,setOpen]=useState(initiallyOpen)
 const [showAllReports,setShowAllReports]=useState(false)
 const [actionBusy,setActionBusy]=useState(false),[actionError,setActionError]=useState('')
 const actionLock=useRef(false)
 const [actionInfo,setActionInfo]=useState('')
 const stopTeam=async()=>{
  if(actionLock.current||!window.confirm(en?'Stop this Team and its active workers?':'협업 작업과 진행 중인 하위 작업을 중단하시겠습니까?'))return
  actionLock.current=true;setActionBusy(true);setActionError('');setActionInfo('')
  try{await post('/api/task-teams/'+encodeURIComponent(team.id)+'/cancel',{});await details.reload();onCreated()}
  catch(error){setActionError((error as Error).message)}
  finally{actionLock.current=false;setActionBusy(false)}
 }
 const retryTeam=async()=>{
  if(actionLock.current||!window.confirm(en?'Start a new execution of this team? Model usage and external actions may incur costs.':'기존 작업을 새 실행으로 다시 시작할까요? 모델 사용 비용이 발생할 수 있습니다.'))return
  actionLock.current=true;setActionBusy(true);setActionError('');setActionInfo('')
  try{
   const result=await post<{newTeamId:string;retryOf:string;freshRun:boolean}>('/api/task-teams/'+encodeURIComponent(team.id)+'/retry',{confirm:true})
   if(result.retryOf!==team.id||!result.freshRun||!result.newTeamId)throw Error(en?'Check the job list before retrying':'다시 실행하기 전에 작업 목록을 확인하세요.')
   setActionInfo((en?'New team queued: ':'새 협업 작업 접수: ')+result.newTeamId);onCreated()
  }catch(error){setActionError((error as Error).message)}
  finally{actionLock.current=false;setActionBusy(false)}
 }
 const details=useRemote<TeamSteps>('/api/jobs/'+encodeURIComponent(team.id)+'/team-steps',
  {teamId:team.id,jobs:[],missing:[]})
 // Current Rust Team.tasks owns the DAG. Legacy plan is only a fallback
 // for displaying historical descriptions, never proof of completion.
 const fresh=!details.loading&&!details.error&&details.value.teamId===team.id&&Array.isArray(details.value.tasks)
 const projected=fresh?details.value.tasks!:undefined
 const rows=(team.result?.tasks as Array<Record<string,unknown>>|undefined)||[]
 const plan:Step[]=projected?projected.map(projectedStep):Array.isArray(team.input?.tasks)
  ?team.input.tasks.map(task=>nativeStep(task,team.id))
  :Array.isArray(team.input?.plan)?team.input.plan:rows.map(row=>({id:String(row.taskId||''),jobId:String(row.childJobId||''),agentId:row.agentId?String(row.agentId):undefined}))
 const records=fresh?new Map(details.value.jobs.map(j=>[j.id,j])):new Map<string,Job>()
 const statuses=Object.fromEntries(plan.map(step=>[step.id,step.dependencySatisfied===true?'completed':(step.status||records.get(step.jobId)?.status)==='completed'?'needs_review':step.status||records.get(step.jobId)?.status||'unknown']))
 const complete=plan.filter(step=>step.dependencySatisfied===true).length
 const unknown=fresh?plan.filter(step=>step.recordState==='foreign_binding').length:plan.length
 const currentStatus=fresh?details.value.status||team.status:team.status
 const review=classifyTeamReview({...team,status:currentStatus})
 const nodes=projected?projected.filter(row=>row.recordState==='present').map(row=>({jobId:row.childJobId,agentId:row.agentId,answer:row.answerExcerpt,evidenceGrade:row.evidenceGrade})):
  Array.isArray(team.input?.tasks)?[]:Array.isArray(team.result?.nodes)?team.result.nodes:rows.map(row=>({agentId:row.agentId,answer:row.answerExcerpt}))
 const reports=nodes.filter(
  (node):node is Record<string,unknown>=>Boolean(node)&&typeof node==='object'&&!Array.isArray(node)
 ).filter(node=>typeof node.answer==='string'&&String(node.answer).length>0)
 useEffect(()=>{
  if(!open||!['queued','running'].includes(team.status||''))return
  const timer=setInterval(()=>{if(!document.hidden)void details.reload()},5000)
  return()=>clearInterval(timer)
 },[open,team.status,details.reload])
 return <div className="rounded-xl border border-divider bg-surface p-4">
  <button type="button" aria-expanded={open} onClick={()=>setOpen(v=>!v)} className="flex w-full min-w-0 justify-between gap-2 text-left">
   <span className="min-w-0"><strong className="block break-words text-[14px] leading-6">{team.input?.goal||team.id}</strong>
    <span className="mt-1 block text-[12px] text-muted">{en?'Steps satisfying execution conditions':'실행 조건 충족 단계'} {fresh?complete+' / '+plan.length:(en?'Checking…':'확인 중…')}
     {unknown>0&&<> · {fresh?(en?'Invalid binding':'기록 불일치'):(en?'Checking':'확인 중')} {unknown}</>}</span></span>
   <span className={'shrink-0 self-start rounded-lg border px-3 py-1.5 text-[12px] font-semibold '+stateClass(currentStatus)}>{humanStatus(currentStatus,en)}</span>
  </button>
  <progress className="mt-3 block h-2 w-full accent-[var(--color-brand)]" value={fresh?complete:undefined} max={Math.max(1,plan.length)}
   aria-label={en?'Team steps completed':'완료된 협업 작업 단계'}/>
  {fresh&&['queued','running'].includes(currentStatus||'')&&<button type="button" disabled={actionBusy} onClick={()=>void stopTeam()} className="mt-3 rounded-lg border border-divider px-3 py-2 text-[11px] text-danger disabled:opacity-50">{en?'Stop team and workers':'협업 및 하위 작업 중단'}</button>}
  {fresh&&['failed','interrupted','needs_review'].includes(currentStatus||'')&&<button type="button" disabled={actionBusy} onClick={()=>void retryTeam()} className="mt-3 rounded-lg border border-divider px-3 py-2 text-[11px] text-brand-hover disabled:opacity-50">{en?'Retry as a new task':'새 작업으로 다시 실행'}</button>}
  {actionInfo&&<p role="status" className="mt-2 break-all text-[11px] text-success">{actionInfo}</p>}
  {actionError&&<p role="alert" className="mt-2 text-[11px] text-danger">{actionError}</p>}
  {reports.length>0&&<section className="mt-4 rounded-xl border border-success-soft bg-surface p-4" aria-label={en?'Recorded worker answers':'저장된 AI 작업 결과'}>
   <div className="flex flex-wrap items-center justify-between gap-2">
    <h4 className="text-[14px] font-bold text-ink">{en?'What the agents found':'AI 작업 결과'}</h4>
    <span className="text-[12px] text-muted">{reports.length}{en?' reports':'개 결과'}</span>
   </div>
   <p className="mt-1 text-[12px] leading-5 text-muted">{en
    ?'These are saved worker answers; the code changes may still require your approval.'
    :'저장된 AI 답변입니다. 코드 수정 사항은 별도 승인·통합이 필요할 수 있습니다.'}</p>
   <div className="mt-3 space-y-3">
    {(showAllReports?reports:reports.slice(0,2)).map((report,index)=><article key={String(report.jobId||index)} className="rounded-lg border border-brand-soft bg-surface p-3">
     <strong className="block text-[12px] text-muted">{agentNames?.[String(report.agentId||'')]||String(report.agentId|| (en?'Agent':'AI'))}
      {report.stepId?' · '+String(report.stepId):''}</strong>
     <p className="mt-2 whitespace-pre-wrap break-words text-[13px] leading-6 text-ink">{String(report.answer)}</p>
    </article>)}
   </div>
   {reports.length>2&&<button type="button" onClick={()=>setShowAllReports(v=>!v)}
    className="mt-3 inline-flex min-h-9 items-center rounded-lg px-3 text-[12px] font-semibold text-brand hover:bg-brand-soft focus-visible:outline-2 focus-visible:outline-offset-2">
    {showAllReports?(en?'Show fewer':'간략히 보기'):(en?'Show all '+reports.length+' reports':'전체 '+reports.length+'개 결과 보기')}
   </button>}
  </section>}
  {review.kind==='integrated'&&<p role="status" className="mt-2 flex items-center gap-1 text-[11px] text-success">
   <Check size={12}/>{en?'Team patches integrated':'팀 패치 통합 완료'}
  </p>}
  {review.kind==='pending'&&<p role="status" className="mt-2 text-[11px] text-warning">
   {en?'Integration still requires owner review for':'검토·통합이 아직 끝나지 않은 패치 대상'} {review.count}{en?' patch jobs':'개'}
   {team.input?.sessionId&&<a href={'#/chat?session='+encodeURIComponent(team.input.sessionId)}
    className="ml-2 underline">{en?'Open owner conversation':'원본 대화 열기'}</a>}
  </p>}
  {review.kind==='gaps'&&<p role="alert" className="mt-2 text-[11px] text-danger">
   {en?'Worker verification gaps block integration':'검증 공백으로 통합 불가'}: {review.count}
  </p>}
  {review.kind==='review'&&<p role="status" className="mt-2 text-[11px] text-warning">
   {en?'Team needs owner review':'팀 실행 결과 확인 필요'}
   {typeof team.result?.reason==='string'&&<>: {team.result.reason}</>}
  </p>}
  {open&&<div className="mt-3 space-y-2">
   {details.loading&&<p role="status" className="text-[11px] text-muted">{en?'Loading durable Worker records…':'저장된 Worker 기록 조회 중…'}</p>}
   {details.error&&<p role="alert" className="text-[11px] text-danger">{en?'Detailed Worker status could not be loaded':'Worker 상세 기록 조회 실패'}: {details.error}
    <button type="button" onClick={()=>void details.reload()} className="ml-2 underline">{en?'Retry':'다시 조회'}</button></p>}
   {fresh&&details.value.missing.length>0&&<p role="alert" className="text-[11px] text-danger">{en?'Missing or mismatched Worker records':'누락되거나 소유권이 일치하지 않는 Worker 기록'}: {details.value.missing.length}</p>}
   {plan.map((step,index)=><WorkerStep key={step.id} index={index} agentNames={agentNames} step={step} job={records.get(step.jobId)} ids={statuses} onCreated={()=>{void details.reload();onCreated()}} teamId={team.id} teamStatus={currentStatus}/>)}
   <p className="mt-2 text-[10px] text-muted">{en?'States come from persisted Worker jobs; a completed step does not mean its patch was integrated.':'단계 상태는 저장된 Worker 기록 기준입니다. 완료된 단계가 패치 병합까지 뜻하지는 않습니다.'}</p>
   {team.error&&<p role="alert" className="text-[11px] text-danger">{team.error}</p>}
   <span className="block break-all font-mono text-[10px] text-muted">{team.id}</span>
  </div>}
 </div>
}
/** Reads existing team job plan and the child job journal. No new planner,
 * no second DAG state and no mutation of the engine's dependency graph. */
export function TeamWorkflowPanel({jobs,filterSessionId,agentNames,visibleIds}:{jobs?:Job[];filterSessionId?:string;agentNames?:Record<string,string>;visibleIds?:string[]}){
 const en=useLocale()==='en'
 const [expanded,setExpanded]=useState(Boolean(filterSessionId))
 // Room panels own the job subscription when no parent supplies one.
 // Native job record events refresh worker status without a manual reload.
 const endpoint=jobs===undefined?(filterSessionId
  ?'/api/jobs?sessionId='+encodeURIComponent(filterSessionId)+'&limit=128'
  :'/api/jobs'):null
 const remote=useRemote<Job[]>(endpoint,[])
 const rows=jobs??remote.value
 const parentTeams=useMemo(()=>rows.filter(j=>j.kind==='team'&&(!visibleIds||visibleIds.includes(j.id))&&(Array.isArray(j.input?.tasks)||Array.isArray(j.input?.plan))
  &&(!filterSessionId||j.input?.sessionId===filterSessionId)).sort((a,b)=>(b.queuedAt||'').localeCompare(a.queuedAt||'')).slice(0,8),[rows,filterSessionId,visibleIds])
 const standaloneExternal=useMemo(()=>rows.filter(j=>(!visibleIds||visibleIds.includes(j.id))&&externalJob(j)&&!j.input?.teamJobId&&!j.input?.parentRunId
  &&(!filterSessionId||j.input?.parentSessionId===filterSessionId)).slice(0,10),[rows,filterSessionId,visibleIds])
 const dependentJobs=useMemo(()=>rows.filter(j=>(!visibleIds||visibleIds.includes(j.id))&&j.input?.delegation===true&&!j.input?.teamJobId&&!j.input?.parentRunId
  &&(j.input?.dependsOnJobIds?.length||0)>0
  &&(!filterSessionId||j.input?.parentSessionId===filterSessionId)).slice(0,18),[rows,filterSessionId,visibleIds])
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
  return filterSessionId?<div className="mt-4 rounded-xl border border-dashed border-success-soft bg-canvas p-5 text-center">
   <p className="text-[13px] font-semibold text-muted">{en?'No collaboration runs yet':'아직 실행한 협업 작업이 없습니다'}</p>
   <p className="mt-1 text-[12px] text-muted">{en?'Start a task above; its agent assignments and results will appear here.':'위에서 작업을 시작하면 AI별 진행 단계와 결과가 이곳에 나타납니다.'}</p>
  </div>:null
 }
 return <Panel className="mt-4 min-w-0 overflow-hidden">
  <button type="button" aria-expanded={expanded} onClick={()=>setExpanded(x=>!x)}
   className="flex w-full flex-wrap items-center justify-between gap-2 p-4 text-left hover:bg-canvas">
   <span className="flex items-center gap-2"><GitBranch size={17} className="text-brand"/><span><strong className="block text-[13px]">{filterSessionId?(en?'Collaboration history':'협업 작업 기록'):(en?'Team dependency graph & external agents':'팀 작업 의존성·외부 에이전트')}</strong>
     <small className="text-[11px] text-muted">{filterSessionId?(en?'Progress, responsible agents and verified results':'진행 단계 · 담당 AI · 실제 작업 결과'):(en?'Recorded native steps, dependencies, actual worker outcomes.':'네이티브 작업 단계·선행 조건·실제 워커 결과를 연결합니다.')}</small></span></span>
   {expanded?<ChevronDown size={15}/>:<ChevronRight size={15}/>}
  </button>
  {expanded&&<div className="space-y-3 border-t border-divider p-4">
   {jobs===undefined&&<button type="button" onClick={refresh} className="text-[11px] text-muted underline">{en?'Refresh native jobs':'작업 기록 새로고침'}</button>}
   {parentTeams.map((team,index)=><TeamCard key={team.id} team={team} summary={jobMap} onCreated={refresh} agentNames={agentNames} initiallyOpen={Boolean(filterSessionId)&&index===0}/>)}
   {dependentJobs.length>0&&<section className="rounded-xl border border-divider bg-surface p-3">
    <h3 className="mb-2 flex items-center gap-2 text-[12px] font-bold"><GitBranch size={14}/>{en?'Dependent delegated jobs':'의존성 있는 개별 위임 작업'}</h3>
    <p className="mb-2 text-[10px] text-muted">{en?'The arrows below represent durable sibling job dependencies, not inferred order.':'개별 위임 작업의 저장된 선행 작업 ID를 그대로 표시합니다. 화면 순서로 의존성을 추정하지 않습니다.'}</p>
    <div className="space-y-2">{dependentJobs.map(job=><WorkerStep key={job.id} job={job} ids={independentStates} onCreated={refresh}
      step={{id:job.input?.teamStepId||job.id.slice(0,8),jobId:job.id,agentId:job.input?.agentId,
       profile:job.input?.delegationProfile,dependsOn:job.input?.dependsOnJobIds}}/>)}</div>
   </section>}
   {standaloneExternal.length>0&&<section className="rounded-xl border border-divider bg-surface p-3">
    <h3 className="mb-2 flex items-center gap-2 text-[12px] font-bold"><Layers3 size={14}/>{en?'Standalone external inspections':'독립 외부 조사 기록'}</h3>
    <div className="space-y-2">{standaloneExternal.map(job=><div key={job.id} className="rounded-lg border border-divider bg-surface p-3">
     <div className="flex flex-wrap justify-between gap-2 text-[11px]"><strong className="min-w-0 break-words">{job.input?.prompt||job.id}</strong>
      <span className={'shrink-0 rounded border px-2 py-0.5 '+stateClass(job.status)}>{job.status}</span></div>
     <ExternalReceipt job={job}/>
     {job.error&&<p className="mt-1 text-[11px] text-danger">{job.error}</p>}
    </div>)}</div>
   </section>}
  </div>}
 </Panel>
}
