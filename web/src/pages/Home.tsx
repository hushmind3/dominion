import {ArrowRight, ChevronRight, Clock3} from 'lucide-react'
import {Button, PageContainer} from '../components/Primitives'
import {NativeAgentPanel} from '../components/NativeAgentPanel'
import {useRemote} from '../components/UI'
import {StatusPill} from '../components/StatusPill'
import {t, useLocale, formatDateTime} from '../i18n'
import type {ChatSession, Provider} from '../api'
type Destination='chat'|'models'|'agents'|'rooms'|'connections'|'memory'|'activity'
export function HomePage({navigate}:{navigate:(page:Destination)=>void}){
 const locale=useLocale()
 const sessions=useRemote<ChatSession[]>('/api/sessions',[])
 const providers=useRemote<Provider[]>('/api/providers',[])
 const jobs=useRemote<any[]>('/api/jobs',[])
 const enabledProviders=providers.value.filter(p=>p.enabled)
 const privateSessions=sessions.value.filter(s=>!s.roomId&&!s.channelContext
  &&s.kind!=='room'&&s.kind!=='channel'&&!s.archived)
 const recent=[...jobs.value]
  .sort((a,b)=>String(b.createdAt||'').localeCompare(String(a.createdAt||'')))
  .slice(0,5)
 const attention=jobs.value.filter(j=>['awaiting_approval','needs_review'].includes(j.status))
 const copy=locale==='en'
  ? {
     greeting:'What can I do for you?',
     lead:'Describe the job in your own words. It runs in the background and reports back.',
     noModel:'No AI is connected yet. Connect one to start.',
     connect:'Connect AI',
     review:'Needs your review',
     reviewDesc:`${attention.length} task${attention.length>1?'s':''} waiting for approval`,
     reviewAction:'Review',
     recent:'Recent work',
     recentEmpty:'Nothing here yet. Start with what you need.',
     start:'Start a conversation',
    }
  : {
     greeting:'무엇을 도와드릴까요?',
     lead:'하고 싶은 일을 그대로 적어주세요. 알아서 진행하고 결과를 보여드립니다.',
     noModel:'아직 AI가 연결되지 않았습니다. 연결하면 바로 시작할 수 있어요.',
     connect:'AI 연결하기',
     review:'확인이 필요해요',
     reviewDesc:`승인을 기다리는 작업이 ${attention.length}개 있습니다`,
     reviewAction:'확인하기',
     recent:'최근 작업',
     recentEmpty:'아직 작업이 없습니다. 필요한 일을 적어서 시작해 보세요.',
     start:'대화 시작',
    }
 return <PageContainer className="!max-w-[900px] !py-10 sm:!py-14">
  <header className="min-w-0">
   <h1 className="m-0 text-[28px] font-semibold leading-tight tracking-[-0.035em] text-ink sm:text-[32px]">{copy.greeting}</h1>
   <p className="mt-2 max-w-xl text-[13px] leading-6 text-muted">{copy.lead}</p>
  </header>
  {enabledProviders.length===0&&<section role="status" className="mt-6 flex flex-wrap items-center gap-3 rounded-xl border border-divider bg-surface px-4 py-3">
   <span className="min-w-[180px] flex-1 text-[12px] text-muted">{copy.noModel}</span>
   <Button type="button" variant="secondary" size="sm" onClick={()=>navigate('models')}>{copy.connect}<ArrowRight size={13}/></Button>
  </section>}
  {attention.length>0&&<section role="status" data-home-attention aria-label={copy.review} className="mt-3 flex flex-wrap items-center gap-3 rounded-xl border border-amber-200/70 bg-amber-50 px-4 py-3">
   <div className="min-w-[180px] flex-1"><strong className="block text-[12px] font-semibold text-amber-900">{copy.review}</strong><span className="text-[11px] text-amber-800/80">{copy.reviewDesc}</span></div>
   <Button type="button" variant="ghost" size="sm" onClick={()=>navigate('activity')} className="!text-amber-900 hover:!bg-amber-100">{copy.reviewAction}<ArrowRight size={13}/></Button>
  </section>}
  <div className="mt-6"><NativeAgentPanel providers={providers.value}/></div>
  <section aria-label={copy.recent} className="mt-10">
   <div className="mb-2 flex items-center justify-between gap-3">
    <h2 className="m-0 flex items-center gap-2 text-[13px] font-semibold text-ink"><Clock3 size={15} className="text-muted"/>{copy.recent}</h2>
    <Button type="button" variant="ghost" size="sm" onClick={()=>navigate('activity')}>{locale==='en'?'See all':'전체 보기'}<ChevronRight size={13}/></Button>
   </div>
   {recent.length
    ? <ul className="divide-y divide-divider overflow-hidden rounded-xl border border-divider bg-surface">
      {recent.map(job=><li key={job.id} className="flex items-center gap-3 px-4 py-3">
       <div className="min-w-0 flex-1">
        <p className="truncate text-[12.5px] font-medium text-ink">{job.input?.goal||job.input?.prompt||job.kind||t('dashboard.jobFallback')}</p>
        <p className="mt-0.5 truncate text-[11px] text-muted">{job.createdAt?formatDateTime(job.createdAt):''}</p>
       </div>
       <StatusPill status={job.status}/>
      </li>)}
     </ul>
    : <div className="flex flex-col items-center justify-center gap-2 rounded-xl border border-dashed border-divider bg-surface px-6 py-10 text-center">
      <p className="m-0 text-[12px] text-muted">{copy.recentEmpty}</p>
      <Button type="button" variant="secondary" size="sm" onClick={()=>navigate('chat')}>{copy.start}<ArrowRight size={13}/></Button>
     </div>}
  </section>
 </PageContainer>
}
