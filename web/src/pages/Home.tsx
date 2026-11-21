import {Activity,ArrowRight,Bot,CheckCircle2,Cpu,FolderOpen,MessageSquare,Play,RadioTower,Workflow} from 'lucide-react'
import {Button,PageContainer} from '../components/Primitives'
import {ExecutionMonitor} from '../components/ExecutionMonitor'
import {NativeAgentPanel} from '../components/NativeAgentPanel'
import {useRemote} from '../components/UI'
import {StatusPill} from '../components/StatusPill'
import {t,useLocale,formatDateTime,jobKindLabel} from '../i18n'
import type {Agent,ChatSession,Provider} from '../api'
type Destination='chat'|'models'|'agents'|'rooms'|'connections'|'memory'|'activity'
export function HomePage({navigate}:{navigate:(page:Destination)=>void}){
 const locale=useLocale()
 const sessions=useRemote<ChatSession[]>('/api/sessions',[])
 const providers=useRemote<Provider[]>('/api/providers',[])
 const agents=useRemote<Agent[]>('/api/agents',[])
 const jobs=useRemote<any[]>('/api/jobs',[])
 const channels=useRemote<any[]>('/api/channels',[])
 const runtime=useRemote<any>('/api/model-library/runtime',{status:'idle'})
 // This endpoint aggregates the latest audited model calls. Without price
 // metadata we show tokens/call counts, never fabricate a money figure.
 const usage=useRemote<{calls:number;totalTokens:number;estimatedCalls:number}>(
  '/api/model-usage',{calls:0,totalTokens:0,estimatedCalls:0}
 )
 const enabledProviders=providers.value.filter(p=>p.enabled)
 const authenticatedProviders=enabledProviders.filter((p:any)=>p.hasKey||p.local).length
 const recent=[...jobs.value].sort((a,b)=>String(b.createdAt||'').localeCompare(String(a.createdAt||''))).slice(0,6)
 const privateSessions=sessions.value.filter(s=>!s.roomId&&!s.channelContext
  &&s.kind!=='room'&&s.kind!=='channel'&&!s.archived)
 const busy=jobs.value.filter(j=>['queued','running','planning','reviewing','repairing'].includes(j.status)).length
 // A paused job requiring owner approval is not "running" and must not be
 // buried in the activity feed below the native execution panel.
 const attention=jobs.value.filter(j=>['awaiting_approval','needs_review'].includes(j.status))
 const metrics=[
  {name:'dashboard.metric.chat',value:privateSessions.length,icon:MessageSquare,page:'chat' as const},
  {name:'dashboard.metric.providers',value:providers.value.filter(p=>p.enabled).length,icon:Cpu,page:'models' as const},
  {name:'dashboard.metric.agents',value:agents.value.length,icon:Bot,page:'agents' as const},
  {name:'dashboard.metric.jobs',value:busy,icon:Activity,page:'activity' as const}
 ]
 const shortcuts=[
  {title:'dashboard.shortcut.chat',desc:'dashboard.shortcut.chat.desc',icon:MessageSquare,page:'chat' as const},
  {title:'dashboard.shortcut.models',desc:'dashboard.shortcut.models.desc',icon:Cpu,page:'models' as const},
  {title:'dashboard.shortcut.agents',desc:'dashboard.shortcut.agents.desc',icon:Bot,page:'agents' as const}
 ]
 return <PageContainer className="!max-w-[1200px] !py-5">
  <div className="mb-5 flex flex-wrap items-end justify-between gap-3"><div><h1 className="m-0 text-[19px] font-bold tracking-tight">{t('dashboard.title')}</h1><p className="mt-1 text-[12px] text-[#7C7469]">{t('dashboard.subtitle')}</p></div><Button type="button" variant="primary" onClick={()=>navigate('chat')} className="flex h-8 items-center gap-2 rounded-lg px-3 text-[11px] font-semibold"><MessageSquare size={14}/> {t('dashboard.openChat')} <ArrowRight size={13}/></Button></div>
  {enabledProviders.length===0&&<section role="status"
   className="mb-5 flex flex-wrap items-center gap-3 rounded-xl border border-[#E5D4C5] bg-[#FFF8F0] p-4">
   <span className="grid size-10 shrink-0 place-items-center rounded-xl bg-[#F4E4D8] text-[#A94E30]">
    <Cpu size={19}/></span>
   <div className="min-w-0 flex-1">
    <strong className="block text-[13px]">{locale==='en'?'Connect an AI model to get started':'첫 실행을 위해 AI 모델을 연결하세요'}</strong>
    <p className="mt-1 text-[11px] text-[#7C7469]">{locale==='en'?
     'Choose a local model, API provider or connected account before running an agent.':
     '로컬 모델·API 업체·연결 계정 중 하나를 설정하면 대화와 자동 작업을 시작할 수 있습니다.'}</p>
   </div>
   <Button type="button" variant="primary" onClick={()=>navigate('models')}
    className="shrink-0 text-[12px]"><ArrowRight size={14}/>
    {locale==='en'?'Set up models':'모델 연결하기'}</Button>
  </section>}
  {attention.length>0&&<section role="status" data-home-attention aria-label={t('dashboard.attention')}
   className="mb-4 flex flex-wrap items-center gap-3 rounded-xl border border-[#E3CFB9] bg-[#FFF9EF] px-4 py-3">
   <Activity size={18} className="text-[#92673C]"/>
   <div className="min-w-0 flex-1 text-[12px] text-[#5C544A]"><strong className="block">{t('dashboard.attention')}</strong>
    <span className="text-[11px]">{t('dashboard.attentionDesc',{count:attention.length})}</span></div>
   <Button type="button" variant="secondary" className="text-xs" onClick={()=>navigate('activity')}>
    {t('dashboard.reviewJobs')} <ArrowRight size={13}/></Button>
  </section>}
  <div className="grid grid-cols-2 gap-3 lg:grid-cols-4">{metrics.map(m=>{const Icon=m.icon;return <Button type="button" key={m.name} data-dashboard-metric={m.page} onClick={()=>navigate(m.page)} className="flex min-h-24 flex-col items-start rounded-xl border border-[#E7E0D5] bg-white p-4 text-left transition-colors hover:border-[#D8CFC2]"><span className="flex w-full items-center justify-between text-[11px] text-[#7C7469]">{t(m.name as any)}<Icon size={16} className="text-[#A79C8E]"/></span><strong className="mt-4 text-[23px] font-bold leading-none tabular-nums text-[#24211D]">{m.value}</strong></Button>})}</div>
  <div className="mt-5 grid gap-4 lg:grid-cols-[minmax(0,1fr)_minmax(270px,.72fr)]">
   <section className="rounded-xl border border-[#E7E0D5] bg-white">
    <div className="border-b border-[#F0EAE1] px-4 py-3"><h2 className="text-[13px] font-bold">{t('dashboard.runNow')}</h2><p className="mt-1 text-[11px] text-[#8A8176]">{t('dashboard.runNow.desc')}</p></div>
    <div className="grid grid-cols-1 gap-0 sm:grid-cols-2">{shortcuts.map(s=>{const Icon=s.icon;return <Button type="button" key={s.title} onClick={()=>navigate(s.page)} className="group flex items-center gap-3 border-b border-[#f0f2f5] px-4 py-4 text-left hover:bg-[#FAF7F1] sm:odd:border-r"><span className="grid size-9 place-items-center rounded-lg bg-[#FAF7F1] text-[#A94E30]"><Icon size={17}/></span><span className="min-w-0 flex-1"><span className="block text-[12px] font-semibold">{t(s.title as any)}</span><small className="mt-1 block text-[11px] text-[#9C9388]">{t(s.desc as any)}</small></span><ArrowRight size={14} className="text-[#D8CFC2] group-hover:text-[#C96442]"/></Button>})}</div>
   </section>
   <section className="rounded-xl border border-[#E7E0D5] bg-white">
    <div className="border-b border-[#F0EAE1] px-4 py-3"><h2 className="text-[13px] font-bold">{t('dashboard.connections')}</h2></div>
    <div className="divide-y divide-[#f0f2f5] px-4">
     <Button onClick={()=>navigate('models')} className="flex w-full items-center gap-3 py-3 text-left text-[12px]"><Cpu size={16} className="text-[#8A8176]"/><span className="flex-1">{t('dashboard.providers')}</span><span className="text-[11px] text-[#8A8176]">{t('dashboard.providers.count',{count:enabledProviders.length,ready:authenticatedProviders})}</span><ArrowRight size={13}/></Button>
     <Button onClick={()=>navigate('models')} className="flex w-full items-center gap-3 py-3 text-left text-[12px]"><Cpu size={16} className="text-[#8A8176]"/><span className="flex-1">{t('dashboard.local')}</span><span className="text-[11px] text-[#8A8176]">{runtime.value.status==='ready'?t('dashboard.local.ready',{engine:runtime.value.engine||'ready'}):t('dashboard.local.idle')}</span><ArrowRight size={13}/></Button>
     <Button onClick={()=>navigate('connections')} className="flex w-full items-center gap-3 py-3 text-left text-[12px]"><RadioTower size={16} className="text-[#8A8176]"/><span className="flex-1">{t('dashboard.channels')}</span><span className="text-[11px] text-[#8A8176]">{t('dashboard.channels.count',{count:channels.value.length})}</span><ArrowRight size={13}/></Button>
     <Button onClick={()=>navigate('agents')} className="flex w-full items-center gap-3 py-3 text-left text-[12px]"><Bot size={16} className="text-[#8A8176]"/><span className="flex-1">{t('dashboard.agents')}</span><span className="text-[11px] text-[#8A8176]">{t('dashboard.agents.count',{count:agents.value.length})}</span><ArrowRight size={13}/></Button>
    </div>
    <div className="m-4 mt-2 flex items-center gap-2 rounded-lg bg-[#F7F2EA] px-3 py-2 text-[11px] text-[#7C7469]"><CheckCircle2 size={14}/> {t('dashboard.note')}</div>
   </section>
  </div>
  <section data-home-usage className="mt-4 flex flex-wrap items-center gap-3 rounded-xl border border-[#E7E0D5] bg-white px-4 py-3">
   <div className="min-w-0 flex-1">
    <h2 className="text-[12px] font-bold text-[#3A342D]">{t('dashboard.usage')}</h2>
    <p className="mt-1 text-[11px] text-[#817467]">{t('dashboard.usageDetail',{
      calls:(usage.value.calls||0).toLocaleString(),
      tokens:(usage.value.totalTokens||0).toLocaleString()
     })}</p>
    {usage.value.estimatedCalls>0&&<p className="mt-1 text-[11px] text-[#92673C]">
     {t('dashboard.usageEstimated',{count:usage.value.estimatedCalls})}</p>}
   </div>
   <Button variant="secondary" size="sm" type="button" onClick={()=>navigate('activity')}>
    {t('dashboard.recent.all')} <ArrowRight size={13}/></Button>
  </section>
  <NativeAgentPanel providers={providers.value}/>
  <section className="mt-4 rounded-xl border border-[#E7E0D5] bg-white">
   <div className="flex items-center justify-between border-b border-[#F0EAE1] px-4 py-3"><h2 className="text-[13px] font-bold">{t('dashboard.recent')}</h2><Button type="button" onClick={()=>navigate('activity')} className="flex items-center gap-1 text-[11px] font-medium text-[#C96442]">{t('dashboard.recent.all')} <ArrowRight size={13}/></Button></div>
   {recent.length?<div className="divide-y divide-[#F0EAE1]">{recent.map(job=><div key={job.id} className="flex items-center gap-3 px-4 py-3"><Workflow size={15} className="text-[#A79C8E]"/><div className="min-w-0 flex-1"><div className="truncate text-[12px] font-semibold">{job.input?.goal||job.input?.prompt||job.kind||t('dashboard.jobFallback')}</div><div className="mt-1 text-[10px] text-[#9C9388]">{jobKindLabel(job.kind)} · {job.createdAt?formatDateTime(job.createdAt):''}</div></div><StatusPill status={job.status} /></div>)}</div>:<div className="flex h-32 items-center justify-center text-[12px] text-[#9C9388]">{t('dashboard.recent.empty')}</div>}
  </section>
  <div className="mt-4"><ExecutionMonitor/></div>
 </PageContainer>
}
