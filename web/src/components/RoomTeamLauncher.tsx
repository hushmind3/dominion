import {useEffect,useRef,useState,type FormEvent} from 'react'
import {ArrowUpRight,Check,GitBranch,LoaderCircle,RefreshCw,ShieldCheck} from 'lucide-react'
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
export function RoomTeamLauncher({roomId,sessionId,participantCount,toolsEnabled,onEnableTools,active}:{
 roomId:string;sessionId?:string;participantCount:number;toolsEnabled:boolean;
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
 return <div className="min-w-0 space-y-3">
  <div className="flex flex-wrap items-center justify-between gap-2">
   <div className="flex min-w-0 items-center gap-2">
    <GitBranch size={17} className="shrink-0 text-[#A4583E]"/>
    <div><strong className="text-[13px] text-[#3A342D]">{en?'Run Team in this room':'이 그룹에서 Team 실행'}</strong>
     <p className="text-[11px] leading-5 text-[#8A8176]">{en?(participantCount+' room agents · 1–8 work steps · up to 4 parallel'):
      ('그룹 AI '+participantCount+'명 중 배정 · 작업 1~8단계 · 최대 4개 동시 실행')}</p>
    </div>
   </div>
   {launchId&&<Button size="sm" variant="ghost" onClick={()=>void launch.reload()} disabled={launch.loading}>
    <RefreshCw size={13}/>{en?'Check status':'상태 확인'}
   </Button>}
  </div>
  {!toolsEnabled&&<div className="flex flex-wrap items-center gap-2 rounded-xl border border-amber-200 bg-amber-50 px-3 py-2">
   <ShieldCheck size={15} className="shrink-0 text-amber-800"/>
   <p className="flex-1 text-[11px] text-amber-900">{en?'Room tools are off. Enable tools to launch Team.':'그룹 도구 사용이 꺼져 있습니다. Team 실행 전 먼저 켜야 합니다.'}</p>
   <Button size="sm" onClick={()=>void onEnableTools()}>{en?'Enable room tools':'그룹 도구 켜기'}</Button>
  </div>}
  {recovered&&requestId&&<p role="status" className="text-[11px] text-amber-800">
   {en?'An earlier launch may have reached the server. Submitting the same goal reuses its original request ID.':'이전 요청의 응답이 끊겼을 수 있습니다. 같은 목표로 다시 누르면 이전 요청 ID를 재사용합니다.'}
  </p>}
  <form className="flex min-w-0 flex-col gap-2" onSubmit={event=>void start(event)}>
   <label className="text-[11px] font-semibold text-[#5C544A]" htmlFor={'team-goal-'+roomId}>{en?'Team goal':'Team 작업 목표'}</label>
   <TextArea id={'team-goal-'+roomId} rows={2} maxLength={4000} value={goal}
    onChange={e=>{setGoal(e.target.value);clearPending()}} disabled={busy}
    placeholder={en?'Describe work and the expected outcome…':'예: 버그를 조사하고 수정안을 만든 뒤 독립 검증해줘'}
    className="!min-h-16 !resize-y !bg-white !text-xs"/>
   <div className="flex flex-wrap items-center gap-2">
    <label className="flex items-center gap-2 text-[11px] text-[#6F675D]">{en?'Parallel workers':'동시 Worker'}
     <SelectInput className="!w-20 !py-1.5 !text-xs" value={parallel} disabled={busy}
      onChange={e=>{setParallel(Number(e.target.value));clearPending()}}>
      {[1,2,3,4].map(n=><option key={n} value={n}>{n}</option>)}
     </SelectInput>
    </label>
    <Button variant="primary" type="submit" size="sm" disabled={busy||!toolsEnabled||!sessionId||!goal.trim()}>
     {busy?<LoaderCircle size={14} className="animate-spin"/>:<ArrowUpRight size={14}/>}
     {busy?(en?'Submitting':'요청 중'):(en?'Launch Team':'Team 실행')}
    </Button>
    <span className="text-[10px] leading-5 text-[#8A8176]">{en?'Lead plans up to 8 steps under existing permissions and budgets.':'Lead가 최대 8단계를 계획하며 기존 권한·예산 규칙을 따릅니다.'}</span>
   </div>
  </form>
  <Notice message={error||launch.error}/>
  {launchId&&!launch.error&&<div role="status" className="rounded-xl border border-[#E7E0D5] bg-white px-3 py-2 text-[11px] text-[#5C544A]">
   <div className="flex flex-wrap items-center gap-2">
    {launch.value.status==='planning'?<LoaderCircle size={13} className="animate-spin"/>:
     failed?<ShieldCheck size={13} className="text-rose-600"/>:<Check size={13} className="text-emerald-600"/>}
    <strong>{launch.value.status==='planning'?(en?'Lead is planning…':'Lead가 작업 계획 생성 중…'):
     launch.value.status==='uncertain'?(en?'Previous planning outcome uncertain':'이전 계획 요청 상태 불확실'):
     failed?(en?'Team launch failed':'Team 실행 요청 실패'):(en?'Worker jobs queued':'Team Worker 작업 예약 완료')}</strong>
    {launch.value.jobId&&<span className="break-all font-mono text-[10px]">{launch.value.jobId}</span>}
   </div>
   {failed&&<p role="alert" className="mt-1 text-rose-700">{launch.value.error||'Team 계획 생성에 실패했습니다.'}</p>}
   {failed&&<p className="mt-1 text-[#8A8176]">{launch.value.status==='uncertain'
    ?(en?'Check the status before creating a new request; the old call may have incurred costs.':'새 요청을 만들기 전에 상태를 확인하세요. 이전 호출에서 비용이 발생했을 수 있습니다.')
    :(en?'A new explicit request is required to retry; previous paid calls are never replayed automatically.':'이전 유료 호출은 자동 재실행하지 않습니다. 다시 실행하려면 직접 요청하세요.')}</p>}
  </div>}
  <TeamWorkflowPanel filterSessionId={sessionId}/>
 </div>
}
