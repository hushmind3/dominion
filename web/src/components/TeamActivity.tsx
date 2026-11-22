import {useEffect,useState} from 'react'
import {Activity,CheckCircle2,ChevronDown,ChevronRight,ClipboardCopy,GitMerge,Network,Play,RefreshCw,RotateCcw,ShieldAlert,Square,UsersRound} from 'lucide-react'
import {get,post,type Agent} from '../api'
import {Button,SelectInput,TextArea,TextInput} from './Primitives'
import {StatusPill} from './StatusPill'
import {Panel,useRemote,refresh} from './UI'
import {useLocale} from '../i18n'

type Worker={id:string;online:boolean;allowWrites:boolean;agentCount:number;agents?:string[];lastHeartbeatMs?:number}
type TeamTask={
 id:string;agentId?:string;status:string;readOnly:boolean;verified:boolean;
 dependsOn:string[];remoteWorkerId?:string;remoteTaskId?:string;
 artifactApplyStatus?:string;jobId?:string;error?:string;
 displayState?:string;pendingApproval?:boolean;workerOnline?:boolean;
 waitingPredecessors?:boolean;artifactApprovalState?:string;artifactApplied?:boolean
}
type Team={
 id:string;status:string;leadSessionId?:string;leadAgentId?:string;
 verified:boolean;createdAt?:string;updatedAt?:string;resumeAvailable?:boolean;
 pendingRemoteArtifactTaskIds:string[];tasks:TeamTask[]
}
type ActivityData={teams:Team[];remoteWorkers:Worker[];remoteConfigured?:boolean;leadJobs?:LeadJob[]}
type ArtifactReview={teamId:string;remoteTaskId:string;status:string;reviewDigest:string;patch:string;files:{path:string;sha256:string}[]}
type LeadJob={id:string;status:string;sessionId?:string;error?:string}
const empty:ActivityData={teams:[],remoteWorkers:[],remoteConfigured:false,leadJobs:[]}
type TeamDraft={open:boolean;prompt:string;leadId:string;workerIds:string[];remoteId:string}
const teamDraftKey='dominion.team.launch.draft.v1'
function restoreTeamDraft():TeamDraft{
 const blank:TeamDraft={open:false,prompt:'',leadId:'',workerIds:[],remoteId:''}
 try{
  const raw=sessionStorage.getItem(teamDraftKey)
  if(!raw||raw.length>12000)return blank
  const draft:Partial<TeamDraft>=JSON.parse(raw)
  if(!draft||typeof draft!=='object'||Array.isArray(draft))return blank
  const names=(Array.isArray(draft.workerIds)?draft.workerIds:[])
   .filter((id):id is string=>typeof id==='string'
    &&id.length>0&&id.length<=80&&/^[a-zA-Z0-9_-]+$/.test(id))
  return {open:draft.open===true,
   prompt:typeof draft.prompt==='string'?draft.prompt.slice(0,4000):'',
   leadId:typeof draft.leadId==='string'?draft.leadId.slice(0,100):'',
   workerIds:[...new Set(names)].slice(0,8),
   remoteId:typeof draft.remoteId==='string'?draft.remoteId.slice(0,100):''}
 }catch{return blank}
}
const target=(teamId:string,taskId:string)=>'/api/activity/teams/'+encodeURIComponent(teamId)+'/artifacts/'+encodeURIComponent(taskId)
function goToLead(sessionId?:string){
 if(sessionId)window.location.hash='#/chat?session='+encodeURIComponent(sessionId)
}
const active=(status:string)=>status==='running'||status==='queued'||status==='launching'||status==='assigned'
const needsAttention=(team:Team)=>team.status==='needs_review'
 ||team.pendingRemoteArtifactTaskIds?.length>0
 ||team.tasks.some(task=>task.pendingApproval
  ||['approval_pending','offline','file_review_pending'].includes(task.displayState||'')
  ||task.artifactApplyStatus==='needs_review'||task.artifactApplyStatus==='pending'
  ||['failed','needs_review'].includes(task.status))
const taskStateLabel=(state:string,en:boolean)=>({
 approval_pending:en?'Approval pending':'실행 승인 대기',
 offline:en?'Worker offline':'Worker 오프라인',
 waiting_predecessors:en?'Waiting for predecessors':'선행 작업 대기',
 file_review_pending:en?'File review pending':'파일 검토·승인 대기',
 applying_files:en?'Applying approved files':'승인된 파일 적용 중'
} as Record<string,string>)[state]||state
const artifactLabel=(status:string,en:boolean)=>({
 pending:en?'Awaiting approval':'승인 대기',applying:en?'Applying':'적용 중',
 applied:en?'Applied':'적용 완료',rejected:en?'Rejected':'거절됨',
 needs_review:en?'Needs review':'검토 필요',blocked:en?'Blocked':'차단됨'
} as Record<string,string>)[status]||status

