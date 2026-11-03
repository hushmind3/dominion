import {useEffect,useState} from 'react'
import {ArrowRight,Bot,BrainCircuit,Check,Edit3,Play,Plus,Settings2,Sparkles,Trash2,UsersRound} from 'lucide-react'
import {del,get,post,type Agent,type Provider,type Room} from '../api'
import {Alert,Badge,Button,Card,Dialog,Empty,Grid,IconButton,Input,Kpi,ListItem,Page,PageHeader,Row,Section,Select,Stack,Text,Textarea,Toggle} from '../design/Primitives'
import {useOperation,useResource} from '../design/useResource'
type Model={id:string;name?:string}
export function AgentsPage(){
 const agents=useResource<Agent[]>('/api/agents',[])
 const providers=useResource<Provider[]>('/api/providers',[])
 const operation=useOperation()
 const [edit,setEdit]=useState<Partial<Agent>|null>(null)
 const [models,setModels]=useState<Model[]>([])
 const [bulk,setBulk]=useState(false)
 const [chosen,setChosen]=useState<string[]>([])
 const [bulkProvider,setBulkProvider]=useState('')
 const [bulkModel,setBulkModel]=useState('')
 const [bulkModels,setBulkModels]=useState<Model[]>([])
 const [notice,setNotice]=useState('')
 const choices=providers.value.filter(x=>x.enabled).map(p=>({value:p.id,label:p.name}))
 useEffect(()=>{let live=true;setModels([]);if(!edit?.providerId)return;get<Model[]>('/api/providers/'+edit.providerId+'/models').then(x=>{if(live)setModels(x)}).catch(()=>{});return()=>{live=false}},[edit?.providerId])
 useEffect(()=>{let live=true;setBulkModels([]);if(!bulkProvider)return;get<Model[]>('/api/providers/'+bulkProvider+'/models').then(x=>{if(live)setBulkModels(x)}).catch(()=>{});return()=>{live=false}},[bulkProvider])
 const fresh=()=>setEdit({name:'새 에이전트',description:'',systemPrompt:'당신은 전문적인 AI 에이전트입니다. 실제 도구 실행과 정확한 결과 보고를 우선합니다.',aliases:[],providerId:'',model:''})
 const save=()=>void operation.run(async()=>{if(!edit?.name?.trim())throw Error('이름을 입력하세요');await post('/api/agents',edit);setEdit(null);await agents.reload()})
 const remove=(a:Agent)=>{if(a.id==='general')return;if(window.confirm(a.name+' 에이전트를 삭제할까요?'))void operation.run(async()=>{await del('/api/agents/'+a.id);await agents.reload()})}
 const apply=()=>void operation.run(async()=>{if(!chosen.length||!bulkProvider||!bulkModel)throw Error('적용할 대상과 모델을 선택하세요');const out=await post<{count:number}>('/api/agents/bulk-model',{agentIds:chosen,providerId:bulkProvider,model:bulkModel});setNotice(out.count+'개 에이전트의 모델 설정을 변경했습니다.');setBulk(false);await agents.reload()})
 return <Page><PageHeader eyebrow="AGENT STUDIO" title="에이전트" description="역할과 모델이 서로 다른 에이전트를 필요한 만큼 만드세요." actions={<><Button onClick={()=>{setBulk(v=>!v);setChosen(agents.value.map(a=>a.id));setNotice('')}}><Settings2 size={16}/> 일괄 모델 설정</Button><Button variant="primary" onClick={fresh}><Plus size={16}/> 에이전트 만들기</Button></>}/>
  <Stack gap="lg">{operation.error&&<Alert message={operation.error} onClose={()=>operation.setError('')}/>}
   {notice&&<Alert tone="positive" message={notice} onClose={()=>setNotice('')}/>}
   <Grid cols={3}><Kpi label="등록된 에이전트" value={agents.value.length} icon={<Bot size={20}/>}/><Kpi label="모델 연결" value={agents.value.filter(a=>a.providerId&&a.model).length} icon={<BrainCircuit size={20}/>}/><Kpi label="연결 가능한 제공자" value={providers.value.filter(p=>p.enabled).length} icon={<Sparkles size={20}/>}/></Grid>
   {bulk&&<Card><Stack gap="md"><Section title="선택한 에이전트에 모델 배정" subtitle={chosen.length+'개 선택'}><Row gap="sm" wrap><Toggle label="전체 선택" checked={chosen.length===agents.value.length&&agents.value.length>0} onChange={v=>setChosen(v?agents.value.map(a=>a.id):[])}/><Select label="제공자" value={bulkProvider} onChange={e=>{setBulkProvider(e.target.value);setBulkModel('')}} placeholder="선택" items={choices}/><Select label="모델" value={bulkModel} onChange={e=>setBulkModel(e.target.value)} items={bulkModels.map(m=>({value:m.id,label:m.name||m.id}))} placeholder="모델 선택"/><Button variant="primary" disabled={!chosen.length||!bulkProvider||!bulkModel||operation.working} onClick={apply}>일괄 적용</Button></Row></Section></Stack></Card>}
   <Grid cols={3}>{agents.value.map(a=><Card key={a.id}><Stack gap="md"><Row justify="between"><Row gap="sm"><span className="grid size-11 place-items-center rounded-xl bg-brand-soft text-brand"><Bot size={22}/></span><Stack gap="xs"><Text weight="bold" size="lg">{a.name}</Text><Text tone="subtle" size="xs">{a.id==='general'?'기본 에이전트':'사용자 정의 에이전트'}</Text></Stack></Row><Row gap="xs">{bulk&&<Toggle label="선택" checked={chosen.includes(a.id)} onChange={v=>setChosen(x=>v?[...new Set([...x,a.id])]:x.filter(id=>id!==a.id))}/>}<IconButton label={a.name+' 편집'} icon={<Edit3 size={16}/>} onClick={()=>setEdit({...a})}/>{a.id!=='general'&&<IconButton label={a.name+' 삭제'} icon={<Trash2 size={16}/>} onClick={()=>remove(a)}/>}</Row></Row><Text tone="muted">{a.description||'역할과 전문 분야를 설정할 수 있습니다.'}</Text><div className="h-px bg-line"/><Row wrap gap="xs"><Badge tone={a.model?'brand':'neutral'}>{a.model||'모델 미설정'}</Badge><Badge>{providers.value.find(p=>p.id===a.providerId)?.name||'제공자 미설정'}</Badge></Row></Stack></Card>)}</Grid>
   {!agents.value.length&&<Empty title="에이전트가 없습니다" action={<Button variant="primary" onClick={fresh}>에이전트 만들기</Button>}/>}
  </Stack>
  <Dialog open={Boolean(edit)} onClose={()=>setEdit(null)} title={edit?.id?'에이전트 편집':'에이전트 만들기'} description="역할, 모델, 별칭을 설정합니다." wide footer={<><Button onClick={()=>setEdit(null)}>취소</Button><Button variant="primary" disabled={!edit?.name?.trim()||operation.working} onClick={save}><Check size={15}/> 저장</Button></>}>
   {edit&&<Stack gap="md"><Input label="이름" value={edit.name||''} onChange={e=>setEdit({...edit,name:e.target.value})}/><Input label="설명" value={edit.description||''} onChange={e=>setEdit({...edit,description:e.target.value})}/><Grid cols={2}><Select label="모델 제공자" value={edit.providerId||''} onChange={e=>setEdit({...edit,providerId:e.target.value,model:''})} items={choices} placeholder="제공자 선택"/><Select label="모델" value={edit.model||''} onChange={e=>setEdit({...edit,model:e.target.value})} items={[...models.map(m=>({value:m.id,label:m.name||m.id})),...(edit.model&&!models.some(m=>m.id===edit.model)?[{value:edit.model,label:edit.model}]:[])]} placeholder="모델 선택"/></Grid><Input label="호출 별칭 (쉼표 구분)" value={(edit.aliases||[]).join(', ')} onChange={e=>setEdit({...edit,aliases:e.target.value.split(',').map(x=>x.trim()).filter(Boolean)})}/><Textarea label="시스템 프롬프트" rows={7} value={edit.systemPrompt||''} onChange={e=>setEdit({...edit,systemPrompt:e.target.value})}/></Stack>}
  </Dialog>
 </Page>
}
export function RoomsPage({openSession}:{openSession:(id:string)=>void}){
 const rooms=useResource<Room[]>('/api/rooms',[])
 const agents=useResource<Agent[]>('/api/agents',[])
 const jobs=useResource<any[]>('/api/jobs',[])
 const operation=useOperation()
 const [editing,setEditing]=useState<Partial<Room>|null>(null)
 const [goals,setGoals]=useState<Record<string,string>>({})
 const fresh=()=>setEditing({name:'새 회의실',description:'',agentIds:[],rounds:1,maxTurns:12})
 const save=()=>void operation.run(async()=>{if(!editing?.name?.trim()||!editing.agentIds?.length)throw Error('회의실 이름과 참여 에이전트를 선택하세요');await post('/api/rooms',editing);setEditing(null);await rooms.reload()})
 const run=(room:Room)=>void operation.run(async()=>{const goal=goals[room.id]?.trim();if(!goal)throw Error('회의 의제를 입력하세요');await post('/api/rooms/'+room.id+'/run',{goal});setGoals(x=>({...x,[room.id]:''}));await jobs.reload()})
 return <Page><PageHeader eyebrow="MULTI-AGENT" title="그룹 · 회의" description="인원 제한 없는 에이전트 회의실입니다. 참여자와 라운드를 직접 구성합니다." actions={<Button variant="primary" onClick={fresh}><Plus size={16}/> 회의실 만들기</Button>}/>
  <Stack gap="lg">{operation.error&&<Alert message={operation.error} onClose={()=>operation.setError('')}/>}
   <Grid cols={2}>{rooms.value.map(room=>{
    const last=jobs.value.find(j=>j.kind==='meeting'&&j.input?.roomId===room.id)
    return <Card key={room.id}><Stack gap="md"><Row justify="between"><Row gap="sm"><span className="grid size-11 place-items-center rounded-xl bg-brand-soft text-brand"><UsersRound size={22}/></span><Stack gap="xs"><Text weight="bold" size="lg">{room.name}</Text><Text tone="subtle" size="xs">{room.description||'에이전트 공동 연구와 토론'}</Text></Stack></Row><IconButton icon={<Settings2 size={17}/>} label="회의실 편집" onClick={()=>setEditing({...room})}/></Row><Row gap="xs" wrap>{room.agentIds.map(id=><Badge key={id} tone="brand">{agents.value.find(a=>a.id===id)?.name||id}</Badge>)}</Row><Row justify="between" wrap><Text tone="subtle" size="xs">{room.agentIds.length}명 · {room.rounds} 라운드</Text>{last&&<Badge tone={last.status==='completed'?'positive':'neutral'}>{last.status}</Badge>}</Row><Row gap="sm"><Input aria-label={room.name+' 회의 의제'} placeholder="회의 의제 입력" value={goals[room.id]||''} onChange={e=>setGoals(x=>({...x,[room.id]:e.target.value}))} onKeyDown={e=>{if(e.key==='Enter')run(room)}}/><Button variant="primary" disabled={!goals[room.id]?.trim()||operation.working} onClick={()=>run(room)}><Play size={16}/> 실행</Button></Row>{room.sessionId&&<Button size="sm" variant="ghost" onClick={()=>openSession(room.sessionId!)}>기록 열기 <ArrowRight size={15}/></Button>}</Stack></Card>
   })}</Grid>{!rooms.value.length&&<Empty icon={<UsersRound size={22}/>} title="회의실이 없습니다" description="필요한 에이전트를 선택해 회의실을 만드세요." action={<Button variant="primary" onClick={fresh}>회의실 만들기</Button>}/>}
  </Stack>
  <Dialog open={Boolean(editing)} onClose={()=>setEditing(null)} title="회의실 구성" wide footer={<><Button onClick={()=>setEditing(null)}>취소</Button><Button variant="primary" onClick={save} disabled={!editing?.name?.trim()||!editing.agentIds?.length}>저장</Button></>}>
  {editing&&<Stack gap="md"><Input label="이름" value={editing.name||''} onChange={e=>setEditing({...editing,name:e.target.value})}/><Input label="설명" value={editing.description||''} onChange={e=>setEditing({...editing,description:e.target.value})}/><Grid cols={2}><Select label="라운드" value={String(editing.rounds||1)} onChange={e=>setEditing({...editing,rounds:Number(e.target.value)})} items={[1,2,3,4,5].map(v=>({value:String(v),label:v+'회'}))}/><Input label="최대 총 발언" type="number" min={1} max={60} value={editing.maxTurns||12} onChange={e=>setEditing({...editing,maxTurns:Number(e.target.value)})}/></Grid><Section title={'참가자 · '+(editing.agentIds?.length||0)+'명'}><Card pad="sm"><Stack gap="sm">{agents.value.map(a=><Row key={a.id} justify="between" wrap><Toggle label={a.name} checked={editing.agentIds?.includes(a.id)||false} onChange={v=>setEditing({...editing,agentIds:v?[...(editing.agentIds||[]),a.id]:(editing.agentIds||[]).filter(id=>id!==a.id)})}/><Text tone="subtle" size="xs">{a.model||'모델 미설정'}</Text></Row>)}</Stack></Card></Section></Stack>}
  </Dialog>
 </Page>
}
