import {useState} from 'react'
import {CheckCircle2,ChevronDown,ChevronRight,GitMerge,Network,RefreshCw,UsersRound} from 'lucide-react'
import {Button} from './Primitives'
import {StatusPill} from './StatusPill'
import {Panel,useRemote} from './UI'
import {useLocale} from '../i18n'

type Worker={id:string;online:boolean;allowWrites:boolean;agentCount:number}
type TeamTask={
 id:string;agentId?:string;status:string;readOnly:boolean;verified:boolean;
 dependsOn:string[];remoteWorkerId?:string;remoteTaskId?:string;
 artifactApplyStatus?:string;jobId?:string;error?:string
}
type Team={
 id:string;status:string;leadSessionId?:string;leadAgentId?:string;
 verified:boolean;createdAt?:string;updatedAt?:string;
 pendingRemoteArtifactTaskIds:string[];tasks:TeamTask[]
}
type Activity={teams:Team[];remoteWorkers:Worker[]}
const empty:Activity={teams:[],remoteWorkers:[]}

function goToLead(sessionId?:string){
 if(!sessionId)return
 // The existing App hash router owns navigation and will reuse ChatPage.
 window.location.hash='#/chat?session='+encodeURIComponent(sessionId)
}

/** Read-only operator view. Native approvals stay within the Lead chat/tool policy. */
export function TeamActivity(){
 const lang=useLocale(),en=lang==='en'
 const {value,loading,error,reload}=useRemote<Activity>('/api/activity/teams',empty)
 const [selected,setSelected]=useState<string>('')
 const running=value.teams.filter(team=>team.status==='running').length
 const review=value.teams.filter(team=>team.status==='needs_review').length
 return <section className="mt-10" aria-label={en?'Agent teams':'에이전트 작업 팀'}>
  <div className="mb-3 flex flex-wrap items-center justify-between gap-3">
   <div className="flex min-w-0 items-center gap-2">
    <UsersRound size={17} className="text-[#7C7469]"/>
    <h3 className="m-0 text-[14px] font-bold text-[#3A342D]">{en?'Agent teams':'멀티에이전트 팀 작업'}</h3>
    {running>0&&<span className="rounded-md bg-[#F7E8E1] px-2 py-0.5 text-[10px] text-[#A94E30]">{en?'Running':'진행'} {running}</span>}
    {review>0&&<span className="rounded-md bg-amber-50 px-2 py-0.5 text-[10px] text-amber-800">{en?'Review':'검토'} {review}</span>}
   </div>
   <Button size="sm" variant="ghost" onClick={()=>void reload()} aria-label={en?'Refresh teams':'팀 작업 새로고침'}><RefreshCw size={13}/>{en?'Refresh':'새로고침'}</Button>
  </div>
  <Panel className="overflow-hidden p-4 sm:p-5">
   {error&&<p role="alert" className="mb-3 text-xs text-rose-600">{error}</p>}
   {!value.teams.length&&<p className="py-4 text-center text-[12px] text-[#8A8176]">{loading?(en?'Loading team activity…':'팀 작업 불러오는 중…'):(en?'No managed team jobs yet. Teams created by an agent appear here.':'아직 팀 작업이 없습니다. 에이전트가 실행한 협업 작업이 여기에 나타납니다.')}</p>}
   <div className="space-y-2">
    {value.teams.map(team=>{
     const open=selected===team.id
     const pending=team.pendingRemoteArtifactTaskIds.length
     const done=team.tasks.filter(task=>task.status==='completed').length
     return <div key={team.id} className="min-w-0 rounded-xl border border-[#E7E0D5] bg-white">
      <Button variant="bare" className="flex w-full min-w-0 items-center gap-3 rounded-xl px-3 py-3 text-left hover:bg-[#FAF7F1]" aria-expanded={open} onClick={()=>setSelected(open?'':team.id)}>
       {open?<ChevronDown size={16} className="shrink-0 text-[#8A8176]"/>:<ChevronRight size={16} className="shrink-0 text-[#8A8176]"/>}
       <span className="min-w-0 flex-1">
        <strong className="block truncate text-[12px] text-[#3A342D]">{team.leadAgentId||'Lead'} · {en?'Team':'팀'} {team.id.slice(-7)}</strong>
        <small className="mt-1 block text-[11px] text-[#8A8176]">{en?'Tasks':'작업'} {done}/{team.tasks.length}{pending>0?' · '+(en?'Files awaiting review':'파일 승인 대기')+' '+pending:''}</small>
       </span>
       <StatusPill status={team.status}/>
      </Button>
      {open&&<div className="border-t border-[#EFE9DF] px-3 py-3">
       <div className="space-y-2">
        {team.tasks.map(task=><div key={task.id} className="flex min-w-0 flex-wrap items-center gap-2 rounded-lg bg-[#FAF7F1] px-3 py-2 text-[11px]">
         <span className="min-w-0 flex-1"><strong className="block truncate text-[#3A342D]">{task.id} · {task.agentId||'Worker'}</strong>
          <small className="text-[#8A8176]">{task.remoteWorkerId?'Remote · '+task.remoteWorkerId:(en?'Local':'로컬')}{task.dependsOn?.length?' · '+(en?'After':'선행')+' '+task.dependsOn.join(', '):''}</small>
         </span>
         {task.verified&&<CheckCircle2 size={14} className="text-emerald-600" aria-label={en?'Verified':'검증됨'}/>}
         <StatusPill status={task.status}/>
         {task.artifactApplyStatus&&task.artifactApplyStatus!=='not_required'&&<span className={'rounded-md px-2 py-0.5 text-[10px] '+(task.artifactApplyStatus==='applied'?'bg-emerald-50 text-emerald-700':'bg-amber-50 text-amber-800')}>{en?'Files: ':'산출물: '}{task.artifactApplyStatus}</span>}
         {task.error&&<p className="w-full break-words text-rose-600">{task.error}</p>}
        </div>)}
       </div>
       {pending>0&&<div className="mt-3 rounded-lg border border-amber-200 bg-amber-50 p-3 text-[11px] text-amber-900">
        <div className="flex items-center gap-2 font-semibold"><GitMerge size={14}/>{en?'Remote changes require Lead approval':'원격 변경 파일 승인 필요'}</div>
        <p className="mt-1 leading-5">{en?'Return to the Lead conversation, review the remote task receipts, and approve the native team_remote_apply action before resuming dependent work.':'Lead 대화에서 원격 작업 결과를 검토한 뒤 team_remote_apply 승인을 진행해야 다음 작업을 실행할 수 있습니다.'}</p>
        <p className="mt-1 break-all font-mono text-[10px]">{team.pendingRemoteArtifactTaskIds.join(', ')}</p>
       </div>}
       {team.leadSessionId&&<Button size="sm" variant="secondary" className="mt-3" onClick={()=>goToLead(team.leadSessionId)}>{en?'Open Lead conversation':'Lead 대화 열기'} <ChevronRight size={13}/></Button>}
      </div>}
     </div>
    })}
   </div>
   {value.remoteWorkers.length>0&&<div className="mt-4 border-t border-[#EFE9DF] pt-3">
    <p className="mb-2 flex items-center gap-2 text-[11px] font-semibold text-[#5C544A]"><Network size={14}/>{en?'Remote Rust workers':'원격 Rust 작업자'}</p>
    <div className="flex flex-wrap gap-2">{value.remoteWorkers.map(node=><span key={node.id} className="inline-flex items-center gap-1.5 rounded-lg border border-[#E7E0D5] bg-[#FAF7F1] px-2.5 py-1.5 text-[11px]"><span className={'size-1.5 rounded-full '+(node.online?'bg-emerald-500':'bg-slate-400')}/>{node.id} · {node.online?(en?'Online':'접속'):(en?'Offline':'오프라인')}{node.allowWrites?' · Writer':''}</span>)}</div>
   </div>}
  </Panel>
 </section>
}
