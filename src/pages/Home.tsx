import {StatusPill} from '../components/StatusPill'
import {Button} from '../components/Primitives'
import {Activity,ArrowRight,BrainCircuit,Cloud,Compass,Layers3,MessageSquareMore,RadioTower,Workflow} from 'lucide-react'
import {Headline,Panel,Subheading,StateTag,useRemote} from '../components/UI'
import type {Agent,ChatSession,Provider} from '../api'
type Navigate=(page:'chat'|'models'|'agents'|'rooms'|'connections'|'workflows'|'memory'|'activity')=>void
export function HomePage({navigate}:{navigate:Navigate}){
 const sessions=useRemote<ChatSession[]>('/api/sessions',[]);
 const providers=useRemote<Provider[]>('/api/providers',[]);
 const agents=useRemote<Agent[]>('/api/agents',[]);
 const jobs=useRemote<any[]>('/api/jobs',[]);
 const channels=useRemote<any[]>('/api/channels',[]);
 const memories=useRemote<any[]>('/api/memories',[]);
 const latest=jobs.value.slice(0,6);
 return <div className="mx-auto w-full max-w-[1320px] px-5 py-9 sm:px-8 lg:px-11 lg:pb-20">
  <Headline eyebrow="OVERVIEW · LOCAL WORKSPACE" title="오늘도, 모든 지능을 한곳에." description="더미 데이터 대신 실제 게이트웨이에서 읽은 모델·에이전트·대화·작업 상태입니다."/>
  <div className="grid grid-cols-2 gap-4 my-6 xl:grid-cols-4">
   {[
    {label:'대화 세션',value:sessions.value.length,icon:MessageSquareMore,tab:'chat' as const},
    {label:'모델 제공자',value:providers.value.length,icon:Compass,tab:'models' as const},
    {label:'에이전트',value:agents.value.length,icon:BrainCircuit,tab:'agents' as const},
    {label:'작업 실행',value:jobs.value.length,icon:Workflow,tab:'workflows' as const}
   ].map(x=>{const Icon=x.icon;return <Button className="flex min-w-0 flex-col gap-2 rounded-2xl border border-slate-200 bg-white p-5 shadow-sm" key={x.label} onClick={()=>navigate(x.tab)}><span className="grid size-11 place-items-center rounded-xl bg-blue-50 text-blue-600"><Icon size={20}/></span><small>{x.label}</small><strong>{x.value.toLocaleString()}</strong><ArrowRight size={15}/></Button>})}
  </div>
  <div className="grid min-w-0 grid-cols-1 gap-5 xl:grid-cols-2">
   <Panel className="min-w-0 p-5"><Subheading title="빠른 시작"/><div className="flex flex-wrap items-center gap-2">{[
    {title:'새 대화',detail:'연결된 AI 모델과 대화하기',icon:MessageSquareMore,page:'chat' as const},
    {title:'모델 찾기',detail:'Hugging Face와 모델 제공자',icon:Compass,page:'models' as const},
    {title:'에이전트 팀',detail:'역할과 모델을 가진 에이전트',icon:BrainCircuit,page:'agents' as const},
    {title:'워크플로',detail:'여러 단계를 연결해 실제 실행',icon:Workflow,page:'workflows' as const}
   ].map(x=>{const Icon=x.icon;return <Button key={x.title} onClick={()=>navigate(x.page)}><span><Icon size={19}/></span><div><strong>{x.title}</strong><small>{x.detail}</small></div><ArrowRight size={16}/></Button>})}</div></Panel>
   <Panel className="min-w-0 p-5"><Subheading title="연결 현황"/><div className="flex items-center gap-3 border-b border-slate-100 py-3 last:border-0"><RadioTower size={21}/><div><strong>메신저 채널</strong><small>{channels.value.length}개 구성 · {channels.value.filter(x=>x.status==='connected').length}개 연결 중</small></div><Button onClick={()=>navigate('connections')}>관리 <ArrowRight size={15}/></Button></div><div className="flex items-center gap-3 border-b border-slate-100 py-3 last:border-0"><BrainCircuit size={21}/><div><strong>장기 저장 기억</strong><small>{memories.value.length}개 저장 항목</small></div><Button onClick={()=>navigate('memory')}>열기 <ArrowRight size={15}/></Button></div><div className="flex items-center gap-3 border-b border-slate-100 py-3 last:border-0"><Cloud size={21}/><div><strong>모델 엔드포인트</strong><small>연결 여부는 모델 허브에서 검사</small></div><Button onClick={()=>navigate('models')}>검사 <ArrowRight size={15}/></Button></div></Panel>
  </div>
  <div className="mt-10"><Subheading title="최근 작업" extra={<Button className="inline-flex items-center gap-2 text-xs font-semibold text-slate-600 hover:text-blue-600" onClick={()=>navigate('activity')}>실행 기록 전체 <ArrowRight size={15}/></Button>}/></div>
  <Panel className="flex min-w-0 flex-col gap-3">{latest.length?latest.map(x=><div className="flex min-w-0 items-center gap-3 rounded-xl border border-slate-100 p-3" key={x.id}><span className="grid size-10 shrink-0 place-items-center rounded-xl bg-blue-50 text-blue-600"><Workflow size={17}/></span><div><strong>{x.kind==='workflow'?'워크플로':x.kind==='meeting'?'그룹 회의':'에이전트 작업'}</strong><small>{x.input?.prompt||x.input?.goal||x.input?.workflowId||x.id}</small></div><StatusPill status={x.status}>{x.status}</StatusPill></div>):<div className="flex min-h-40 flex-col items-center justify-center gap-2 p-7 text-center text-slate-500"><strong>아직 실행된 작업이 없습니다.</strong><p>대화창의 /goal 또는 워크플로에서 실행하면 이곳에 기록됩니다.</p></div>}</Panel>
 </div>
}
