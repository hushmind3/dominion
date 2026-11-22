import {useEffect} from 'react'
import {CheckCircle2,ChevronRight,LoaderCircle,RefreshCw,ShieldAlert} from 'lucide-react'
import {useLocale} from '../i18n'
import {Button} from './Primitives'
import {useRemote} from './UI'
import {selectNativeRoomTeamRuns,type NativeRoomJob,type NativeTeamActivity} from '../services/nativeRoomTeams'

const activityEmpty:NativeTeamActivity={teams:[]}
const stateLabel=(value:string,en:boolean)=>{
 const labels:Record<string,[string,string]>={
  queued:['대기','Queued'],assigned:['배정','Assigned'],running:['실행 중','Running'],
  completed:['완료','Completed'],verified:['검증 완료','Verified'],
  needs_review:['검토 필요','Review required'],approval_pending:['승인 대기','Approval pending'],
  file_review_pending:['파일 검토','File review'],waiting_predecessors:['선행 작업 대기','Waiting dependencies'],
  offline:['워커 오프라인','Worker offline'],failed:['실패','Failed'],
  cancelled:['중단','Cancelled'],launching:['시작 중','Starting']
 }
 return (labels[value]||[value,value])[en?1:0]
}
export function NativeRoomTeamActivity({roomId,sessionId,active,agentNames}:{
 roomId:string;sessionId?:string;active:boolean;agentNames:Record<string,string>
}){
 const en=useLocale()==='en'
 const jobs=useRemote<NativeRoomJob[]>(active?'/api/jobs':null,[])
 const activity=useRemote<NativeTeamActivity>(active?'/api/activity/teams':null,activityEmpty)
 useEffect(()=>{
  if(!active)return
  const tick=window.setInterval(()=>{
   if(document.visibilityState==='visible'){
    void jobs.reload();void activity.reload()
   }
  },5000)
  return()=>window.clearInterval(tick)
 },[active,jobs.reload,activity.reload])
 const entries=selectNativeRoomTeamRuns(jobs.value,activity.value.teams||[],roomId,sessionId)
 const refresh=()=>{void jobs.reload();void activity.reload()}
 return <section className="mt-4 rounded-xl border border-divider bg-surface p-4" aria-label={en?'Native collaboration progress':'네이티브 협업 진행 상황'}>
  <header className="mb-3 flex items-center justify-between gap-3">
   <strong className="text-sm text-ink">{en?'Saved native work':'실제 작업 기록'}</strong>
   <Button type="button" variant="ghost" size="sm" onClick={refresh}
    disabled={jobs.loading||activity.loading} aria-label={en?'Refresh progress':'진행 현황 새로고침'}>
    <RefreshCw size={14}/>{en?'Refresh':'새로고침'}
   </Button>
  </header>
  {(jobs.error||activity.error)&&<p role="alert" className="mb-3 text-xs text-rose-700">
   {jobs.error||activity.error}
  </p>}
  {!entries.length&&(jobs.loading||activity.loading)&&<p role="status" className="flex items-center gap-2 text-xs text-muted"><LoaderCircle size={14} className="animate-spin"/>{en?'Loading work history':'작업 기록 불러오는 중'}</p>}
  {!entries.length&&!jobs.loading&&!activity.loading&&!jobs.error&&!activity.error&&
   <p className="text-sm text-muted">{en?'No native collaboration jobs saved for this room.':'이 그룹에 저장된 네이티브 협업 작업이 없습니다.'}</p>}
  <div className="space-y-3">
   {entries.map(({lead,teams})=><article key={lead.id} className="rounded-lg border border-divider bg-white p-3">
    <div className="flex flex-wrap items-start justify-between gap-2">
     <div className="min-w-0 flex-1">
      <p className="break-words text-sm font-semibold text-ink">{lead.input?.goal||lead.id}</p>
      <p className="mt-1 text-xs text-muted">{en?'Lead job':'Lead 작업'} · {stateLabel(lead.status||'queued',en)}</p>
     </div>
     {lead.sessionId&&<a href={'#/chat?session='+encodeURIComponent(lead.sessionId)}
      className="inline-flex items-center gap-1 text-xs font-semibold text-brand">
      {en?'Open Lead chat':'Lead 대화'}<ChevronRight size={14}/>
     </a>}
    </div>
    {lead.error&&<p role="alert" className="mt-2 text-xs text-rose-700">{lead.error}</p>}
    {!teams.length&&<p className="mt-3 text-xs text-muted">{en?'Native Team is not created yet. The Lead job status above is authoritative.':'아직 실제 팀 DAG는 생성되지 않았습니다. 위 Lead 작업 상태가 현재 기록입니다.'}</p>}
    {teams.map(team=><section key={team.id} className="mt-3 rounded-lg border border-divider bg-canvas p-3">
     <div className="flex flex-wrap items-center justify-between gap-2">
      <strong className="text-xs text-ink">{en?'Team':'팀'} · {team.id}</strong>
      <span className="text-xs font-semibold text-muted">{stateLabel(team.status||'running',en)}</span>
     </div>
     <div className="mt-2 flex items-center gap-2 text-xs text-muted">
      {team.verified?<CheckCircle2 size={14} className="text-emerald-600"/>:<ShieldAlert size={14}/>}
      {team.verified?(en?'Native evidence verified':'네이티브 검증 완료'):
       (en?'Verification and owner review remain separate':'검증·소유자 승인은 별도로 확인')}
     </div>
     <div className="mt-3 space-y-2">
      {(team.tasks||[]).map(task=><div key={task.id} className="flex flex-wrap items-center gap-2 border-t border-divider pt-2 text-xs">
       <span className="min-w-0 flex-1 text-ink">{agentNames[task.agentId||'']||task.agentId||task.id}</span>
       <span className={task.verified?'font-semibold text-emerald-700':'text-muted'}>
        {task.verified?(en?'Verified':'검증됨'):stateLabel(task.displayState||task.status||'queued',en)}
       </span>
       {task.pendingApproval&&<span className="font-semibold text-amber-700">{en?'Approval needed':'승인 필요'}</span>}
       {task.error&&<span className="w-full break-words text-rose-700">{task.error}</span>}
      </div>)}
     </div>
    </section>)}
   </article>)}
  </div>
 </section>
}
