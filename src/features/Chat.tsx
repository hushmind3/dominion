import {useEffect,useRef,useState,type KeyboardEvent} from 'react'
import {ArrowRight,BrainCircuit,Check,Compass,History,LoaderCircle,Plus,Send,ShieldAlert,X} from 'lucide-react'
import {get,post,streamChat,type Agent,type ChatMessage,type ChatSession,type Provider} from '../api'
import {Alert,Badge,Button,Dialog,IconButton,Row,Select,Stack,Text,Textarea,Toggle} from '../design/Primitives'
import {useResource} from '../design/useResource'
import {ChatBubble,ChatFrame,ChatHistory,ChatHistoryList,ChatMain,ChatThinking,ChatWelcome,SessionEntry,ToolState,Transcript} from './ChatParts'
type Model={id:string;name:string}
export function ChatPage({sessionId,onSession,openModels}:{sessionId?:string;onSession:(id:string|undefined)=>void;openModels:()=>void}){
 const sessions=useResource<ChatSession[]>('/api/sessions',[])
 const agents=useResource<Agent[]>('/api/agents',[])
 const providers=useResource<Provider[]>('/api/providers',[])
 const [current,setCurrent]=useState<string|undefined>(sessionId)
 const [messages,setMessages]=useState<ChatMessage[]>([])
 const [text,setText]=useState('')
 const [agent,setAgent]=useState('general')
 const [provider,setProvider]=useState('')
 const [model,setModel]=useState('')
 const [models,setModels]=useState<Model[]>([])
 const [modelError,setModelError]=useState('')
 const [error,setError]=useState('')
 const [busy,setBusy]=useState(false)
 const [live,setLive]=useState('')
 const [reasoning,setReasoning]=useState('')
 const [reasoningKo,setReasoningKo]=useState(true)
 const [historyOpen,setHistoryOpen]=useState(false)
 const [status,setStatus]=useState('')
 const [approval,setApproval]=useState<{id:string;tool:string;args:unknown}|null>(null)
 const composer=useRef<HTMLTextAreaElement>(null)
 useEffect(()=>{setCurrent(sessionId)},[sessionId])
 useEffect(()=>{let active=true;if(!current){setMessages([]);return}
  get<ChatMessage[]>('/api/sessions/'+current+'/messages').then(x=>{if(active)setMessages(x)}).catch(e=>{if(active)setError(e.message)})
  return()=>{active=false}
 },[current])
 useEffect(()=>{
  const list=providers.value.filter(p=>p.enabled)
  if(!list.length){setProvider('');return}
  setProvider(p=>list.some(x=>x.id===p)?p:(list.find(x=>x.id==='dominion-native')||list[0]).id)
 },[providers.value])
 useEffect(()=>{
  let active=true;setModels([]);setModel('');setModelError('')
  if(!provider)return
  get<Model[]>('/api/providers/'+provider+'/models').then(list=>{if(!active)return;setModels(list);setModel(prev=>list.some(x=>x.id===prev)?prev:list[0]?.id||'')}).catch(e=>{if(active)setModelError(e.message)})
  return()=>{active=false}
 },[provider])
 const newSession=()=>{setCurrent(undefined);onSession(undefined);setMessages([]);setLive('');setReasoning('');setError('');setHistoryOpen(false);composer.current?.focus()}
 const selectSession=(s:ChatSession)=>{setCurrent(s.id);onSession(s.id);if(s.providerId)setProvider(s.providerId);if(s.model)setModel(s.model);if(s.agentId)setAgent(s.agentId);setHistoryOpen(false);setLive('');setReasoning('');setStatus('');setError('')}
 const send=async()=>{
  const input=text.trim()
  if(!input||busy)return
  if(!provider||!model){setError('모델 라이브러리에서 실행 모델을 선택하거나 외부 API를 연결하세요.');return}
  setBusy(true);setError('');setLive('');setReasoning('');setText('');setStatus('')
  try{
   if(input.startsWith('/goal ')){
    const job=await post<{id:string}>('/api/jobs',{kind:'prompt',input:{prompt:input.slice(6),agentId:agent,providerId:provider,model}})
    setStatus('작업 등록 · '+job.id.slice(0,8));return
   }
   let id=current
   if(!id){const s=await post<ChatSession>('/api/sessions',{title:'새 대화',agentId:agent,providerId:provider,model});id=s.id;setCurrent(id);onSession(id);void sessions.reload()}
   const session=id
   setMessages(old=>[...old,{id:'pending-'+Date.now(),session_id:session!,role:'user',content:input,created_at:new Date().toISOString()}])
   await streamChat({sessionId:session,agentId:agent,providerId:provider,model,input,toolsEnabled:true,reasoningLanguage:reasoningKo?'ko':undefined},ev=>{
    if(ev.type==='text')setLive(x=>x+ev.text)
    if(ev.type==='reasoning')setReasoning(x=>x+ev.text)
    if(ev.type==='approval')setApproval({id:ev.id,tool:ev.tool,args:ev.args})
    if(ev.type==='tool_start')setStatus('실행 중 · '+ev.name)
    if(ev.type==='tool_end')setStatus('완료 · '+ev.name)
    if(ev.type==='tool_error')setStatus('도구 오류 · '+ev.error)
    if(ev.type==='error')setError(ev.message)
   })
   setMessages(await get<ChatMessage[]>('/api/sessions/'+session+'/messages'))
   void sessions.reload();setLive('');setReasoning('')
  }catch(e){setError((e as Error).message)}
  finally{setBusy(false);setApproval(null)}
 }
 const approve=async(allow:boolean)=>{if(!approval)return;try{await post('/api/approvals',{id:approval.id,allow});setApproval(null)}catch(e){setError((e as Error).message)}}
 const onKey=(e:KeyboardEvent<HTMLTextAreaElement>)=>{if(e.key==='Enter'&&!e.shiftKey&&!e.nativeEvent.isComposing){e.preventDefault();void send()}}
 const list=sessions.value.filter(s=>!s.roomId)
 const history=<><div className="flex items-center justify-between gap-2 p-4"><div><Text size="xs" tone="subtle" weight="bold">CONVERSATIONS</Text><p className="mt-1 text-sm font-bold text-ink">최근 대화</p></div><IconButton label="새 대화" icon={<Plus size={18}/>} onClick={newSession}/></div><div className="px-3 pb-3"><Button variant="quiet" onClick={newSession} className="!w-full"><Plus size={16}/> 새 대화</Button></div><ChatHistoryList>{list.map(s=><SessionEntry key={s.id} title={s.title||'새 대화'} selected={s.id===current} onClick={()=>selectSession(s)}/>)}</ChatHistoryList><div className="border-t border-line p-4"><Text size="xs" tone="subtle">대화는 로컬 데이터베이스에 저장됩니다.</Text></div></>
 return <><ChatFrame history={<ChatHistory>{history}</ChatHistory>} main={<ChatMain toolbar={<><Row gap="sm" wrap><IconButton icon={<History size={19}/>} label="대화 목록" onClick={()=>setHistoryOpen(true)} size="sm"/><BrainCircuit size={19} className="text-brand"/><Select label="제공자" value={provider} onChange={e=>setProvider(e.target.value)} items={providers.value.filter(x=>x.enabled).map(p=>({value:p.id,label:p.name}))} placeholder="제공자 선택"/><Select label="모델" value={model} onChange={e=>setModel(e.target.value)} items={models.map(m=>({value:m.id,label:m.name||m.id}))} placeholder={models.length?'모델 선택':'모델 없음'}/></Row><Row gap="xs"><Badge tone={providers.value.find(x=>x.id===provider)?.local?'positive':'neutral'}>{providers.value.find(x=>x.id===provider)?.local?'LOCAL':'API'}</Badge><IconButton icon={<Compass size={18}/>} label="모델 라이브러리" onClick={openModels}/></Row></>} transcript={<Transcript ready={Boolean(messages.length||busy||live||reasoning)}>{!messages.length&&!busy?<ChatWelcome onPrompt={s=>{setText(s);composer.current?.focus()}}/>:messages.filter(m=>m.role!=='tool').map(m=><ChatBubble key={m.id} role={m.role} author={m.role==='user'?'You':agents.value.find(a=>a.id===m.agent_id)?.name} content={m.content} reasoning={m.reasoning} onCopy={m.role==='assistant'?()=>void navigator.clipboard.writeText(m.content):undefined}/>)}{busy&&<ChatThinking text={live} reasoning={reasoning}/>}<ToolState text={status}/></Transcript>} composer={<Stack gap="sm">{approval&&<Alert tone="warning" message={'도구 실행 승인 요청: '+approval.tool+' · '+JSON.stringify(approval.args).slice(0,120)}/>}
 {approval&&<Row justify="end"><Button onClick={()=>void approve(false)}>거부</Button><Button variant="primary" onClick={()=>void approve(true)}><Check size={15}/> 승인</Button></Row>}
 {error&&<Alert message={error} onClose={()=>setError('')}/>}
 {modelError&&!model&&<Row justify="between" wrap><Text tone="negative" size="xs">모델 연결 실패: {modelError}</Text><Button size="sm" onClick={openModels}>연결 관리 <ArrowRight size={14}/></Button></Row>}
 <Textarea ref={composer} label="메시지 입력" rows={2} value={text} onChange={e=>setText(e.target.value)} onKeyDown={onKey} placeholder="메시지를 입력하거나 /goal 명령을 사용하세요…"/>
 <Row justify="between" wrap gap="sm"><Row gap="sm" wrap><Toggle checked={reasoningKo} onChange={setReasoningKo} label="한국어 추론 요청"/><Select label="에이전트" value={agent} onChange={e=>setAgent(e.target.value)} items={agents.value.map(a=>({value:a.id,label:a.name}))}/></Row><Button variant="primary" disabled={!text.trim()||busy} loading={busy} onClick={()=>void send()}><Send size={17}/> 전송</Button></Row>
 <Row justify="between" wrap><Text size="xs" tone="subtle">모델에서 실제로 반환한 추론만 표시합니다.</Text><Text size="xs" tone="subtle">Enter 전송 · Shift+Enter 줄바꿈</Text></Row>
 </Stack>}/>}/>
 <Dialog open={historyOpen} onClose={()=>setHistoryOpen(false)} title="대화 목록"><Stack gap="sm"><Button variant="primary" onClick={newSession}><Plus size={16}/> 새 대화</Button>{list.map(s=><SessionEntry key={s.id} title={s.title} selected={s.id===current} onClick={()=>selectSession(s)}/>)}</Stack></Dialog>
 </>
}
