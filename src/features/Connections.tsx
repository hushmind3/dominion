import {useEffect,useMemo,useState} from 'react'
import {Activity,ArrowRight,Check,ExternalLink,Globe2,KeyRound,MessageSquareMore,Plus,RadioTower,Settings2,ShieldCheck,Trash2,UsersRound} from 'lucide-react'
import {del,get,post,type Agent,type Provider,type Room} from '../api'
import {Alert,Badge,Button,Card,Dialog,Empty,Grid,Highlight,IconButton,Input,Kpi,Page,PageHeader,Row,SearchBox,Section,Select,Stack,Tabs,Text,Toggle} from '../design/Primitives'
import {useOperation,useResource} from '../design/useResource'
type Field={key:string;label:string;type:'secret'|'text'|'url';required?:boolean;placeholder?:string}
type Entry={id:string;name:string;category:string;mode:string;description:string;docs:string;available:boolean;status:string;features:string[];fields:Field[];instructions?:string[];requiresPublicUrl?:boolean}
type Channel={id:string;name:string;kind:string;agentId:string;providerId:string;model:string;enabled:boolean;mentionOnly:boolean;roomId?:string;roomPolicy?:string;status:string;runtimeError?:string;configured:boolean;missing?:string[];credentialsPresent:Record<string,boolean>}
type OAuth={id:string;configured:boolean;scope:string}
type Account={id:string;provider:string;displayName:string;scopes:string;hasToken:boolean}
type Editor={entry:Entry;record:Record<string,any>}
export function ConnectionsPage(){
 const registry=useResource<Entry[]>('/api/channels/catalog',[])
 const channels=useResource<Channel[]>('/api/channels',[])
 const oauth=useResource<OAuth[]>('/api/oauth/providers',[])
 const accounts=useResource<Account[]>('/api/oauth/connections',[])
 const agents=useResource<Agent[]>('/api/agents',[])
 const rooms=useResource<Room[]>('/api/rooms',[])
 const providers=useResource<Provider[]>('/api/providers',[])
 const op=useOperation()
 const [tab,setTab]=useState('channels')
 const [query,setQuery]=useState('')
 const [category,setCategory]=useState('all')
 const [editor,setEditor]=useState<Editor|null>(null)
 const [notice,setNotice]=useState('')
 const [probe,setProbe]=useState<Record<string,string>>({})
 useEffect(()=>{const listener=(event:MessageEvent)=>{if(event.data==='dominion-oauth-complete'){void accounts.reload();setNotice('계정 연결을 확인했습니다.')}};window.addEventListener('message',listener);return()=>window.removeEventListener('message',listener)},[])
 const filtered=useMemo(()=>registry.value.filter(x=>(category==='all'||x.category===category)&&[x.id,x.name,x.description].join(' ').toLowerCase().includes(query.toLowerCase())),[registry.value,category,query])
 const edit=(entry:Entry,record?:Channel)=>{
  if(!entry.available)return
  const data:Record<string,any>=record?{...record}:{name:entry.name,kind:entry.id,enabled:false,mentionOnly:true,roomId:'',roomPolicy:'all_or_named',agentId:'general',providerId:'',model:''}
  for(const field of entry.fields)if(field.type==='secret')data[field.key]=''
  setEditor({entry,record:data})
 }
 const setField=(key:string,value:any)=>setEditor(x=>x?{...x,record:{...x.record,[key]:value}}:null)
 const save=()=>void op.run(async()=>{if(!editor)return;await post('/api/channels',editor.record);setNotice(editor.entry.name+' 연결 설정을 저장했습니다.');setEditor(null);await channels.reload()})
 const remove=(x:Channel)=>{if(window.confirm(x.name+' 연결을 제거할까요?'))void op.run(async()=>{await del('/api/channels/'+x.id);await channels.reload()})}
 const diagnose=(x:Channel)=>void op.run(async()=>{setProbe(v=>({...v,[x.id]:'진단 중'}));const result=await post<any>('/api/channels/'+x.id+'/diagnose',{});setProbe(v=>({...v,[x.id]:result.verified?'인증 확인됨':result.message||'연결 점검 필요'}))})
 const connect=async(id:string)=>{
  const windowRef=window.open('','dominion_oauth','width=650,height=800')
  try{const response=await get<{url:string}>('/api/oauth/'+id+'/start');if(windowRef)windowRef.location.href=response.url;else window.location.href=response.url}catch(e){windowRef?.close();op.setError((e as Error).message)}
 }
 const categories=[{value:'all',label:'전체'},{value:'messaging',label:'메신저'},{value:'workplace',label:'업무'},{value:'community',label:'커뮤니티'},{value:'decentralized',label:'분산형'}]
 return <Page><PageHeader eyebrow="CHANNELS & ACCOUNTS" title="연결 · 채널" description="메신저 채널과 계정 연결을 한곳에서 관리합니다. 준비된 드라이버와 실제 연결 상태를 구분합니다." actions={<Button onClick={()=>{void registry.reload();void channels.reload()}}><Activity size={16}/> 새로고침</Button>}/>
 <Stack gap="lg">
  <Grid cols={3}><Kpi label="발견된 채널" value={registry.value.length} icon={<Globe2 size={20}/>}/><Kpi label="드라이버 준비" value={registry.value.filter(x=>x.available).length} icon={<RadioTower size={20}/>}/><Kpi label="실제 연결 중" value={channels.value.filter(x=>x.status==='connected').length} icon={<Check size={20}/>}/></Grid>
  {(registry.error||op.error)&&<Alert message={registry.error||op.error} onClose={()=>op.setError('')}/>}
  {notice&&<Alert tone="positive" message={notice} onClose={()=>setNotice('')}/>}
  <Card pad="none"><Tabs value={tab} onChange={setTab} items={[{value:'channels',label:'메신저 · 채널',count:registry.value.length},{value:'oauth',label:'OAuth · 계정',count:accounts.value.length}]}/>
   {tab==='channels'&&<div className="p-4 sm:p-6"><Stack gap="lg"><Row gap="md" wrap><SearchBox value={query} onChange={setQuery} placeholder="Telegram, Slack, Matrix, WhatsApp 등"/><Select label="분류" value={category} onChange={e=>setCategory(e.target.value)} items={categories}/></Row><Grid cols={3}>{filtered.map(item=>{
    const bindings=channels.value.filter(x=>x.kind===item.id)
    return <Card key={item.id} pad="sm"><Stack gap="md"><Row justify="between"><span className="grid size-11 place-items-center rounded-xl bg-brand-soft text-brand"><MessageSquareMore size={22}/></span><Badge tone={item.available?'positive':'warning'}>{item.available?'드라이버 준비':'미구현'}</Badge></Row><Stack gap="xs"><Text weight="bold" size="lg">{item.name}</Text><Text tone="muted">{item.description}</Text></Stack><Row gap="xs" wrap>{(item.features||[]).slice(0,4).map(f=><Badge key={f}>{f}</Badge>)}</Row><Stack gap="sm">{bindings.map(x=><Card key={x.id} pad="sm" tone="subtle"><Stack gap="sm"><Row justify="between"><Text weight="semibold">{x.name}</Text><Badge tone={x.status==='connected'?'positive':x.status==='error'?'negative':'neutral'}>{x.status}</Badge></Row>{x.runtimeError&&<Text tone="negative" size="xs">{x.runtimeError}</Text>}{probe[x.id]&&<Text size="xs" tone="muted">{probe[x.id]}</Text>}<Row wrap gap="xs"><Button size="sm" onClick={()=>edit(item,x)}><Settings2 size={14}/> 수정</Button><Button size="sm" onClick={()=>diagnose(x)}><Activity size={14}/> 진단</Button><IconButton label="채널 삭제" icon={<Trash2 size={15}/>} onClick={()=>remove(x)} size="sm"/></Row></Stack></Card>)}</Stack><Row justify="between" wrap><Button size="sm" variant="ghost" onClick={()=>window.open(item.docs,'_blank','noopener,noreferrer')}><ExternalLink size={14}/> 공식 문서</Button><Button size="sm" variant="quiet" disabled={!item.available} onClick={()=>edit(item)}><Plus size={15}/> 연결 추가</Button></Row></Stack></Card>
   })}</Grid>{!filtered.length&&<Empty title="검색 결과가 없습니다"/>}<Highlight title="카탈로그와 실제 실행은 다릅니다" icon={<ShieldCheck size={16}/>}>설정된 연결, 인증 완료, 메시지 수신 가능 상태를 구분하여 표시합니다. 채널별 필요한 HTTPS 웹훅 설정은 개별 연결에서 안내합니다.</Highlight></Stack></div>}
   {tab==='oauth'&&<div className="p-4 sm:p-6"><Grid cols={3}>{oauth.value.map(x=>{const existing=accounts.value.find(a=>a.provider===x.id);return <Card key={x.id}><Stack gap="md"><span className="grid size-12 place-items-center rounded-xl bg-brand-soft text-brand"><KeyRound size={24}/></span><Text weight="bold" size="lg">{x.id==='github'?'GitHub':'Google · YouTube'}</Text><Text tone="muted">OAuth 계정 연결</Text><Badge tone={existing?'positive':x.configured?'brand':'warning'}>{existing?'연결됨 · '+existing.displayName:x.configured?'앱 설정됨':'설정 필요'}</Badge><Button disabled={!x.configured} onClick={()=>void connect(x.id)} variant="quiet">{existing?'다시 연결':'계정 연결'} <ArrowRight size={15}/></Button></Stack></Card>})}</Grid></div>}
  </Card>
 </Stack>
 <Dialog open={Boolean(editor)} onClose={()=>setEditor(null)} title={editor?.entry.name||'채널 연결'} description={editor?.entry.description} wide footer={<><Button onClick={()=>setEditor(null)}>취소</Button><Button variant="primary" disabled={!editor?.record.name||op.working} onClick={save}><Check size={16}/> 저장</Button></>}>
  {editor&&<Stack gap="md"><Input label="연결 이름" value={editor.record.name||''} onChange={e=>setField('name',e.target.value)}/>{editor.entry.fields.map(field=><Input key={field.key} label={field.label+(field.required?' *':'')} type={field.type==='secret'?'password':field.type==='url'?'url':'text'} value={editor.record[field.key]||''} placeholder={field.placeholder||''} help={field.type==='secret'&&Boolean(editor.record.id)?'기존 비밀키는 표시되지 않습니다. 변경할 때만 입력하세요.':undefined} onChange={e=>setField(field.key,e.target.value)}/>)}
  <Select label="응답 방식" value={editor.record.roomId||''} onChange={e=>{setField('roomId',e.target.value)}} items={[{value:'',label:'단일 에이전트'},...rooms.value.map(r=>({value:r.id,label:'회의실 · '+r.name}))]}/>
  {editor.record.roomId&&<Select label="회의실 발언 정책" value={editor.record.roomPolicy||'all_or_named'} onChange={e=>setField('roomPolicy',e.target.value)} items={[{value:'all_or_named',label:'이름 지정 시 해당 인원 · 없으면 전체'},{value:'named_only',label:'호출된 에이전트만'},{value:'round_robin',label:'순서대로 1명씩'}]}/>}
  <Grid cols={2}><Select label="에이전트" value={editor.record.agentId||'general'} onChange={e=>setField('agentId',e.target.value)} items={agents.value.map(a=>({value:a.id,label:a.name}))}/><Select label="모델 제공자" value={editor.record.providerId||''} onChange={e=>setField('providerId',e.target.value)} items={[{value:'',label:'에이전트 기본값'},...providers.value.map(p=>({value:p.id,label:p.name}))]}/></Grid>
  <Input label="모델 ID (선택)" value={editor.record.model||''} onChange={e=>setField('model',e.target.value)}/>
  <Toggle label="연결 활성화" checked={Boolean(editor.record.enabled)} onChange={v=>setField('enabled',v)}/>
  <Toggle label="멘션이 있을 때만 답변" checked={Boolean(editor.record.mentionOnly)} onChange={v=>setField('mentionOnly',v)}/></Stack>}
 </Dialog>
 </Page>
}
