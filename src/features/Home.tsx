import {ArrowRight,BrainCircuit,Cloud,Compass,MessageSquareMore,RadioTower,Workflow} from 'lucide-react'
import {type Agent,type ChatSession,type Provider} from '../api'
import {Badge,Button,Card,Empty,Grid,Kpi,ListItem,Page,PageHeader,Row,Section,Stack,Text} from '../design/Primitives'
import {useResource} from '../design/useResource'
type Dest='chat'|'models'|'agents'|'rooms'|'connections'|'workflows'|'memory'|'activity'
export function HomePage({navigate}:{navigate:(page:Dest)=>void}){
 const sessions=useResource<ChatSession[]>('/api/sessions',[])
 const providers=useResource<Provider[]>('/api/providers',[])
 const agents=useResource<Agent[]>('/api/agents',[])
 const jobs=useResource<any[]>('/api/jobs',[])
 const channels=useResource<any[]>('/api/channels',[])
 const memories=useResource<any[]>('/api/memories',[])
 const links=[{name:'새 대화',description:'모델과 대화하고 도구를 실행합니다',icon:<MessageSquareMore size={19}/>,page:'chat' as const},{name:'모델 찾기',description:'GGUF·MLX 모델을 설치하고 실행합니다',icon:<Compass size={19}/>,page:'models' as const},{name:'에이전트 만들기',description:'역할과 모델을 가진 에이전트를 구성합니다',icon:<BrainCircuit size={19}/>,page:'agents' as const},{name:'워크플로',description:'단계별 작업을 연결하고 실행합니다',icon:<Workflow size={19}/>,page:'workflows' as const}]
 return <Page><PageHeader eyebrow="WORKSPACE OVERVIEW" title="모든 지능을 한곳에서." description="모델, 에이전트, 도구, 작업 현황을 실제 DOMINION 서버 데이터에서 표시합니다."/>
 <Stack gap="xl"><Grid cols={4}><Kpi label="대화 세션" value={sessions.value.length} icon={<MessageSquareMore size={20}/>} onClick={()=>navigate('chat')}/><Kpi label="모델 제공자" value={providers.value.filter(x=>x.enabled).length} icon={<Compass size={20}/>} onClick={()=>navigate('models')}/><Kpi label="에이전트" value={agents.value.length} icon={<BrainCircuit size={20}/>} onClick={()=>navigate('agents')}/><Kpi label="작업 실행" value={jobs.value.length} icon={<Workflow size={20}/>} onClick={()=>navigate('workflows')}/></Grid>
 <Grid cols={2}>
  <Section title="빠른 시작" subtitle="가장 많이 사용하는 작업"><Card pad="none">{links.map(x=><ListItem key={x.name} title={x.name} subtitle={x.description} leading={x.icon} onClick={()=>navigate(x.page)} actions={<Button size="sm" variant="ghost" onClick={()=>navigate(x.page)}><ArrowRight size={16}/></Button>}/>)}</Card></Section>
  <Section title="연결 현황" subtitle="현재 작업공간 설정"><Card pad="none"><ListItem title="메신저 채널" subtitle={channels.value.length+'개 등록 · '+channels.value.filter(x=>x.status==='connected').length+'개 연결됨'} leading={<RadioTower size={20}/>} actions={<Button size="sm" onClick={()=>navigate('connections')}>관리 <ArrowRight size={14}/></Button>}/><ListItem title="장기 저장 기억" subtitle={memories.value.length+'개 저장 항목'} leading={<BrainCircuit size={20}/>} actions={<Button size="sm" onClick={()=>navigate('memory')}>열기 <ArrowRight size={14}/></Button>}/><ListItem title="모델 엔드포인트" subtitle={providers.value.filter(p=>p.enabled).length+'개 활성 제공자'} leading={<Cloud size={20}/>} actions={<Button size="sm" onClick={()=>navigate('models')}>확인 <ArrowRight size={14}/></Button>}/></Card></Section>
 </Grid>
 <Section title="최근 작업" actions={<Button size="sm" onClick={()=>navigate('activity')}>전체 기록 <ArrowRight size={14}/></Button>}><Card pad="none">{jobs.value.slice(0,8).map(j=><ListItem key={j.id} title={j.kind==='meeting'?'그룹 회의':j.kind==='workflow'?'워크플로':'에이전트 작업'} subtitle={j.input?.prompt||j.input?.goal||j.id} leading={<Workflow size={19}/>} meta={<Badge tone={j.status==='completed'?'positive':j.status==='failed'?'negative':'neutral'}>{j.status}</Badge>}/>)}{!jobs.value.length&&<Empty title="아직 실행 기록이 없습니다" description="대화에서 /goal 명령을 쓰거나 워크플로를 실행하면 기록됩니다."/>}</Card></Section>
 </Stack></Page>
}
