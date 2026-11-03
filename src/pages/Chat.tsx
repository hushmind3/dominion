import {Button,TextInput,TextArea,SelectInput} from '../components/Primitives'
import {FormEvent,useEffect,useRef,useState,type KeyboardEvent} from 'react'
import {ArrowRight,Bot,BrainCircuit,Check,ChevronDown,ChevronRight,CircleHelp,Clock3,Code2,Command,Compass,Copy,CornerDownLeft,History,LoaderCircle,MessageCircle,MoreHorizontal,Plus,Send,ShieldAlert,Sparkles,TerminalSquare,Trash2,X} from 'lucide-react'
import {Agent,ChatMessage,ChatSession,Provider,get,post,streamChat} from '../api'
import {Action,Empty,Notice,StateTag,useRemote} from '../components/UI'
import {SlashMenu,slashCandidates,slashCommands,type SlashChoice} from '../components/SlashMenu'
type Model={id:string;name:string}
export function ChatPage({sessionId,onSession,openModels}:{sessionId?:string;onSession:(x:string|undefined)=>void;openModels:()=>void}){
 const sessions=useRemote<ChatSession[]>('/api/sessions',[]);
 const agents=useRemote<Agent[]>('/api/agents',[]);
 const providers=useRemote<Provider[]>('/api/providers',[]);
 const [current,setCurrent]=useState<string|undefined>(sessionId);
 const [messages,setMessages]=useState<ChatMessage[]>([]);
 const [input,setInput]=useState(''),[providerId,setProviderId]=useState('lmstudio'),[model,setModel]=useState(''),[agentId,setAgentId]=useState('general'),[models,setModels]=useState<Model[]>([]);
 const [modelError,setModelError]=useState(''),[error,setError]=useState(''),[busy,setBusy]=useState(false),[live,setLive]=useState(''),[thought,setThought]=useState(''),[thinking,setThinking]=useState(true),[reasoningKo,setReasoningKo]=useState(true),[showHistory,setShowHistory]=useState(true),[approval,setApproval]=useState<{id:string;tool:string;args:any}|null>(null),[toolStatus,setToolStatus]=useState('');
 const [slashOpen,setSlashOpen]=useState(false),[slashActive,setSlashActive]=useState(0),[commandInfo,setCommandInfo]=useState('');
 const bottom=useRef<HTMLDivElement>(null),inputRef=useRef<HTMLTextAreaElement>(null);
 const slashItems=slashOpen?slashCandidates(input,agents.value,models):[];
 useEffect(()=>{if(sessionId!==undefined)setCurrent(sessionId)},[sessionId]);
 useEffect(()=>{if(current){get<ChatMessage[]>('/api/sessions/'+current+'/messages').then(setMessages).catch(()=>{})}else setMessages([])},[current]);
 useEffect(()=>{let valid=true;setModels([]);setModelError('');if(!providerId)return;get<Model[]>('/api/providers/'+providerId+'/models').then(x=>{if(!valid)return;setModels(x);if(x.length)setModel(old=>x.some(m=>m.id===old)?old:x[0].id);else setModel('')}).catch(e=>{if(valid){setModelError(e.message);setModel('')}});return()=>{valid=false}},[providerId]);
 useEffect(()=>{bottom.current?.scrollIntoView({behavior:'smooth',block:'end'})},[messages,live,thought,toolStatus]);
 const pickSession=(s:ChatSession)=>{setCurrent(s.id);onSession(s.id);if(s.providerId)setProviderId(s.providerId);if(s.model)setModel(s.model);if(s.agentId)setAgentId(s.agentId);setLive('');setThought('');setError('')};
 const resetLocal=()=>{
  setCurrent(undefined);onSession(undefined);setMessages([]);setLive('');setThought('');setError('');
 };
 const executeCommand=async(text:string)=>{
  const found=text.trim().match(/^\/([a-zA-Z]+)(?:\s+([\s\S]*))?$/);
  if(!found){setError('명령어 형식이 올바르지 않습니다. /help로 확인하세요.');return;}
  const name=found[1].toLowerCase(),arg=(found[2]||'').trim();
  if(!slashCommands.some(x=>x.key===name)){setError('알 수 없는 명령어: /'+name+' · /help로 확인하세요.');return;}
  if(['name','goal','model','agent'].includes(name)&&!arg){
   setInput('/'+name+' ');setSlashOpen(name==='model'||name==='agent');inputRef.current?.focus();return;
  }
  setError('');setCommandInfo('');setInput('');setSlashOpen(false);
  try{
   if(name==='new'){resetLocal();setCommandInfo('새 대화를 시작합니다.');return;}
   if(name==='history'){setShowHistory(true);await sessions.reload();setCommandInfo('왼쪽에 저장된 대화 기록을 표시했습니다.');return;}
   if(name==='model'){
    const found=models.find(x=>x.id===arg);if(!found)throw Error('현재 제공자에 등록되지 않은 모델입니다: '+arg);
    setModel(found.id);setCommandInfo('모델 전환: '+found.id);return;
   }
   if(name==='agent'){
    const found=agents.value.find(x=>x.id===arg);if(!found)throw Error('등록되지 않은 에이전트입니다: '+arg);
    setAgentId(found.id);setCommandInfo('에이전트 전환: '+found.name);return;
   }
   if(name==='think'){
    const enabled=arg==='on'?true:arg==='off'?false:!thinking;
    setThinking(enabled);setCommandInfo('추론 표시: '+(enabled?'켜짐':'꺼짐'));return;
   }
   if(name==='help'){setCommandInfo(slashCommands.map(x=>x.command+' · '+x.name).join('\n'));return;}
   if(name==='stop'){
    if(!current)throw Error('중지할 실행 세션이 없습니다.');
    const answer=await post<{cancelled:boolean}>('/api/sessions/'+current+'/cancel',{});
    setCommandInfo(answer.cancelled?'실행 중단 요청을 전달했습니다.':'현재 실행 중인 작업이 없습니다.');return;
   }
   if(name==='status'){
    const state=await get<{agents:number;providers:number;jobs:number;executions:number}>('/api/status');
    setCommandInfo('에이전트 '+state.agents+'개 · 제공자 '+state.providers+'개 · 실행 '+state.executions+'개 · 대기/진행 작업 '+state.jobs+'개');return;
   }
   if(name==='tools'){
    const found=await get<{name:string}[]>('/api/tools');
    setCommandInfo('사용 가능한 도구 '+found.filter(x=>x.name).length+'개\n'+found.filter(x=>x.name).map(x=>x.name).join(' · '));return;
   }
   if(name==='goal'){
    const goal=await post<{id:string}>('/api/goals',{goal:arg,executor:{agentId,providerId,model}});
    setCommandInfo('자율 작업 등록: '+goal.id.slice(0,8)+' · 실행 기록에서 상태를 확인할 수 있습니다.');return;
   }
   if(['mcp','skills','memory','workflows','agents','settings'].includes(name)){
    const route=name==='mcp'||name==='skills'?'extensions':name;
    window.location.hash='#/'+route;return;
   }
   if(!current)throw Error('현재 세션이 없습니다. /new로 대화를 시작하세요.');
   if(name==='name'){
    await post('/api/sessions/'+current+'/rename',{title:arg});await sessions.reload();
    setCommandInfo('대화 이름 변경: '+arg);return;
   }
   if(name==='clear'){
    if(!window.confirm('현재 세션의 대화 기록을 지우시겠습니까?'))return;
    await post('/api/sessions/'+current+'/clear',{});setMessages([]);setLive('');setThought('');
    setCommandInfo('현재 대화 기록을 초기화했습니다.');return;
   }
   if(name==='fork'){
    const copied=await post<ChatSession>('/api/sessions/'+current+'/fork',{});
    setCurrent(copied.id);onSession(copied.id);await sessions.reload();
    setCommandInfo('새 분기 세션을 만들었습니다: '+copied.title);return;
   }
   if(name==='compact'){
    if(!model||!providerId)throw Error('컨텍스트 압축에 사용할 모델을 연결하세요.');
    const result=await post<{summarized:number;remaining:number}>('/api/sessions/'+current+'/compact',{providerId,model});
    setCommandInfo('컨텍스트 압축 완료 · 요약된 메시지 '+result.summarized+'개 · 최근 메시지 '+result.remaining+'개 유지');return;
   }
   if(name==='export'){
    const exported=await get<ChatMessage[]>('/api/sessions/'+current+'/messages');
    const blob=new Blob([JSON.stringify({sessionId:current,messages:exported},null,2)],{type:'application/json'});
    const url=URL.createObjectURL(blob),link=document.createElement('a');link.href=url;link.download='dominion-session-'+current+'.json';
    link.click();URL.revokeObjectURL(url);setCommandInfo('세션 JSON 파일을 내보냈습니다.');return;
   }
  }catch(e){setError((e as Error).message);}
 };
 const chooseSlash=(item:SlashChoice)=>{
  setSlashActive(0);
  if(item.submenu||item.requiresArg){
   setInput(item.command+' ');setSlashOpen(Boolean(item.submenu));inputRef.current?.focus();return;
  }
  setSlashOpen(false);void executeCommand(item.command);
 };
 const send=async(e?:FormEvent)=>{
  e?.preventDefault();const text=input.trim();if(!text)return;
  if(text.startsWith('/')){
   if(busy&&text!=='/stop')return;
   await executeCommand(text);return;
  }
  if(busy)return;
  // Unconfigured sessions are routed automatically to an available provider.
  setBusy(true);setError('');setCommandInfo('');setLive('');setThought('');setInput('');setSlashOpen(false);setToolStatus('');
  try{
   let id=current;
   if(!id){const session=await post<ChatSession>('/api/sessions',{title:'새 대화',agentId,providerId,model});id=session.id;setCurrent(id);onSession(id);sessions.reload();}
   const session=id;
   setMessages(old=>[...old,{id:'pending-'+Date.now(),session_id:session!,role:'user',content:text,created_at:new Date().toISOString()}]);
   await streamChat({sessionId:session,agentId,providerId,model,input:text,toolsEnabled:true,reasoningLanguage:reasoningKo?'ko':undefined},event=>{
    if(event.type==='text')setLive(old=>old+event.text);
    if(event.type==='reasoning')setThought(old=>old+event.text);
    if(event.type==='approval')setApproval({id:event.id,tool:event.tool,args:event.args});
    if(event.type==='tool_start')setToolStatus('실행 중 · '+event.name);
    if(event.type==='tool_end')setToolStatus('완료 · '+event.name);
    if(event.type==='tool_error')setToolStatus('도구 오류 · '+event.error);
    if(event.type==='assistant_saved'){void get<ChatMessage[]>('/api/sessions/'+session+'/messages').then(setMessages);setLive('');setThought('');}
    if(event.type==='error')setError(event.message);
   });
   setMessages(await get<ChatMessage[]>('/api/sessions/'+session+'/messages'));sessions.reload();
  }catch(e){setError((e as Error).message)}finally{setBusy(false);setApproval(null);}
 };
 const approve=async(ok:boolean)=>{if(!approval)return;try{await post('/api/approvals',{id:approval.id,allow:ok});setApproval(null)}catch(e){setError((e as Error).message)}};
 const appendKey=(e:KeyboardEvent<HTMLTextAreaElement>)=>{
  if(e.nativeEvent.isComposing)return;
  if(slashOpen){
   if(e.key==='Escape'){e.preventDefault();setSlashOpen(false);return;}
   if(e.key==='ArrowDown'){e.preventDefault();setSlashActive(x=>slashItems.length?(x+1)%slashItems.length:0);return;}
   if(e.key==='ArrowUp'){e.preventDefault();setSlashActive(x=>slashItems.length?(x+slashItems.length-1)%slashItems.length:0);return;}
   if((e.key==='Enter'||e.key==='Tab')&&slashItems.length){
    e.preventDefault();chooseSlash(slashItems[Math.min(slashActive,slashItems.length-1)]);return;
   }
  }
  if(e.key==='Enter'&&!e.shiftKey){e.preventDefault();void send();}
 };
 const selectedProvider=providers.value.find(x=>x.id===providerId);
 return <div className={'grid h-[calc(100vh-72px)] min-h-[560px] grid-cols-1 bg-white '+(showHistory?'md:grid-cols-[235px_minmax(0,1fr)]':'')}>
  <div className={'flex min-h-0 flex-col overflow-hidden border-r border-slate-200 bg-slate-50 max-md:hidden '+(!showHistory?'hidden':'')}>
   <div className="flex items-center justify-between gap-3 px-5 pb-4 pt-6"><div className="min-w-0"><span className="block text-[10px] font-extrabold uppercase tracking-widest text-blue-600">CONVERSATIONS</span><strong className="mt-1 block text-sm text-slate-800">최근 대화</strong></div><Button size="icon" variant="ghost" onClick={()=>{setCurrent(undefined);onSession(undefined);inputRef.current?.focus()}} title="새 대화"><Plus size={19}/></Button></div>
   <Button className="mx-3 mb-4 inline-flex items-center justify-center gap-2 rounded-xl border border-blue-100 bg-blue-50 px-4 py-2.5 text-xs font-bold text-blue-700 hover:bg-blue-100" onClick={()=>{setCurrent(undefined);onSession(undefined);setMessages([]);inputRef.current?.focus()}}><Plus size={17}/> 새 대화 시작</Button>
   <div className="min-h-0 flex-1 overflow-y-auto px-2">{sessions.value.filter(s=>!s.roomId).map(s=><Button onClick={()=>pickSession(s)} key={s.id} className={'flex w-full items-center gap-3 rounded-xl px-3 py-3 text-left text-xs text-slate-500 hover:bg-blue-50 '+(current===s.id?'bg-blue-50 font-bold text-blue-700':'')}><MessageCircle size={16}/><span>{s.title}</span></Button>)}{!sessions.value.length&&<div className="p-5 text-center text-xs text-slate-400">첫 번째 대화를 시작해보세요.</div>}</div>
   <div className="border-t border-slate-200 p-4 text-[11px] text-slate-500"><span><Clock3 size={14}/> 대화는 로컬 DB에 저장됩니다</span></div>
  </div>
  <div className="flex min-h-0 min-w-0 flex-col">
   <div className="flex h-16 shrink-0 items-center justify-between gap-4 border-b border-slate-200 px-5"><div className="flex min-w-0 items-center gap-3"><Button size="icon" variant="ghost" onClick={()=>setShowHistory(x=>!x)} title="대화 목록"><History size={18}/></Button><div className="grid size-9 place-items-center rounded-xl bg-blue-50 text-blue-600"><BrainCircuit size={17}/></div><div className="flex min-w-0 items-center gap-2 text-xs font-semibold text-slate-600 [&_select]:max-w-[190px] [&_select]:border-0 [&_select]:bg-transparent [&_select]:p-1 [&_select]:text-xs"><SelectInput aria-label="모델 제공자" value={providerId} onChange={e=>{setProviderId(e.target.value);setModel('')}}>{providers.value.filter(p=>p.enabled).map(p=><option value={p.id} key={p.id}>{p.name}</option>)}</SelectInput><span> / </span><SelectInput aria-label="모델 선택" value={model} onChange={e=>setModel(e.target.value)}><option value="">{models.length?'모델 선택':'모델 없음'}</option>{models.map(m=><option key={m.id} value={m.id}>{m.name}</option>)}</SelectInput><ChevronDown size={14}/></div></div><div className="flex items-center gap-3"><span className={"inline-flex items-center gap-2 text-[10px] font-bold uppercase tracking-wider "+(model&&!modelError?"text-emerald-700":"text-slate-400")}><i/>{model&&!modelError?(selectedProvider?.local?"LOCAL ENGINE":"CLOUD / API"):"MODEL NOT READY"}</span><Button size="icon" variant="ghost" title="모델 허브" onClick={openModels}><Compass size={18}/></Button></div></div>
   <div className="min-h-0 flex-1 overflow-y-auto scroll-smooth">{!messages.length&&!busy?<div className="mx-auto my-12 flex max-w-3xl flex-col items-center gap-4 px-5 text-center [&_h1]:m-0 [&_h1]:text-3xl [&_h1]:font-extrabold [&_h1]:tracking-tight [&_p]:m-0 [&_p]:max-w-xl [&_p]:text-sm [&_p]:leading-7 [&_p]:text-slate-500"><div className="relative mb-3 grid size-24 place-items-center rounded-full bg-blue-50"><div className="grid size-16 place-items-center rounded-2xl bg-gradient-to-br from-blue-400 to-blue-700 text-white shadow-lg"><Sparkles size={33} strokeWidth={1.7}/></div></div><span className="text-[10px] font-bold tracking-[.19em] text-blue-500">YOUR INTELLIGENCE, YOUR RULES.</span><h1>생각을 넘어,<br/><em>실행까지.</em></h1><p>질문하고, 연구하고, 만들어보세요.<br/>DOMINION은 모델과 도구를 연결해 실제 작업을 수행합니다.</p><div className="flex w-full max-w-md flex-col gap-2">{['현재 폴더의 파일을 분석해줘','새 프로젝트의 구조를 설계해줘','작업공간에 README.md 파일을 만들어줘'].map(t=><Button key={t} onClick={()=>{setInput(t);inputRef.current?.focus()}}><Sparkles size={14}/>{t}<ArrowRight size={14}/></Button>)}</div></div>:<div className="mx-auto flex min-h-full w-full max-w-4xl flex-col gap-7 px-5 py-7">{messages.filter(m=>m.role!=='tool').map(m=><Message key={m.id} message={m} agentName={agents.value.find(a=>a.id===m.agent_id)?.name} />)}{(live||thought||busy)&&<div className="flex min-w-0 items-start gap-3 assistant"><div className="grid size-9 shrink-0 place-items-center rounded-xl bg-slate-100 text-slate-700 ai"><Sparkles size={17}/></div><div className="relative min-w-0 flex-1"><div className="mb-2 flex items-center gap-2 text-xs font-bold text-slate-700">DOMINION <span>LIVE</span></div>{thought&&<details className="mb-4 rounded-xl border border-slate-200 bg-slate-50 p-4 text-xs leading-6 text-slate-600" open={thinking}><summary onClick={e=>{e.preventDefault();setThinking(x=>!x)}}><BrainCircuit size={16}/> 추론 과정 <ChevronRight size={15}/></summary><div>{thought}</div></details>}{live&&<div className="min-w-0 break-words text-sm leading-7 text-slate-800 [&_p]:my-2 [&_pre]:overflow-x-auto [&_pre]:rounded-xl [&_pre]:bg-slate-950 [&_pre]:p-4 [&_pre]:text-slate-100 [&_code]:break-words [&_ul]:list-disc [&_ul]:pl-6">{live}</div>}{busy&&!live&&<div className="flex items-center gap-2 text-xs text-slate-400"><i/><i/><i/></div>}</div></div>}{toolStatus&&<div className="inline-flex items-center gap-2 rounded-lg bg-slate-50 px-3 py-1.5 text-xs text-slate-500"><TerminalSquare size={15}/>{toolStatus}</div>}<div ref={bottom}/></div>}</div>
   <div className="sticky bottom-0 z-10 border-t border-slate-100 bg-white/95 p-4 backdrop-blur-sm">{approval&&<div className="mb-3 flex flex-wrap items-center justify-between gap-3 rounded-xl border border-amber-200 bg-amber-50 p-3 text-xs text-amber-900"><div><ShieldAlert size={20}/><span><strong>컴퓨터 작업 승인 요청</strong><small>{approval.tool} · {JSON.stringify(approval.args).slice(0,150)}</small></span></div><div><Action variant="secondary" onClick={()=>approve(false)}>거부</Action><Action onClick={()=>approve(true)}><Check size={15}/> 실행 승인</Action></div></div>}
    {error&&<div className="mb-3 rounded-xl border border-rose-200 bg-rose-50 p-3 text-xs text-rose-700"><CircleHelp size={16}/>{error}<Button onClick={()=>setError('')}><X size={14}/></Button></div>}
    {modelError&&!model&&<div className="mb-3 text-xs text-slate-500">모델 서버 미연결: {modelError} <Button onClick={openModels}>연결 관리 <ArrowRight size={13}/></Button></div>}
    {commandInfo&&<div role="status" className="mb-3 max-h-36 overflow-auto whitespace-pre-wrap rounded-xl border border-blue-100 bg-blue-50 p-3 text-xs leading-6 text-slate-700">{commandInfo}</div>}
    <div className="relative">
    {slashOpen&&<SlashMenu items={slashItems} active={slashActive} onChoose={chooseSlash}/>}
    <form onSubmit={send} className="overflow-hidden rounded-2xl border border-slate-200 bg-white p-3 shadow-sm focus-within:border-blue-300 focus-within:ring-2 focus-within:ring-blue-100"><TextArea ref={inputRef} rows={2} placeholder="메시지 입력 또는 / 명령어" value={input} onChange={e=>{const next=e.target.value;setInput(next);setSlashOpen(next.startsWith('/')&&!next.includes('\n'));setSlashActive(0)}} onKeyDown={appendKey} aria-controls="dominion-slash-menu" aria-expanded={slashOpen}/><div className="flex flex-wrap items-center justify-between gap-2"><div className="flex flex-wrap items-center gap-3"><label className="inline-flex items-center gap-2 text-xs text-slate-600"><TextInput type="checkbox" checked={reasoningKo} onChange={e=>setReasoningKo(e.target.checked)}/><BrainCircuit size={15}/> 한국어 추론 요청</label><SelectInput aria-label="에이전트" value={agentId} onChange={e=>setAgentId(e.target.value)}>{agents.value.map(a=><option key={a.id} value={a.id}>{a.name}</option>)}</SelectInput></div><Button className="grid size-10 shrink-0 place-items-center rounded-xl bg-blue-600 text-white hover:bg-blue-700 disabled:opacity-40" disabled={busy||!input.trim()} type="submit" title="보내기">{busy?<LoaderCircle size={19} className="animate-spin"/>:<Send size={19}/>}</Button></div></form></div><div className="mt-2 text-center text-[10px] text-slate-400"><span>모델이 제공한 추론 정보만 표시됩니다</span><span>Enter 전송 · Shift+Enter 줄바꿈</span></div>
   </div>
  </div>
 </div>
}
function Message({message,agentName}:{message:ChatMessage;agentName?:string}){
 const [open,setOpen]=useState(false);const human=message.role==='user';
 return <div className={'flex min-w-0 items-start gap-3 '+(human?'':'')}><div className={'grid size-9 shrink-0 place-items-center rounded-xl '+(human?'bg-slate-200 text-slate-700':'bg-blue-50 text-blue-700')}>{human?'U':<Sparkles size={17}/>}</div><div className="relative min-w-0 flex-1"><div className="mb-2 flex items-center gap-2 text-xs font-bold text-slate-700">{human?'You':agentName||'DOMINION'}</div>{message.reasoning&&<details className="mb-4 rounded-xl border border-slate-200 bg-slate-50 p-4 text-xs leading-6 text-slate-600" open={open}><summary onClick={e=>{e.preventDefault();setOpen(v=>!v)}}><BrainCircuit size={15}/> 추론 과정 펼치기 <ChevronRight size={15}/></summary><div>{message.reasoning}</div></details>}<div className={"min-w-0 break-words text-sm leading-7 text-slate-800 [&_p]:my-2 [&_pre]:overflow-x-auto [&_pre]:rounded-xl [&_pre]:bg-slate-950 [&_pre]:p-4 [&_pre]:text-slate-100 [&_code]:break-words [&_ul]:list-disc [&_ul]:pl-6 "+(human?"rounded-xl bg-slate-100 px-4 py-3":"")}>{message.content||(!human&&message.tool_calls?.length?'도구 실행 요청':'' )}</div>{!human&&message.content&&<Button className="inline-flex items-center gap-1.5 rounded-lg px-2 py-1 text-[11px] text-slate-500 hover:bg-slate-100" onClick={()=>navigator.clipboard.writeText(message.content)} title="복사"><Copy size={14}/></Button>}</div></div>
}