/** One native operator surface within the existing Activity page, not a new menu. */
export function TeamActivity(){
 const en=useLocale()==='en'
 const activity=useRemote<ActivityData>('/api/activity/teams',empty)
 const agents=useRemote<Agent[]>('/api/agents',[])
 const [restoredDraft]=useState<TeamDraft>(restoreTeamDraft)
 const [createOpen,setCreateOpen]=useState(restoredDraft.open)
 const [selected,setSelected]=useState('')
 const [teamFilter,setTeamFilter]=useState<'all'|'running'|'attention'>('all')
 const [teamSearch,setTeamSearch]=useState('')
 const [prompt,setPrompt]=useState(restoredDraft.prompt)
 const [leadId,setLeadId]=useState(restoredDraft.leadId)
 const [workerIds,setWorkerIds]=useState<string[]>(restoredDraft.workerIds)
 const [remoteId,setRemoteId]=useState(restoredDraft.remoteId)
 const [platform,setPlatform]=useState<'powershell'|'posix'>('powershell')
 const [busy,setBusy]=useState('')
 const [error,setError]=useState('')
 const [notice,setNotice]=useState('')
 const [review,setReview]=useState<ArtifactReview|null>(null)
 const [queuedJobId,setQueuedJobId]=useState('')
 const queued=useRemote<LeadJob>(queuedJobId?'/api/jobs/'+encodeURIComponent(queuedJobId):null,{id:'',status:''})
 // Persisted, server-owned jobs remain visible after reload, even before a
 // real Team DAG is created. Just-launched jobs retain a faster status view.
 const persistedLeads=activity.value.leadJobs||[]
 const leadJobs=queuedJobId&&!persistedLeads.some(job=>job.id===queuedJobId)
  ? [queued.value.id===queuedJobId?queued.value:{id:queuedJobId,status:'queued'},...persistedLeads]
  : persistedLeads.map(job=>job.id===queuedJobId&&queued.value.id===job.id?queued.value:job)
 const runnable=agents.value.filter(agent=>agent.id&&agent.providerId&&agent.model&&agent.enabled!==false&&agent.disabled!==true)
 const lead=leadId?runnable.find(agent=>agent.id===leadId):runnable[0]
 const unavailableLead=Boolean(leadId&&!lead)
 const workers=activity.value.remoteWorkers
 // Remote Agents are registered on their own PCs, not necessarily on the
 // Lead machine. Show their advertised IDs without forging local models.
 const remoteOnly=[...new Set(workers.filter(node=>node.online)
  .flatMap(node=>node.agents||[]))].filter(id=>id.length>0&&
   !agents.value.some(agent=>agent.id===id)).slice(0,64)
 const workerChoices=[
  ...runnable.map(agent=>({id:agent.id,name:agent.name,remoteOnly:false})),
  ...remoteOnly.map(id=>({id,name:id,remoteOnly:true}))
 ]
 const unavailableWorkers=workerIds.filter(id=>!workerChoices.some(choice=>choice.id===id))
 const hasDraft=Boolean(prompt.trim()||leadId||workerIds.length||remoteId)
 const remoteRequired=workerIds.filter(id=>remoteOnly.includes(id))
 const compatible=workers.filter(node=>node.online
  && workerIds.some(id=>(node.agents||[]).includes(id))
  && remoteRequired.every(id=>(node.agents||[]).includes(id)))
 const selectedRemote=compatible.some(node=>node.id===remoteId)?remoteId:''
 const invalidRemote=remoteRequired.length>0&&!selectedRemote
 const unavailableRemote=Boolean(remoteId&&!selectedRemote)
 const inProgress=activity.value.teams.filter(team=>team.status==='running').length
 const reviewCount=activity.value.teams.filter(needsAttention).length
 const visibleTeams=[...activity.value.teams]
  .filter(team=>teamFilter==='all'||teamFilter==='running'&&team.status==='running'
   ||teamFilter==='attention'&&needsAttention(team))
  .filter(team=>!teamSearch.trim()||[
   team.id,team.leadAgentId,...team.tasks.flatMap(task=>[
    task.id,task.agentId,task.remoteWorkerId,task.error
   ])].filter(Boolean).join(' ').toLowerCase().includes(teamSearch.trim().toLowerCase()))
  .sort((a,b)=>Number(needsAttention(b))-Number(needsAttention(a)))
 // Preserve only the unsent composer state in the current browser tab.
 // Restoring a draft never enqueues work; only an explicit click can do so.
 useEffect(()=>{
  try{
   if(!createOpen&&!hasDraft)sessionStorage.removeItem(teamDraftKey)
   else sessionStorage.setItem(teamDraftKey,JSON.stringify({
    open:createOpen,prompt,leadId,workerIds,remoteId
   } satisfies TeamDraft))
  }catch{/* Browser storage may be unavailable. */}
 },[createOpen,prompt,leadId,workerIds,remoteId,hasDraft])
 useEffect(()=>{
  const timer=setInterval(()=>{if(document.visibilityState==='visible')void activity.reload()},30000)
  return()=>clearInterval(timer)
 },[activity.reload])
 const toggle=(id:string,checked:boolean)=>{
  setWorkerIds(previous=>checked?
   previous.includes(id)||previous.length>=8?previous:[...previous,id]:
   previous.filter(item=>item!==id))
  setRemoteId('')
 }
 const reload=()=>{void activity.reload();refresh('/api/activity/teams','/api/jobs')}
 const discardDraft=()=>{
  setPrompt('');setLeadId('');setWorkerIds([]);setRemoteId('');setCreateOpen(false)
  try{sessionStorage.removeItem(teamDraftKey)}catch{}
 }
 const launch=async()=>{
  if(!lead||!workerIds.length||!prompt.trim()||busy||invalidRemote
   ||unavailableWorkers.length>0||unavailableLead||unavailableRemote)return
  setBusy('launch');setError('');setNotice('')
  try{
   const saved=await post<{status:string;jobId:string}>('/api/activity/teams/start',{
    prompt:prompt.trim(),leadAgentId:lead.id,workerAgentIds:workerIds,
    ...(selectedRemote?{remoteWorkerId:selectedRemote}:{})
   })
   setQueuedJobId(saved.jobId)
   setNotice(en?'Lead job queued. The team appears once the Lead commits its native DAG.':'Lead 작업이 대기열에 등록됐습니다. Lead가 실제 작업 DAG를 생성하면 팀 목록에 나타납니다.')
   discardDraft();reload()
  }catch(e){setError((e as Error).message)}finally{setBusy('')}
 }
 const cancelLead=async(job:LeadJob)=>{
  if(busy||!active(job.status))return
  if(!window.confirm(en?'Stop this queued Lead job? In-flight effects may need review.':'대기 중이거나 실행 중인 Lead 작업을 중단하시겠습니까? 진행 중인 효과는 검토가 필요할 수 있습니다.'))return
  setBusy('cancelLead'+job.id);setError('');setNotice('')
  try{
   await post('/api/jobs/'+encodeURIComponent(job.id)+'/cancel',{})
   setNotice(en?'Lead cancellation recorded. Uncertain effects still require review.':'Lead 중단 요청을 저장했습니다. 결과가 불확실한 실행은 검토가 필요합니다.')
   if(queuedJobId===job.id)void queued.reload()
   reload()
  }catch(e){setError((e as Error).message)}finally{setBusy('')}
 }
 const control=async(team:Team,operation:'stop'|'resume')=>{
  if(busy)return
  if(operation==='stop'&&!window.confirm(en?'Stop this team and its active workers? In-flight effects may require review.':'이 팀과 실행 중인 Worker를 중단하시겠습니까? 진행 중이던 파일 변경은 검토가 필요할 수 있습니다.'))return
  setBusy(operation+team.id);setError('');setNotice('')
  try{
   const result=await post<{status?:string}>('/api/activity/teams/'+encodeURIComponent(team.id)+'/'+operation,{})
   setNotice(operation==='stop'?
    (en?'Team stop requested; inspect in-flight effects.':'팀 중단 요청을 저장했습니다. 진행 중이던 작업 결과는 확인해 주세요.'):
    (en?'Existing team execution resumed.':'기존 팀 ID로 작업을 재개했습니다.'))
   if(operation==='resume'&&result.status==='needs_review'){
    setNotice(en?'Team needs review before dependent work can continue.':'선행 작업을 검토해야 다음 Worker가 진행할 수 있습니다.')
   }
   reload()
  }catch(e){setError((e as Error).message)}finally{setBusy('')}
 }
 const openReview=async(teamId:string,taskId:string)=>{
  setBusy('review'+taskId);setError('');setReview(null)
  try{setReview(await get<ArtifactReview>(target(teamId,taskId)))}
  catch(e){setError((e as Error).message)}
  finally{setBusy('')}
 }
 const decide=async(approve:boolean)=>{
  if(!review||busy)return
  const message=approve?
   (en?'Apply exactly the reviewed remote changes to the local workspace?':'검토한 원격 파일 변경을 현재 작업 공간에 적용하시겠습니까?'):
   (en?'Reject these remote changes without modifying local files?':'이 원격 변경을 거절하고 로컬 파일을 그대로 유지하시겠습니까?')
  if(!window.confirm(message))return
  const current=review
  setBusy('decision');setError('');setNotice('')
  try{
   await post(target(current.teamId,current.remoteTaskId)+(approve?'/apply':'/reject'),
    approve?{approved:true,reviewDigest:current.reviewDigest}:{rejected:true})
   setNotice(approve?
    (en?'Approved files were applied and verified. The native scheduler continues eligible dependents.':'승인된 파일을 적용하고 해시 검증을 마쳤습니다. 실행 가능한 후속 Worker는 네이티브 스케줄러에서 이어집니다.'):
    (en?'Remote changes rejected; no local files were written.':'원격 변경을 거절했습니다. 로컬 파일은 수정하지 않았습니다.'))
   setReview(null);reload()
  }catch(e){setError((e as Error).message)}finally{setBusy('')}
 }
 const coordinatorSetup=(platform==='powershell'?[
  '$env:DOMINION_REMOTE_CLUSTER_TOKEN = "PASTE_A_32_PLUS_CHAR_RANDOM_SECRET"',
  '$env:DOMINION_REMOTE_LISTEN = "127.0.0.1:4941"',
  '.\\dominion-rust.exe'
 ]:[
  'export DOMINION_REMOTE_CLUSTER_TOKEN="$(openssl rand -hex 32)"',
  'export DOMINION_REMOTE_LISTEN="127.0.0.1:4941"',
  './dominion-rust'
 ]).join('\n')
 const workerSetup=(platform==='powershell'?[
  '$env:DOMINION_REMOTE_CLUSTER_TOKEN = "SAME_SECRET_AS_COORDINATOR"',
  '$env:DOMINION_REMOTE_COORDINATOR_URL = "https://your-private-controller.example"',
  '$env:DOMINION_REMOTE_WORKER_ID = "worker-b"',
  '.\\dominion-rust.exe remote-worker'
 ]:[
  'export DOMINION_REMOTE_CLUSTER_TOKEN="SAME_SECRET_AS_COORDINATOR"',
  'export DOMINION_REMOTE_COORDINATOR_URL="https://your-private-controller.example"',
  'export DOMINION_REMOTE_WORKER_ID="worker-b"',
  './dominion-rust remote-worker'
 ]).join('\n')
 return <section className="dominion-team-workspace mt-8" aria-label={en?'Agent teams':'에이전트 작업 팀'}>
  <div className="dominion-team-hero relative mb-4 overflow-hidden rounded-[24px] px-5 py-6 sm:px-7">
   <div className="relative z-10 flex flex-wrap items-start justify-between gap-5">
    <div className="min-w-0 flex-1">
     <span className="text-[10px] font-extrabold uppercase tracking-[.19em] text-[#F6BB9E]">DOMINION / TEAM ORCHESTRATION</span>
     <h3 className="mt-2 !text-[22px] font-extrabold text-white sm:!text-[25px]">{en?'Agent teams':'멀티에이전트 팀 작업'}</h3>
     <p className="mt-2 max-w-xl text-[12px] leading-6 text-[#DDD4CB]">{en?'Plan with a Lead, execute through Workers, then inspect real outputs and required reviews.':'Lead가 계획하고 Worker가 실행합니다. 진행 상황과 검토할 파일 변경을 한곳에서 확인하세요.'}</p>
    </div>
    <div className="flex shrink-0 flex-wrap items-center gap-2">
     <Button size="sm" variant="bare" onClick={()=>setCreateOpen(o=>!o)} aria-expanded={createOpen} className="!min-h-10 !rounded-xl !bg-[#F6D8C5] !px-4 !font-bold !text-[#5E3627] hover:!bg-white"><Play size={15}/>{hasDraft?(en?'Continue team draft':'팀 초안 이어쓰기'):(en?'New team task':'팀 작업 시작')}</Button>
     <Button size="sm" variant="bare" onClick={reload} aria-label={en?'Refresh teams':'팀 새로고침'} className="!size-10 !rounded-xl !border !border-white/20 !bg-white/10 !text-white hover:!bg-white/20"><RefreshCw size={16}/></Button>
    </div>
   </div>
   <div className="relative z-10 mt-6 grid grid-cols-3 gap-2">
    <div className="dominion-team-stat"><span>{en?'Saved teams':'저장된 팀'}</span><strong>{activity.loading||activity.error?'—':activity.value.teams.length}</strong></div>
    <div className="dominion-team-stat"><span>{en?'Running':'진행 중'}</span><strong>{activity.loading||activity.error?'—':inProgress}</strong></div>
    <div className="dominion-team-stat"><span>{en?'Review needed':'검토 필요'}</span><strong>{activity.loading||activity.error?'—':reviewCount}</strong></div>
   </div>
  </div>
  {createOpen&&<Panel className="mb-4 space-y-3 p-4 sm:p-5">
   <div><h4 className="m-0 text-[13px] font-semibold">{en?'Start a native Lead / Worker task':'네이티브 Lead / Worker 작업 시작'}</h4>
    <p className="mt-1 text-[11px] text-[#8A8176]">{en?'The Lead plans and commits a real native Team; worker IDs do not create fake completed tasks.':'Lead가 실제 팀 작업을 계획하고 저장합니다. Worker 목록만 선택했다고 팀이 완료된 것으로 처리하지 않습니다.'}</p></div>
   <TextArea rows={3} maxLength={4000} placeholder={en?'Describe the desired outcome and acceptance checks':'목표, 필요한 산출물, 성공 조건을 입력하세요.'} value={prompt} onChange={e=>setPrompt(e.target.value)}/>
   <div className="grid gap-3 sm:grid-cols-2">
    <label className="text-[11px] font-semibold text-[#5C544A]">{en?'Lead agent':'Lead 에이전트'}
     <SelectInput className="mt-1" value={lead?.id||''} onChange={e=>setLeadId(e.target.value)}>
      {!runnable.length&&<option value="">{en?'Configure an agent first':'먼저 에이전트를 설정하세요'}</option>}
      {unavailableLead&&<option value="">{en?'Select an available Lead again':'사용 가능한 Lead를 다시 선택하세요'}</option>}
      {runnable.map(a=><option value={a.id} key={a.id}>{a.name} · {a.model}</option>)}
     </SelectInput>
    </label>
    <label className="text-[11px] font-semibold text-[#5C544A]">{en?'Execution device':'Worker 실행 장치'}
     <SelectInput className="mt-1" value={selectedRemote} onChange={e=>setRemoteId(e.target.value)}>
      <option value="">{en?'Local Rust workers':'이 PC의 Rust Worker'}</option>
      {compatible.map(node=><option key={node.id} value={node.id}>{node.id} · {node.allowWrites?(en?'Read / Write':'읽기·쓰기'):(en?'Read only':'읽기 전용')} ({workerIds.filter(id=>(node.agents||[]).includes(id)).length}/{workerIds.length})</option>)}
     </SelectInput>
    </label>
   </div>
   <div>
    <div className="mb-1 flex items-center justify-between text-[11px] font-semibold text-[#5C544A]"><span>{en?'Worker agents (up to 8)':'Worker 에이전트 선택 (최대 8)'}</span><span>{workerIds.length}/8</span></div>
    <div className="grid max-h-40 gap-1 overflow-y-auto rounded-xl border border-[#E7E0D5] bg-white p-2 sm:grid-cols-2">
     {workerChoices.map(a=><label key={a.id} className="flex min-w-0 cursor-pointer items-center gap-2 rounded-lg px-2 py-2 text-[11px] hover:bg-[#FAF7F1]">
      <TextInput type="checkbox" checked={workerIds.includes(a.id)} disabled={!workerIds.includes(a.id)&&workerIds.length>=8} onChange={e=>toggle(a.id,e.target.checked)}/>
      <span className="min-w-0 truncate">{a.name} <small className="text-[#8A8176]">({a.id}) {a.remoteOnly?(en?'Remote only':'원격 전용'):''}</small></span>
     </label>)}
     {!workerChoices.length&&<p className="px-2 text-xs text-[#8A8176]">{en?'No runnable agents.':'실행 가능한 에이전트가 없습니다.'}</p>}
    </div>
   </div>
   {selectedRemote&&<p role="status" className="text-[11px] text-[#7C7469]">{en
    ?'This device can run '+workerIds.filter(id=>(workers.find(node=>node.id===selectedRemote)?.agents||[]).includes(id)).length+' selected agents; others stay local.'
    :'이 장치에서 '+workerIds.filter(id=>(workers.find(node=>node.id===selectedRemote)?.agents||[]).includes(id)).length+'명 실행 가능 · 나머지는 로컬에서 실행됩니다.'}</p>}
   {agents.error&&<p role="alert" className="text-[11px] text-rose-700">{en?'Could not refresh available agents; please retry before launching.':'실행 가능한 에이전트 정보를 갱신하지 못했습니다. 새로고침 후 실행하세요.'}</p>}
   {invalidRemote&&<p role="alert" className="text-[11px] text-amber-800">{en?'Remote-only agents need an online device that advertises all selected remote IDs.':'원격 전용 에이전트를 선택하려면 해당 ID를 모두 제공하는 온라인 원격 장치를 지정해야 합니다.'}</p>}
   {unavailableLead&&<p role="alert" className="break-words text-[11px] text-amber-800">{en?'The previously selected Lead is unavailable. Select another Lead before launch.':'이전에 선택한 Lead를 사용할 수 없습니다. 다른 Lead를 직접 선택한 뒤 실행하세요.'}</p>}
   {unavailableRemote&&<div role="alert" className="flex flex-wrap items-center gap-2 text-[11px] text-amber-800"><span>{en?'The selected remote device is unavailable. Choose an available device or switch to local execution.':'선택한 원격 장치를 사용할 수 없습니다. 다른 장치를 선택하거나 로컬 실행으로 전환하세요.'}</span><Button size="sm" variant="ghost" onClick={()=>setRemoteId('')}>{en?'Switch to local':'로컬 실행으로 변경'}</Button></div>}
   {unavailableWorkers.length>0&&<div role="alert" className="flex flex-wrap items-center gap-2 text-[11px] text-amber-800"><span className="break-words">{en?'Unavailable selected Workers: ':'현재 사용할 수 없는 Worker: '}{unavailableWorkers.join(', ')}</span><Button size="sm" variant="ghost" onClick={()=>{setWorkerIds(ids=>ids.filter(id=>!unavailableWorkers.includes(id)));setRemoteId('')}}>{en?'Remove unavailable Workers':'사용 불가 Worker 제외'}</Button></div>}
   <div className="flex flex-wrap items-center justify-between gap-2">
    <div className="flex flex-wrap items-center gap-2">
     <span className="text-[10px] text-[#8A8176]">{en?'Unsent draft stored in this tab':'보내지 않은 초안은 현재 탭에 임시 저장됩니다'}</span>
     {hasDraft&&<Button size="sm" variant="ghost" onClick={discardDraft} disabled={Boolean(busy)}>{en?'Discard draft':'초안 지우기'}</Button>}
    </div>
    <Button variant="primary" disabled={Boolean(busy)||Boolean(agents.error)||Boolean(activity.error)||agents.loading||activity.loading||!prompt.trim()||!lead||workerIds.length<1||invalidRemote||unavailableWorkers.length>0||unavailableLead||unavailableRemote} onClick={()=>void launch()}><Play size={14}/>{busy==='launch'?(en?'Queuing…':'등록 중…'):(en?'Queue Lead task':'Lead 작업 시작')}</Button>
   </div>
  </Panel>}
  {error&&<p role="alert" className="mb-3 break-words rounded-xl border border-rose-200 bg-rose-50 p-3 text-xs text-rose-700">{error}</p>}
  {notice&&<p role="status" className="mb-3 rounded-xl border border-[#E7E0D5] bg-[#FAF7F1] p-3 text-xs text-[#5C544A]">{notice}</p>}
  {leadJobs.slice(0,5).map(job=><div key={job.id} className="mb-2 flex min-w-0 flex-wrap items-center gap-2 rounded-lg bg-[#FAF7F1] p-3 text-[11px]">
   <Activity size={14} className="shrink-0"/><span className="min-w-0 flex-1 break-all">{en?'Saved Lead job':'저장된 Lead 작업'} · {job.id.slice(-12)}</span><StatusPill status={job.status||'queued'}/>
   {job.sessionId&&<Button size="sm" variant="ghost" onClick={()=>goToLead(job.sessionId)}>{en?'Open Lead':'Lead 대화'} <ChevronRight size={13}/></Button>}
   {active(job.status)&&<Button size="sm" variant="danger" disabled={Boolean(busy)} onClick={()=>void cancelLead(job)}><Square size={13}/>{en?'Stop':'중단'}</Button>}
   {job.error&&<span role="alert" className="w-full break-words text-rose-700">{job.error}</span>}
  </div>)}
  <Panel className="dominion-team-board overflow-hidden !rounded-2xl p-4 sm:p-5">
   <div className="mb-3 flex flex-col gap-2 border-b border-[#EFE9DF] pb-3 sm:flex-row sm:items-center">
    <TextInput aria-label={en?'Find a team or Worker':'팀·Worker 검색'}
     value={teamSearch} onChange={event=>setTeamSearch(event.target.value)}
     placeholder={en?'Find team, agent or failure':'팀·에이전트·오류 검색'} className="sm:flex-1"/>
    <SelectInput aria-label={en?'Team status filter':'팀 상태 필터'}
     value={teamFilter} onChange={event=>setTeamFilter(event.target.value as 'all'|'running'|'attention')}
     className="sm:w-44">
     <option value="all">{en?'All teams':'전체 팀'}</option>
     <option value="running">{en?'Running':'진행 중'}</option>
     <option value="attention">{en?'Needs action':'조치 필요'}</option>
    </SelectInput>
   </div>
   {activity.error&&<div role="alert" className="mb-3 flex flex-wrap items-center gap-2 rounded-lg border border-rose-200 bg-rose-50 p-3 text-[11px] text-rose-700">
    <span className="min-w-0 flex-1">{en?'Team state could not be refreshed. Displayed data may be stale.':'팀 상태를 갱신하지 못했습니다. 표시된 정보가 이전 기록일 수 있습니다.'}</span>
    <Button size="sm" variant="secondary" onClick={reload}><RefreshCw size={13}/>{en?'Retry':'다시 시도'}</Button>
   </div>}
   {activity.loading&&<p role="status" className="py-5 text-center text-xs text-[#8A8176]">{en?'Loading teams…':'팀 작업 불러오는 중…'}</p>}
   {!activity.loading&&!activity.error&&!activity.value.teams.length&&<p className="py-5 text-center text-xs text-[#8A8176]">{en?'No saved teams yet. Start a Lead task above.':'아직 저장된 팀이 없습니다. 위에서 Lead 작업을 시작할 수 있습니다.'}</p>}
   {!activity.loading&&!activity.error&&activity.value.teams.length>0&&!visibleTeams.length&&<p className="py-5 text-center text-xs text-[#8A8176]">{en?'No teams match this search or filter.':'조건에 맞는 팀이 없습니다. 필터를 바꿔 보세요.'}</p>}
   <div className="space-y-2">
    {visibleTeams.map(team=>{
     const open=selected===team.id
     const done=team.tasks.filter(task=>task.status==='completed').length
     const pending=team.pendingRemoteArtifactTaskIds.length
     const width=team.tasks.length?Math.round(100*done/team.tasks.length):0
     return <div key={team.id} className="dominion-team-card min-w-0 rounded-xl border border-[#E7E0D5] bg-white">
      <Button variant="bare" className="flex w-full min-w-0 items-center gap-3 rounded-xl px-3 py-3 text-left hover:bg-[#FAF7F1]" onClick={()=>{setSelected(open?'':team.id);setReview(null)}} aria-expanded={open}>
       {open?<ChevronDown size={16} className="shrink-0 text-[#8A8176]"/>:<ChevronRight size={16} className="shrink-0 text-[#8A8176]"/>}
       <span className="min-w-0 flex-1"><strong className="block truncate text-[12px] text-[#3A342D]">{team.leadAgentId||'Lead'} · {en?'Team':'팀'} {team.id.slice(-9)}</strong>
        <small className="mt-1 block text-[11px] text-[#8A8176]">{en?'Tasks':'작업'} {done}/{team.tasks.length}{pending>0?' · '+(en?'File review':'파일 승인 대기')+' '+pending:''}</small>
        <span className="mt-2 block h-1.5 overflow-hidden rounded-full bg-[#F3EEE5]"><span className="block h-full rounded-full bg-[#C96442]" style={{width:width+'%'}}/></span>
       </span><StatusPill status={team.status}/>
      </Button>
      {open&&<div className="border-t border-[#EFE9DF] px-3 py-3">
       <div className="space-y-2">
        {team.tasks.map(task=><div key={task.id} className="flex min-w-0 flex-wrap items-center gap-2 rounded-lg bg-[#FAF7F1] px-3 py-2 text-[11px]">
         <span className="min-w-0 flex-1"><strong className="block truncate text-[#3A342D]">{task.id} · {task.agentId||'Worker'}</strong>
          <small className="text-[#8A8176]">{task.remoteWorkerId?(en?'Remote device: ':'원격 장치: ')+task.remoteWorkerId:(en?'Local · this PC':'로컬 · 이 PC')}{' · '}{task.readOnly?(en?'Read-only':'읽기 전용'):(en?'Writer · approval required':'Writer · 변경 승인 필요')}{task.dependsOn?.length?' · '+(en?'After ':'선행: ')+task.dependsOn.join(', '):''}</small>
         </span>
         {task.verified&&<CheckCircle2 size={14} className="text-emerald-600" aria-label={en?'Verified':'검증됨'}/>}
         <StatusPill status={task.status}/>
         {task.displayState&&task.displayState!==task.status&&<span role="status" className="rounded-md bg-amber-50 px-2 py-0.5 text-[10px] text-amber-900">{taskStateLabel(task.displayState,en)}</span>}
         {task.artifactApprovalState==='approved'&&<span className="rounded-md bg-sky-50 px-2 py-0.5 text-[10px] text-sky-800">{en?'File changes approved':'파일 변경 승인됨'}</span>}
         {task.artifactApplied&&<span className="rounded-md bg-emerald-50 px-2 py-0.5 text-[10px] text-emerald-800">{en?'Local files applied and verified':'로컬 적용·해시 검증 완료'}</span>}
         {task.artifactApplyStatus&&task.artifactApplyStatus!=='not_required'&&<span className={'rounded-md px-2 py-0.5 text-[10px] '+(task.artifactApplyStatus==='applied'?'bg-emerald-50 text-emerald-700':'bg-amber-50 text-amber-800')}>{en?'Files: ':'산출물: '}{artifactLabel(task.artifactApplyStatus,en)}</span>}
         {task.remoteTaskId&&task.artifactApplyStatus==='pending'&&<Button size="sm" variant="secondary" disabled={Boolean(busy)||Boolean(activity.error)} onClick={()=>void openReview(team.id,task.remoteTaskId!)}><GitMerge size={12}/>{en?'Review diff':'파일 변경 검토'}</Button>}
         {task.error&&<p className="w-full break-words text-rose-600">{task.error}</p>}
        </div>)}
       </div>
       {pending>0&&<p className="mt-3 rounded-lg border border-amber-200 bg-amber-50 p-3 text-[11px] text-amber-900"><ShieldAlert size={14} className="mr-1 inline"/>{en?'Review and approve remote file changes before resuming dependents.':'원격 파일 변경을 먼저 검토·승인해야 후속 Worker가 실행됩니다.'}</p>}
       {review&&review.teamId===team.id&&<div className="mt-3 rounded-xl border border-[#DAC9B4] bg-[#FFFDF9] p-3">
        <div className="flex flex-wrap items-center justify-between gap-2"><strong className="text-[12px]">{en?'Verified file diff preview':'실제 파일 변경 미리보기'}</strong>
         <Button size="sm" variant="ghost" onClick={()=>setReview(null)}>{en?'Close':'닫기'}</Button>
        </div>
        <p className="mt-2 break-all font-mono text-[10px] text-[#8A8176]">{review.files.map(file=>file.path).join(' · ')}</p>
        <p className="mt-1 break-all text-[10px] text-[#8A8176]">SHA-256: {review.reviewDigest}</p>
        <pre className="mt-2 max-h-[360px] overflow-auto whitespace-pre-wrap break-all rounded-lg bg-[#24211D] p-3 text-[11px] leading-5 text-white">{review.patch}</pre>
        <div className="mt-3 flex flex-wrap justify-end gap-2">
         <Button size="sm" variant="danger" disabled={Boolean(busy)||Boolean(activity.error)} onClick={()=>void decide(false)}>{en?'Reject changes':'변경 거절'}</Button>
         <Button size="sm" variant="primary" disabled={Boolean(busy)||Boolean(activity.error)} onClick={()=>void decide(true)}><GitMerge size={13}/>{en?'Approve & apply':'승인 후 적용'}</Button>
        </div>
       </div>}
       <div className="mt-3 flex flex-wrap gap-2">
        {team.leadSessionId&&<Button size="sm" variant="ghost" onClick={()=>goToLead(team.leadSessionId)}>{en?'Lead conversation':'Lead 대화'} <ChevronRight size={13}/></Button>}
        {team.status==='running'&&<Button size="sm" variant="secondary" disabled={Boolean(busy)||Boolean(activity.error)||team.resumeAvailable===false} onClick={()=>void control(team,'resume')} title={team.resumeAvailable===false?(en?'Approval, offline Worker or file review blocks manual continuation':'승인 대기·장치 오프라인·파일 검토가 끝나야 재개할 수 있습니다.'):undefined}><RotateCcw size={13}/>{en?'Resume':'작업 재개'}</Button>}
        {team.status==='running'&&<Button size="sm" variant="danger" disabled={Boolean(busy)||Boolean(activity.error)} onClick={()=>void control(team,'stop')}><Square size={13}/>{en?'Stop team':'팀 중단'}</Button>}
       </div>
      </div>}
     </div>
    })}
   </div>
   <div className="mt-4 border-t border-[#EFE9DF] pt-3">
    <div className="flex flex-wrap items-center gap-2"><Network size={14} className="text-[#7C7469]"/><strong className="text-[11px] text-[#5C544A]">{en?'Remote Rust workers':'원격 Rust Worker'}</strong><span className="text-[10px] text-[#8A8176]">{activity.value.remoteConfigured?(en?'Coordinator configured':'제어면 설정됨'):(en?'Coordinator not configured':'제어면 미설정')}</span></div>
    <div className="mt-2 flex flex-wrap gap-2">{workers.map(node=><span key={node.id} title={(node.agents||[]).join(', ')} className="inline-flex items-center gap-1.5 rounded-lg border border-[#E7E0D5] bg-[#FAF7F1] px-2.5 py-1.5 text-[11px]"><span className={'size-1.5 rounded-full '+(node.online?'bg-emerald-500':'bg-slate-400')}/>{node.id} · {node.online?(en?'Online':'접속'):(en?'Offline':'오프라인')} · {node.agentCount} {en?'agents':'에이전트'}{node.allowWrites?' · Writer':''}</span>)}</div>
    {!workers.length&&<p className="mt-2 text-[11px] text-[#8A8176]">{en?'No registered remote devices. Local workers remain available.':'등록된 원격 장치가 없습니다. 로컬 Worker는 정상적으로 사용할 수 있습니다.'}</p>}
    <details className="mt-3 text-[11px] text-[#7C7469]"><summary className="cursor-pointer font-semibold">{en?'Connect another PC (manual credential setup)':'다른 PC 연결 방법 · 인증 정보는 직접 설정'}</summary>
     <p className="mt-2 leading-5">{en?'Enable the opt-in remote-only listener on the coordinator and connect the other PC over private HTTPS or encrypted overlay. Never expose the desktop API or paste cluster secrets into chat.':'리더 PC에서 원격 전용 포트를 활성화한 뒤 다른 PC가 비공개 HTTPS 또는 암호화 사설망을 통해 접속하도록 설정합니다. 데스크톱 API를 외부에 공개하지 않습니다.'}</p>
     <label className="mt-2 block max-w-[220px] text-[11px] font-semibold">{en?'Command shell':'명령 실행 환경'}
      <SelectInput className="mt-1" value={platform} onChange={e=>setPlatform(e.target.value as 'powershell'|'posix')}>
       <option value="powershell">Windows PowerShell</option><option value="posix">macOS / Linux</option>
      </SelectInput>
     </label>
     <div className="mt-3 flex items-center justify-between gap-2"><strong>{en?'1. Coordinator PC':'1. 리더 PC'}</strong><Button size="sm" variant="ghost" onClick={()=>void navigator.clipboard.writeText(coordinatorSetup)}><ClipboardCopy size={12}/>{en?'Copy':'복사'}</Button></div>
     <pre className="mt-1 overflow-x-auto rounded-lg bg-[#FAF7F1] p-3 text-[10px]">{coordinatorSetup}</pre>
     <p className="mt-2 text-[10px] leading-5">{en?'Loopback is safe by default. Other machines require an authenticated private HTTPS proxy or VPN listener.':'기본 루프백 포트는 이 PC에서만 연결됩니다. 다른 PC는 인증된 HTTPS 프록시 또는 VPN의 비공개 인터페이스로 연결하세요.'}</p>
     <div className="mt-3 flex items-center justify-between gap-2"><strong>{en?'2. Worker PC':'2. Worker PC'}</strong><Button size="sm" variant="ghost" onClick={()=>void navigator.clipboard.writeText(workerSetup)}><ClipboardCopy size={12}/>{en?'Copy':'복사'}</Button></div>
     <pre className="mt-1 overflow-x-auto rounded-lg bg-[#FAF7F1] p-3 text-[10px]">{workerSetup}</pre>
    </details>
   </div>
  </Panel>
 </section>
}

