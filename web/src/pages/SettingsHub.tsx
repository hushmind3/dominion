import {lazy,useEffect,useState} from 'react'
import {Activity,Bot,Layers3,Settings2,ChartNoAxesCombined} from 'lucide-react'
import {ViewTabs} from '../components/ui/WorkspaceUI'
import {useLocale} from '../i18n'
import type {PageId} from '../workspacePages'

const OperationsPage=lazy(()=>import('./Operations').then(module=>({default:module.OperationsPage})))
const ModelHub=lazy(()=>import('./Models').then(module=>({default:module.ModelHub})))
const CapabilitiesPage=lazy(()=>import('./Capabilities').then(module=>({default:module.CapabilitiesPage})))
const HomePage=lazy(()=>import('./Home').then(module=>({default:module.HomePage})))

type SettingsView='general'|'ai'|'features'|'overview'|'activity'
function readSettingsView():SettingsView{
 const [path,query]=window.location.hash.split('?')
 if(path!=='#/settings')return 'general'
 const view=new URLSearchParams(query).get('view')
 return view==='ai'||view==='features'||view==='overview'||view==='activity'?view:'general'
}

/** One settings entry point for model connections and native capability inventory.
 * Channel/MCP/skills live in Service Connections; prompts live in Memory.
 * Historical deep links are normalized by useWorkspaceNavigation. */
export function SettingsHub({navigate}:{navigate:(page:PageId,sessionId?:string)=>void}){
 const en=useLocale()==='en'
 const tr=(ko:string,english:string)=>en?english:ko
 const [view,setView]=useState<SettingsView>(readSettingsView)
 useEffect(()=>{
  const sync=()=>setView(readSettingsView())
  window.addEventListener('hashchange',sync);window.addEventListener('popstate',sync)
  return()=>{window.removeEventListener('hashchange',sync);window.removeEventListener('popstate',sync)}
 },[])
 const choose=(next:SettingsView)=>{
  setView(next)
  window.location.hash=next==='general'?'#/settings':'#/settings?view='+next
 }
 return <>
  <div className="mx-auto w-full max-w-[1320px] px-5 pt-5 sm:px-7 lg:px-6">
   <ViewTabs value={view} onChange={choose} label={tr('설정과 기능 관리','Settings and capabilities')} options={[
    {id:'general',label:tr('일반 설정','General settings'),icon:Settings2},
    {id:'ai',label:tr('AI 연결','AI connections'),icon:Bot},
    {id:'features',label:tr('기본 기능','Built-in capabilities'),icon:Layers3},
    {id:'overview',label:tr('사용 현황','Overview'),icon:ChartNoAxesCombined},
    {id:'activity',label:tr('작업 기록','Activity'),icon:Activity},
   ]}/>
  </div>
  {view==='general'&&<OperationsPage section="settings"/>}
  {view==='ai'&&<ModelHub/>}
  {view==='features'&&<div className="mx-auto w-full max-w-[1320px] px-5 pb-8 sm:px-7 lg:px-6"><CapabilitiesPage embedded navigate={()=>{}}/></div>}
  {view==='overview'&&<HomePage navigate={navigate}/>}
  {view==='activity'&&<OperationsPage section="activity"/>}
 </>
}
