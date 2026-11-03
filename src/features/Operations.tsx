import {useState} from 'react'
import {Activity,CalendarClock,Check,Clock3,KeyRound,Lock,Plus,RefreshCw,Settings2,ShieldCheck,Trash2,Workflow} from 'lucide-react'
import {del,post,setAuthToken,type Agent,type Provider} from '../api'
import {Alert,Badge,Button,Card,Dialog,Empty,Grid,Highlight,IconButton,Input,KeyValue,ListItem,Page,PageHeader,Row,Section,Select,Stack,Text,Textarea,Toggle} from '../design/Primitives'
import {useOperation,useResource} from '../design/useResource'
function Jobs({jobs}:{jobs:any[]}){
 return <Card pad="none">{jobs.slice(0,35).map(j=><ListItem key={j.id} leading={<Workflow size={19}/>} title={j.kind==='meeting'?'그룹 회의':j.kind==='workflow'?'워크플로 실행':'에이전트 작업'} subtitle={<><span>{j.input?.goal||j.input?.prompt||j.input?.workflowId||'작업 실행'}</span>{j.error&&<Text tone="negative" size="xs"> · {j.error}</Text>}</>} meta={<Badge tone={j.status==='completed'?'positive':j.status==='failed'?'negative':'neutral'}>{j.status}</Badge>}/>)}{!jobs.length&&<Empty icon={<Workflow size={24}/>} title="작업 기록이 없습니다"/>}</Card>
}
export function OperationsPage({section}:{section:'automations'|'activity'|'settings'}){
 const jobs=useResource<any[]>('/api/jobs',[])
 const automations=useResource<any[]>('/api/automations',[])
 const audit=useResource<any[]>('/api/audit',[])
 const agents=useResource<Agent[]>('/api/agents',[])
 const providers=useResource<Provider[]>('/api/providers',[])
 const policy=useResource<any>('/api/policy',{id:'default',autoApprove:[],autoApproveMcp:false,autoApproveAll:false})
 const op=useOperation()
 const [editing,setEditing]=useState<any|null>(null)
 const [token,setToken]=useState('')
 const [notice,setNotice]=useState('')
 const fresh=()=>setEditing({kind:'prompt',input:{prompt:'',agentId:'general',providerId:'',model:''},intervalMinutes:1440,nextAt:new Date(Date.now()+3600000).toISOString(),enabled:true})
 const save=()=>void op.run(async()=>{await post('/api/automations',editing);setEditing(null);setNotice('예약 작업을 저장했습니다.');await automations.reload()})
 const remove=(v:any)=>{if(window.confirm('이 자동화를 삭제하시겠습니까?'))void op.run(async()=>{await del('/api/automations/'+v.id);await automations.reload()})}
 const savePolicy=(value:any)=>void op.run(async()=>{await post('/api/policy',value);await policy.reload();setNotice('실행 권한 설정이 변경되었습니다.')})
 const setInput=(key:string,value:any)=>setEditing((x:any)=>({...x,input:{...x.input,[key]:value}}))
 const format=(value:string)=>{try{return new Date(value).toLocaleString('ko-KR')}catch{return value}}
 return <Page><PageHeader eyebrow={section==='settings'?'ENGINE CONFIGURATION':section==='activity'?'OBSERVABILITY':'AUTONOMOUS EXECUTION'} title={section==='settings'?'설정':section==='activity'?'실행 기록':'자동화'} description={section==='settings'?'권한과 게이트웨이 접근을 관리합니다.':section==='activity'?'도구·에이전트 실행 결과와 이벤트를 확인합니다.':'반복 작업을 등록하고 스케줄 실행을 관리합니다.'} actions={section==='automations'?<Button variant="primary" onClick={fresh}><Plus size={16}/> 자동화 추가</Button>:section==='activity'?<Button onClick={()=>{void jobs.reload();void audit.reload()}}><RefreshCw size={15}/> 새로고침</Button>:undefined}/>
 <Stack gap="lg">{op.error&&<Alert message={op.error} onClose={()=>op.setError('')}/>}
 {notice&&<Alert tone="positive" message={notice} onClose={()=>setNotice('')}/>}
 {section==='automations'&&<>
  <Highlight title="서버 실행 시간에 따라 예약 실행" icon={<CalendarClock size={16}/>}>서버가 켜져 있을 때 자동화를 실행합니다. 예약과 작업 기록은 로컬 DB에 보관됩니다.</Highlight>
  <Section title="예약된 작업" subtitle={automations.value.length+'개 구성'}>
   <Card pad="none">{automations.value.map(a=><ListItem key={a.id} title={a.kind==='meeting'?'그룹 회의':a.kind==='workflow'?'워크플로':'에이전트 작업'} subtitle={(a.input?.prompt||a.input?.goal||'작업 예약')+' · '+a.intervalMinutes+'분 간격 · 다음 '+format(a.nextAt)} leading={<Clock3 size={20}/>} meta={<Badge tone={a.enabled?'positive':'neutral'}>{a.enabled?'활성':'비활성'}</Badge>} actions={<><IconButton label="예약 수정" size="sm" icon={<Settings2 size={16}/>} onClick={()=>setEditing({...a})}/><IconButton label="예약 삭제" size="sm" icon={<Trash2 size={16}/>} onClick={()=>remove(a)}/></>}/>)}{!automations.value.length&&<Empty icon={<CalendarClock size={23}/>} title="예약된 자동화가 없습니다"/>}</Card>
  </Section><Section title="작업 대기열"><Jobs jobs={jobs.value}/></Section>
 </>}
 {section==='activity'&&<><Section title="에이전트 작업"><Jobs jobs={jobs.value}/></Section><Section title="시스템 이벤트" subtitle="최근 감사 원장 기록"><Card pad="none">{audit.value.length?audit.value.map((a:any)=><ListItem key={a.id} title={a.event} subtitle={JSON.stringify(a.detail).slice(0,280)} leading={<Activity size={19}/>} meta={<Text tone="subtle" size="xs">{format(a.created_at||a.createdAt)}</Text>}/>):<Empty icon={<Activity size={23}/>} title="시스템 이벤트가 없습니다"/>}</Card></Section></>}
 {section==='settings'&&<Grid cols={2}>
  <Card><Stack gap="lg"><Row gap="sm"><ShieldCheck size={22} className="text-brand"/><Text weight="bold" size="lg">도구 실행 권한</Text></Row><Text tone="muted">파일 수정, 셸 실행, MCP 호출에 대한 승인 방식을 설정합니다.</Text><Toggle label="모든 컴퓨터 도구 자동 승인" checked={Boolean(policy.value.autoApproveAll)} onChange={v=>savePolicy({...policy.value,autoApproveAll:v})}/><Toggle label="MCP 호출 자동 승인" checked={Boolean(policy.value.autoApproveMcp)} onChange={v=>savePolicy({...policy.value,autoApproveMcp:v})}/></Stack></Card>
  <Card><Stack gap="md"><Row gap="sm"><Lock size={22} className="text-brand"/><Text weight="bold" size="lg">게이트웨이 접근</Text></Row><Text tone="muted">기본 로컬 주소: 127.0.0.1:4317. 원격 연결은 관리 토큰이 필요합니다.</Text><Input label="브라우저에 적용할 관리자 토큰" type="password" value={token} onChange={e=>setToken(e.target.value)} placeholder="DOMINION_ADMIN_TOKEN"/><Button onClick={()=>{setAuthToken(token);setNotice('현재 창에 관리자 토큰을 적용했습니다.')}}><KeyRound size={15}/> 현재 창에 적용</Button></Stack></Card>
  <Card><Stack gap="md"><Text weight="bold" size="lg">시스템 정보</Text><KeyValue label="엔진" value="DOMINION 0.1"/><KeyValue label="서버" value="Node.js"/><KeyValue label="데이터베이스" value="SQLite · WAL"/><KeyValue label="실행 방식" value="Local-first"/></Stack></Card>
 </Grid>}
 </Stack>
 <Dialog open={Boolean(editing)} onClose={()=>setEditing(null)} title="자동화 설정" wide footer={<><Button onClick={()=>setEditing(null)}>취소</Button><Button variant="primary" onClick={save} disabled={op.working||!editing}>저장</Button></>}>
 {editing&&<Stack gap="md"><Select label="작업 유형" value={editing.kind} onChange={e=>setEditing({...editing,kind:e.target.value})} items={[{value:'prompt',label:'에이전트 작업'},{value:'meeting',label:'그룹 회의'},{value:'workflow',label:'워크플로'}]}/><Textarea label={editing.kind==='meeting'?'회의 의제':'작업 지시'} value={editing.kind==='meeting'?editing.input?.goal||'':editing.input?.prompt||''} onChange={e=>setInput(editing.kind==='meeting'?'goal':'prompt',e.target.value)} rows={4}/><Grid cols={2}><Input label="반복 간격 (분)" type="number" min={1} value={editing.intervalMinutes||1440} onChange={e=>setEditing({...editing,intervalMinutes:Number(e.target.value)})}/><Input label="다음 실행" type="datetime-local" value={editing.nextAt?new Date(editing.nextAt).toISOString().slice(0,16):''} onChange={e=>setEditing({...editing,nextAt:new Date(e.target.value).toISOString()})}/></Grid>{editing.kind==='prompt'?<><Select label="에이전트" value={editing.input?.agentId||'general'} onChange={e=>setInput('agentId',e.target.value)} items={agents.value.map(a=>({value:a.id,label:a.name}))}/><Select label="모델 제공자" value={editing.input?.providerId||''} onChange={e=>setInput('providerId',e.target.value)} items={[{value:'',label:'에이전트 기본값'},...providers.value.map(p=>({value:p.id,label:p.name}))]}/><Input label="모델 ID" value={editing.input?.model||''} onChange={e=>setInput('model',e.target.value)}/></>:editing.kind==='meeting'?<Input label="회의실 ID" value={editing.input?.roomId||''} onChange={e=>setInput('roomId',e.target.value)}/>:<Input label="워크플로 ID" value={editing.input?.workflowId||''} onChange={e=>setInput('workflowId',e.target.value)}/>}<Toggle label="활성화" checked={Boolean(editing.enabled)} onChange={v=>setEditing({...editing,enabled:v})}/></Stack>}
 </Dialog>
 </Page>
}