/** Active team details are shown in Chat without duplicating the Activity UI. */
export function TeamRunStrip({sessionId,active:visible,onDraft}:{
 sessionId?:string;active:boolean;onDraft:(prompt:string)=>void
}){
 const en=useLocale()==='en'
 const activity=useRemote<ActivityData>(visible&&sessionId?'/api/activity/teams':null,empty)
 const [busy,setBusy]=useState(false),[error,setError]=useState('')
 const team=activity.value.teams.find(row=>row.leadSessionId===sessionId
  && (row.status==='running'||row.status==='needs_review'))
 if(!team)return null
 const pending=team.pendingRemoteArtifactTaskIds.length
 const done=team.tasks.filter(task=>task.status==='completed').length
 const action=async()=>{
  if(pending){window.location.hash='#/activity';return}
  if(team.status==='needs_review'){
   onDraft((en?'Inspect the uncertain native worker results for team ':'팀의 미확정 Worker 결과를 검토하고 필요하면 안전하게 복구해 주세요. teamId: ')+team.id)
   return
  }
  setBusy(true);setError('')
  try{
   await post('/api/activity/teams/'+encodeURIComponent(team.id)+'/resume',{})
   void activity.reload()
  }catch(e){setError((e as Error).message)}
  finally{setBusy(false)}
 }
 return <div role="status" aria-label={en?'Active agent team':'실행 중인 에이전트 팀'} className="mb-2 flex min-w-0 flex-wrap items-center gap-2 rounded-xl border border-[#E7E0D5] bg-[#FAF7F1] px-3 py-2 text-[11px]">
  <UsersRound size={14} className="shrink-0 text-[#7C7469]"/>
  <span className="min-w-0 flex-1 text-[#5C544A]"><strong>{en?'Team':'팀'} {team.id.slice(-7)}</strong> · {done}/{team.tasks.length}{pending>0&&<span className="ml-1 font-semibold text-amber-800">{en?'File review pending':'파일 검토 대기'} {pending}</span>}</span>
  <StatusPill status={team.status}/>
  <Button size="sm" variant="ghost" disabled={busy||(!pending&&team.status==='running'&&team.resumeAvailable===false)} onClick={()=>void action()}>{pending?(en?'Review files':'파일 검토'):(en?'Resume':'이어가기')}<ChevronRight size={13}/></Button>
  {error&&<p className="w-full text-rose-600">{error}</p>}
 </div>
}
