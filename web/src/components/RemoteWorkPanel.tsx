import {useEffect,useMemo,useState} from 'react'
import {Check,ChevronDown,ChevronRight,Cloud,FileDiff,LoaderCircle,RefreshCw,ShieldAlert} from 'lucide-react'
import {get,post,type Agent} from '../api'
import {useLocale} from '../i18n'
import {Button,SelectInput,TextArea} from './Primitives'
import {Panel,useRemote} from './UI'

type Worker={id:string;online:boolean;allowWrites:boolean;agents?:string[];agentCount:number;lastHeartbeatMs?:number}
type Task={id:string;agentId?:string;displayState?:string;status?:string;verified?:boolean;remoteTaskId?:string|null;artifactApprovalState?:string|null;error?:string}
type Team={id:string;status:string;verified?:boolean;resumeAvailable?:boolean;tasks:Task[];pendingRemoteArtifactTaskIds?:string[]}
type Activity={remoteConfigured:boolean;remoteWorkers:Worker[];teams:Team[];leadJobs:{id:string;status:string;error?:string}[]}
type Preview={teamId:string;remoteTaskId:string;status:string;patch:string;reviewDigest:string;files:{path:string;sha256:string}[]}
const initial:Activity={remoteConfigured:false,remoteWorkers:[],teams:[],leadJobs:[]}
const label=(status:string,en:boolean)=>{
 const names:Record<string,[string,string]>={
  queued:['대기 중','Queued'],running:['실행 중','Running'],
  completed:['완료','Completed'],failed:['실패','Failed'],
  needs_review:['검토 필요','Needs review'],cancelled:['중단됨','Stopped'],
  approval_pending:['승인 대기','Approval pending'],offline:['오프라인','Offline'],
  waiting_predecessors:['선행 작업 대기','Waiting for dependencies'],
  file_review_pending:['파일 검토 필요','Patch review needed'],
  verified:['검증 완료','Verified']
 }
 return (names[status]||[status,status])[en?1:0]
}
function RemotePatchReview({teamId,taskId,onChanged}:{
 teamId:string;taskId:string;onChanged:()=>void
}){
 const en=useLocale()==='en'
 const [open,setOpen]=useState(false)
 const [preview,setPreview]=useState<Preview|null>(null)
 const [loading,setLoading]=useState(false)
 const [busy,setBusy]=useState(false)
 const [error,setError]=useState('')
 const root='/api/activity/teams/'+encodeURIComponent(teamId)+'/artifacts/'+encodeURIComponent(taskId)
 const inspect=async()=>{
  setLoading(true);setError('')
  try{
   const data=await get<Preview>(root)
   if(data.teamId!==teamId||data.remoteTaskId!==taskId||!data.reviewDigest)
    throw Error(en?'The patch does not belong to this Team task.':'다른 팀 작업의 패치이므로 열 수 없습니다.')
   setPreview(data)
  }catch(e){setError((e as Error).message)}
  finally{setLoading(false)}
 }
 const decide=async(accept:boolean)=>{
  if(busy||!preview)return
  if(!window.confirm(accept?(en?'Apply the reviewed patch to your local workspace?':'검토한 파일 변경을 로컬 작업공간에 적용하시겠습니까?'):
   (en?'Reject these remote changes?':'이 원격 파일 변경을 거부하시겠습니까?')))return
  setBusy(true);setError('')
  try{
   await post(root+(accept?'/apply':'/reject'),
    accept?{approved:true,reviewDigest:preview.reviewDigest}:{rejected:true})
   setPreview(null);setOpen(false);onChanged()
  }catch(e){setError((e as Error).message)}
  finally{setBusy(false)}
 }
 return <div className="mt-2 rounded-lg border border-amber-200 bg-amber-50 p-3">
  <Button type="button" variant="bare" onClick={()=>{if(!open&&!preview)void inspect();setOpen(x=>!x)}}
   aria-expanded={open} className="flex w-full items-center justify-between text-xs font-semibold text-amber-900">
   <span className="inline-flex items-center gap-2"><FileDiff size={15}/>
    {en?'Remote changes require your review':'원격 파일 변경 검토 필요'}</span>
   {open?<ChevronDown size={15}/>:<ChevronRight size={15}/>}
  </Button>
  {open&&<div className="mt-3">
   {loading&&<p role="status" className="text-xs text-muted"><LoaderCircle size={13} className="inline animate-spin"/> {en?'Loading patch':'패치 불러오는 중'}</p>}
   {preview&&<>
    <p className="mb-2 text-xs text-amber-900">{preview.files.length}{en?' changed files':'개 파일 변경'} · SHA-256 {preview.reviewDigest.slice(0,16)}…</p>
    <pre className="max-h-72 overflow-auto whitespace-pre-wrap break-all rounded-lg border border-divider bg-surface p-3 text-xs text-ink">{preview.patch}</pre>
    <div className="mt-2 flex flex-wrap gap-2">
     <Button variant="primary" type="button" disabled={busy} onClick={()=>void decide(true)}><Check size={14}/>{en?'Apply reviewed patch':'검토 후 적용'}</Button>
     <Button variant="secondary" type="button" disabled={busy} onClick={()=>void decide(false)}>{en?'Reject':'거부'}</Button>
    </div>
   </>}
   {error&&<p role="alert" className="mt-2 text-xs text-rose-700">{error}</p>}
  </div>}
 </div>
}

