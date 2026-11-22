import {Activity,ArrowRight,Bot,Cpu,MessageSquare,RadioTower,Workflow} from 'lucide-react'
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
 return <PageContainer className="native-home !max-w-[1280px] !py-5">
  <header className="native-home-heading">
   <div>
    <span className="native-home-eyebrow">DOMINION / WORKSPACE</span>
    <h1>{locale==='en'?'Your workspace':'작업 공간'}</h1>
    <p>{locale==='en'?'Give an instruction, follow its progress, review the result.':'할 일을 입력하고, 실행을 확인하고, 결과를 검토하세요.'}</p>
   </div>
   <Button type="button" variant="ghost" onClick={()=>navigate('chat')} className="native-home-chat-link">
    <MessageSquare size={16}/>{t('dashboard.openChat')}<ArrowRight size={15}/>
   </Button>
  </header>
  {enabledProviders.length===0&&<section role="status" className="native-home-notice">
   <Cpu size={18}/>
   <span>{locale==='en'?'Connect a model to start a task.':'작업을 시작하려면 AI 모델을 연결하세요.'}</span>
   <Button type="button" variant="ghost" onClick={()=>navigate('models')}>
    {locale==='en'?'Connect model':'모델 연결하기'}<ArrowRight size={14}/>
   </Button>
  </section>}
  {attention.length>0&&<section role="status" data-home-attention aria-label={t('dashboard.attention')} className="native-home-notice native-home-notice--review">
   <Activity size={17}/>
   <div><strong>{t('dashboard.attention')}</strong><span>{t('dashboard.attentionDesc',{count:attention.length})}</span></div>
   <Button type="button" variant="ghost" onClick={()=>navigate('activity')}>
    {t('dashboard.reviewJobs')}<ArrowRight size={14}/>
   </Button>
  </section>}
  <NativeAgentPanel providers={providers.value}/>
  <section className="native-home-overview" aria-label={locale==='en'?'Workspace overview':'작업 현황'}>
   <div className="native-home-metrics">
    {metrics.map(m=>{const Icon=m.icon;return <Button type="button" key={m.name}
     data-dashboard-metric={m.page} onClick={()=>navigate(m.page)}
     className="native-home-metric">
     <Icon size={15}/><span>{t(m.name as any)}</span><strong>{m.value}</strong>
    </Button>})}
   </div>
   <div data-home-usage className="native-home-usage">
    <span>{t('dashboard.usage')}</span>
    <p>{t('dashboard.usageDetail',{
     calls:(usage.value.calls||0).toLocaleString(),
     tokens:(usage.value.totalTokens||0).toLocaleString()
    })}</p>
    {usage.value.estimatedCalls>0&&<small>{t('dashboard.usageEstimated',{count:usage.value.estimatedCalls})}</small>}
   </div>
  </section>
  <details className="native-home-more">
   <summary><span>{locale==='en'?'Activity and connections':'최근 활동과 연결 현황'}</span><span>{recent.length} {locale==='en'?'recent jobs':'개 최근 작업'}</span></summary>
   <div className="native-home-more-body">
    <div className="native-home-connections">
     <Button type="button" onClick={()=>navigate('models')}><Cpu size={15}/>{t('dashboard.providers')}<span>{enabledProviders.length} / {authenticatedProviders}</span></Button>
     <Button type="button" onClick={()=>navigate('models')}><Cpu size={15}/>{t('dashboard.local')}<span>{runtime.value.status==='ready'?t('dashboard.local.ready',{engine:runtime.value.engine||'ready'}):t('dashboard.local.idle')}</span></Button>
     <Button type="button" onClick={()=>navigate('connections')}><RadioTower size={15}/>{t('dashboard.channels')}<span>{channels.value.length}</span></Button>
     <Button type="button" onClick={()=>navigate('agents')}><Bot size={15}/>{t('dashboard.agents')}<span>{agents.value.length}</span></Button>
    </div>
    <h2>{t('dashboard.recent')}</h2>
    {recent.length?<div className="native-home-recent">{recent.map(job=><div key={job.id}>
     <Workflow size={15}/><span><strong>{job.input?.goal||job.input?.prompt||job.kind||t('dashboard.jobFallback')}</strong>
     <small>{jobKindLabel(job.kind)} · {job.createdAt?formatDateTime(job.createdAt):''}</small></span>
     <StatusPill status={job.status}/></div>)}</div>:
     <p className="native-home-empty">{t('dashboard.recent.empty')}</p>}
    <ExecutionMonitor/>
   </div>
  </details>
 </PageContainer>
}
