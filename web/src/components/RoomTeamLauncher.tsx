import {useEffect,useRef,useState,type FormEvent} from 'react'
import {AlertCircle,ArrowRight,Clock3,LoaderCircle,RefreshCw,Settings2,ShieldCheck,UsersRound} from 'lucide-react'
import {post} from '../api'
import {Button,SelectInput,TextArea} from './Primitives'
import {Notice,refresh,useRemote} from './UI'
import {TeamWorkflowPanel} from './TeamWorkflowPanel'
import {useLocale} from '../i18n'

type Launch={id:string;roomId:string;sessionId:string;status:'planning'|'queued'|'failed'|'uncertain';goal:string;maxParallel:number;jobId?:string|null;error?:string|null}
type PendingLaunch={goal:string;maxParallel:number;requestId:string}
const storageKey=(roomId:string)=>'dominion.room.team.launch.'+roomId
const pendingKey=(roomId:string)=>'dominion.room.team.pending.'+roomId
function readPending(roomId:string):PendingLaunch|null{
 try{
  const p=JSON.parse(localStorage.getItem(pendingKey(roomId))||'null') as Partial<PendingLaunch>|null
  if(p&&typeof p.goal==='string'&&p.goal.length>0&&p.goal.length<=4000&&
   typeof p.requestId==='string'&&/^[\da-f-]{36}$/i.test(p.requestId)&&
   typeof p.maxParallel==='number'&&p.maxParallel>=1&&p.maxParallel<=4)
   return {goal:p.goal,maxParallel:p.maxParallel,requestId:p.requestId}
 }catch{}
 return null
}

/** Room-native Team composer. Always an explicit user action; not a new agent
 * executor. Server persists the nonce before paid planning and owns the DAG. */
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
 const launch=useRemote<Launch>(endpoint,{id:'',roomId,sessionId:sessionId||'',status:'planning',goal:'',maxParallel:3})
 useEffect(()=>{
  if(!active||!launchId||launch.value.status!=='planning'||launch.error)return
  const timer=setInterval(()=>void launch.reload(),2000)
  return()=>clearInterval(timer)
 },[active,launchId,launch.value.status,launch.error,launch.reload])
 useEffect(()=>{
  if(launch.value.status==='queued'&&launch.value.jobId)refresh('/api/jobs')
 },[launch.value.status,launch.value.jobId])
 const start=async(event:FormEvent)=>{
  event.preventDefault()
  const trimmed=goal.trim()
  if(!trimmed||!toolsEnabled||inFlight.current)return
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
 const failed=launch.value.status==='failed'||launch.value.status==='uncertain'
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
    ?'The lead picks suitable agents from your existing room. Not all members are automatically assigned.'
    :'현재 그룹의 AI 중 작업에 맞는 멤버가 배정됩니다. 9명 모두가 반드시 실행되는 것은 아닙니다.'}</p>
   <div className="room-team-agent-list">
    {agents.map(agent=><span className="room-team-agent-chip" key={agent.id} title={agent.name}>{agent.name}</span>)}
   </div>
  </section>
  <section className="space-y-4" aria-label={en?'Assign work':'협업 작업 만들기'}>
   <div className="flex items-center gap-3">
    <span className="room-team-section-number">1</span>
    <div><h3 className="text-[15px] font-bold">{en?'Describe the result you want':'어떤 일을 맡길까요?'}</h3>
     <p className="text-[12px] text-muted">{en?'Say what should be done and what a successful result looks like.':'해야 할 일과 원하는 최종 결과를 적어주세요.'}</p>
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
     placeholder={en?'Example: Find the cause of these bugs, prepare fixes, verify them, and report the results.':'예: 현재 프로젝트의 오류 원인을 찾아 수정하고 테스트 결과까지 정리해 주세요.'}
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
         {en?'1–4 tasks at once. The system decides the order and dependencies.':'1~4개를 동시에 처리합니다. 순서와 선행 조건은 AI가 결정합니다.'}</small>
       </span>
       <SelectInput aria-label={en?'Concurrent tasks':'동시 실행 작업 수'} className="!w-24 !py-2 !text-[13px]"
        value={parallel} disabled={busy} onChange={e=>{setParallel(Number(e.target.value));clearPending()}}>
        {[1,2,3,4].map(n=><option key={n} value={n}>{n}</option>)}
       </SelectInput>
      </label>
     </div>
    </details>
    <div className="flex flex-wrap items-center justify-between gap-3 pt-1">
     <p className="text-[12px] leading-5 text-muted">
      {en?'Up to 8 steps. Existing budget and tool permissions apply.':'최대 8개 단계로 분담하며 기존 도구 권한과 예산을 따릅니다.'}
     </p>
     <Button variant="primary" type="submit" className="!min-h-11 !px-5 !text-[14px]"
      disabled={busy||!toolsEnabled||!sessionId||!goal.trim()}>
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
    {launch.value.status==='planning'?<LoaderCircle size={19} className="shrink-0 animate-spin"/>:
     failed?<AlertCircle size={19} className="shrink-0"/>:<Clock3 size={19} className="shrink-0"/>}
    <div className="min-w-0">
     <strong className="block text-[13px]">{launch.value.status==='planning'?(en?'Making the task plan…':'AI가 작업 계획을 만드는 중입니다…'):
      launch.value.status==='uncertain'?(en?'Planning status is uncertain':'계획 생성 결과를 확인해야 합니다'):
      failed?(en?'Could not start the task':'작업을 시작하지 못했습니다'):(en?'Work has been queued':'계획을 만들고 작업을 예약했습니다')}</strong>
     {failed?<p role="alert" className="mt-1 text-[12px] leading-5">{launch.value.error||'작업 계획 생성에 실패했습니다.'}</p>:
      <p className="mt-1 text-[12px] leading-5">{launch.value.status==='planning'
       ?(en?'Planning may use the selected model. You can close this window and reopen it later.':'창을 닫아도 요청 기록이 남습니다. 다시 열어 진행 상태를 확인할 수 있습니다.')
       :(en?'Queued is not the same as finished. Check the steps below.':'예약 완료는 작업 완료가 아닙니다. 아래 단계별 진행 상황을 확인하세요.')}</p>}
     {launch.value.status==='uncertain'&&<p className="mt-1 text-[12px]">{en?'Review the previous call before trying a new request.':'새 작업을 시작하기 전에 이전 호출의 상태와 비용을 확인하세요.'}</p>}
    </div>
   </div>}
   {launchId&&launch.loading&&<p role="status" className="flex items-center gap-2 text-[12px] text-muted">
    <LoaderCircle size={15} className="animate-spin"/>{en?'Loading saved task…':'저장된 작업 상태 불러오는 중…'}
   </p>}
   <TeamWorkflowPanel filterSessionId={sessionId} agentNames={Object.fromEntries(agents.map(a=>[a.id,a.name]))}/>
  </section>
 </div>
}