/** Sub2 native remote-worker control surface. Its device authentication,
 * task execution, patch verification and approvals remain in the Rust engine. */
export function RemoteWorkPanel(){
 const en=useLocale()==='en'
 const remote=useRemote<Activity>('/api/activity/teams',initial)
 const agents=useRemote<Agent[]>('/api/agents',[])
 const [open,setOpen]=useState(false)
 const [workerId,setWorkerId]=useState('')
 const [leadId,setLeadId]=useState('')
 const [workersSelected,setWorkersSelected]=useState<string[]>([])
 const [goal,setGoal]=useState('')
 const [busy,setBusy]=useState(false)
 const [error,setError]=useState('')
 const [message,setMessage]=useState('')
 const chosen=remote.value.remoteWorkers.find(item=>item.id===workerId)
 const leadCandidates=agents.value.filter(agent=>agent.enabled!==false&&agent.providerId&&agent.model)
 const selectedLead=leadCandidates.some(agent=>agent.id===leadId)?leadId:leadCandidates[0]?.id||''
 const actualSelected=workersSelected.filter(id=>chosen?.agents?.includes(id)).slice(0,8)
 const availableIds=chosen?.agents||[]
 useEffect(()=>{
  if(!open)return
  const timer=window.setInterval(()=>{if(document.visibilityState==='visible')void remote.reload()},5000)
  return()=>window.clearInterval(timer)
 },[open,remote.reload])
 const selectWorker=(id:string)=>{
  setWorkerId(id);setWorkersSelected([]);setMessage('');setError('')
 }
 const toggle=(id:string,checked:boolean)=>setWorkersSelected(old=>
  checked?[...new Set([...old,id])].slice(0,8):old.filter(item=>item!==id))
 const act=async(path:string,body:unknown,success:string)=>{
  setBusy(true);setError('');setMessage('')
  try{await post(path,body);setMessage(success);await remote.reload()}
  catch(e){setError((e as Error).message)}
  finally{setBusy(false)}
 }
 const launch=async()=>{
  if(!chosen?.online||!selectedLead||!actualSelected.length||!goal.trim()||busy)return
  if(!window.confirm(en?'Start a real native Lead/Worker task?':'네이티브 Lead·Worker 작업을 시작하시겠습니까?'))return
  setBusy(true);setError('');setMessage('')
  try{
   const result=await post<{status:string;jobId:string}>('/api/activity/teams/start',{
    prompt:goal.trim(),leadAgentId:selectedLead,workerAgentIds:actualSelected,remoteWorkerId:chosen.id
   })
   setGoal('')
   setMessage((en?'Native Lead queued: ':'실제 Lead 작업 예약: ')+result.jobId)
   await remote.reload()
  }catch(e){setError((e as Error).message)}
  finally{setBusy(false)}
 }
 const teams=remote.value.teams.filter(team=>(team.tasks||[]).some(task=>Boolean(task.remoteTaskId)||Boolean(task.agentId&&remote.value.remoteWorkers.some(w=>w.agents?.includes(task.agentId!)))))
 return <Panel className="mt-5 min-w-0 overflow-hidden">
  <Button variant="bare" type="button" onClick={()=>setOpen(v=>!v)}
   aria-expanded={open} className="flex w-full items-center justify-between gap-3 p-4 text-left">
   <span className="flex items-center gap-2 text-sm font-semibold text-ink"><Cloud size={17} className="text-brand"/>
    {en?'Native remote workers':'네이티브 원격 워커'}
    <span className="text-xs font-normal text-muted">({remote.value.remoteWorkers.length})</span>
   </span>
   {open?<ChevronDown size={16}/>:<ChevronRight size={16}/>}
  </Button>
  {open&&<div className="space-y-4 border-t border-divider p-4">
   <div className="flex flex-wrap items-center justify-between gap-2">
    <p className="text-xs text-muted">{en?'Uses the dedicated Rust worker control plane, not a parallel web executor.':'서브2 Rust 원격 실행기를 사용합니다. 별도 웹 실행 엔진은 만들지 않습니다.'}</p>
    <Button type="button" variant="secondary" size="sm" onClick={()=>void remote.reload()} disabled={remote.loading}><RefreshCw size={14}/>{en?'Refresh':'새로고침'}</Button>
   </div>
   {!remote.value.remoteConfigured&&<p role="status" className="rounded-lg border border-amber-200 bg-amber-50 p-3 text-xs text-amber-900">
    <ShieldAlert size={14} className="mr-1 inline"/>{en?'Configure the native remote control plane before using distributed workers.':'원격 실행을 사용하려면 Rust 분산 제어 환경 설정이 필요합니다.'}
   </p>}
   {remote.error&&<p role="alert" className="text-xs text-rose-700">{remote.error}</p>}
   <div className="grid gap-2 sm:grid-cols-2">
    {remote.value.remoteWorkers.map(worker=><div key={worker.id} className="rounded-lg border border-divider bg-surface p-3 text-xs">
     <div className="flex items-center justify-between gap-2">
      <strong className="break-all text-ink">{worker.id}</strong>
      <span className={worker.online?'text-emerald-700':'text-amber-700'}>{worker.online?(en?'Online':'온라인'):(en?'Offline':'오프라인')}</span>
     </div>
     <p className="mt-1 text-muted">{(worker.agents||[]).length}{en?' agents':'개 AI'} · {worker.allowWrites?(en?'Writes permitted':'쓰기 허용'):(en?'Read-only':'읽기 전용')}</p>
    </div>)}
   </div>
   {remote.value.remoteWorkers.length===0&&!remote.loading&&
    <p className="text-xs text-muted">{en?'No native remote worker has registered.':'연결된 네이티브 원격 워커가 없습니다.'}</p>}
   <section className="space-y-3 rounded-xl border border-divider bg-canvas p-4">
    <h3 className="text-sm font-bold text-ink">{en?'Start distributed work':'분산 작업 시작'}</h3>
    <div className="grid gap-3 sm:grid-cols-2">
     <label className="text-xs font-medium text-ink">{en?'Remote device':'원격 장치'}
      <SelectInput className="mt-1" value={workerId} onChange={e=>selectWorker(e.target.value)}>
       <option value="">{en?'Choose a worker':'워커 선택'}</option>
       {remote.value.remoteWorkers.map(worker=><option key={worker.id} value={worker.id}>{worker.id} ({worker.online?'online':'offline'})</option>)}
      </SelectInput>
     </label>
     <label className="text-xs font-medium text-ink">{en?'Lead AI':'Lead AI'}
      <SelectInput className="mt-1" value={selectedLead} onChange={e=>setLeadId(e.target.value)}>
       {!leadCandidates.length&&<option value="">{en?'Connect a model first':'모델을 먼저 연결하세요'}</option>}
       {leadCandidates.map(agent=><option key={agent.id} value={agent.id}>{agent.name}</option>)}
      </SelectInput>
     </label>
    </div>
    {chosen&&<fieldset className="grid gap-2 sm:grid-cols-2">
     <legend className="mb-2 text-xs font-semibold">{en?'Worker AIs (up to 8)':'원격 Worker AI (최대 8개)'}</legend>
     {availableIds.map(id=><label key={id} className="flex items-center gap-2 text-xs text-ink">
      <input type="checkbox" checked={actualSelected.includes(id)}
       disabled={busy||(!actualSelected.includes(id)&&actualSelected.length>=8)}
       onChange={e=>toggle(id,e.target.checked)} className="accent-brand"/>
      {agents.value.find(a=>a.id===id)?.name||id}
     </label>)}
     {availableIds.length===0&&<p className="text-xs text-muted">{en?'Worker has no advertised agents':'광고된 Worker AI가 없습니다.'}</p>}
    </fieldset>}
    <TextArea rows={3} value={goal} onChange={e=>setGoal(e.target.value)} maxLength={4000}
     placeholder={en?'Describe the task and verification criteria':'작업 목표와 검증 기준을 입력하세요'}/>
    <Button variant="primary" type="button" onClick={()=>void launch()}
     disabled={busy||!chosen?.online||!selectedLead||actualSelected.length===0||!goal.trim()}>
     {busy?<LoaderCircle size={14} className="animate-spin"/>:<Cloud size={14}/>}
     {en?'Queue native distributed task':'네이티브 분산 작업 시작'}
    </Button>
   </section>
   <section className="space-y-3">
    <h3 className="text-sm font-bold text-ink">{en?'Recorded remote Teams':'저장된 원격 팀 기록'}</h3>
    {teams.map(team=><article key={team.id} className="rounded-xl border border-divider p-3">
     <div className="flex flex-wrap items-start justify-between gap-2">
      <div className="min-w-0"><strong className="text-xs text-ink">{team.id}</strong><p className="text-xs text-muted">{label(team.status,en)} · {team.verified?(en?'Verified':'검증 완료'):(en?'Review state from Rust':'Rust 검증 상태')}</p></div>
      {team.status==='running'&&<div className="flex flex-wrap gap-2">
       <Button size="sm" variant="secondary" disabled={busy||!team.resumeAvailable} onClick={()=>void act('/api/activity/teams/'+encodeURIComponent(team.id)+'/resume',{},en?'Resume requested':'재개 요청 완료')}>{en?'Resume':'재개'}</Button>
       <Button size="sm" variant="secondary" disabled={busy} onClick={()=>{
        if(window.confirm(en?'Stop this native Team?':'이 네이티브 팀을 중단하시겠습니까?'))
         void act('/api/activity/teams/'+encodeURIComponent(team.id)+'/stop',{},en?'Stop requested':'중단 요청 완료')
       }}>{en?'Stop':'중단'}</Button>
      </div>}
     </div>
     {(team.tasks||[]).map(task=><div key={task.id} className="mt-2 border-t border-divider pt-2 text-xs">
      <div className="flex flex-wrap items-center justify-between gap-2"><span className="text-ink">{task.agentId||task.id}</span>
       <span className="text-muted">{label(task.displayState||task.status||'queued',en)}</span></div>
      {task.remoteTaskId&&task.artifactApprovalState==='awaiting_review'&&
       <RemotePatchReview teamId={team.id} taskId={task.remoteTaskId} onChanged={()=>void remote.reload()}/>}
      {task.error&&<p role="alert" className="mt-1 text-rose-700">{task.error}</p>}
     </div>)}
    </article>)}
    {!teams.length&&<p className="text-xs text-muted">{en?'No remote Team records yet.':'원격 팀 작업 기록이 없습니다.'}</p>}
   </section>
   {message&&<p role="status" className="rounded-lg bg-emerald-50 p-3 text-xs text-emerald-700">{message}</p>}
   {error&&<p role="alert" className="rounded-lg bg-rose-50 p-3 text-xs text-rose-700">{error}</p>}
  </div>}
 </Panel>
}
