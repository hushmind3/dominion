import {useEffect,useRef,useState,type FormEvent} from 'react'
import {AlertCircle,ArrowRight,Clock3,LoaderCircle,RefreshCw,Settings2,ShieldCheck,UsersRound} from 'lucide-react'
import {post} from '../api'
import {Button,SelectInput,TextArea} from './Primitives'
import {Notice,refresh,useRemote} from './UI'
import {TeamWorkflowPanel} from './TeamWorkflowPanel'
import {useLocale} from '../i18n'

type Launch={id:string;roomId:string;sessionId:string;status:'planning'|'queued'|'running'|'completed'|'needs_review'|'failed'|'cancelled'|'interrupted'|'uncertain';goal:string;maxParallel:number;taskCount?:number;jobId?:string|null;error?:string|null}
type PendingLaunch={goal:string;maxParallel:number;requestId:string}
const storageKey=(roomId:string)=>'dominion.room.team.launch.'+roomId
const pendingKey=(roomId:string)=>'dominion.room.team.pending.'+roomId
function readPending(roomId:string):PendingLaunch|null{
 try{
  const p=JSON.parse(localStorage.getItem(pendingKey(roomId))||'null') as Partial<PendingLaunch>|null
  if(p&&typeof p.goal==='string'&&p.goal.length>0&&p.goal.length<=4000&&
   typeof p.requestId==='string'&&/^[\da-f-]{36}$/i.test(p.requestId)&&
   typeof p.maxParallel==='number'&&p.maxParallel>=1&&p.maxParallel<=4)
   return {goal:p.goal,maxParallel:Math.min(3,p.maxParallel),requestId:p.requestId}
 }catch{}
 return null
}

/** Room-native Team composer. Always an explicit user action; not a new agent
 * executor. Server uses a durable request identity and owns the DAG. */
