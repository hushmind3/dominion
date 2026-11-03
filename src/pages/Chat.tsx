import {Button,TextInput,TextArea,SelectInput} from '../components/Primitives'
import {FormEvent,useEffect,useRef,useState,type KeyboardEvent} from 'react'
import {ArrowRight,Bot,BrainCircuit,Check,ChevronDown,ChevronRight,CircleHelp,Clock3,Code2,Command,Compass,Copy,CornerDownLeft,History,LoaderCircle,MessageCircle,MoreHorizontal,Plus,Send,ShieldAlert,Sparkles,TerminalSquare,Trash2,X} from 'lucide-react'
import {Agent,ChatMessage,ChatSession,Provider,get,post,streamChat} from '../api'
import {Action,Empty,Notice,StateTag,useRemote} from '../components/UI'
type Model={id:string;name:string}
export function ChatPage({sessionId,onSession,openModels}:{sessionId?:string;onSession:(x:string|undefined)=>void;openModels:()=>void}){
 const sessions=useRemote<ChatSession[]>('/api/sessions',[]);
 const agents=useRemote<Agent[]>('/api/agents',[]);
 const providers=useRemote<Provider[]>('/api/providers',[]);
 const [current,setCurrent]=useState<string|undefined>(sessionId);
 const [messages,setMessages]=useState<ChatMessage[]>([]);
 const [input,setInput]=useState(''),[providerId,setProviderId]=useState('lmstudio'),[model,setModel]=useState(''),[agentId,setAgentId]=useState('general'),[models,setModels]=useState<Model[]>([]);
 const [modelError,setModelError]=useState(''),[error,setError]=useState(''),[busy,setBusy]=useState(false),[live,setLive]=useState(''),[thought,setThought]=useState(''),[thinking,setThinking]=useState(true),[reasoningKo,setReasoningKo]=useState(true),[showHistory,setShowHistory]=useState(true),[approval,setApproval]=useState<{id:string;tool:string;args:any}|null>(null),[toolStatus,setToolStatus]=useState('');
 const bottom=useRef<HTMLDivElement>(null),inputRef=useRef<HTMLTextAreaElement>(null);
 useEffect(()=>{if(sessionId!==undefined)setCurrent(sessionId)},[sessionId]);
 useEffect(()=>{if(current){get<ChatMessage[]>('/api/sessions/'+current+'/messages').then(setMessages).catch(()=>{})}else setMessages([])},[current]);
 useEffect(()=>{let valid=true;setModels([]);setModelError('');if(!providerId)return;get<Model[]>('/api/providers/'+providerId+'/models').then(x=>{if(!valid)return;setModels(x);if(x.length)setModel(old=>x.some(m=>m.id===old)?old:x[0].id);else setModel('')}).catch(e=>{if(valid){setModelError(e.message);setModel('')}});return()=>{valid=false}},[providerId]);
 useEffect(()=>{bottom.current?.scrollIntoView({behavior:'smooth',block:'end'})},[messages,live,thought,toolStatus]);
 const pickSession=(s:ChatSession)=>{setCurrent(s.id);onSession(s.id);if(s.providerId)setProviderId(s.providerId);if(s.model)setModel(s.model);if(s.agentId)setAgentId(s.agentId);setLive('');setThought('');setError('')};
 const send=async(e?:FormEvent)=>{e?.preventDefault();const text=input.trim();if(!text||busy)return;
  if(!providerId||!model){setError('모델 허브에서 실행 중인 모델이나 API 모델을 먼저 선택해주세요.');return}
  setBusy(true);setError('');setLive('');setThought('');setInput('');setToolStatus('');
  try{
   if(text.startsWith('/goal ')){
    const j=await post('/api/jobs',{kind:'prompt',input:{prompt:text.slice(6),agentId,providerId,model}});
    setToolStatus('백그라운드 작업 등록됨 · '+j.id.slice(0,8));
    sessions.reload();return;
   }
   let id=current;
   if(!id){const s=await post<ChatSession>('/api/sessions',{title:'새 대화',agentId,providerId,model});id=s.id;setCurrent(id);onSession(id);sessions.reload();}
   const session=id;
   setMessages(old=>[...old,{id:'pending-'+Date.now(),session_id:session!,role:'user',content:text,created_at:new Date().toISOString()}]);
   await streamChat({sessionId:session,agentId,providerId,model,input:text,toolsEnabled:true,reasoningLanguage:reasoningKo?'ko':undefined},x=>{
    if(x.type==='text')setLive(old=>old+x.text);
    if(x.type==='reasoning')setThought(old=>old+x.text);
    if(x.type==='approval')setApproval({id:x.id,tool:x.tool,args:x.args});
    if(x.type==='tool_start')setToolStatus('실행 중 · '+x.name);
    if(x.type==='tool_end')setToolStatus('완료 · '+x.name);
    if(x.type==='tool_error')setToolStatus('도구 오류 · '+x.error);
    if(x.type==='assistant_saved'){void get<ChatMessage[]>('/api/sessions/'+session+'/messages').then(setMessages);setLive('');setThought('');}
    if(x.type==='error')setError(x.message);
   });
   setMessages(await get<ChatMessage[]>('/api/sessions/'+session+'/messages'));sessions.reload();
  }catch(e){setError((e as Error).message)}finally{setBusy(false);setApproval(null);}
 };
 const approve=async(ok:boolean)=>{if(!approval)return;try{await post('/api/approvals',{id:approval.id,allow:ok});setApproval(null)}catch(e){setError((e as Error).message)}};
 const appendKey=(e:KeyboardEvent<HTMLTextAreaElement>)=>{if(e.key==='Enter'&&!e.shiftKey){e.preventDefault();void send()}};
 const selectedProvider=providers.value.find(x=>x.id===providerId);
 return <div className="chat-layout">
  <div className={'conversation-list '+(!showHistory?'collapsed':'')}>
   <div className="conversation-header"><div><span className="mini-eyebrow">CONVERSATIONS</span><strong>최근 대화</strong></div><Button className="icon-button small" onClick={()=>{setCurrent(undefined);onSession(undefined);inputRef.current?.focus()}} title="새 대화"><Plus size={19}/></Button></div>
   <Button className="new-chat" onClick={()=>{setCurrent(undefined);onSession(undefined);setMessages([]);inputRef.current?.focus()}}><Plus size={17}/> 새 대화 시작</Button>
   <div className="conversation-scroll">{sessions.value.filter(s=>!s.roomId).map(s=><Button onClick={()=>pickSession(s)} key={s.id} className={'conversation-item '+(current===s.id?'selected':'')}><MessageCircle size={16}/><span>{s.title}</span></Button>)}{!sessions.value.length&&<div className="conversation-empty">첫 번째 대화를 시작해보세요.</div>}</div>
   <div className="conversation-bottom"><span><Clock3 size={14}/> 대화는 로컬 DB에 저장됩니다</span></div>
  </div>
  <div className="chat-main">
   <div className="chat-toolbar"><div className="chat-toolbar-left"><Button className="icon-button small history-toggle" onClick={()=>setShowHistory(x=>!x)} title="대화 목록"><History size={18}/></Button><div className="active-model-icon"><BrainCircuit size={17}/></div><div className="model-select"><SelectInput aria-label="모델 제공자" value={providerId} onChange={e=>{setProviderId(e.target.value);setModel('')}}>{providers.value.filter(p=>p.enabled).map(p=><option value={p.id} key={p.id}>{p.name}</option>)}</SelectInput><span> / </span><SelectInput aria-label="모델 선택" value={model} onChange={e=>setModel(e.target.value)}><option value="">{models.length?'모델 선택':'모델 없음'}</option>{models.map(m=><option key={m.id} value={m.id}>{m.name}</option>)}</SelectInput><ChevronDown size={14}/></div></div><div className="chat-toolbar-right"><span className="model-live"><i/>{selectedProvider?.local?'LOCAL ENGINE':'CLOUD / API'}</span><Button className="icon-button small" title="모델 허브" onClick={openModels}><Compass size={18}/></Button></div></div>
   <div className="chat-scroll">{!messages.length&&!busy?<div className="chat-welcome"><div className="welcome-orbit"><div className="welcome-core"><Sparkles size={33} strokeWidth={1.7}/></div></div><span className="welcome-overline">YOUR INTELLIGENCE, YOUR RULES.</span><h1>생각을 넘어,<br/><em>실행까지.</em></h1><p>질문하고, 연구하고, 만들어보세요.<br/>DOMINION은 모델과 도구를 연결해 실제 작업을 수행합니다.</p><div className="prompt-suggestions">{['현재 폴더의 파일을 분석해줘','새 프로젝트의 구조를 설계해줘','/goal 오늘 할 일을 정리하고 실행해줘'].map(t=><Button key={t} onClick={()=>{setInput(t);inputRef.current?.focus()}}><Sparkles size={14}/>{t}<ArrowRight size={14}/></Button>)}</div></div>:<div className="chat-messages">{messages.filter(m=>m.role!=='tool').map(m=><Message key={m.id} message={m} agentName={agents.value.find(a=>a.id===m.agent_id)?.name} />)}{(live||thought||busy)&&<div className="message-row assistant"><div className="message-icon ai"><Sparkles size={17}/></div><div className="message-body"><div className="message-author">DOMINION <span>LIVE</span></div>{thought&&<details className="reason-box" open={thinking}><summary onClick={e=>{e.preventDefault();setThinking(x=>!x)}}><BrainCircuit size={16}/> 추론 과정 <ChevronRight size={15}/></summary><div>{thought}</div></details>}{live&&<div className="message-content">{live}</div>}{busy&&!live&&<div className="typing"><i/><i/><i/></div>}</div></div>}{toolStatus&&<div className="execution-caption"><TerminalSquare size={15}/>{toolStatus}</div>}<div ref={bottom}/></div>}</div>
   <div className="composer-region">{approval&&<div className="approval-banner"><div><ShieldAlert size={20}/><span><strong>컴퓨터 작업 승인 요청</strong><small>{approval.tool} · {JSON.stringify(approval.args).slice(0,150)}</small></span></div><div><Action variant="secondary" onClick={()=>approve(false)}>거부</Action><Action onClick={()=>approve(true)}><Check size={15}/> 실행 승인</Action></div></div>}
    {error&&<div className="chat-error"><CircleHelp size={16}/>{error}<Button onClick={()=>setError('')}><X size={14}/></Button></div>}
    {modelError&&!model&&<div className="model-hint">모델 서버 미연결: {modelError} <Button onClick={openModels}>연결 관리 <ArrowRight size={13}/></Button></div>}
    <form onSubmit={send} className="composer"><TextArea ref={inputRef} rows={2} placeholder="메시지를 입력하거나 /goal로 작업을 시작하세요..." value={input} onChange={e=>setInput(e.target.value)} onKeyDown={appendKey}/><div className="composer-bottom"><div className="composer-options"><label className="toggle-line"><TextInput type="checkbox" checked={reasoningKo} onChange={e=>setReasoningKo(e.target.checked)}/><BrainCircuit size={15}/> 한국어 추론 요청</label><SelectInput aria-label="에이전트" value={agentId} onChange={e=>setAgentId(e.target.value)}>{agents.value.map(a=><option key={a.id} value={a.id}>{a.name}</option>)}</SelectInput></div><Button className="send-button" disabled={busy||!input.trim()} type="submit" title="보내기">{busy?<LoaderCircle size={19} className="spin"/>:<Send size={19}/>}</Button></div></form><div className="composer-caption"><span>모델이 제공한 추론 정보만 표시됩니다</span><span>Enter 전송 · Shift+Enter 줄바꿈</span></div>
   </div>
  </div>
 </div>
}
function Message({message,agentName}:{message:ChatMessage;agentName?:string}){
 const [open,setOpen]=useState(false);const human=message.role==='user';
 return <div className={'message-row '+(human?'human':'assistant')}><div className={'message-icon '+(human?'user':'ai')}>{human?'U':<Sparkles size={17}/>}</div><div className="message-body"><div className="message-author">{human?'You':agentName||'DOMINION'}</div>{message.reasoning&&<details className="reason-box" open={open}><summary onClick={e=>{e.preventDefault();setOpen(v=>!v)}}><BrainCircuit size={15}/> 추론 과정 펼치기 <ChevronRight size={15}/></summary><div>{message.reasoning}</div></details>}<div className="message-content">{message.content||(!human&&message.tool_calls?.length?'도구 실행 요청':'' )}</div>{!human&&message.content&&<Button className="copy-message" onClick={()=>navigator.clipboard.writeText(message.content)} title="복사"><Copy size={14}/></Button>}</div></div>
}
