import {Activity,ArrowRight,Bot,ChevronDown,ChevronRight,Command,Cpu,MessageSquare,RadioTower,Workflow} from 'lucide-react'
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
 const connections=[
  {label:t('dashboard.providers'),value:`${enabledProviders.length} / ${authenticatedProviders}`,icon:Cpu,page:'models' as const},
  {label:t('dashboard.local'),value:runtime.value.status==='ready'?t('dashboard.local.ready',{engine:runtime.value.engine||'ready'}):t('dashboard.local.idle'),icon:Cpu,page:'models' as const},
  {label:t('dashboard.channels'),value:String(channels.value.length),icon:RadioTower,page:'connections' as const},
  {label:t('dashboard.agents'),value:String(agents.value.length),icon:Bot,page:'agents' as const}
 ]
 return <PageContainer className="!max-w-[1180px] !py-8">
  <header className="flex flex-wrap items-end justify-between gap-4">
   <div className="min-w-0">
    <span className="block text-[10px] font-semibold uppercase tracking-[0.16em] text-muted">DOMINION / WORKSPACE</span>
    <h1 className="mt-1.5 text-[26px] font-semibold leading-tight tracking-[-0.03em] text-ink">{locale==='en'?'Your workspace':'작업 공간'}</h1>
    <p className="mt-1 text-[13px] leading-6 text-muted">{locale==='en'?'Give an instruction, follow its progress, review the result.':'할 일을 입력하고, 실행을 확인하고, 결과를 검토하세요.'}</p>
   </div>
   <div className="flex items-center gap-2">
    {busy>0&&<span className="inline-flex items-center gap-1.5 rounded-full border border-divider bg-surface px-2.5 py-1 text-[11px] text-muted"><i className="size-1.5 animate-pulse rounded-full bg-emerald-500"/>{busy}</span>}
    <span className="hidden items-center gap-1 rounded-lg border border-divider bg-surface px-2 py-1.5 text-[10px] font-medium text-muted sm:inline-flex"><Command size={11}/>K</span>
    <Button type="button" variant="secondary" size="sm" onClick={()=>navigate('chat')} className="!rounded-lg">
     <MessageSquare size={14}/>{t('dashboard.openChat')}
    </Button>
   </div>
  </header>
  {enabledProviders.length===0&&<section role="status" className="mt-6 flex flex-wrap items-center gap-3 rounded-xl border border-divider bg-surface px-4 py-3">
   <Cpu size={16} className="shrink-0 text-muted"/>
   <span className="min-w-[160px] flex-1 text-[12px] text-muted">{locale==='en'?'Connect a model to start a task.':'작업을 시작하려면 AI 모델을 연결하세요.'}</span>
   <Button type="button" variant="ghost" size="sm" onClick={()=>navigate('models')}>
    {locale==='en'?'Connect model':'모델 연결하기'}<ArrowRight size={13}/>
   </Button>
  </section>}
  {attention.length>0&&<section role="status" data-home-attention aria-label={t('dashboard.attention')} className="mt-3 flex flex-wrap items-center gap-3 rounded-xl border border-amber-200/70 bg-amber-50 px-4 py-3">
   <Activity size={16} className="shrink-0 text-amber-700"/>
   <div className="min-w-[160px] flex-1"><strong className="block text-[12px] font-semibold text-amber-900">{t('dashboard.attention')}</strong><span className="text-[11px] text-amber-800/80">{t('dashboard.attentionDesc',{count:attention.length})}</span></div>
   <Button type="button" variant="ghost" size="sm" onClick={()=>navigate('activity')} className="!text-amber-900 hover:!bg-amber-100">
    {t('dashboard.reviewJobs')}<ArrowRight size={13}/>
   </Button>
  </section>}
  <div className="mt-6"><NativeAgentPanel providers={providers.value}/></div>
  <section aria-label={locale==='en'?'Workspace overview':'작업 현황'} className="mt-6 overflow-hidden rounded-xl border border-divider bg-surface">
   <div className="grid grid-cols-2 divide-y divide-divider sm:grid-cols-4 sm:divide-x sm:divide-y-0">
    {metrics.map(m=>{const Icon=m.icon;return <button type="button" key={m.name}
     data-dashboard-metric={m.page} onClick={()=>navigate(m.page)}
     className="group flex min-w-0 items-center gap-3 px-4 py-4 text-left transition-colors hover:bg-canvas">
     <span className="grid size-8 shrink-0 place-items-center rounded-lg bg-canvas text-muted transition-colors group-hover:text-brand"><Icon size={15}/></span>
     <span className="min-w-0 flex-1 truncate text-[11px] text-muted">{t(m.name as any)}</span>
     <strong className="text-[18px] font-semibold leading-none tabular-nums tracking-[-0.02em] text-ink">{m.value}</strong>
    </button>})}
   </div>
   <div data-home-usage className="flex flex-wrap items-center gap-x-3 gap-y-1 border-t border-divider px-4 py-3 text-[11px] text-muted">
    <span className="font-medium text-ink/80">{t('dashboard.usage')}</span>
    <span>{t('dashboard.usageDetail',{
     calls:(usage.value.calls||0).toLocaleString(),
     tokens:(usage.value.totalTokens||0).toLocaleString()
    })}</span>
    {usage.value.estimatedCalls>0&&<span className="text-amber-700">{t('dashboard.usageEstimated',{count:usage.value.estimatedCalls})}</span>}
   </div>
  </section>
  <details className="group mt-6 border-t border-divider">
   <summary className="flex cursor-pointer list-none items-center justify-between gap-3 py-4 text-[12px] text-muted transition-colors hover:text-ink [&::-webkit-details-marker]:hidden">
    <span className="font-medium">{locale==='en'?'Activity and connections':'최근 활동과 연결 현황'}</span>
    <span className="flex items-center gap-1.5 text-[11px]">{recent.length} {locale==='en'?'recent jobs':'개 최근 작업'}<ChevronDown size={14} className="transition-transform group-open:rotate-180"/></span>
   </summary>
   <div className="grid gap-8 pb-6 lg:grid-cols-[minmax(0,0.85fr)_minmax(0,1.45fr)]">
    <div>
     <h2 className="mb-2 text-[10px] font-semibold uppercase tracking-[0.14em] text-muted">{locale==='en'?'Connections':'연결 현황'}</h2>
     <div className="grid gap-1.5">
      {connections.map(item=>{const Icon=item.icon;return <button type="button" key={item.label} onClick={()=>navigate(item.page)}
       className="group flex min-w-0 items-center gap-2.5 rounded-lg border border-divider bg-surface px-3 py-2.5 text-left text-[11px] text-ink transition-colors hover:bg-canvas">
       <Icon size={14} className="shrink-0 text-muted"/><span className="min-w-0 flex-1 truncate">{item.label}</span>
       <span className="max-w-[140px] truncate text-[10px] text-muted">{item.value}</span>
       <ChevronRight size={13} className="shrink-0 text-muted/60 transition-transform group-hover:translate-x-0.5"/>
      </button>})}
     </div>
    </div>
    <div className="min-w-0">
     <h2 className="mb-1 text-[10px] font-semibold uppercase tracking-[0.14em] text-muted">{t('dashboard.recent')}</h2>
     {recent.length?<ul className="divide-y divide-divider">
      {recent.map(job=><li key={job.id} className="flex items-center gap-3 py-3">
       <Workflow size={14} className="shrink-0 text-muted"/>
       <div className="min-w-0 flex-1">
        <p className="truncate text-[12px] font-medium text-ink">{job.input?.goal||job.input?.prompt||job.kind||t('dashboard.jobFallback')}</p>
        <p className="mt-0.5 truncate text-[10px] text-muted">{jobKindLabel(job.kind)} · {job.createdAt?formatDateTime(job.createdAt):''}</p>
       </div>
       <StatusPill status={job.status}/>
      </li>)}
     </ul>:<div className="flex flex-col items-center justify-center gap-2 rounded-xl border border-dashed border-divider bg-canvas/50 px-6 py-10 text-center">
      <Workflow size={20} className="text-muted/70"/>
      <p className="text-[12px] text-muted">{t('dashboard.recent.empty')}</p>
      <Button type="button" variant="ghost" size="sm" onClick={()=>navigate('chat')}>{t('dashboard.openChat')}<ArrowRight size={13}/></Button>
     </div>}
     <div className="mt-4"><ExecutionMonitor/></div>
    </div>
   </div>
  </details>
 </PageContainer>
}