export function RoomTeamLauncher({roomId,sessionId,participantCount,agents,toolsEnabled,onEnableTools,active}:{
 roomId:string;sessionId?:string;participantCount:number;agents:{id:string;name:string}[];toolsEnabled:boolean;
 onEnableTools:()=>Promise<void>;active:boolean
}){
 const en=useLocale()==='en'
 const restored=useRef(readPending(roomId))
 const [goal,setGoal]=useState(restored.current?.goal||'')
 const [parallel,setParallel]=useState(restored.current?.maxParallel||3)
 const [requestId,setRequestId]=useState<string|null>(restored.current?.requestId||null)
 const [recovered,setRecovered]=useState(Boolean(restored.current))
 const [launchId,setLaunchId]=useState<string|null>(()=>{
  try{return localStorage.getItem(storageKey(roomId))}catch{return null}
 })
 const [busy,setBusy]=useState(false)
 const inFlight=useRef(false)
 const [error,setError]=useState('')
 const endpoint=launchId?'/api/rooms/'+encodeURIComponent(roomId)+'/team-launches/'+encodeURIComponent(launchId):null
 const launch=useRemote<Launch>(endpoint,{id:'',roomId,sessionId:sessionId||'',status:'queued',goal:'',maxParallel:3})
 useEffect(()=>{
  if(!active||!launchId||!['queued','running','planning'].includes(launch.value.status)||launch.error)return
  const timer=setInterval(()=>void launch.reload(),2000)
  return()=>clearInterval(timer)
 },[active,launchId,launch.value.status,launch.error,launch.reload])
 useEffect(()=>{
  if(launch.value.jobId)refresh('/api/jobs')
 },[launch.value.status,launch.value.jobId])
 const start=async(event:FormEvent)=>{
  event.preventDefault()
  const trimmed=goal.trim()
  if(!trimmed||(!toolsEnabled&&!requestId)||inFlight.current)return
  inFlight.current=true;setBusy(true);setError('')
  const id=requestId||crypto.randomUUID()
  setRequestId(id)
  try{localStorage.setItem(pendingKey(roomId),JSON.stringify({
   goal:trimmed,maxParallel:parallel,requestId:id
  }))}catch{}
  try{
   const result=await post<Launch>('/api/rooms/'+encodeURIComponent(roomId)+'/team-launches',{
    goal:trimmed,maxParallel:parallel,clientRequestId:id,
   })
   setLaunchId(result.id)
   try{
    localStorage.setItem(storageKey(roomId),result.id)
    localStorage.removeItem(pendingKey(roomId))
   }catch{}
   setRecovered(false);setRequestId(null);setGoal('')
   refresh('/api/jobs')
  }catch(e){setError((e as Error).message)}
  finally{inFlight.current=false;setBusy(false)}
 }
 const failed=['failed','uncertain','needs_review','cancelled','interrupted'].includes(launch.value.status)
 const running=['queued','running','planning'].includes(launch.value.status)
 const clearPending=()=>{
  try{localStorage.removeItem(pendingKey(roomId))}catch{}
  setRequestId(null);setRecovered(false)
 }
 const lastLaunchReady=Boolean(launchId&&!launch.loading&&!launch.error)
 return <div className="room-team-workspace">
  <section className="room-team-members" aria-label={en?'Room team members':'그룹 참여 AI'}>
   <div className="flex items-center justify-between gap-3">
    <div className="flex items-center gap-2"><UsersRound size={18} className="text-brand"/>
     <strong className="text-[14px]">{en?'Available room agents':'함께 일할 AI'}</strong>
    </div>
    <span className="rounded-full bg-surface px-3 py-1 text-[12px] font-semibold text-muted">
     {en?participantCount+' members':participantCount+'명 참여'}
    </span>
   </div>
   <p className="mt-1 text-[12px] leading-5 text-muted">{en
    ?'The server assigns up to four enabled agents with a configured provider, in room order.'
    :'그룹 순서에 따라 연결된 활성 AI 중 최대 4명이 배정됩니다.'}</p>
   <div className="room-team-agent-list">
    {agents.map(agent=><span className="room-team-agent-chip" key={agent.id} title={agent.name}>{agent.name}</span>)}
   </div>
  </section>
  <section className="space-y-4" aria-label={en?'Assign work':'협업 작업 만들기'}>
   <div className="flex items-center gap-3">
    <span className="room-team-section-number">1</span>
    <div><h3 className="text-[15px] font-bold">{en?'Describe the result you want':'어떤 일을 맡길까요?'}</h3>
     <p className="text-[12px] text-muted">{en?'Describe the research or inspection you want. This room launcher runs read-only workers.':'조사하거나 확인할 내용을 적어주세요. 현재 그룹 협업은 읽기 전용으로 실행됩니다.'}</p>
    </div>
   </div>
   {!toolsEnabled&&<div className="room-team-permission" role="status">
    <ShieldCheck size={20} className="shrink-0 text-warning"/>
    <div className="min-w-0 flex-1">
     <strong className="text-[13px]">{en?'Tool access is off':'그룹의 도구 사용이 꺼져 있습니다'}</strong>
     <p className="mt-1 text-[12px] leading-5">{en?'Allow this room to use its existing tools before starting a task.':'AI가 실제 조사를 하고 작업을 수행하려면 이 그룹의 도구 사용을 켜야 합니다.'}</p>
    </div>
    <Button size="sm" variant="secondary" className="!min-h-10 shrink-0" onClick={()=>void onEnableTools()}>
     {en?'Enable tools':'도구 사용 켜기'}
    </Button>
   </div>}
   {recovered&&requestId&&<p role="status" className="rounded-lg bg-warning-soft px-3 py-2 text-[12px] leading-5 text-warning">
    {en?'A previous attempt may have reached the server. Try again without editing to reuse that request.':'이전 요청이 서버에 전달되었을 수 있습니다. 내용을 바꾸지 않고 다시 실행하면 중복 작업 없이 같은 요청을 확인합니다.'}
   </p>}
   <form className="space-y-3" onSubmit={event=>void start(event)}>
    <label className="block text-[13px] font-semibold" htmlFor={'team-goal-'+roomId}>
     {en?'Task description':'맡길 작업'}
    </label>
    <TextArea id={'team-goal-'+roomId} rows={4} maxLength={4000} value={goal} autoFocus
     onChange={e=>{setGoal(e.target.value);clearPending()}} disabled={busy}
     placeholder={en?'Example: Inspect the project, identify the cause of a bug, and suggest fixes with evidence.':'예: 프로젝트에서 오류 원인을 찾아 근거와 수정 방안을 보고해 주세요.'}
     className="!min-h-28 !rounded-xl !bg-surface !px-4 !py-3 !text-[14px] !leading-6"/>
    <details className="room-team-advanced">
     <summary className="flex cursor-pointer items-center gap-2 text-[13px] font-semibold text-muted focus-visible:outline-2 focus-visible:outline-offset-2">
      <Settings2 size={16}/>{en?'Execution settings':'실행 설정'}
      <span className="ml-auto text-[12px] font-normal">{en?parallel+' concurrent tasks':'동시에 '+parallel+'개 작업'}</span>
     </summary>
     <div className="mt-3 border-t border-divider pt-3">
      <label className="flex flex-wrap items-center justify-between gap-3 text-[13px] text-ink">
       <span><strong>{en?'Concurrent tasks':'동시 실행 작업 수'}</strong>
        <small className="mt-1 block text-[12px] text-muted">
         {en?'1–3 workers at once. The engine enforces the concurrency limit.':'1~3개 작업을 동시에 실행하며 백엔드가 제한을 적용합니다.'}</small>
       </span>
       <SelectInput aria-label={en?'Concurrent tasks':'동시 실행 작업 수'} className="!w-24 !py-2 !text-[13px]"
        value={parallel} disabled={busy} onChange={e=>{setParallel(Number(e.target.value));clearPending()}}>
        {[1,2,3].map(n=><option key={n} value={n}>{n}</option>)}
       </SelectInput>
      </label>
     </div>
    </details>
    <div className="flex flex-wrap items-center justify-between gap-3 pt-1">
     <p className="text-[12px] leading-5 text-muted">
      {en?'Up to four read-only workers. No file writes or shell commands are granted.':'최대 4명의 읽기 전용 작업입니다. 파일 수정·셸 실행 권한은 부여하지 않습니다.'}
     </p>
     <Button variant="primary" type="submit" className="!min-h-11 !px-5 !text-[14px]"
      disabled={busy||(!toolsEnabled&&!requestId)||!goal.trim()}>
      {busy?<LoaderCircle size={17} className="animate-spin"/>:<ArrowRight size={18}/>}
      {busy?(en?'Starting task…':'작업 시작 중…'):(en?'Start collaboration':'협업 작업 시작')}
     </Button>
    </div>
   </form>
  </section>
  <section className="room-team-results" aria-label={en?'Tasks and outcomes':'작업 진행 및 결과'}>
   <header className="flex flex-wrap items-center justify-between gap-3">
    <div className="flex items-center gap-3">
     <span className="room-team-section-number">2</span>
     <div><h3 className="text-[15px] font-bold">{en?'Progress and results':'진행 상황과 결과'}</h3>
      <p className="text-[12px] text-muted">{en?'Each agent’s work appears below as it is recorded.':'AI별 진행 단계와 실제 결과를 여기서 확인합니다.'}</p>
     </div>
    </div>
    {launchId&&<Button size="sm" variant="secondary" className="!min-h-9 !text-[12px]" onClick={()=>void launch.reload()} disabled={launch.loading}>
     <RefreshCw size={15}/>{en?'Refresh':'새로고침'}
    </Button>}
   </header>
   <Notice message={error||launch.error}/>
   {lastLaunchReady&&<div role="status" className={'room-team-launch-status '+(failed?'room-team-launch-status-error':'')}>
    {running?<LoaderCircle size={19} className="shrink-0 animate-spin"/>:
     failed?<AlertCircle size={19} className="shrink-0"/>:<Clock3 size={19} className="shrink-0"/>}
    <div className="min-w-0">
     <strong className="block text-[13px]">{({
      planning:en?'Checking the queued job':'접수된 작업 확인 중',
      queued:en?'Work queued':'협업 작업 대기 중',running:en?'Workers running':'협업 작업 실행 중',
      completed:en?'Team execution complete':'협업 실행 완료',needs_review:en?'Review needed':'실행 결과 확인 필요',
      cancelled:en?'Stopped':'작업 중단됨',interrupted:en?'Interrupted':'실행이 중단됨',
      failed:en?'Execution failed':'협업 실행 실패',uncertain:en?'Job state uncertain':'작업 상태 확인 필요'
     } as Record<string,string>)[launch.value.status]||launch.value.status}</strong>
     {typeof launch.value.taskCount==='number'&&<p className="mt-1 text-[12px] text-muted">{en?'Workers':'실제 배정 작업'} {launch.value.taskCount} · {en?'Concurrency':'동시 실행 상한'} {launch.value.maxParallel}</p>}
     {failed&&launch.value.error&&<p role="alert" className="mt-1 text-[12px] leading-5">{launch.value.error}</p>}
     {running&&<p className="mt-1 text-[12px] leading-5">{en?'Queued is not the same as finished. Check the steps below.':'예약 완료는 작업 완료가 아닙니다. 아래 단계별 진행 상황을 확인하세요.'}</p>}
     {launch.value.status==='completed'&&<p className="mt-1 text-[12px] leading-5">{en?'Review recorded results and evidence below. Completion does not imply file changes.':'저장된 결과와 검증 근거를 아래에서 확인하세요. 실행 완료가 파일 수정을 의미하지는 않습니다.'}</p>}
     {launch.value.status==='uncertain'&&<p className="mt-1 text-[12px]">{en?'Review the previous call before trying a new request.':'새 작업을 시작하기 전에 이전 호출의 상태와 비용을 확인하세요.'}</p>}
    </div>
   </div>}
   {launchId&&launch.loading&&<p role="status" className="flex items-center gap-2 text-[12px] text-muted">
    <LoaderCircle size={15} className="animate-spin"/>{en?'Loading saved task…':'저장된 작업 상태 불러오는 중…'}
   </p>}
   {(sessionId||(launch.value.id&&launch.value.sessionId))&&<TeamWorkflowPanel filterSessionId={sessionId||launch.value.sessionId} agentNames={Object.fromEntries(agents.map(a=>[a.id,a.name]))}/>}
  </section>
 </div>
}
