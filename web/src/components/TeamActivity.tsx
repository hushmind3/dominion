import {useRef,useState} from 'react'
import {CheckCircle2,RefreshCw,Square,UsersRound} from 'lucide-react'
import {post,type Agent} from '../api'
import {useLocale} from '../i18n'
import {Button,SelectInput,TextArea} from './Primitives'
import {useRemote} from './UI'

type NativeTask={id:string;agentId?:string;displayState?:string;status:string;verified?:boolean;pendingApproval?:boolean}
type NativeTeam={id:string;leadSessionId?:string;status:string;verified:boolean;resumeAvailable:boolean;tasks:NativeTask[]}
type LeadJob={id:string;status:string;error?:string|null}
type Activity={leadJobs:LeadJob[];teams:NativeTeam[]}
const empty:Activity={leadJobs:[],teams:[]}

/** Operator view over durable Sub2 Team and Lead jobs, not a second planner.
 * Existing Rust team_controls owns validation, worker admission and approval.
 */
export function TeamActivity(){
 const en=useLocale()==='en'
 const activity=useRemote<Activity>('/api/activity/teams',empty)
 const agents=useRemote<Agent[]>('/api/agents',[])
 const candidates=agents.value.filter(agent=>agent.enabled!==false&&agent.providerId&&agent.model)
 const [leadId,setLeadId]=useState('')
 const [workers,setWorkers]=useState<string[]>([])
 const [prompt,setPrompt]=useState('')
 const [error,setError]=useState('')
 const [notice,setNotice]=useState('')
 const [busy,setBusy]=useState(false)
 const inFlight=useRef(false)
 const lead=candidates.some(agent=>agent.id===leadId)?leadId:candidates[0]?.id||''
 const chosen=workers.filter(id=>id!==lead&&candidates.some(a=>a.id===id)).slice(0,8)
 const run=async()=>{
  if(inFlight.current||!lead||!chosen.length||!prompt.trim())return
  inFlight.current=true;setBusy(true);setError('');setNotice('')
  try{
   const result=await post<{jobId:string}>('/api/activity/teams/start',{
    prompt:prompt.trim(),leadAgentId:lead,workerAgentIds:chosen
   })
   setNotice((en?'Lead queued: ':'Lead 작업 예약: ')+result.jobId)
   setPrompt('')
   await activity.reload()
  }catch(e){setError((e as Error).message)}
  finally{inFlight.current=false;setBusy(false)}
 }
 const control=async(id:string,action:'stop'|'resume')=>{
  if(inFlight.current)return
  if(action==='stop'&&!window.confirm(en?'Stop this native Team?':'네이티브 팀을 중단하시겠습니까?'))return
  inFlight.current=true;setBusy(true);setError('');setNotice('')
  try{
   await post('/api/activity/teams/'+encodeURIComponent(id)+'/'+action,{})
   await activity.reload()
  }catch(e){setError((e as Error).message)}
  finally{inFlight.current=false;setBusy(false)}
 }
 return <section className="mt-5 space-y-4" aria-label={en?'Native Team activity':'네이티브 팀 작업'}>
  <div className="flex flex-wrap items-center justify-between gap-2">
   <h2 className="flex items-center gap-2 text-base font-bold text-ink"><UsersRound size={18}/>{en?'Team activity':'팀 작업 현황'}</h2>
   <Button type="button" variant="secondary" size="sm" onClick={()=>void activity.reload()} disabled={activity.loading}>
    <RefreshCw size={14}/>{en?'Refresh':'새로고침'}
   </Button>
  </div>
  <form onSubmit={event=>{event.preventDefault();void run()}} className="grid gap-3 rounded-xl border border-divider bg-surface p-4">
   <h3 className="text-sm font-semibold">{en?'Start a native Lead/Worker task':'네이티브 Lead·Worker 작업 시작'}</h3>
   <div className="grid gap-3 sm:grid-cols-2">
    <label className="text-xs font-semibold">{en?'Lead':'Lead AI'}
     <SelectInput className="mt-1" value={lead} onChange={event=>{setLeadId(event.target.value);setWorkers(v=>v.filter(id=>id!==event.target.value))}}>
      {!candidates.length&&<option value="">{en?'Connect a model first':'모델을 먼저 연결하세요'}</option>}
      {candidates.map(a=><option value={a.id} key={a.id}>{a.name}</option>)}
     </SelectInput>
    </label>
    <fieldset className="grid gap-1 text-xs"><legend className="mb-1 font-semibold">{en?'Workers (1–8)':'Worker AI (1~8개)'}</legend>
     {candidates.filter(a=>a.id!==lead).map(a=><label key={a.id} className="flex items-center gap-2">
      <input type="checkbox" checked={chosen.includes(a.id)} disabled={busy||(!chosen.includes(a.id)&&chosen.length>=8)}
       onChange={event=>setWorkers(v=>event.target.checked?[...v,a.id]:v.filter(id=>id!==a.id))}/>
      {a.name}
     </label>)}
     {candidates.length<=1&&<span className="text-muted">{en?'Create one more agent first.':'Worker로 사용할 AI를 1개 더 연결하세요.'}</span>}
    </fieldset>
   </div>
   <TextArea rows={3} maxLength={4000} value={prompt} onChange={event=>setPrompt(event.target.value)}
    placeholder={en?'Describe the work and expected result':'작업 목표와 원하는 결과를 입력하세요'}/>
   <div className="flex justify-end"><Button type="submit" variant="primary" disabled={busy||!lead||!chosen.length||!prompt.trim()}>
    {en?'Start team':'팀 작업 시작'}
   </Button></div>
  </form>
  {error&&<p role="alert" className="text-xs text-rose-700">{error}</p>}
  {notice&&<p role="status" className="text-xs text-emerald-700">{notice}</p>}
  {activity.error&&<p role="alert" className="text-xs text-rose-700">{activity.error}</p>}
  {(activity.value.leadJobs||[]).filter(job=>['queued','running','failed'].includes(job.status)).map(job=><p key={job.id} className="rounded-lg border border-divider p-2 text-xs text-muted">
   Lead {job.id} · {job.status}{job.error?' · '+job.error:''}
  </p>)}
  {(activity.value.teams||[]).slice(0,15).map(team=><article key={team.id} className="rounded-xl border border-divider bg-surface p-3">
   <div className="flex flex-wrap items-center justify-between gap-3">
    <div><strong className="break-all text-sm text-ink">{team.id}</strong>
     <p className="mt-1 flex items-center gap-1 text-xs text-muted">
      {team.verified&&<CheckCircle2 size={14} className="text-emerald-600"/>}
      {team.status} · {team.verified?(en?'Verified':'검증됨'):(en?'Verification pending':'검증 미완료')}
     </p>
    </div>
    {team.status==='running'&&<div className="flex items-center gap-2">
     {team.resumeAvailable&&<Button size="sm" variant="secondary" disabled={busy} onClick={()=>void control(team.id,'resume')}>
      {en?'Resume':'재개'}
     </Button>}
     <Button size="sm" variant="secondary" disabled={busy} onClick={()=>void control(team.id,'stop')}>
      <Square size={13}/>{en?'Stop':'중단'}
     </Button>
    </div>}
   </div>
   <div className="mt-2 grid gap-1">
    {(team.tasks||[]).map(task=><div key={task.id} className="flex flex-wrap justify-between gap-2 border-t border-divider pt-2 text-xs">
     <span>{agents.value.find(a=>a.id===task.agentId)?.name||task.agentId||task.id}</span>
     <span className={task.pendingApproval?'text-amber-700':task.verified?'text-emerald-700':'text-muted'}>
      {task.pendingApproval?(en?'Approval needed':'승인 대기'):task.verified?(en?'Verified':'검증됨'):task.displayState||task.status}
     </span>
    </div>)}
   </div>
  </article>)}
  {!activity.loading&&!activity.error&&!(activity.value.teams||[]).length&&
   <p className="text-xs text-muted">{en?'No native Team record yet.':'저장된 네이티브 팀 작업이 없습니다.'}</p>}
 </section>
}
