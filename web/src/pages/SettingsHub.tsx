import {lazy,useEffect,useState} from 'react'
import {Activity,Archive,Bot,Layers3,Settings2,ChartNoAxesCombined,MessageSquare,ShieldCheck,ChevronDown} from 'lucide-react'
import {useLocale} from '../i18n'
import {workspaceView} from '../utils/workspaceRouting'
import type {PageId} from '../workspacePages'
const OperationsPage=lazy(()=>import('./Operations').then(module=>({default:module.OperationsPage})))
const ModelHub=lazy(()=>import('./Models').then(module=>({default:module.ModelHub})))
const CapabilitiesPage=lazy(()=>import('./Capabilities').then(module=>({default:module.CapabilitiesPage})))
const HomePage=lazy(()=>import('./Home').then(module=>({default:module.HomePage})))
type SettingsView='general'|'reply'|'ai'|'security'|'data'|'features'|'overview'|'activity'
const views:SettingsView[]=['general','reply','ai','security','data','features','overview','activity']
function readSettingsView():SettingsView{
 const view=workspaceView(window.location.hash,'settings') as SettingsView|null
 return view&&views.includes(view)?view:'general'
}
export function SettingsHub({navigate}:{navigate:(page:PageId,sessionId?:string)=>void}){
 const en=useLocale()==='en',tr=(ko:string,english:string)=>en?english:ko
 const [view,setView]=useState<SettingsView>(readSettingsView),[more,setMore]=useState(false)
 useEffect(()=>{
  const sync=()=>setView(readSettingsView())
  window.addEventListener('hashchange',sync);window.addEventListener('popstate',sync)
  return()=>{window.removeEventListener('hashchange',sync);window.removeEventListener('popstate',sync)}
 },[])
 const choose=(next:SettingsView)=>{setView(next);setMore(false);window.location.hash=next==='general'?'#/settings':'#/settings?view='+next}
 const options=[
  {id:'general',label:tr('일반','General'),icon:Settings2},
  {id:'reply',label:tr('답변','Replies'),icon:MessageSquare},
  {id:'ai',label:tr('AI 모델','AI models'),icon:Bot},
  {id:'security',label:tr('권한 및 보안','Permissions'),icon:ShieldCheck},
  {id:'data',label:tr('백업','Backup'),icon:Archive},
  {id:'features',label:tr('기능 관리','Capabilities'),icon:Layers3},
  {id:'overview',label:tr('사용 현황','Usage'),icon:ChartNoAxesCombined},
  {id:'activity',label:tr('작업 기록','Activity'),icon:Activity},
 ] as const
 const button=(item:typeof options[number])=><button key={item.id} type="button" aria-current={view===item.id?'page':undefined} onClick={()=>choose(item.id)} className="flex min-h-10 shrink-0 items-center gap-2 rounded-lg px-3 py-2 text-sm text-muted hover:bg-canvas hover:text-ink aria-[current=page]:bg-selected aria-[current=page]:font-semibold aria-[current=page]:text-ink"><item.icon size={16}/>{item.label}</button>
 const secondary=options.slice(5),activeExtra=secondary.find(x=>x.id===view)
 return <>
  <nav aria-label={tr('설정 메뉴','Settings sections')} className="mx-auto w-full max-w-[1320px] px-5 pt-4 sm:px-7 lg:px-6">
   <div className="flex flex-wrap items-center gap-1 border-b border-divider pb-3">{options.slice(0,5).map(button)}
    <button type="button" aria-expanded={more} onClick={()=>setMore(v=>!v)} className="flex min-h-10 items-center gap-1 rounded-lg px-3 text-sm text-muted hover:bg-canvas">{activeExtra?.label||tr('더 보기','More')}<ChevronDown size={15}/></button>
   </div>
   {more&&<div className="flex flex-wrap gap-1 border-b border-divider py-2">{secondary.map(button)}</div>}
  </nav>
  {view==='general'&&<OperationsPage section="settings" settingsArea="general"/>}
  {view==='security'&&<OperationsPage section="settings" settingsArea="security"/>}
  {view==='data'&&<OperationsPage section="settings" settingsArea="data"/>}
  {view==='ai'&&<ModelHub/>}
  {view==='features'&&<><div className="mx-auto w-full max-w-[1320px] px-5 pt-4 sm:px-7 lg:px-6"><CapabilitiesPage embedded navigate={()=>navigate('connections')}/></div><OperationsPage section="settings" settingsArea="runtime"/></>}
  {view==='overview'&&<HomePage navigate={navigate}/>}
  {view==='activity'&&<OperationsPage section="activity"/>}
 </>
}
