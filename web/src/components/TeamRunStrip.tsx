import {useRemote} from './UI'
import {useLocale} from '../i18n'
type Team={id:string;leadSessionId?:string;status:string;verified:boolean;tasks:{id:string;verified?:boolean}[]}
type Activity={teams:Team[]}
export function TeamRunStrip({sessionId,active=true}:{sessionId?:string;active?:boolean}){
 const en=useLocale()==='en'
 const activity=useRemote<Activity>(sessionId&&active?'/api/activity/teams':null,{teams:[]})
 const teams=(activity.value.teams||[]).filter(t=>t.leadSessionId===sessionId)
 if(!teams.length)return null
 return <aside role="status" aria-label={en?'Native team execution':'네이티브 팀 실행'} className="mx-auto w-full max-w-5xl border-t border-divider bg-canvas px-4 py-2 text-xs text-muted">
  {teams.map(team=><div key={team.id} className="flex flex-wrap items-center gap-2">
   <strong className="text-ink">{en?'Team':'팀'} · {team.id}</strong>
   <span>{team.status}</span>
   <span className={team.verified?'font-semibold text-emerald-700':'text-muted'}>
    {team.verified?(en?'Verified':'검증 완료'):`${(team.tasks||[]).filter(x=>x.verified).length}/${(team.tasks||[]).length} ${en?'verified':'검증'}`}
   </span>
  </div>)}
 </aside>
}
