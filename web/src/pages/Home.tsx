import {Activity,ArrowRight,Bot,CheckCircle2,Cpu,FolderOpen,LoaderCircle,MessageSquare,Play,RadioTower,Workflow} from 'lucide-react'
import {useState} from 'react'
import {Button,PageContainer} from '../components/Primitives'
import {ExecutionMonitor} from '../components/ExecutionMonitor'
import {WorkQueue} from '../components/WorkQueue'
import {NativeAgentPanel} from '../components/NativeAgentPanel'
import {useRemote} from '../components/UI'
import {WorkspaceHeader,WorkspaceMetrics} from '../components/WorkspaceOverview'
import {StatusPill} from '../components/StatusPill'
import {t,useLocale,formatDateTime,jobKindLabel} from '../i18n'
import type {Agent,ChatSession,Provider} from '../api'
type Destination='chat'|'models'|'agents'|'rooms'|'connections'|'memory'|'activity'
export function HomePage({navigate}:{navigate:(page:Destination)=>void}){
 useLocale()
 const sessions=useRemote<ChatSession[]>('/api/sessions',[])
 const providers=useRemote<Provider[]>('/api/providers',[])
 const agents=useRemote<Agent[]>('/api/agents',[])
 const jobs=useRemote<any[]>('/api/jobs',[])
 const channels=useRemote<any[]>('/api/channels',[])
 const runtime=useRemote<any>('/api/model-library/runtime',{status:'idle'})
 const [retrying,setRetrying]=useState(false)
 const dashboardSources=[sessions,providers,agents,jobs,channels,runtime]
 const failedSources=dashboardSources.filter(source=>source.error)
 const retryFailed=async()=>{
  if(!failedSources.length||retrying)return
  setRetrying(true)
  await Promise.allSettled(failedSources.map(source=>source.reload()))
  setRetrying(false)
 }
 const countLabel=(source:{loading:boolean;error:string},format:()=>string)=>
  source.loading?t('dashboard.loading'):source.error?t('dashboard.unavailable'):format()
 const enabledProviders=providers.value.filter(p=>p.enabled)
 const authenticatedProviders=enabledProviders.filter((p:any)=>p.hasKey||p.local).length
 const busy=jobs.value.filter(j=>['queued','running','planning','reviewing','repairing'].includes(j.status)).length
 const metrics=[
  {name:'dashboard.metric.chat',value:sessions.value.length,source:sessions,icon:MessageSquare,page:'chat' as const},
  {name:'dashboard.metric.providers',value:providers.value.filter(p=>p.enabled).length,source:providers,icon:Cpu,page:'models' as const},
  {name:'dashboard.metric.agents',value:agents.value.length,source:agents,icon:Bot,page:'agents' as const},
  {name:'dashboard.metric.jobs',value:busy,source:jobs,icon:Activity,page:'activity' as const}
 ]
 const shortcuts=[
  {title:'dashboard.shortcut.chat',desc:'dashboard.shortcut.chat.desc',icon:MessageSquare,page:'chat' as const},
  {title:'dashboard.shortcut.models',desc:'dashboard.shortcut.models.desc',icon:Cpu,page:'models' as const},
  {title:'dashboard.shortcut.agents',desc:'dashboard.shortcut.agents.desc',icon:Bot,page:'agents' as const}
 ]
 return <PageContainer className="!max-w-7xl !py-7">
  <WorkspaceHeader title={t('dashboard.title')} description={t('dashboard.subtitle')} actions={<Button type="button" variant="primary" onClick={()=>navigate('chat')}><MessageSquare size={16}/>{t('dashboard.openChat')}<ArrowRight size={14}/></Button>}/>
  {failedSources.length>0&&<div role="alert" className="mb-4 flex flex-wrap items-center justify-between gap-3 rounded-xl border border-amber-200 bg-amber-50 px-4 py-3 text-[12px] text-amber-900"><span>{t('dashboard.partialError',{count:failedSources.length})}</span><Button type="button" variant="secondary" size="sm" onClick={()=>void retryFailed()} disabled={retrying} aria-live="polite">{retrying?<><LoaderCircle size={13} className="animate-spin"/>{t('dashboard.retrying')}</>:t('dashboard.retry')}</Button></div>}
  <WorkspaceMetrics items={metrics.map(metric=>({id:metric.name,label:t(metric.name as any),icon:metric.icon,value:metric.source.error?'—':metric.value,loading:metric.source.loading,onClick:()=>navigate(metric.page)}))}/>
  <div className="mt-5 grid gap-4 lg:grid-cols-[minmax(0,1fr)_minmax(270px,.72fr)]">
   <section className="rounded-xl border border-[#E7E0D5] bg-white">
    <div className="border-b border-[#F0EAE1] px-4 py-3"><h2 className="text-[13px] font-bold">{t('dashboard.runNow')}</h2><p className="mt-1 text-[11px] text-[#8A8176]">{t('dashboard.runNow.desc')}</p></div>
    <div className="grid grid-cols-1 gap-0 sm:grid-cols-2">{shortcuts.map(s=>{const Icon=s.icon;return <Button type="button" key={s.title} onClick={()=>navigate(s.page)} className="group flex items-center gap-3 border-b border-[#f0f2f5] px-4 py-4 text-left hover:bg-[#FAF7F1] sm:odd:border-r"><span className="grid size-9 place-items-center rounded-lg bg-[#FAF7F1] text-[#A94E30]"><Icon size={17}/></span><span className="min-w-0 flex-1"><span className="block text-[12px] font-semibold">{t(s.title as any)}</span><small className="mt-1 block text-[11px] text-[#9C9388]">{t(s.desc as any)}</small></span><ArrowRight size={14} className="text-[#D8CFC2] group-hover:text-[#C96442]"/></Button>})}</div>
   </section>
   <section className="rounded-xl border border-[#E7E0D5] bg-white">
    <div className="border-b border-[#F0EAE1] px-4 py-3"><h2 className="text-[13px] font-bold">{t('dashboard.connections')}</h2></div>
    <div className="divide-y divide-[#f0f2f5] px-4">
     <Button onClick={()=>navigate('models')} className="flex w-full items-center gap-3 py-3 text-left text-[12px]"><Cpu size={16} className="text-[#8A8176]"/><span className="flex-1">{t('dashboard.providers')}</span><span className="text-[11px] text-[#8A8176]">{countLabel(providers,()=>t('dashboard.providers.count',{count:enabledProviders.length,ready:authenticatedProviders}))}</span><ArrowRight size={13}/></Button>
     <Button onClick={()=>navigate('models')} className="flex w-full items-center gap-3 py-3 text-left text-[12px]"><Cpu size={16} className="text-[#8A8176]"/><span className="flex-1">{t('dashboard.local')}</span><span className="text-[11px] text-[#8A8176]">{countLabel(runtime,()=>runtime.value.status==='ready'?t('dashboard.local.ready',{engine:runtime.value.engine||'ready'}):t('dashboard.local.idle'))}</span><ArrowRight size={13}/></Button>
     <Button onClick={()=>navigate('connections')} className="flex w-full items-center gap-3 py-3 text-left text-[12px]"><RadioTower size={16} className="text-[#8A8176]"/><span className="flex-1">{t('dashboard.channels')}</span><span className="text-[11px] text-[#8A8176]">{countLabel(channels,()=>t('dashboard.channels.count',{count:channels.value.length}))}</span><ArrowRight size={13}/></Button>
     <Button onClick={()=>navigate('agents')} className="flex w-full items-center gap-3 py-3 text-left text-[12px]"><Bot size={16} className="text-[#8A8176]"/><span className="flex-1">{t('dashboard.agents')}</span><span className="text-[11px] text-[#8A8176]">{countLabel(agents,()=>t('dashboard.agents.count',{count:agents.value.length}))}</span><ArrowRight size={13}/></Button>
    </div>
    <div className="m-4 mt-2 flex items-center gap-2 rounded-lg bg-[#F7F2EA] px-3 py-2 text-[11px] text-[#7C7469]"><CheckCircle2 size={14}/> {t('dashboard.note')}</div>
   </section>
  </div>
  <NativeAgentPanel providers={providers.value}/>
  <section className="mt-4 rounded-xl border border-[#E7E0D5] bg-white">
   <div className="flex items-center justify-between border-b border-[#F0EAE1] px-4 py-3"><h2 className="text-[13px] font-bold">{t('dashboard.recent')}</h2><Button type="button" onClick={()=>navigate('activity')} className="flex items-center gap-1 text-[11px] font-medium text-[#C96442]">{t('dashboard.recent.all')} <ArrowRight size={13}/></Button></div>
   <WorkQueue jobs={jobs.value} loading={jobs.loading} error={jobs.error}
    compact onRefresh={()=>void jobs.reload()} onOpenActivity={()=>navigate('activity')}/>
  </section>
  <div className="mt-4"><ExecutionMonitor/></div>
 </PageContainer>
}
