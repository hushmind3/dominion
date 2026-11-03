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
 return <div className="content-wrap">
  <Headline eyebrow="OVERVIEW · LOCAL WORKSPACE" title="오늘도, 모든 지능을 한곳에." description="더미 데이터 대신 실제 게이트웨이에서 읽은 모델·에이전트·대화·작업 상태입니다."/>
  <div className="home-metrics">
   {[
    {label:'대화 세션',value:sessions.value.length,icon:MessageSquareMore,tab:'chat' as const},
    {label:'모델 제공자',value:providers.value.length,icon:Compass,tab:'models' as const},
    {label:'에이전트',value:agents.value.length,icon:BrainCircuit,tab:'agents' as const},
    {label:'작업 실행',value:jobs.value.length,icon:Workflow,tab:'workflows' as const}
   ].map(x=>{const Icon=x.icon;return <button className="home-metric" key={x.label} onClick={()=>navigate(x.tab)}><span className="home-metric-icon"><Icon size={20}/></span><small>{x.label}</small><strong>{x.value.toLocaleString()}</strong><ArrowRight size={15}/></button>})}
  </div>
  <div className="home-two-column">
   <Panel className="home-section"><Subheading title="빠른 시작"/><div className="home-actions">{[
    {title:'새 대화',detail:'연결된 AI 모델과 대화하기',icon:MessageSquareMore,page:'chat' as const},
    {title:'모델 찾기',detail:'Hugging Face와 모델 제공자',icon:Compass,page:'models' as const},
    {title:'에이전트 팀',detail:'역할과 모델을 가진 에이전트',icon:BrainCircuit,page:'agents' as const},
    {title:'워크플로',detail:'여러 단계를 연결해 실제 실행',icon:Workflow,page:'workflows' as const}
   ].map(x=>{const Icon=x.icon;return <button key={x.title} onClick={()=>navigate(x.page)}><span><Icon size={19}/></span><div><strong>{x.title}</strong><small>{x.detail}</small></div><ArrowRight size={16}/></button>})}</div></Panel>
   <Panel className="home-section"><Subheading title="연결 현황"/><div className="home-connection"><RadioTower size={21}/><div><strong>메신저 채널</strong><small>{channels.value.length}개 구성 · {channels.value.filter(x=>x.status==='connected').length}개 연결 중</small></div><button onClick={()=>navigate('connections')}>관리 <ArrowRight size={15}/></button></div><div className="home-connection"><BrainCircuit size={21}/><div><strong>장기 저장 기억</strong><small>{memories.value.length}개 저장 항목</small></div><button onClick={()=>navigate('memory')}>열기 <ArrowRight size={15}/></button></div><div className="home-connection"><Cloud size={21}/><div><strong>모델 엔드포인트</strong><small>연결 여부는 모델 허브에서 검사</small></div><button onClick={()=>navigate('models')}>검사 <ArrowRight size={15}/></button></div></Panel>
  </div>
  <div className="section-gap"><Subheading title="최근 작업" extra={<button className="text-button" onClick={()=>navigate('activity')}>실행 기록 전체 <ArrowRight size={15}/></button>}/></div>
  <Panel className="home-job-list">{latest.length?latest.map(x=><div className="home-job" key={x.id}><span className="home-job-icon"><Workflow size={17}/></span><div><strong>{x.kind==='workflow'?'워크플로':x.kind==='meeting'?'그룹 회의':'에이전트 작업'}</strong><small>{x.input?.prompt||x.input?.goal||x.input?.workflowId||x.id}</small></div><span className={'job-status '+x.status}>{x.status}</span></div>):<div className="empty"><strong>아직 실행된 작업이 없습니다.</strong><p>대화창의 /goal 또는 워크플로에서 실행하면 이곳에 기록됩니다.</p></div>}</Panel>
 </div>
}
