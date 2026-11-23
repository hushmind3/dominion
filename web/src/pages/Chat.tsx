import {ConversationSettings} from '../components/ConversationSettings'
import {ExecutionPermissions} from '../components/ExecutionPermissions'
import {TeamRunStrip} from '../components/TeamActivity'
import {ConversationSwitch} from '../components/WorkspaceShell'
import {TokenUsage} from '../components/TokenUsage'
import {MarkdownMessage} from '../components/MarkdownMessage'
import {AttachmentLink} from '../components/AttachmentLink'
import {createPortal} from 'react-dom'
import {Button,TextInput,TextArea,SelectInput} from '../components/Primitives'
import {FormEvent,useEffect,useMemo,useRef,useState,type ClipboardEvent,type DragEvent,type KeyboardEvent} from 'react'
import {ArrowRight,ArrowUp,BrainCircuit,Check,ChevronDown,ChevronRight,CircleHelp,Clock3,Code2,Command,Compass,Copy,Cpu,CornerDownLeft,HardDrive,Menu,Settings,List,LoaderCircle,MessageCircle,MoreHorizontal,Globe2,Paperclip,Plus,Power,RefreshCw,Search,ShieldAlert,ShieldCheck,Sparkles,Square,TerminalSquare,Trash2,X} from 'lucide-react'
import {Agent,Approval,ChatMessage,ChatSession,Provider,get,post,del,resolveApproval,streamChat,koreanReasoningEnabled} from '../api'
import {Action,Empty,Notice,StateTag,refresh,useRemote} from '../components/UI'
import {SlashMenu,slashCandidates,slashCommandLabel,slashCommands,type SlashChoice} from '../components/SlashMenu'
import {ConversationTargetPicker} from '../components/chat/ConversationTargetPicker'
import {ConversationRail} from '../components/chat/ConversationRail'
import {ReasoningSelect,reasoningOptions} from '../components/ReasoningSelect'
import {useProviderModels} from '../hooks/useProviderModels'
import {useTranscriptScroll} from '../hooks/useTranscriptScroll'
import {useAnchoredPanel} from '../hooks/useAnchoredPanel'
import {useJob,jobFinished,type JobState} from '../hooks/useJob'
import {t,useLocale,type LocaleKey} from '../i18n'
import {translateError} from '../i18n/errors'

type SessionUsageBreakdown={byPurpose?:{purpose:string;calls:number;promptTokens:number;completionTokens:number;totalTokens:number;observedCachedTokens:number;cacheObservedCalls:number;estimatedCalls:number;tokensSinceClear:number}[];recentPromptProfiles?:{profile?:{totalEstimatedTokens?:number;components?:Record<string,number>}}[];latestSourceCharacterCounts?:{fixedSystemPrompt?:number;ownerInstructions?:number;agentPersona?:number;systemOverride?:number;retrievedMemoryAndExperience?:number;selectedHistory?:number};hasClearMarker?:boolean}
type LearningResult={memories:number;skill:boolean}
type LearningJob=JobState & {result?:LearningResult}
type ModelOverride={scope?:string;providerId:string;model:string}
type InstalledModel={id:string;repo:string;label:string;format:string;bytes:number;source?:string}
type Runtime={status:string;id?:string;name?:string;model?:string;error?:string}
type PendingFile=File
type UploadedFile={id:string;sessionId:string}
const maxFileBytes=8*1024*1024,maxTotalBytes=24*1024*1024,maxFileCount=8
async function fileToBase64(file:File):Promise<string>{
 const data=new Uint8Array(await file.arrayBuffer())
 let binary=''
 for(let at=0;at<data.length;at+=32768)binary+=String.fromCharCode(...data.subarray(at,at+32768))
 return btoa(binary)
}
function supportedChatFile(file:File):boolean{
 return /^(?:image\/(?:png|jpeg|gif|webp)|text\/|application\/(?:json|xml))/.test(file.type)||
  /\.(?:txt|md|csv|ts|tsx|js|mjs|json|py|rs|yaml|yml|xml|html|css|log|sql)$/i.test(file.name)
}
const chatDraftPrefix='dominion.chat.draft.v1:'
type ChatExecutionMode='chat'|'agent'
const chatModePrefix='dominion.chat.execution-mode.v1:'
const chatWebPrefix='dominion.chat.web-search.v1:'
function readChatWeb(scope:string):boolean{
 try{return sessionStorage.getItem(chatWebPrefix+scope)==='1'}
 catch{return false}
}
function storeChatWeb(scope:string,enabled:boolean){
 try{sessionStorage.setItem(chatWebPrefix+scope,enabled?'1':'0')}
 catch{/* Storage may be blocked by the browser. */}
}
function readChatMode(scope:string,fallback:ChatExecutionMode='chat'):ChatExecutionMode{
 try{
  const saved=sessionStorage.getItem(chatModePrefix+scope)
  return saved==='agent'||saved==='chat'?saved:fallback
 }catch{return fallback}
}
function storeChatMode(scope:string,mode:ChatExecutionMode){
 try{sessionStorage.setItem(chatModePrefix+scope,mode)}
 catch{/* Session storage can be unavailable in private mode. */}
}
function readChatDraft(scope:string):string{
 try{
  const text=sessionStorage.getItem(chatDraftPrefix+scope)
  return text&&text.length<=64000?text:''
 }catch{return ''}
}
function storeChatDraft(scope:string,text:string){
 try{
  if(text)sessionStorage.setItem(chatDraftPrefix+scope,text)
  else sessionStorage.removeItem(chatDraftPrefix+scope)
 }catch{/* Private mode or quota may reject browser storage. */}
}
const formatDraftBytes=(bytes:number)=>bytes>=1048576
 ?(bytes/1048576).toFixed(1)+' MiB'
 :bytes>=1024?(bytes/1024).toFixed(1)+' KiB':bytes+' B'
