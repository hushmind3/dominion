import {Activity,ArrowRight,ArrowUpRight,BookOpenText,BrainCircuit,Check,Cloud,Compass,MessageSquareMore,Play,RadioTower,ShieldCheck,Sparkles,Workflow} from 'lucide-react'
import {Button,PageContainer} from '../components/Primitives'
import {StatusPill} from '../components/StatusPill'
import {ExecutionMonitor} from '../components/ExecutionMonitor'
import {useRemote} from '../components/UI'
import type {Agent,ChatSession,Provider} from '../api'

type Destination='chat'|'models'|'agents'|'rooms'|'connections'|'workflows'|'memory'|'activity'
export function HomePage({navigate}:{navigate:(page:Destination)=>void}){
 const sessions=useRemote<ChatSession[]>('/api/sessions',[])
 const providers=useRemote<Provider[]>('/api/providers',[])
 const agents=useRemote<Agent[]>('/api/agents',[])
 const jobs=useRemote<any[]>('/api/jobs',[])
 const channels=useRemote<any[]>('/api/channels',[])
 const memories=useRemote<any[]>('/api/memories',[])
 const active=jobs.value.filter(item=>['running','planning','queued','repairing','reviewing'].includes(item.status)).length
 const latest=jobs.value.slice(0,5)
 const metrics=[
  {name:'대화 세션',value:sessions.value.length,detail:'대화 기록',icon:MessageSquareMore,target:'chat' as const,tint:'bg-blue-50 text-blue-600'},
  {name:'모델 제공자',value:providers.value.length,detail:'로컬 + API',icon:Compass,target:'models' as const,tint:'bg-indigo-50 text-indigo-600'},
  {name:'에이전트',value:agents.value.length,detail:'역할별 실행',icon:BrainCircuit,target:'agents' as const,tint:'bg-violet-50 text-violet-600'},
  {name:'실행 작업',value:jobs.value.length,detail:active+'개 진행 중',icon:Activity,target:'activity' as const,tint:'bg-cyan-50 text-cyan-700'}
 ]
 const setup=[
  {title:'모델 연결',detail:'로컬 모델 또는 API 제공자를 등록합니다',target:'models' as const,icon:Compass,done:providers.value.length>0},
  {title:'에이전트 구성',detail:'역할과 모델을 지정하여 실행 주체를 만듭니다',target:'agents' as const,icon:BrainCircuit,done:agents.value.length>0},
  {title:'실행 시작',detail:'대화, 에이전트, 단계별 워크플로에서 공통 엔진을 사용합니다',target:'workflows' as const,icon:Workflow,done:jobs.value.length>0}
 ]
 return <PageContainer className="!max-w-[1480px] !px-4 !py-6 sm:!px-8 lg:!px-10 lg:!py-8">
  <ExecutionMonitor/>
  <div className="mb-6 flex flex-wrap items-center justify-between gap-3">
   <div><p className="m-0 text-[10px] font-extrabold tracking-[.21em] text-blue-700">YOUR COMMAND CENTER</p><h2 className="mb-0 mt-1 text-[25px] font-extrabold tracking-tight text-slate-900 sm:text-3xl">워크스페이스 개요</h2></div>
   <span className="flex items-center gap-2 rounded-full border border-slate-200 bg-white px-3.5 py-2 text-[11px] font-semibold text-slate-600"><span className="size-2 rounded-full bg-emerald-500"/>실제 저장된 데이터 기준</span>
  </div>
  <section className="relative isolate overflow-hidden rounded-[26px] bg-[#101a30] px-6 py-8 text-white shadow-[0_18px_45px_rgba(15,23,42,.16)] sm:px-9 sm:py-10">
   <div aria-hidden="true" className="pointer-events-none absolute inset-0 -z-10 bg-[radial-gradient(circle_at_82%_40%,rgba(61,115,255,.38),transparent_38%),linear-gradient(135deg,#111a2e_15%,#182c4e_100%)]"/>
   <div aria-hidden="true" className="pointer-events-none absolute -right-12 -top-24 -z-10 size-[350px] rounded-full border border-white/10 sm:size-[520px]"/>
   <div aria-hidden="true" className="pointer-events-none absolute right-12 top-14 -z-10 size-60 rounded-full border border-white/10"/>
   <div className="relative max-w-[690px]">
    <p className="mb-3 flex items-center gap-2 text-[11px] font-bold tracking-[.19em] text-cyan-300"><Sparkles size={15}/> LOCAL-FIRST AI WORKSPACE</p>
    <h3 className="m-0 text-3xl font-black leading-[1.2] tracking-tight sm:text-[42px]">아이디어를 실행으로.<span className="mt-1 block text-blue-300">모델부터 자동화까지.</span></h3>
    <p className="mb-7 mt-4 max-w-xl text-sm leading-7 text-slate-300">모델을 선택하고, 에이전트를 구성하고, 실제 작업을 실행하세요. 대화와 도구 실행 결과는 이 작업 공간에 기록됩니다.</p>
    <div className="flex flex-wrap gap-3">
     <Button variant="primary" className="!min-h-11 !border-blue-400 !bg-blue-500 !px-5 !font-bold hover:!bg-blue-400" onClick={()=>navigate('chat')}><MessageSquareMore size={17}/> 새 대화 <ArrowRight size={16}/></Button>
     <Button variant="secondary" className="!min-h-11 !border-white/20 !bg-white/10 !px-5 !font-bold !text-white hover:!bg-white/20" onClick={()=>navigate('workflows')}><Play size={17}/> 실행 엔진</Button>
    </div>
   </div>
   <div className="relative mt-9 flex flex-wrap gap-x-6 gap-y-2 border-t border-white/15 pt-4 text-[11px] text-slate-300"><span className="flex items-center gap-2"><ShieldCheck size={15} className="text-cyan-300"/> 로컬 우선 제어</span><span className="flex items-center gap-2"><Workflow size={15} className="text-cyan-300"/> 지속 작업 큐</span><span className="flex items-center gap-2"><BookOpenText size={15} className="text-cyan-300"/> 실행 이력 보존</span></div>
  </section>

  <div className="mb-8 mt-5 grid grid-cols-2 gap-3 lg:grid-cols-4 lg:gap-4">
   {metrics.map(metric=>{const Icon=metric.icon;return <Button key={metric.name} variant="ghost" onClick={()=>navigate(metric.target)} className="group !flex !min-h-[132px] !w-full !items-stretch !justify-between !gap-2 !rounded-2xl !border !border-slate-200 !bg-white !p-4 !text-left shadow-sm transition-all hover:!-translate-y-0.5 hover:!border-blue-200 hover:!shadow-md sm:!p-5">
    <div className="flex min-w-0 flex-col items-start"><span className={'grid size-9 place-items-center rounded-xl '+metric.tint}><Icon size={18}/></span><span className="mt-3 text-[11px] font-semibold text-slate-500">{metric.name}</span><strong className="mt-0.5 text-[26px] font-extrabold leading-none tabular-nums text-slate-900">{metric.value.toLocaleString('ko-KR')}</strong><small className="mt-2 text-[10px] text-slate-400">{metric.detail}</small></div>
    <ArrowUpRight size={17} className="shrink-0 text-slate-300 transition-colors group-hover:text-blue-600"/>
   </Button>})}
  </div>
  <div className="grid min-w-0 gap-5 xl:grid-cols-[minmax(0,1.18fr)_minmax(320px,.82fr)]">
   <div className="flex min-w-0 flex-col gap-5">
    <section className="rounded-2xl border border-slate-200 bg-white p-5 shadow-sm sm:p-6">
     <div className="mb-5 flex flex-wrap items-end justify-between gap-2"><div><div className="text-[10px] font-extrabold tracking-[.17em] text-blue-600">GET STARTED</div><h3 className="mb-0 mt-1 text-lg font-extrabold text-slate-900">실행 준비</h3></div><span className="text-xs font-semibold text-slate-400">{setup.filter(item=>item.done).length} / 3 단계</span></div>
     <div className="space-y-2.5">{setup.map((item,index)=>{const Icon=item.icon;return <Button key={item.title} variant="ghost" onClick={()=>navigate(item.target)} className="group !flex !w-full !items-center !justify-start !gap-3 !rounded-xl !border !border-slate-100 !bg-slate-50/80 !px-4 !py-3.5 !text-left hover:!border-blue-200 hover:!bg-blue-50/50">
      <span className={'grid size-10 shrink-0 place-items-center rounded-xl '+(item.done?'bg-emerald-100 text-emerald-700':'bg-white text-blue-600 shadow-sm')}>{item.done?<Check size={20}/>:<Icon size={20}/>}</span>
      <div className="min-w-0 flex-1"><strong className="block text-[13px] font-bold text-slate-800">{index+1}. {item.title}</strong><small className="mt-1 block text-[11px] leading-5 text-slate-500">{item.detail}</small></div><span className={'hidden rounded-md px-2 py-1 text-[10px] font-semibold sm:inline '+(item.done?'bg-emerald-50 text-emerald-700':'bg-white text-slate-500')}>{item.done?'등록됨':'시작하기'}</span><ArrowRight size={16} className="shrink-0 text-slate-400 group-hover:text-blue-600"/>
     </Button>})}</div>
     {(providers.error||agents.error||jobs.error)&&<p role="alert" className="mt-4 rounded-lg bg-rose-50 px-3 py-2 text-xs text-rose-700">데이터를 불러오지 못했습니다. 게이트웨이 연결 상태를 확인하세요.</p>}
    </section>
    <section className="rounded-2xl border border-slate-200 bg-white p-5 shadow-sm sm:p-6">
     <div className="mb-4 flex items-center justify-between gap-3"><h3 className="m-0 text-lg font-extrabold text-slate-900">최근 실행</h3><Button variant="ghost" size="sm" onClick={()=>navigate('activity')} className="!text-blue-600">전체 기록 <ArrowRight size={15}/></Button></div>
     <div className="space-y-2">{latest.length?latest.map(job=><div key={job.id} className="flex min-w-0 items-center gap-3 rounded-xl border border-slate-100 px-3 py-3"><span className="grid size-9 shrink-0 place-items-center rounded-xl bg-slate-100 text-slate-600"><Activity size={16}/></span><div className="min-w-0 flex-1"><strong className="block truncate text-xs font-bold text-slate-800">{job.kind==='workflow'?'워크플로':job.kind==='meeting'?'그룹 회의':job.kind==='goal'?'자율 목표':'에이전트 작업'}</strong><small className="mt-1 block truncate text-[11px] text-slate-500">{job.input?.goal||job.input?.prompt||job.input?.workflowId||job.id}</small></div><StatusPill status={job.status}>{job.status}</StatusPill></div>):<div className="flex min-h-32 flex-col items-center justify-center gap-2 rounded-xl border border-dashed border-slate-200 bg-slate-50/70 p-5 text-center"><Activity size={22} className="text-slate-400"/><strong className="text-xs text-slate-600">아직 실행 내역이 없습니다</strong><span className="text-[11px] text-slate-400">실행하면 작업 상태가 기록됩니다</span></div>}</div>
    </section>
   </div>
   <div className="flex min-w-0 flex-col gap-5">
    <section className="rounded-2xl border border-slate-200 bg-white p-5 shadow-sm sm:p-6">
     <div className="mb-5 flex items-center justify-between gap-3"><div><div className="text-[10px] font-extrabold tracking-[.17em] text-blue-600">CONNECTIONS</div><h3 className="mb-0 mt-1 text-lg font-extrabold text-slate-900">연결 및 자원</h3></div><RadioTower size={20} className="text-slate-400"/></div>
     <div className="divide-y divide-slate-100">
      <div className="flex items-center gap-3 py-4 first:pt-0"><span className="grid size-10 place-items-center rounded-xl bg-blue-50 text-blue-600"><Compass size={19}/></span><div className="min-w-0 flex-1"><strong className="block text-xs text-slate-800">모델 제공자</strong><small className="text-[11px] text-slate-500">{providers.value.length}개 등록 · 실제 연결 확인은 모델 허브</small></div><Button variant="ghost" size="icon" aria-label="모델 관리" onClick={()=>navigate('models')}><ArrowRight size={17}/></Button></div>
      <div className="flex items-center gap-3 py-4"><span className="grid size-10 place-items-center rounded-xl bg-cyan-50 text-cyan-700"><RadioTower size={19}/></span><div className="min-w-0 flex-1"><strong className="block text-xs text-slate-800">메신저 채널</strong><small className="text-[11px] text-slate-500">{channels.value.filter(x=>x.status==='connected').length}개 연결 / {channels.value.length}개 등록</small></div><Button variant="ghost" size="icon" aria-label="채널 관리" onClick={()=>navigate('connections')}><ArrowRight size={17}/></Button></div>
      <div className="flex items-center gap-3 py-4 last:pb-0"><span className="grid size-10 place-items-center rounded-xl bg-violet-50 text-violet-600"><BookOpenText size={19}/></span><div className="min-w-0 flex-1"><strong className="block text-xs text-slate-800">기억 라이브러리</strong><small className="text-[11px] text-slate-500">저장된 기억 {memories.value.length}개</small></div><Button variant="ghost" size="icon" aria-label="기억 관리" onClick={()=>navigate('memory')}><ArrowRight size={17}/></Button></div>
     </div>
    </section>
    <section className="overflow-hidden rounded-2xl border border-blue-100 bg-gradient-to-br from-[#eaf1ff] to-white p-6 shadow-sm"><div className="grid size-11 place-items-center rounded-xl bg-white text-blue-600 shadow-sm"><Cloud size={22}/></div><h3 className="mb-2 mt-4 text-base font-extrabold text-slate-900">외부 모델도 한곳에서</h3><p className="mb-5 text-xs leading-6 text-slate-600">로컬 추론 엔진과 OpenAI 호환 API를 같은 대화와 작업 실행기에 연결합니다.</p><Button variant="secondary" className="!border-blue-200 !text-blue-700" onClick={()=>navigate('models')}>모델 허브 열기 <ArrowRight size={16}/></Button></section>
   </div>
  </div>
 </PageContainer>
}