const formatBytes=(bytes:number)=>{
 const gb=Number(bytes||0)/1024/1024/1024
 return gb>=1?gb.toFixed(1)+' GB':Math.max(0,Math.round(Number(bytes||0)/1024/1024))+' MB'
}
export function ChatPage({sessionId,initialAgent,initialProvider,initialModel,onSession,openModels,openSettings,switchMode,active=true}:{sessionId?:string;initialAgent?:string;initialProvider?:string;initialModel?:string;onSession:(x:string|undefined)=>void;openModels:()=>void;openSettings:()=>void;switchMode:(mode:'chat'|'rooms')=>void;active?:boolean}){
 const loc=useLocale()
 const sessions=useRemote<ChatSession[]>('/api/sessions',[],active);
 const agents=useRemote<Agent[]>('/api/agents',[],active);
 const providers=useRemote<Provider[]>('/api/providers',[],active);
 const current=sessionId;
 const selectedSession=sessions.value.find(session=>session.id===current);
 const transcript=useRemote<ChatMessage[]>(current?'/api/sessions/'+encodeURIComponent(current)+'/messages':null,[],active);
 const messages=transcript.value
 const messageUsage=useMemo(()=>messages.reduce((sum,message)=>sum+(Number(message.metadata?.usage?.totalTokens)||0),0),[messages])
 const [serverUsage,setServerUsage]=useState(0);
 const [usageBreakdown,setUsageBreakdown]=useState<SessionUsageBreakdown>();
 const [usageUnavailable,setUsageUnavailable]=useState(false)
 const approvals=useRemote<Approval[]>('/api/approvals',[],active);
 // Composer state belongs to its exact conversation, not the current tab's
 // last displayed textarea. Switching Chat sessions cannot transplant an
 // unfinished owner instruction into another run.
 const draftScope=current||'new'
 const draftValues=useRef<Record<string,string>>({})
 const [,redrawDraft]=useState(0)
 const input=Object.hasOwn(draftValues.current,draftScope)
  ?draftValues.current[draftScope]:readChatDraft(draftScope)
 const setInput=(update:string|((previous:string)=>string))=>{
  const before=Object.hasOwn(draftValues.current,draftScope)
   ?draftValues.current[draftScope]:readChatDraft(draftScope)
  const value=typeof update==='function'?update(before):update
  draftValues.current[draftScope]=value
  storeChatDraft(draftScope,value)
  redrawDraft(revision=>revision+1)
 };
 const modeValues=useRef<Record<string,ChatExecutionMode>>({})
 const [,redrawMode]=useState(0)
 const executionMode=Object.hasOwn(modeValues.current,draftScope)?
  modeValues.current[draftScope]:readChatMode(draftScope,
   selectedSession?.conversationMode==='agent'||(!current&&initialAgent)?'agent':'chat')
 const setExecutionMode=(mode:ChatExecutionMode)=>{
  modeValues.current[draftScope]=mode
  storeChatMode(draftScope,mode)
  redrawMode(revision=>revision+1)
 }
 const webChoice=useRef<Record<string,boolean>>({})
 const [,redrawWeb]=useState(0)
 const webSearchEnabled=Object.hasOwn(webChoice.current,draftScope)
  ?webChoice.current[draftScope]:readChatWeb(draftScope)
 const setWebSearchEnabled=(enabled:boolean)=>{
  webChoice.current[draftScope]=enabled
  storeChatWeb(draftScope,enabled)
  redrawWeb(revision=>revision+1)
 }
 // Keep an unsent owner request recoverable when a provider/network turn
 // fails, without replacing anything the owner typed during that attempt.
 const restoreFailedDraft=(scope:string,text:string)=>{
  const existing=Object.hasOwn(draftValues.current,scope)
   ?draftValues.current[scope]:readChatDraft(scope)
  if(existing.trim())return
  draftValues.current[scope]=text
  storeChatDraft(scope,text)
  redrawDraft(revision=>revision+1)
 }
 const fileDrafts=useRef<Record<string,PendingFile[]>>({})
 const uploadedFiles=useRef<WeakMap<File,UploadedFile>>(new WeakMap())
 const [,redrawFiles]=useState(0)
 const draftFiles=fileDrafts.current[draftScope]||[]
 const addFiles=(selection:FileList|null)=>{
  if(!selection)return
  const next=[...draftFiles]
  for(const file of Array.from(selection)){
   if(!supportedChatFile(file)){setError('이미지(PNG/JPG/WEBP/GIF) 또는 텍스트 파일만 첨부할 수 있습니다.');continue}
   if(file.size>maxFileBytes){setError('파일 한 개는 최대 8MiB입니다.');continue}
   if(next.some(item=>item.name===file.name&&item.size===file.size&&item.lastModified===file.lastModified))continue
   if(next.length>=maxFileCount||next.reduce((sum,item)=>sum+item.size,0)+file.size>maxTotalBytes){
    setError('첨부는 최대 8개, 합계 24MiB까지 가능합니다.');break
   }
   next.push(file)
  }
  fileDrafts.current[draftScope]=next
  redrawFiles(value=>value+1)
 }
 const removeFile=(at:number)=>{
  fileDrafts.current[draftScope]=draftFiles.filter((_,i)=>i!==at)
  redrawFiles(value=>value+1)
 }
 const [agentChoice,setAgentChoice]=useState<{scope?:string;id:string}>();
 const [override,setOverride]=useState<ModelOverride>();
 const defaultAgent=agents.value.find(agent=>agent.id==='default')||agents.value.find(agent=>agent.name==='기본값')||agents.value.find(agent=>agent.id==='general');
 const agentId=(agentChoice?.scope===current?agentChoice?.id:undefined)??selectedSession?.agentId??(!current?initialAgent:undefined)??defaultAgent?.id??'';
 const activeAgent=agents.value.find(agent=>agent.id===agentId);
 const manual=override&&override.scope===current?override:
  !current&&initialProvider?{providerId:initialProvider,model:initialModel??''}:undefined;
 const enabledProviders=providers.value.filter(provider=>provider.enabled!==false);
 const sessionProvider=enabledProviders.find(provider=>provider.id===selectedSession?.providerId);
 const providerId=manual?.providerId??(executionMode==='agent'
  ?activeAgent?.providerId||sessionProvider?.id||enabledProviders[0]?.id||''
  :sessionProvider?.id||enabledProviders[0]?.id||'');
 const selectedProvider=enabledProviders.find(provider=>provider.id===providerId);
 const model=manual?.model||((executionMode==='agent'&&activeAgent?.providerId===providerId?activeAgent.model:'')||
  (selectedSession?.providerId===providerId&&(executionMode==='chat'||selectedSession.agentId===agentId)?selectedSession.model:'')||
  selectedProvider?.model||'');
 const catalogue=useProviderModels(providerId);
 const {value:models,error:modelError}=catalogue;
 const selectedModel=models.find(item=>item.id===model);
 const canDisableThinking=selectedProvider?.kind!=='codex'||reasoningOptions(selectedModel,true).includes('none');
 const setRoute=(providerId:string,model:string)=>setOverride({scope:current,providerId,model});
 useEffect(()=>{if(!current){setOverride(undefined);setAgentChoice(undefined)}},[initialAgent,initialProvider,initialModel]);
 const [isDropping,setIsDropping]=useState(false)
 const handleFileDrop=(event:DragEvent<HTMLFormElement>)=>{
  setIsDropping(false)
  if(!event.dataTransfer.files.length)return
  // Only file drops are intercepted. Dropped text still reaches textarea.
  event.preventDefault()
  addFiles(event.dataTransfer.files)
 }
 const handleClipboardFiles=(event:ClipboardEvent<HTMLTextAreaElement>)=>{
  if(event.clipboardData.files.length===0)return
  event.preventDefault()
  addFiles(event.clipboardData.files)
 }
 const [sessionFilter,setSessionFilter]=useState('')
 const [deletingSessionId,setDeletingSessionId]=useState('')
 const filteredSessions=sessions.value.filter(session=>!session.roomId
  && session.title.toLocaleLowerCase('ko').includes(sessionFilter.trim().toLocaleLowerCase('ko')))
 const [turnSession,setTurnSession]=useState<string>();
 const [savedMessageId,setSavedMessageId]=useState<string>();
 const [approvalBusy,setApprovalBusy]=useState(false);
 const [liveApproval,setLiveApproval]=useState<Approval|null>(null);
 const [learningJob,setLearningJob]=useState<{id:string;scope:string}>();
 const job=useJob<LearningJob>(learningJob?'/api/jobs/'+encodeURIComponent(learningJob.id):null);
 const approval=(liveApproval?.sessionId===current?liveApproval:null)||
  approvals.value.find(item=>item.sessionId===current&&item.status==='pending');
 const [error,setError]=useState(''),[busy,setBusy]=useState(false),[live,setLive]=useState(''),[thought,setThought]=useState(''),[thinking,setThinking]=useState(false),[reasoningKo,setReasoningKo]=useState(koreanReasoningEnabled),[showReasoning,setShowReasoning]=useState(true),[showSettings,setShowSettings]=useState(false),[toolsEnabled,setToolsEnabled]=useState(true),[thinkingEnabled,setThinkingEnabled]=useState(true),[reasoningEffort,setReasoningEffort]=useState(''),[autoLearn,setAutoLearn]=useState(false),[showHistory,setShowHistory]=useState(()=>{try{if(window.innerWidth<768)return false;return localStorage.getItem('dominion.chat.sidebar')!=='0'}catch{return true}}),[toolStatus,setToolStatus]=useState('');
 useEffect(()=>{setReasoningEffort('');setThinkingEnabled(true)},[providerId,model,agentId,current]);
 const [permissionOpen,setPermissionOpen]=useState(false);
 const [modelMenu,setModelMenu]=useState(false),[installed,setInstalled]=useState<InstalledModel[]>([]),[modelsLoading,setModelsLoading]=useState(false),[runtime,setRuntime]=useState<Runtime>({status:'idle'}),[modelBusy,setModelBusy]=useState(''),[unloading,setUnloading]=useState(false),[learning,setLearning]=useState(false),[menuError,setMenuError]=useState('');
 const [slashOpen,setSlashOpen]=useState(false),[slashActive,setSlashActive]=useState(0),[commandInfo,setCommandInfo]=useState('');
 const fileInputRef=useRef<HTMLInputElement>(null);
 const inputRef=useRef<HTMLTextAreaElement>(null);
 const stream=useRef<AbortController|null>(null);
 const activeTurnRequestId=useRef<string|undefined>(undefined);
 const activeTurnSession=useRef<string|undefined>(undefined);
 const stoppingRef=useRef(false);
 const [stopping,setStopping]=useState(false);
 const view=useRef(current);
 view.current=current;
 const viewingTurn=turnSession===current&&(!savedMessageId||!messages.some(message=>message.id===savedMessageId));
 const slashItems=slashOpen?slashCandidates(input,agents.value,models):[];
 const transcriptRevision=JSON.stringify([messages.at(-1)?.id,messages.length,viewingTurn?live.length:0,viewingTurn?thought.length:0,viewingTurn?toolStatus:'',approval?.id]);
 const usageRevision=JSON.stringify([messages.at(-1)?.id,messages.length]);
 const tokenUsageTip=useMemo(()=>{
  if(!usageBreakdown)return undefined
  const lines=["세션 전체 누적 · 삭제된 대화의 사용량도 포함"]
  for(const row of usageBreakdown.byPurpose||[]){
   lines.push(`${row.purpose==="chat"?"채팅":"보조 "+row.purpose}: ${row.totalTokens.toLocaleString()}토큰 (${row.calls}회)`)
   lines.push(`  입력 ${row.promptTokens.toLocaleString()} · 출력 ${row.completionTokens.toLocaleString()} · 캐시 확인 ${row.observedCachedTokens.toLocaleString()}${row.estimatedCalls?" · 추정치 포함":""}`)
  }
  if(usageBreakdown.hasClearMarker)lines.push(`대화 기록 삭제 이후: ${(usageBreakdown.byPurpose||[]).reduce((sum,row)=>sum+(row.tokensSinceClear||0),0).toLocaleString()}토큰`)
  const source=usageBreakdown.latestSourceCharacterCounts
  if(source){
   lines.push(`최근 입력 원문 크기: 전체 시스템 ${(source.fixedSystemPrompt||0).toLocaleString()}자 · 에이전트 지침 ${(source.agentPersona||0).toLocaleString()}자 · 공통 지침 ${(source.ownerInstructions||0).toLocaleString()}자`)
   lines.push(`  과거 대화 ${(source.selectedHistory||0).toLocaleString()}자 · 검색된 기억 ${(source.retrievedMemoryAndExperience||0).toLocaleString()}자 · 추가 지시 ${(source.systemOverride||0).toLocaleString()}자`)
  }
  const latest=usageBreakdown.recentPromptProfiles?.[0]?.profile
  if(latest?.components){
   const largest=Object.entries(latest.components).filter(([_,v])=>v>0).sort((a,b)=>b[1]-a[1]).slice(0,4)
   lines.push(`최근 요청 구성 추정 ${(latest.totalEstimatedTokens||0).toLocaleString()}토큰: ${largest.map(([name,value])=>`${name} ${value.toLocaleString()}`).join(" · ")}`)
  }
  lines.push("캐시 입력은 총 입력에 포함되며, 캐시 수치는 제공자가 보고한 경우만 표시")
  return lines.join("\n")
 },[usageBreakdown]);
 useEffect(()=>{if(!current){setServerUsage(0);setUsageBreakdown(undefined);setUsageUnavailable(false);return}let alive=true;get<{totalTokens?:number;breakdown?:SessionUsageBreakdown}>('/api/sessions/'+encodeURIComponent(current)+'/usage').then(usage=>{if(alive){setServerUsage(Number(usage.totalTokens)||0);setUsageBreakdown(usage.breakdown);setUsageUnavailable(false)}}).catch(()=>{if(alive)setUsageUnavailable(true)});return()=>{alive=false}},[current,usageRevision])
 const {scrollRef,onScroll,jumpDown}=useTranscriptScroll(current,transcriptRevision);
 const modelPanel=useAnchoredPanel(modelMenu,()=>setModelMenu(false),380);
 useEffect(()=>()=>stream.current?.abort(),[]);
 useEffect(()=>{if(window.innerWidth<768)return;try{localStorage.setItem('dominion.chat.sidebar',showHistory?'1':'0')}catch{}},[showHistory]);
 useEffect(()=>{
  if(!active)return;
  const controller=new AbortController();
  let timer:ReturnType<typeof setTimeout>;
  const check=async()=>{
   try{const state=await get<Runtime>('/api/model-library/runtime',controller.signal);if(!controller.signal.aborted)setRuntime(state)}
   catch{if(!controller.signal.aborted)setRuntime({status:'unknown'})}
   if(!controller.signal.aborted&&providerId==='dominion-native')timer=setTimeout(check,4000)
  };
  void check();
  return()=>{controller.abort();clearTimeout(timer)};
 },[providerId,active]);
 const loadModelMenu=async()=>{
  setMenuError('');setModelsLoading(true)
  try{
   const [list,state]=await Promise.all([get<InstalledModel[]>('/api/model-library/installed'),get<Runtime>('/api/model-library/runtime')])
   setInstalled(list);setRuntime(state)
  }catch(e){setMenuError((e as Error).message)}finally{setModelsLoading(false)}
 }
 const openModelMenu=()=>{
  const next=!modelMenu;setModelMenu(next);setPermissionOpen(false);setShowSettings(false)
  if(next)void loadModelMenu()
 }
 const switchLocalModel=async(m:InstalledModel)=>{
  if(modelBusy)return
  setModelBusy(m.id);setMenuError('')
  try{
   const state=await get<Runtime>('/api/model-library/runtime');
   const ready=state.status==='ready'&&state.id===m.id?state:
    await post<Runtime>('/api/model-library/runtime/load',{id:m.id,context:4096});
   setRuntime(ready);
   if(ready.status!=='ready'||ready.id!==m.id||!ready.model)throw Error(ready.error||t('chat.model.loadFailed'));
   await providers.reload();
   if(providerId==='dominion-native')void catalogue.reload();
   setRoute('dominion-native',ready.model);
   setCommandInfo(t('chat.model.switched',{repo:m.repo,label:m.label}))
   setModelMenu(false)
  }catch(e){setMenuError((e as Error).message)}finally{setModelBusy('')}
 }
 const unloadModel=async()=>{
  if(unloading)return
  if(!window.confirm(t('chat.model.unloadConfirm')))return
  setUnloading(true);setError('')
  try{
   await post('/api/model-library/runtime/unload',{})
   setRuntime({status:'idle'});if(manual?.providerId==='dominion-native')setOverride(undefined)
   await providers.reload();setCommandInfo(t('chat.model.unloaded'))
  }catch(e){setError((e as Error).message)}finally{setUnloading(false)}
 }
 const reportLearning=(result:LearningResult)=>setCommandInfo(
  t('chat.learn.done',{memories:result.memories,skills:result.skill?t('chat.learn.skillSuggested'):t('chat.learn.none')})+
  '\n'+t('chat.learn.hint'));
 useEffect(()=>{
  if(!learningJob||!jobFinished(job.value?.status))return;
  if(learningJob.scope===current){
   if(job.value?.status==='completed'&&job.value.result)reportLearning(job.value.result);
   else setError(job.value?.error||t('chat.learn.incomplete'));
  }
  setLearningJob(undefined);setLearning(false);refresh();
 },[job.value,learningJob,current]);
 const learnSession=async()=>{
  if(!current||learning||selectedSession?.roomId)return;
  setLearning(true);setError('');setCommandInfo('');
  let queued=false;
  try{
   const response=await post<{jobId?:string;result?:LearningResult;message?:string}>('/api/sessions/'+current+'/learn',{});
   if(response.jobId){queued=true;setLearningJob({id:response.jobId,scope:current});return;}
   if(view.current===current){
    if(response.message)setCommandInfo(response.message);
    else if(response.result)reportLearning(response.result);
   }
  }catch(e){if(view.current===current)setError((e as Error).message)}
  finally{if(!queued)setLearning(false)}
 };
 const chooseAgent=(id:string)=>{setAgentChoice({scope:current,id});setOverride(undefined);setExecutionMode('agent')};
 const chooseConversationModel=(id:string,nextModel:string)=>{setExecutionMode('chat');setAgentChoice(undefined);setRoute(id,nextModel)};
 const pickSession=(s:ChatSession)=>{
  onSession(s.id);setAgentChoice(undefined);setOverride(undefined);
  setError('');setCommandInfo('');setSlashOpen(false);
  if(window.innerWidth<768)setShowHistory(false);
 };
 const removePrivateSession=async(session:ChatSession)=>{
  if(deletingSessionId)return;
  if(stream.current&&activeTurnSession.current===session.id){
   setError(t('chat.deleteSessionBusy'));return;
  }
  if(!window.confirm(t('chat.deleteSessionConfirm',{name:session.title})))return;
  setDeletingSessionId(session.id);setError('');
  try{
   await del('/api/sessions/'+encodeURIComponent(session.id));
   delete draftValues.current[session.id];
   delete modeValues.current[session.id];
   delete fileDrafts.current[session.id];
   try{
    for(const prefix of [chatDraftPrefix,chatModePrefix,chatWebPrefix])
     sessionStorage.removeItem(prefix+session.id);
   }catch{/* Browser storage may be unavailable. */}
   if(current===session.id){
    onSession(undefined);setTurnSession(undefined);setSavedMessageId(undefined);
   }
   await sessions.reload();
   refresh('/api/sessions');
  }catch(e){setError(translateError((e as Error).message))}
  finally{setDeletingSessionId('')}
 };
 const resetLocal=()=>{
  onSession(undefined);setAgentChoice(undefined);setOverride(undefined);
  // Keep the prior conversation's unsent text under its original scope.
  setError('');setCommandInfo('');setSlashOpen(false);
  if(window.innerWidth<768)setShowHistory(false);
  inputRef.current?.focus({preventScroll:true});
 };
 // A browser abort alone leaves the native model running. First wait for
 // the server's cancellation acknowledgement AND native session release, then
 // close the local SSE reader and unblock the composer.
 const stopChat=async()=>{
  if(stoppingRef.current)return;
  const id=activeTurnSession.current??current;
  const requestId=activeTurnRequestId.current;
  const controller=stream.current;
  if(!id){
   if(controller){
    controller.abort();stream.current=null;activeTurnRequestId.current=undefined;
    setBusy(false);setLive('');setThought('');setToolStatus('');setTurnSession(undefined);
    setCommandInfo(t('chat.stopped'));
   }else setError(t('chat.cmd.noSessionStop'));
   return;
  }
  stoppingRef.current=true;setStopping(true);setError('');
  try{
   // The cancellation acknowledgement reports the request tombstone, not
   // whether the model/tool and durable execution journal have settled.
   const outcome=await post<{cancelled:boolean;requestRecorded?:boolean}>(
    '/api/sessions/'+encodeURIComponent(id)+'/cancel',requestId?{requestId}:{});
   const statusPath='/api/sessions/'+encodeURIComponent(id)+'/cancel-status';
   let stopped=false;
   for(let attempt=0;attempt<60;attempt++){
    const status=await get<{stopped:boolean}>(statusPath);
    if(status.stopped){stopped=true;break;}
    await new Promise<void>(resolve=>setTimeout(resolve,200));
   }
   if(!stopped)throw Error(t('chat.stopPending'));
   controller?.abort();
   if(stream.current===controller){
    stream.current=null;activeTurnSession.current=undefined;activeTurnRequestId.current=undefined;
    setBusy(false);setLive('');setThought('');setLiveApproval(null);
    setToolStatus('');setTurnSession(undefined);setSavedMessageId(undefined);
   }
   refresh('/api/sessions','/api/approvals','/api/sessions/'+encodeURIComponent(id)+'/messages','/api/sessions/'+encodeURIComponent(id)+'/usage');
   setCommandInfo(outcome.cancelled||outcome.requestRecorded||controller?t('chat.stopped'):t('chat.cmd.nothingRunning'));
  }catch(e){
   setError(translateError((e as Error).message));
  }finally{
   stoppingRef.current=false;setStopping(false);
  }
 };
 const executeCommand=async(text:string)=>{
  const found=text.trim().match(/^\/([a-zA-Z]+)(?:\s+([\s\S]*))?$/);
  if(!found){setError(t('chat.cmd.invalid'));return;}
  const name=found[1].toLowerCase(),arg=(found[2]||'').trim();
  if(!slashCommands.some(x=>x.key===name)){setError(t('chat.cmd.unknown',{name}));return;}
  if(['name','model','agent'].includes(name)&&!arg){
   setInput('/'+name+' ');setSlashOpen(name==='model'||name==='agent');inputRef.current?.focus({preventScroll:true});return;
  }
  setError('');setCommandInfo('');setInput('');setSlashOpen(false);
  try{
   if(name==='new'){resetLocal();setCommandInfo(t('chat.cmd.new'));return;}
   if(name==='history'){setShowHistory(true);await sessions.reload();setCommandInfo(t('chat.cmd.history'));return;}
   if(name==='model'){
    const found=models.find(x=>x.id===arg);if(!found)throw Error(t('chat.cmd.modelMissing',{model:arg}));
    setRoute(providerId,found.id);setCommandInfo(t('chat.cmd.modelSwitched',{model:found.id}));return;
   }
   if(name==='agent'){
    const found=agents.value.find(x=>x.id===arg);if(!found)throw Error(t('chat.cmd.agentMissing',{agent:arg}));
    chooseAgent(found.id);setCommandInfo(t('chat.cmd.agentSwitched',{agent:found.name}));return;
   }
   if(name==='think'){
    const enabled=arg==='on'?true:arg==='off'?false:!thinking;
    setThinking(enabled);setCommandInfo(t('chat.cmd.thinking',{state:enabled?t('chat.cmd.on'):t('chat.cmd.off')}));return;
   }
   if(name==='help'){setCommandInfo(slashCommands.map(x=>x.command+' · '+slashCommandLabel(x,loc).name).join('\n'));return;}
   if(name==='stop'){await stopChat();return;}
   if(name==='status'){
    const state=await get<{agents:number;providers:number;jobs:number;executions:number}>('/api/status');
    setCommandInfo(t('chat.cmd.status',{agents:state.agents,providers:state.providers,executions:state.executions,jobs:state.jobs}));return;
   }
   if(name==='tools'){
    const found=await get<{name:string}[]>('/api/tools');
    setCommandInfo(t('chat.cmd.tools',{count:found.filter(x=>x.name).length,list:found.filter(x=>x.name).map(x=>x.name).join(' · ')}));return;
   }
   if(['mcp','skills','memory','agents','settings'].includes(name)){
    const route=name==='mcp'||name==='skills'?'extensions':name;
    window.location.hash='#/'+route;return;
   }
   if(!current)throw Error(t('chat.cmd.noSession'));
   if(name==='name'){
    await post('/api/sessions/'+current+'/rename',{title:arg});await sessions.reload();
    setCommandInfo(t('chat.cmd.renamed',{name:arg}));return;
   }
   if(name==='clear'){
    if(!window.confirm(t('chat.cmd.confirmClear')))return;
    await post('/api/sessions/'+current+'/clear',{});await transcript.reload();
    setCommandInfo(t('chat.cmd.cleared'));return;
   }
   if(name==='fork'){
    const copied=await post<ChatSession>('/api/sessions/'+current+'/fork',{});
    pickSession(copied);await sessions.reload();
    setCommandInfo(t('chat.cmd.forked',{title:copied.title}));return;
   }
   if(name==='compact'){
    if(!model||!providerId)throw Error(t('chat.cmd.needModel'));
    const result=await post<{summarized:number;remaining:number}>('/api/sessions/'+current+'/compact',{providerId,model});
    await transcript.reload();
    setCommandInfo(t('chat.cmd.compacted',{summarized:result.summarized,remaining:result.remaining}));return;
   }
   if(name==='export'){
    const exported=await get<ChatMessage[]>('/api/sessions/'+current+'/messages');
    const blob=new Blob([JSON.stringify({sessionId:current,messages:exported},null,2)],{type:'application/json'});
    const url=URL.createObjectURL(blob),link=document.createElement('a');link.href=url;link.download='dominion-session-'+current+'.json';
    link.click();URL.revokeObjectURL(url);setCommandInfo(t('chat.cmd.exported'));return;
   }
  }catch(e){setError((e as Error).message);}
 };
 const chooseSlash=(item:SlashChoice)=>{
  setSlashActive(0);
  if(item.submenu||item.requiresArg){
   setInput(item.command+' ');setSlashOpen(Boolean(item.submenu));inputRef.current?.focus({preventScroll:true});return;
  }
  setSlashOpen(false);void executeCommand(item.command);
 };
 const send=async(e?:FormEvent)=>{
  e?.preventDefault();
  const filesToSend=[...draftFiles]
  const text=input.trim()||(filesToSend.length?'첨부한 파일을 확인해 주세요.':'')
  if(!text)return;
  if(text.startsWith('/')){
   if(busy&&text!=='/stop')return;
   await executeCommand(text);return;
  }
  if(stream.current||stoppingRef.current||selectedSession?.roomId||(current&&sessions.loading))return;
  if(providerId&&!selectedProvider){setError(t('chat.err.agentApi'));return;}
  if(!providerId){setError(t('chat.err.connectApi'));return;}
  if(!model){setError(t('chat.err.pickModel'));return;}
  const controller=new AbortController();
  const requestId=crypto.randomUUID();
  activeTurnRequestId.current=requestId;
  stream.current=controller;
  activeTurnSession.current=current;
  setTurnSession(current);setSavedMessageId(undefined);
  setBusy(true);setError('');setCommandInfo('');setLive('');setThought('');setSlashOpen(false);setToolStatus('');
  jumpDown();
  let id=current;
  let saved=false;
  try{
   if(!id){
    const session=await post<ChatSession>('/api/sessions',{
     title:t('chat.sessionTitle'),
     conversationMode:executionMode,
     agentId:executionMode==='agent'?agentId:undefined,
     providerId:executionMode==='chat'?providerId:manual?.providerId,
     model:executionMode==='chat'?model:manual?.model
    });
    if(controller.signal.aborted)return;
    id=session.id;
    if(filesToSend.length){
     fileDrafts.current[id]=filesToSend
     delete fileDrafts.current.new
     redrawFiles(value=>value+1)
    }
    modeValues.current[id]=executionMode
    storeChatMode(id,executionMode)
    webChoice.current[id]=webSearchEnabled
    storeChatWeb(id,webSearchEnabled)
    activeTurnSession.current=id;
    if(view.current===current){
     onSession(id);setTurnSession(id);
     if(agentId)setAgentChoice({id:agentId,scope:id});
     if(manual)setOverride({...manual,scope:id});
    }
    void sessions.reload();
   }
   const attachmentIds:string[]=[]
   if(filesToSend.length)setToolStatus('첨부 파일 업로드 중…')
   for(const file of filesToSend){
    if(controller.signal.aborted)return
    const reused=uploadedFiles.current.get(file)
    if(reused?.sessionId===id){attachmentIds.push(reused.id);continue}
    const uploaded=await post<{id:string}>('/api/attachments',{
     sessionId:id,name:file.name,mimeType:file.type||'application/octet-stream',
     data:await fileToBase64(file)
    })
    if(controller.signal.aborted)return
    uploadedFiles.current.set(file,{id:uploaded.id,sessionId:id!})
    attachmentIds.push(uploaded.id)
   }
   setToolStatus('')
   setInput(previous=>previous.trim()===text?'':previous);
   if(inputRef.current)inputRef.current.style.height='';
   await streamChat({
    sessionId:id,requestId,conversationMode:executionMode,
    webSearch:executionMode==='chat'&&webSearchEnabled,
    agentId:executionMode==='agent'?agentId:undefined,
    providerId:executionMode==='chat'?providerId:manual?.providerId,
    model:executionMode==='chat'?model:manual?.model,input:text,attachmentIds,
    toolsEnabled:executionMode==='agent'&&toolsEnabled,
    toolsExecutionEnabled:executionMode==='agent'&&toolsEnabled,
    autoLearning:executionMode==='agent'&&autoLearn,
    reasoningLanguage:reasoningKo?'ko':'original',reasoningEffort:reasoningEffort||undefined,thinking:selectedProvider?.kind==='codex'?(reasoningEffort==='none'?false:canDisableThinking?thinkingEnabled:undefined):thinkingEnabled,
   },event=>{
    if(controller.signal.aborted||stoppingRef.current||stream.current!==controller)return;
    if(event.type==='assistant_saved'){
     saved=true;setSavedMessageId(event.messageId)
     if(id&&filesToSend.length){fileDrafts.current[id]=[];redrawFiles(value=>value+1)}
     refresh('/api/sessions',...(id?['/api/sessions/'+encodeURIComponent(id)+'/messages']:[]))
    }
    if(event.type==='text')setLive(old=>old+event.text);
    if(event.type==='reasoning')setThought(old=>old+event.text);
    if(event.type==='approval'){setLiveApproval({id:event.id,tool:event.tool,args:event.args,sessionId:id!,status:'pending'});void approvals.reload();}
    if(event.type==='tool_start'){setLiveApproval(null);setToolStatus(t('chat.tool.running',{name:event.name}));}
    if(event.type==='tool_end')setToolStatus(t('chat.tool.done',{name:event.name}));
    if(event.type==='tool_error')setToolStatus(t('chat.tool.error',{error:translateError(event.error)}));
    if(event.type==='goal_progress')setToolStatus(t('chat.tool.goal',{event:event.event,status:event.status}));
    if(event.type==='error')throw Error(event.message);
   },controller.signal);
  }catch(e){
   if(!controller.signal.aborted){
    if(!saved)restoreFailedDraft(id||draftScope,text)
    if(view.current===id)setError(translateError((e as Error).message))
   }
  }finally{
   if(stream.current===controller){
    stream.current=null;activeTurnSession.current=undefined;activeTurnRequestId.current=undefined;
    refresh('/api/sessions',...(id?['/api/sessions/'+encodeURIComponent(id)+'/messages']:[]));
    setBusy(false);setLiveApproval(null);setToolStatus('');
    if(!saved){setLive('');setThought('');}
   }
  }
 };
 const approve=async(ok:boolean)=>{
  if(!approval||approvalBusy)return;
  setApprovalBusy(true);
  try{await resolveApproval(approval.id,ok);setLiveApproval(null)}
  catch(e){setError((e as Error).message)}
  finally{await approvals.reload();setApprovalBusy(false)}
 };
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
 useEffect(()=>{if(!active){setModelMenu(false);setPermissionOpen(false);setShowSettings(false);setSlashOpen(false)}else setReasoningKo(koreanReasoningEnabled())},[active]);
 return <div className={'relative grid h-dvh min-h-0 grid-cols-1 grid-rows-[minmax(0,1fr)] bg-[#FFFDF9] '+(showHistory?'md:grid-cols-[232px_minmax(0,1fr)]':'md:grid-cols-[56px_minmax(0,1fr)]')}>
  {showHistory&&<Button type="button" aria-label={t('chat.listCollapse')} className="absolute inset-y-0 right-0 left-[232px] z-20 !h-auto !w-auto cursor-default !rounded-none !border-0 !bg-slate-900/30 md:hidden" onClick={()=>setShowHistory(false)}/>}
  <aside id="conversation-sidebar" aria-label={t('chat.recent')} className={'flex min-h-0 flex-col overflow-hidden border-r border-[#E7E0D5] bg-[#FAF7F1] '+(showHistory?'max-md:absolute max-md:inset-y-0 max-md:left-0 max-md:z-30 max-md:w-[232px] max-md:shadow-2xl':'hidden')}>
   <div className="flex min-h-16 shrink-0 items-center gap-2 px-3">
    <span className="grid size-8 shrink-0 place-items-center rounded-xl border border-[#EED0BC] bg-[#F7E8E1] text-lg font-bold text-[#C96442]">D</span>
    <strong className="min-w-0 flex-1 truncate text-lg font-semibold tracking-tight text-[#24211D]">dominion</strong>
    <Button size="icon" variant="ghost" type="button" onClick={()=>setShowHistory(false)}
     aria-label={t('chat.listCollapse')} aria-controls="conversation-sidebar" aria-expanded={true} title={t('chat.listCollapse')}><Menu size={19}/></Button>
   </div>
   <Button className="mx-3 mb-3 inline-flex items-center justify-center gap-2 rounded-xl border border-[#EED9CC] bg-[#C96442] px-4 py-2.5 text-xs font-bold text-white hover:bg-[#A94E30]" onClick={resetLocal}><Plus size={17}/> {t('chat.newChat')}</Button>
   <div className="mx-3 mb-3"><ConversationSwitch mode="chat" onChange={switchMode}/></div>
   <div className="chat-session-search"><Search size={14}/><input type="search" aria-label="대화 목록 검색" placeholder="대화 검색" value={sessionFilter} onChange={e=>setSessionFilter(e.target.value)}/>{sessionFilter&&<button type="button" aria-label="검색 지우기" onClick={()=>setSessionFilter('')}><X size={13}/></button>}</div>
   <div className="min-h-0 flex-1 overflow-y-auto px-2">{filteredSessions.map(s=><div key={s.id} className={'group flex min-w-0 items-center rounded-xl '+(current===s.id?'bg-[#F7E8E1]':'hover:bg-[#F7E8E1]')}>
     <Button variant="bare" onClick={()=>pickSession(s)} title={s.title} className={'!min-w-0 flex-1 !justify-start gap-3 !rounded-xl !px-3 !py-2.5 text-left text-xs '+(current===s.id?'font-bold text-[#A94E30]':'text-[#7C7469]')}><MessageCircle size={15} className="shrink-0"/><span className="min-w-0 truncate">{s.title}</span></Button>
     <Button type="button" size="icon" variant="ghost" disabled={Boolean(deletingSessionId)||Boolean(stream.current&&activeTurnSession.current===s.id)} title={t('chat.deleteSession')} aria-label={s.title+' '+t('chat.deleteSession')} className="!mr-1 !size-7 shrink-0 !rounded-lg text-[#8A8176] hover:!bg-rose-50 hover:!text-rose-700" onClick={()=>void removePrivateSession(s)}><Trash2 size={13}/></Button>
    </div>)}{sessions.loading&&!filteredSessions.length?<div className="p-5 text-center text-xs leading-5 text-[#9C9388]">{t('chat.sessionsLoading')}</div>:sessions.error&&!filteredSessions.length?<div role="status" className="p-5 text-center text-xs leading-5 text-[#9C9388]">{t('chat.sessionsUnavailable')}</div>:!filteredSessions.length&&<div className="p-5 text-center text-xs leading-5 text-[#9C9388]">{sessionFilter.trim()?'일치하는 대화가 없습니다.':t('chat.first')}</div>}</div>
   <div className="shrink-0 border-t border-[#E7E0D5] p-2"><Button variant="ghost" onClick={openSettings} className="w-full !justify-start gap-2 !text-xs" aria-label={t('nav.settings')}><Settings size={17}/>{t('nav.settings')}</Button></div>
  </aside>
  {!showHistory&&<ConversationRail sidebarId="conversation-sidebar" onExpand={()=>setShowHistory(true)}
   onNew={resetLocal} onSwitchMode={switchMode} onSettings={openSettings}/>}
  <div className="flex min-h-0 min-w-0 flex-col">
   <header className="chat-header relative flex h-13 shrink-0 items-center justify-between gap-2 border-b border-[#E7E0D5] bg-[#FFFDF9] px-4">
    <Button size="icon" variant="ghost" onClick={()=>setShowHistory(x=>!x)} title={showHistory?t('chat.listCollapse'):t('chat.listExpand')} aria-label={showHistory?t('chat.listCollapse'):t('chat.listExpand')}><Menu size={18}/></Button>
    <ConversationSwitch mode="chat" onChange={switchMode}/>
    <div className="flex min-w-0 flex-1 items-center gap-1.5">
     <div className="chat-model-controls relative flex min-w-0 items-center gap-1">
      <Button variant="ghost" size="sm" ref={modelPanel.trigger} onClick={openModelMenu} title={t('chat.localModel')} className="ml-auto !border !border-[#E7E0D5] !bg-white !text-[#5C544A] hover:!bg-[#FAF7F1]" aria-label={t('chat.localModel')} aria-expanded={modelMenu}><HardDrive size={14}/><span className="hidden lg:inline">{t('chat.localModel')}</span><ChevronDown size={12}/></Button>
      <Button variant="ghost" size="sm" title={t('chat.unload')} aria-label={t('chat.unload')} onClick={()=>void unloadModel()} disabled={unloading||runtime.status!=='ready'} className="!border !border-[#E7E0D5] !bg-white !text-[#5C544A] hover:!bg-[#FAF7F1] disabled:opacity-40"><Power size={14}/><span className="hidden xl:inline">{unloading?t('chat.unloading'):t('chat.unload')}</span></Button>
      {modelMenu&&modelPanel.position&&createPortal(
       <div ref={modelPanel.panel} style={modelPanel.position} className="z-[80] overflow-y-auto overscroll-contain rounded-2xl border border-[#E7E0D5] bg-[#FFFDF9] shadow-[0_18px_50px_rgba(36,33,29,.14)]">
        <div className="flex items-center justify-between gap-2 border-b border-[#EFE9DF] px-3 py-2.5"><div className="min-w-0"><strong className="text-[12px] text-[#3A342D]">{t('chat.localModel')}</strong><p className="truncate text-[10px] text-[#9C9388]">{runtime.status==='ready'?t('chat.running')+' · '+(runtime.name||''):runtime.status==='loading'?t('chat.loadingModel'):t('chat.noLocal')}</p></div><Button size="icon" variant="ghost" title={t('models.refresh')} onClick={()=>void loadModelMenu()}><RefreshCw size={15}/></Button></div>
        {menuError&&<div className="border-b border-rose-100 bg-rose-50 px-3 py-2 text-[11px] text-rose-700">{menuError}</div>}
        <div className="max-h-[320px] overflow-y-auto p-1.5">{modelsLoading&&<div className="flex items-center justify-center gap-2 p-5 text-[11px] text-[#9C9388]"><LoaderCircle size={14} className="animate-spin"/> {t('chat.loadingModel')}</div>}{!modelsLoading&&installed.map(m=><Button type="button" variant="ghost" key={m.id} disabled={Boolean(modelBusy)} onClick={()=>void switchLocalModel(m)} className={'flex w-full items-center gap-3 !justify-start !border-0 rounded-lg px-2.5 py-2.5 text-left hover:bg-[#FAF7F1] disabled:opacity-60 '+(runtime.status==='ready'&&runtime.id===m.id?'bg-emerald-50/60':'')}>
         <span className="grid size-8 shrink-0 place-items-center rounded-lg bg-[#F3EEE5] text-[#7C7469]"><HardDrive size={15}/></span>
         <span className="min-w-0 flex-1"><strong className="block truncate text-[12px] text-[#3A342D]">{m.repo}</strong><small className="text-[10px] text-[#9C9388]">{m.label} · {m.format.toUpperCase()} · {formatBytes(m.bytes)}</small></span>
         {runtime.status==='ready'&&runtime.id===m.id?<span className="shrink-0 rounded bg-emerald-100 px-2 py-1 text-[10px] font-semibold text-emerald-700">{t('chat.inUse')}</span>:modelBusy===m.id?<span className="shrink-0 text-[10px] font-semibold text-[#C96442]">{t('chat.loadingShort')}</span>:<span className="shrink-0 text-[10px] font-semibold text-[#C96442]">{t('chat.runShort')}</span>}
        </Button>)}{!modelsLoading&&!installed.length&&<div className="p-5 text-center text-[11px] text-[#9C9388]">{t('chat.noInstalled')}</div>}</div>
        <Button type="button" variant="ghost" onClick={()=>{setModelMenu(false);openModels()}} className="flex w-full items-center justify-between !rounded-none border-t border-[#EFE9DF] px-3 py-2.5 text-[11px] font-semibold text-[#5C544A] hover:bg-[#FAF7F1]">{t('chat.openModels')} <ArrowRight size={13}/></Button>
       </div>
      ,document.body)}
     </div>
    </div>
    <div className="flex shrink-0 items-center gap-1">
     {usageUnavailable?<span role="status" title={t('usage.unavailable')} className="hidden rounded-lg border border-amber-200 bg-amber-50 px-2 py-1 text-[10px] font-semibold text-amber-800 sm:inline-flex">{messageUsage>0?t('usage.loadedOnly',{count:messageUsage.toLocaleString()}):t('usage.unavailable')}</span>:(serverUsage||messageUsage)>0&&<span title={tokenUsageTip} className="hidden rounded-lg border border-[#E7DDCC] bg-[#F7F1E6] px-2 py-1 text-[10px] font-semibold text-[#8A775F] sm:inline-flex">{t('usage.sessionTotal',{count:(serverUsage||messageUsage).toLocaleString()})}</span>}

     
     <Button size="icon" variant="ghost" title={t('chat.modelManage')} aria-label={t('chat.modelManage')} onClick={openModels}><Cpu size={18}/></Button>
     <Button size="icon" variant="ghost" className="conversation-settings" title={t('nav.settings')} aria-label={t('nav.settings')} onClick={openSettings}><Settings size={18}/></Button>
    </div>
   </header>
   <div ref={scrollRef} onScroll={onScroll} data-chat-scroll className="min-h-0 flex-1 overflow-y-auto overscroll-contain">{current&&transcript.loading&&!messages.length?<div className="flex min-h-full items-center justify-center p-6 text-sm text-[#8A8176]">{t('chat.transcriptLoading')}</div>:current&&transcript.error&&!messages.length?<div role="status" className="flex min-h-full items-center justify-center p-6 text-sm text-[#8A8176]">{t('chat.transcriptUnavailable')}</div>:!messages.length&&!(busy&&viewingTurn)?<div className="mx-auto flex min-h-full max-w-xl flex-col justify-center px-5 py-8">
    <div className="mb-6 flex items-center gap-3"><span className="grid size-11 place-items-center rounded-2xl bg-[#F7E8E1] text-[#C96442]"><Sparkles size={22}/></span><div><h1 className="text-[24px] font-bold tracking-[-.03em] text-[#24211D]">{t('chat.newChatHeading')}</h1><p className="mt-1 text-[13px] text-[#8A8176]">{t('chat.newChatDesc')}</p></div></div>
    <div className="rounded-2xl border border-[#E7E0D5] bg-[#FFFDF9] p-5"><div className="mb-2 text-[11px] font-semibold text-[#8A8176]">{t('chat.examples')}</div>{[t('chat.example1'),t('chat.example2'),t('chat.example3')].map(example=><Button type="button" key={example} onClick={()=>{setInput(example);inputRef.current?.focus({preventScroll:true})}} className="flex w-full items-center justify-between border-t border-[#EFE9DF] px-2 py-3 text-left text-[12px] text-[#5C544A] hover:bg-[#FFFDF9]">{example}<ArrowRight size={14} className="shrink-0 text-[#9C9388]"/></Button>)}</div>
    {!model&&<Button type="button" onClick={openModels} className="mt-4 inline-flex items-center gap-2 self-start text-[12px] font-semibold text-[#C96442]"><Compass size={15}/> {t('chat.connectFirst')} <ArrowRight size={14}/></Button>}
   </div>:<div className="chat-transcript mx-auto flex min-h-full w-full max-w-[860px] flex-col gap-6 px-6 py-7">{messages.filter(m=>m.role!=='tool').map(m=><Message key={m.id} message={m} agentName={agents.value.find(a=>a.id===m.agent_id)?.name} showReasoning={showReasoning} />)}{viewingTurn&&(live||thought||busy)&&<div className="chat-turn chat-turn-assistant chat-turn-live"><div className="grid size-9 shrink-0 place-items-center rounded-xl bg-[#F3EEE5] text-[#4A443C] ai"><Sparkles size={17}/></div><div className="chat-turn-body"><div className="mb-2 flex items-center gap-2 text-xs font-bold text-[#4A443C]">DOMINION <span>{t('ui.label.live')}</span>{busy&&thought&&!showReasoning&&<span className="inline-flex items-center gap-1 rounded bg-[#F3EEE5] px-1.5 py-0.5 text-[10px] font-medium text-[#7C7469]"><BrainCircuit size={11}/> {t('chat.thinkingNow')}</span>}</div>{showReasoning&&thought&&<details className="mb-3 max-h-56 overflow-y-auto rounded-xl border border-[#E7E0D5] bg-[#FAF7F1] p-3 text-[11px] leading-5 text-[#7C7469]" open={thinking}><summary className="cursor-pointer select-none" onClick={e=>{e.preventDefault();setThinking(x=>!x)}}><BrainCircuit size={14} className="inline"/> {t('chat.reasoningProcess',{count:thought.length.toLocaleString()} )} <ChevronRight size={13} className="inline"/></summary><div className="mt-2 whitespace-pre-wrap">{thought}</div></details>}{live&&<div className="chat-turn-text">{live}</div>}{busy&&!live&&<div className="flex items-center gap-2 text-xs text-[#9C9388]"><i/><i/><i/></div>}</div></div>}{viewingTurn&&toolStatus&&<div className="inline-flex items-center gap-2 rounded-lg bg-[#FAF7F1] px-3 py-1.5 text-xs text-[#7C7469]"><TerminalSquare size={15}/>{toolStatus}</div>}</div>}</div>
   <div className="conversation-composer-wrap sticky bottom-0 z-10"><div className="mx-auto w-full max-w-[860px]">{approval&&<div className="mb-3 flex flex-wrap items-center justify-between gap-3 rounded-xl border border-amber-200 bg-amber-50 p-3 text-xs text-amber-900"><div><ShieldAlert size={20}/><span><strong>{t('chat.approval')}</strong><small>{approval.tool} · {JSON.stringify(approval.args??{}).slice(0,150)}</small></span></div><div><Action variant="secondary" disabled={approvalBusy} onClick={()=>approve(false)}>{t('chat.deny')}</Action><Action disabled={approvalBusy} onClick={()=>approve(true)}><Check size={15}/> {t('chat.approve')}</Action></div></div>}
    <Notice message={agents.error||providers.error||approvals.error}/>
    {(error||transcript.error||job.error)&&<div className="mb-3 rounded-xl border border-rose-200 bg-rose-50 p-3 text-xs text-rose-700"><CircleHelp size={16}/>{error||transcript.error||translateError(job.error||'')}<Button onClick={()=>setError('')}><X size={14}/></Button></div>}
    {modelError&&!model&&<div className="mb-3 text-xs text-[#7C7469]">{t('chat.noModel')}: {modelError} <Button onClick={openModels}>{t('chat.manage')} <ArrowRight size={13}/></Button></div>}
    {commandInfo&&<div role="status" className="mb-3 max-h-36 overflow-auto whitespace-pre-wrap rounded-xl border border-[#EED9CC] bg-[#F7E8E1] p-3 text-xs leading-6 text-[#4A443C]">{commandInfo}</div>}
    <TeamRunStrip sessionId={current} active={active} onDraft={draft=>{setInput(draft);setSlashOpen(false);inputRef.current?.focus({preventScroll:true})}}/>
    <div className="relative">
    {slashOpen&&<SlashMenu items={slashItems} active={slashActive} onChoose={chooseSlash}/>}
    <form onSubmit={send} className={'conversation-composer'+(isDropping?' chat-composer-drop':'')}
     onDragOver={event=>{if(Array.from(event.dataTransfer.types).includes('Files')){event.preventDefault();setIsDropping(true)}}}
     onDragLeave={event=>{if(!event.currentTarget.contains(event.relatedTarget as Node))setIsDropping(false)}}
     onDrop={handleFileDrop}>
     <div className="chat-execution-mode" role="group" aria-label="채팅 실행 방식">
      <Button type="button" className="chat-execution-mode-option" aria-pressed={executionMode==='chat'} disabled={busy} onClick={()=>setExecutionMode('chat')}><MessageCircle size={14}/> 일반 대화</Button>
      <Button type="button" className="chat-execution-mode-option" aria-pressed={executionMode==='agent'} disabled={busy} onClick={()=>setExecutionMode('agent')}><TerminalSquare size={14}/> 에이전트 실행</Button>
      <span className="chat-execution-mode-hint">{executionMode==='chat'?(webSearchEnabled?'공개 웹 검색만 허용':'도구 사용 없이 모델과 대화'):'파일·도구·MCP 작업 가능'}</span>
     </div>
     {draftFiles.length>0&&<div className="chat-file-drafts" aria-label="첨부 대기 파일">{draftFiles.map((file,i)=><span key={i} className="chat-file-chip"><Paperclip size={13}/><span title={file.name}>{file.name}</span><small>{formatDraftBytes(file.size)}</small><button type="button" aria-label={file.name+' 첨부 취소'} disabled={busy} onClick={()=>removeFile(i)}><X size={13}/></button></span>)}</div>}
     <input ref={fileInputRef} type="file" className="sr-only" multiple accept="image/png,image/jpeg,image/webp,image/gif,.txt,.md,.csv,.ts,.tsx,.js,.mjs,.json,.py,.rs,.yaml,.yml,.xml,.html,.css,.log,.sql" aria-label="첨부 파일 선택" disabled={busy} onChange={e=>{addFiles(e.currentTarget.files);e.currentTarget.value=''}}/>
     <TextArea ref={inputRef} rows={1} placeholder={t("chat.inputPlaceholder")} value={input} onChange={e=>{const next=e.target.value;setInput(next);setSlashOpen(next.startsWith('/')&&!next.includes('\n'));setSlashActive(0);const el=e.currentTarget;el.style.height='auto';el.style.height=Math.min(el.scrollHeight,180)+'px'}} onKeyDown={appendKey} onPaste={handleClipboardFiles} aria-controls="dominion-slash-menu" aria-expanded={slashOpen} className="conversation-composer-input"/>
     <div className="chat-composer-controls conversation-composer-controls">
      <div className="chat-composer-options conversation-composer-options">
       <ConversationTargetPicker active={active} disabled={busy}
        providers={enabledProviders} agents={agents.value}
        providerId={providerId} model={model} agentId={agentId} mode={executionMode}
        onModel={chooseConversationModel} onPersona={chooseAgent} onManage={openModels}/>
       <Button type="button" variant="ghost" disabled={busy} className="chat-file-add" title="이미지 또는 텍스트 파일 첨부" onClick={()=>fileInputRef.current?.click()}><Paperclip size={15}/><span className="hidden sm:inline">첨부</span></Button>
        {executionMode==='chat'&&<Button type="button" variant="ghost" disabled={busy}
         className="chat-web-search-toggle" aria-pressed={webSearchEnabled}
         title={webSearchEnabled?'공개 웹 검색 사용 중':'공개 웹에서 검색하고 출처 확인'}
         onClick={()=>setWebSearchEnabled(!webSearchEnabled)}><Globe2 size={14}/>웹 검색</Button>}
       {executionMode==='agent'&&<ExecutionPermissions active={active} open={permissionOpen} onOpenChange={open=>{setPermissionOpen(open);if(open){setShowSettings(false);setModelMenu(false)}}} onError={setError} onChanged={label=>setCommandInfo(t('chat.perm.changed',{label}))}/>}

       <ReasoningSelect value={reasoningEffort} onChange={effort=>{setReasoningEffort(effort);setThinkingEnabled(effort!=='none')}} model={selectedModel} requireCatalog={selectedProvider?.kind==='codex'} defaultLabel={executionMode==='agent'&&activeAgent?.reasoningEffort?`에이전트 설정 (${activeAgent.reasoningEffort})`:t('chat.reasoningAuto')} className="chat-reasoning !h-7 !w-auto !border-0 !bg-transparent !py-1 !pl-1.5 !pr-6 !text-[11px] !font-semibold !text-[#5C544A]"/>

       <ConversationSettings open={showSettings} onOpenChange={open=>{setShowSettings(open);if(open){setPermissionOpen(false);setModelMenu(false)}}} showReasoning={showReasoning} setShowReasoning={setShowReasoning} reasoningKo={reasoningKo} setReasoningKo={setReasoningKo} thinkingEnabled={thinkingEnabled} setThinkingEnabled={setThinkingEnabled} canDisableThinking={canDisableThinking} reasoningEffort={reasoningEffort} setReasoningEffort={setReasoningEffort} toolsEnabled={executionMode==='agent'&&toolsEnabled} setToolsEnabled={value=>{if(value)setExecutionMode('agent');setToolsEnabled(value)}} autoLearn={executionMode==='agent'&&autoLearn} setAutoLearn={value=>{if(value)setExecutionMode('agent');setAutoLearn(value)}} learning={learning} learnDisabled={!current||Boolean(selectedSession?.roomId)||busy} onLearn={learnSession}/>
      </div>
      <div className="ml-auto flex items-center gap-2">
       <span className="hidden text-[10px] text-[#9C9388] sm:inline">{t('chat.enterHint')}</span>
       {busy?<Button className="conversation-composer-send !bg-rose-600 hover:!bg-rose-700" type="button" onClick={()=>void stopChat()} disabled={stopping} title={stopping?t('chat.stopping'):t('chat.stop')} aria-label={stopping?t('chat.stopping'):t('chat.stop')}>{stopping?<LoaderCircle size={18} className="animate-spin"/>:<Square size={18} fill="currentColor"/>}</Button>
       :<Button className="conversation-composer-send" disabled={stopping||Boolean(selectedSession?.roomId)||(Boolean(current)&&sessions.loading)||(!input.trim()&&!draftFiles.length)} type="submit" title={t('chat.send')}><ArrowUp size={19}/></Button>}
      </div>
     </div>
    </form></div>
   </div></div>
  </div>
 </div>
}
function Message({message,agentName,showReasoning}:{message:ChatMessage;agentName?:string;showReasoning?:boolean}){
 const [open,setOpen]=useState(true)
 const human=message.role==='user'
 const attached=(Array.isArray(message.metadata?.attachments)?message.metadata.attachments:[]) as {id:string;name:string;size?:number;isImage?:boolean}[]
 const sources=Array.isArray(message.metadata?.sources)
  ?message.metadata.sources.filter(source=>typeof source.title==='string'
   &&typeof source.url==='string'&&/^https?:\/\//i.test(source.url)).slice(0,8):[]
 return <div className={'chat-turn '+(human?'chat-turn-owner':'chat-turn-assistant')} data-message data-role={message.role}>
  {!human&&<div className="grid size-9 shrink-0 place-items-center rounded-xl bg-[#F7E8E1] text-[#A94E30]"><Sparkles size={17}/></div>}
  <div className="chat-turn-body">
   {!human&&<div className="chat-turn-byline">{agentName||'DOMINION'}<TokenUsage usage={message.metadata?.usage} className="ml-auto rounded border border-[#E7DDCC] bg-[#F7F1E6] px-1.5 py-0.5 text-[9px] font-medium text-[#8A775F]"/></div>}
   {showReasoning&&message.reasoning&&<details className="mb-3 max-h-56 overflow-y-auto rounded-xl border border-[#E7E0D5] bg-[#FAF7F1] p-3 text-[11px] leading-5 text-[#7C7469]" open={open}><summary className="cursor-pointer select-none" onClick={e=>{e.preventDefault();setOpen(v=>!v)}}><BrainCircuit size={14} className="inline"/> {t('chat.reasoningProcess',{count:message.reasoning.length.toLocaleString()})} <ChevronRight size={13} className="inline"/></summary><div className="mt-2 whitespace-pre-wrap">{message.reasoning}</div></details>}
   {(message.content||!human)&&<div className="chat-turn-text">{human?message.content:<MarkdownMessage text={message.content||(!human&&message.tool_calls?.length?t('chat.toolRequest'):'')}/>}</div>}
   {human&&attached.length>0&&<div className="chat-saved-attachments">{attached.map(file=><AttachmentLink key={file.id} file={file}/>)}</div>}
   {!human&&sources.length>0&&<details className="chat-web-sources" aria-label="웹 검색 결과">
    <summary>웹 검색 결과 {sources.length}개</summary>
    <div>{sources.map((source,i)=><a key={source.url} href={source.url} target="_blank" rel="noopener noreferrer">
     <span className="chat-web-source-number">{i+1}</span>
     <span className="chat-web-source-title">{source.title}</span>
     {source.published&&<small>{source.published}</small>}
    </a>)}</div>
   </details>}
   {!human&&message.content&&<Button className="chat-turn-copy inline-flex items-center gap-1.5 rounded-lg px-2 py-1 text-[11px] text-[#7C7469] hover:bg-[#F3EEE5]" onClick={()=>void navigator.clipboard.writeText(message.content)} title={t('chat.copy')}><Copy size={14}/></Button>}
  </div>
 </div>
}
