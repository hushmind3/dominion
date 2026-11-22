import {ConversationSettings} from '../components/ConversationSettings'
import {ExecutionPermissions} from '../components/ExecutionPermissions'
import {ChatSessionInspector} from '../components/ChatSessionInspector'
import {ChatMessageRow} from '../components/ChatMessageRow'
import {TokenUsage} from '../components/TokenUsage'
import {ChatRichText} from '../components/ChatRichText'
import {ChatAttachment} from '../components/ChatAttachment'
import {base64File,mergeChatFiles,type ChatFileDraft} from '../utils/chatAttachments'
import {createPortal} from 'react-dom'
import {Button,TextInput,TextArea,SelectInput} from '../components/Primitives'
import {FormEvent,useEffect,useMemo,useRef,useState,type KeyboardEvent} from 'react'
import {ArrowRight,ArrowUp,BrainCircuit,FileText,Paperclip,Check,ChevronDown,ChevronRight,CircleHelp,Clock3,Code2,Command,Compass,Cpu,CornerDownLeft,HardDrive,Menu,Settings,List,LoaderCircle,MessageCircle,MoreHorizontal,Plus,Power,RefreshCw,ShieldAlert,ShieldCheck,Sparkles,Square,TerminalSquare,Trash2,X,Archive,RotateCcw,PanelRight} from 'lucide-react'
import {Agent,Approval,ChatMessage,ChatSession,Provider,get,post,resolveApproval,streamChat,koreanReasoningEnabled} from '../api'
import {Action,Empty,Notice,StateTag,refresh,useRemote} from '../components/UI'
import {SlashMenu,slashCandidates,slashCommandLabel,slashCommands,type SlashChoice} from '../components/SlashMenu'
import {ModelPicker} from '../components/ModelPicker'
import {ReasoningSelect,reasoningOptions} from '../components/ReasoningSelect'
import {useProviderModels} from '../hooks/useProviderModels'
import {useTranscriptScroll} from '../hooks/useTranscriptScroll'
import {useChatDraft} from '../hooks/useChatDraft'
import {useAnchoredPanel} from '../hooks/useAnchoredPanel'
import {useJob,jobFinished,type JobState} from '../hooks/useJob'
import {t,useLocale,type LocaleKey} from '../i18n'
import {translateError} from '../i18n/errors'

type ChatMode='chat'|'agent'
type LearningResult={memories:number;skill:boolean}
type LearningJob=JobState & {result?:LearningResult}
type ModelOverride={scope?:string;providerId:string;model:string}
type InstalledModel={id:string;repo:string;label:string;format:string;bytes:number;source?:string}
type Runtime={status:string;id?:string;name?:string;model?:string;error?:string}
const formatBytes=(bytes:number)=>{
 const gb=Number(bytes||0)/1024/1024/1024
 return gb>=1?gb.toFixed(1)+' GB':Math.max(0,Math.round(Number(bytes||0)/1024/1024))+' MB'
}
const isPrivateChatSession=(session:ChatSession)=>!session.roomId&&!session.channelContext&&session.kind!=='room'&&session.kind!=='channel'
export function ChatPage({sessionId,initialAgent,initialProvider,initialModel,onSession,openModels,openSettings,active=true}:{sessionId?:string;initialAgent?:string;initialProvider?:string;initialModel?:string;onSession:(x:string|undefined)=>void;openModels:()=>void;openSettings:()=>void;active?:boolean}){
 const loc=useLocale()
 const sessions=useRemote<ChatSession[]>('/api/sessions',[],active);
 const agents=useRemote<Agent[]>('/api/agents',[],active);
 const providers=useRemote<Provider[]>('/api/providers',[],active);
 const current=sessionId;
 const selectedSession=sessions.value.find(session=>session.id===current);
 const boundSession=Boolean(selectedSession&&!isPrivateChatSession(selectedSession));
 const transcript=useRemote<ChatMessage[]>(current?'/api/sessions/'+encodeURIComponent(current)+'/messages':null,[],active);
 const messages=transcript.value
 const messageUsage=useMemo(()=>messages.reduce((sum,message)=>sum+(Number(message.metadata?.usage?.totalTokens)||0),0),[messages])
 type SessionUsage={totalTokens?:number;calls?:number;promptTokens?:number;completionTokens?:number;
  cachedPromptTokensKnown?:number;newPromptTokensKnown?:number;cacheReportedCalls?:number;
  cacheUnknownCalls?:number;estimatedCalls?:number}
 const [serverUsage,setServerUsage]=useState<SessionUsage>({});
 const [showInspector,setShowInspector]=useState(false);
 const approvals=useRemote<Approval[]>('/api/approvals',[],active);
 const [input,setInput]=useChatDraft(current);
 const [fileSelection,setFileSelection]=useState<{scope?:string;files:ChatFileDraft[]}>()
 const draftFiles=fileSelection&&fileSelection.scope===current?fileSelection.files:[]
 const [historyQuery,setHistoryQuery]=useState(''),[showArchived,setShowArchived]=useState(false),[archiveBusy,setArchiveBusy]=useState('');
 const [modeChoice,setModeChoice]=useState<{scope?:string;mode:ChatMode}>()
 const mode:ChatMode=(modeChoice&&modeChoice.scope===current?modeChoice.mode:undefined)??
  (selectedSession?.mode==='agent'||(!selectedSession?.mode&&Boolean(selectedSession?.agentId))||(!current&&Boolean(initialAgent))?'agent':'chat')
 const agentMode=mode==='agent'
 const [agentChoice,setAgentChoice]=useState<{scope?:string;id:string}>();
 const [override,setOverride]=useState<ModelOverride>();
 const defaultAgent=agents.value.find(agent=>agent.id==='default')||agents.value.find(agent=>agent.name==='기본값')||agents.value.find(agent=>agent.id==='general');
 const agentId=agentMode?((agentChoice?.scope===current?agentChoice?.id:undefined)??selectedSession?.agentId??(!current?initialAgent:undefined)??defaultAgent?.id??''):'';
 const activeAgent=agents.value.find(agent=>agent.id===agentId);
 const manual=override&&override.scope===current?override:
  !current&&initialProvider?{providerId:initialProvider,model:initialModel??''}:undefined;
 const enabledProviders=providers.value.filter(provider=>provider.enabled!==false);
 const sessionProvider=enabledProviders.find(provider=>provider.id===selectedSession?.providerId);
 const providerId=manual?.providerId??(activeAgent?.providerId||sessionProvider?.id||enabledProviders[0]?.id||'');
 const selectedProvider=enabledProviders.find(provider=>provider.id===providerId);
 const model=manual?.model||((activeAgent?.providerId===providerId?activeAgent.model:'')||
  (selectedSession?.providerId===providerId&&(!agentMode||selectedSession.agentId===agentId)?selectedSession.model:'')||selectedProvider?.model||'');
 const catalogue=useProviderModels(providerId);
 const {value:models,error:modelError}=catalogue;
 const selectedModel=models.find(item=>item.id===model);
 const canDisableThinking=selectedProvider?.kind!=='codex'||reasoningOptions(selectedModel,true).includes('none');
 const setRoute=(providerId:string,model:string)=>setOverride({scope:current,providerId,model});
 useEffect(()=>{if(!current){setOverride(undefined);setAgentChoice(undefined);setModeChoice(undefined)}},[initialAgent,initialProvider,initialModel]);
 const [turnSession,setTurnSession]=useState<string>();
 // Display the user's message as soon as Send is pressed. The server's
 // authoritative owner row replaces it once the transcript is reloaded.
 const [pendingOwner,setPendingOwner]=useState<{scope?:string;text:string;priorUserId?:string;failed?:boolean}>();
 const [savedMessageId,setSavedMessageId]=useState<string>();
 const [approvalBusy,setApprovalBusy]=useState(false);
 const [liveApproval,setLiveApproval]=useState<Approval|null>(null);
 const [learningJob,setLearningJob]=useState<{id:string;scope:string}>();
 const job=useJob<LearningJob>(learningJob?'/api/jobs/'+encodeURIComponent(learningJob.id):null);
 const approval=(liveApproval?.sessionId===current?liveApproval:null)||
  approvals.value.find(item=>item.sessionId===current&&item.status==='pending');
 const [error,setError]=useState(''),[busy,setBusy]=useState(false),[live,setLive]=useState(''),[streamInterrupted,setStreamInterrupted]=useState(false),[thought,setThought]=useState(''),[thinking,setThinking]=useState(false),[reasoningKo,setReasoningKo]=useState(koreanReasoningEnabled),[showReasoning,setShowReasoning]=useState(true),[showSettings,setShowSettings]=useState(false),[toolsEnabled,setToolsEnabled]=useState(true),[thinkingEnabled,setThinkingEnabled]=useState(true),[reasoningEffort,setReasoningEffort]=useState(''),[autoLearn,setAutoLearn]=useState(false),[showHistory,setShowHistory]=useState(()=>{try{if(window.innerWidth<768)return false;return localStorage.getItem('dominion.chat.sidebar.focused-v2')==='1'}catch{return true}}),[toolStatus,setToolStatus]=useState('');
 useEffect(()=>{setReasoningEffort('');setThinkingEnabled(true)},[providerId,model,agentId,current]);
 const [permissionOpen,setPermissionOpen]=useState(false);
 const [modelMenu,setModelMenu]=useState(false),[installed,setInstalled]=useState<InstalledModel[]>([]),[modelsLoading,setModelsLoading]=useState(false),[runtime,setRuntime]=useState<Runtime>({status:'idle'}),[modelBusy,setModelBusy]=useState(''),[unloading,setUnloading]=useState(false),[learning,setLearning]=useState(false),[menuError,setMenuError]=useState('');
 const [slashOpen,setSlashOpen]=useState(false),[slashActive,setSlashActive]=useState(0),[commandInfo,setCommandInfo]=useState('');
 const inputRef=useRef<HTMLTextAreaElement>(null);
 const fileInputRef=useRef<HTMLInputElement>(null);
 const chooseFiles=(picked:FileList|null)=>{
  if(!picked?.length)return
  try{
   const files=mergeChatFiles(draftFiles,Array.from(picked))
   setFileSelection({scope:current,files});setError('')
  }catch(e){setError((e as Error).message)}
  if(fileInputRef.current)fileInputRef.current.value=''
 }
 useEffect(()=>{
  const el=inputRef.current
  if(!el)return
  el.style.height='auto'
  el.style.height=Math.min(el.scrollHeight,180)+'px'
 },[current]);
 const stream=useRef<AbortController|null>(null);
 const activeTurnSession=useRef<string|undefined>(undefined);
 const stoppingRef=useRef(false);
 const [stopping,setStopping]=useState(false);
 const view=useRef(current);
 view.current=current;
 const viewingTurn=turnSession===current&&(!savedMessageId||!messages.some(message=>message.id===savedMessageId));
 const lastOwner=messages.filter(message=>message.role==='user').at(-1);
 const pendingVisible=Boolean(pendingOwner&&pendingOwner.scope===current
  && (!lastOwner||lastOwner.id===pendingOwner.priorUserId||lastOwner.content!==pendingOwner.text));
 const historyEntries=useMemo(()=>{
  const query=historyQuery.trim().toLocaleLowerCase()
  const entries=sessions.value.filter(isPrivateChatSession)
   .filter(session=>showArchived?session.archived===true:session.archived!==true)
   .filter(session=>!query||session.title.toLocaleLowerCase().includes(query))
  const lastActivity=(session:ChatSession)=>Date.parse(session.updatedAt||session.createdAt||'')||0
  return entries.sort((left,right)=>lastActivity(right)-lastActivity(left))
 },[sessions.value,historyQuery,showArchived])
 const slashItems=slashOpen?slashCandidates(input,agents.value,models):[];
 const transcriptRevision=JSON.stringify([messages.at(-1)?.id,messages.length,pendingVisible?pendingOwner?.text:'',viewingTurn?live.length:0,viewingTurn?thought.length:0,viewingTurn?toolStatus:'',approval?.id]);
 const usageRevision=JSON.stringify([messages.at(-1)?.id,messages.length]);
 useEffect(()=>{if(!current){setServerUsage({});return}let alive=true;get<SessionUsage>('/api/sessions/'+encodeURIComponent(current)+'/usage').then(usage=>{if(alive)setServerUsage(usage)}).catch(()=>{});return()=>{alive=false}},[current,usageRevision])
 const sessionTokens=Number(serverUsage.totalTokens)||messageUsage
 const sessionCalls=Number(serverUsage.calls)||0
 const usageDetails=[
  t('usage.calls',{count:sessionCalls.toLocaleString()}),
  t('usage.input',{count:(Number(serverUsage.promptTokens)||0).toLocaleString()}),
  t('usage.output',{count:(Number(serverUsage.completionTokens)||0).toLocaleString()}),
  t('usage.cacheKnown',{count:(Number(serverUsage.cachedPromptTokensKnown)||0).toLocaleString(),
   calls:(Number(serverUsage.cacheReportedCalls)||0).toLocaleString()}),
  ...(Number(serverUsage.cacheUnknownCalls)>0?[
   t('usage.cacheUnknownCalls',{count:Number(serverUsage.cacheUnknownCalls).toLocaleString()})
  ]:[]),
 ].join(' · ')
 const {scrollRef,onScroll,jumpDown}=useTranscriptScroll(current,transcriptRevision);
 const modelPanel=useAnchoredPanel(modelMenu,()=>setModelMenu(false),380);
 useEffect(()=>()=>stream.current?.abort(),[]);
 useEffect(()=>{if(window.innerWidth<768)return;try{localStorage.setItem('dominion.chat.sidebar.focused-v2',showHistory?'1':'0')}catch{}},[showHistory]);
 // Chat stays mounted behind the settings overlay. Resize from a desktop
 // session to phone width without leaving the entire chat obscured by its
 // previously open desktop history drawer.
 useEffect(()=>{
  const media=window.matchMedia('(max-width:767px)')
  const sync=(narrow:boolean)=>{if(narrow)setShowHistory(false)}
  sync(media.matches)
  const changed=(event:MediaQueryListEvent)=>sync(event.matches)
  media.addEventListener('change',changed)
  return()=>media.removeEventListener('change',changed)
 },[])
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
  if(!current||learning||boundSession)return;
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
 const chooseAgent=(id:string)=>{setModeChoice({scope:current,mode:'agent'});setAgentChoice({scope:current,id});setOverride(undefined)};
 const pickSession=(s:ChatSession)=>{
  onSession(s.id);setAgentChoice(undefined);setOverride(undefined);setModeChoice(undefined);
  setError('');setCommandInfo('');setSlashOpen(false);
  if(window.innerWidth<768)setShowHistory(false);
 };
 const toggleArchive=async(session:ChatSession)=>{
  if(archiveBusy)return
  setArchiveBusy(session.id);setError('');setCommandInfo('')
  try{
   await post('/api/sessions/'+encodeURIComponent(session.id)+'/archive',{archived:!session.archived})
   await sessions.reload()
   setCommandInfo(t(session.archived?'chat.sessionRestored':'chat.sessionArchived'))
  }catch(e){setError((e as Error).message)}finally{setArchiveBusy('')}
 };
 const resetLocal=()=>{
  setFileSelection(undefined)
  // Keep the old conversation's unsent draft. A second New action on an
  // already-new conversation intentionally clears that new draft.
  if(!current)setInput('');
  onSession(undefined);setAgentChoice(undefined);setOverride(undefined);setModeChoice(undefined);
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
  const controller=stream.current;
  if(!id){
   if(controller){
    controller.abort();stream.current=null;
    setBusy(false);setLive('');setThought('');setToolStatus('');setTurnSession(undefined);
    setPendingOwner(undefined);setCommandInfo(t('chat.stopped'));
   }else setError(t('chat.cmd.noSessionStop'));
   return;
  }
  stoppingRef.current=true;setStopping(true);setError('');
  try{
   const outcome=await post<{cancelled:boolean;stopped:boolean}>('/api/sessions/'+encodeURIComponent(id)+'/cancel',{});
   if(!outcome.stopped)throw Error(t('chat.stopPending'));
   controller?.abort();
   if(stream.current===controller){
    stream.current=null;activeTurnSession.current=undefined;
    setBusy(false);setLive('');setThought('');setLiveApproval(null);
    setToolStatus('');setTurnSession(undefined);setSavedMessageId(undefined);
   }
   refresh('/api/sessions','/api/approvals','/api/sessions/'+encodeURIComponent(id)+'/messages','/api/sessions/'+encodeURIComponent(id)+'/usage');
   setCommandInfo(outcome.cancelled||controller?t('chat.stopped'):t('chat.cmd.nothingRunning'));
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
  e?.preventDefault();const text=input.trim()||(draftFiles.length?t('chat.fileOnlyPrompt'):'');if(!text)return;
  if(text.startsWith('/')){
   if(draftFiles.length){setError(t('chat.fileSlashError'));return}
   if(busy&&text!=='/stop')return;
   await executeCommand(text);return;
  }
  if(stream.current||stoppingRef.current||boundSession||(current&&sessions.loading))return;
  if(providerId&&!selectedProvider){setError(t('chat.err.agentApi'));return;}
  if(!providerId){setError(t('chat.err.connectApi'));return;}
  if(!model){setError(t('chat.err.pickModel'));return;}
  const controller=new AbortController();
  stream.current=controller;
  activeTurnSession.current=current;
  setTurnSession(current);setSavedMessageId(undefined);
  setPendingOwner({scope:current,text,priorUserId:messages.filter(message=>message.role==='user').at(-1)?.id});
  setBusy(true);setError('');setCommandInfo('');setLive('');setThought('');setStreamInterrupted(false);setSlashOpen(false);setToolStatus('');
  jumpDown();
  let id=current;
  let saved=false;
  let unsavedFailure=false;
  try{
   if(!id){
    const session=await post<ChatSession>('/api/sessions',{title:t('chat.sessionTitle'),mode,agentId:agentMode?agentId:null,providerId:agentMode?manual?.providerId:providerId,model:agentMode?manual?.model:model});
    if(controller.signal.aborted)return;
    id=session.id;
    activeTurnSession.current=id;
    if(view.current===current){
     onSession(id);setTurnSession(id);
     setFileSelection(previous=>previous&&previous.scope===current?{...previous,scope:id}:previous);
     setPendingOwner(previous=>previous&&previous.scope===current?{...previous,scope:id}:previous);
     setModeChoice({scope:id,mode});
     if(agentMode&&agentId)setAgentChoice({id:agentId,scope:id});
     if(manual)setOverride({...manual,scope:id});
    }
    void sessions.reload();
   }
   const attachmentIds:string[]=[]
   for(const draft of draftFiles){
    if(controller.signal.aborted)return
    let fileId=draft.uploadedSession===id?draft.uploadedId:undefined
    if(!fileId){
     const data=await base64File(draft.file)
     if(controller.signal.aborted)return
     const uploaded=await post<{id:string}>('/api/attachments',{
      sessionId:id,name:draft.file.name,mimeType:draft.file.type||'application/octet-stream',
      data
     })
     if(controller.signal.aborted)return
     fileId=uploaded.id
     if(!fileId)throw Error(t('chat.fileUploadFailed'))
     const savedFileId=fileId
     setFileSelection(previous=>previous&&previous.scope===id?{
      ...previous,files:previous.files.map(entry=>entry.file===draft.file?
       {...entry,uploadedId:savedFileId,uploadedSession:id}:entry)
     }:previous)
    }
    attachmentIds.push(fileId)
   }
   if(controller.signal.aborted)return
   setInput(previous=>previous.trim()===text?'':previous);
   if(inputRef.current)inputRef.current.style.height='';
   await streamChat({
    sessionId:id,mode,agentId:agentMode?agentId:'',providerId:agentMode?manual?.providerId:providerId,model:agentMode?manual?.model:model,input:text,
    attachmentIds,
    toolsEnabled:agentMode&&toolsEnabled,toolsExecutionEnabled:agentMode&&toolsEnabled,autoLearning:agentMode&&autoLearn,
    reasoningLanguage:reasoningKo?'ko':'original',reasoningEffort:reasoningEffort||undefined,thinking:selectedProvider?.kind==='codex'?(reasoningEffort==='none'?false:canDisableThinking?thinkingEnabled:undefined):thinkingEnabled,
   },event=>{
    if(controller.signal.aborted||stoppingRef.current||stream.current!==controller)return;
    if(event.type==='assistant_saved'){saved=true;setSavedMessageId(event.messageId);setFileSelection(previous=>previous?.scope===id?undefined:previous);refresh('/api/sessions',...(id?['/api/sessions/'+encodeURIComponent(id)+'/messages']:[]));}
    if(event.type==='text')setLive(old=>old+event.text);
    if(event.type==='reasoning')setThought(old=>old+event.text);
    if(event.type==='approval'){setLiveApproval({id:event.id,tool:event.tool,args:event.args,sessionId:id!,status:'pending'});void approvals.reload();}
    if(event.type==='tool_start'){setLiveApproval(null);setToolStatus(t('chat.tool.running',{name:event.name}));}
    if(event.type==='tool_end')setToolStatus(t('chat.tool.done',{name:event.name}));
    if(event.type==='tool_error')setToolStatus(t('chat.tool.error',{error:translateError(event.error)}));
    if(event.type==='goal_progress')setToolStatus(t('chat.tool.goal',{event:event.event,status:event.status}));
    if(event.type==='error')throw Error(event.message);
   },controller.signal);
   // A terminal event without a saved message is not a completed answer.
   // The server may have emitted text before an unrecorded failure.
   if(!saved&&!controller.signal.aborted)throw Error('응답이 저장되지 않았습니다. 실행 기록을 확인하세요.');
  }catch(e){
   unsavedFailure=!saved&&!controller.signal.aborted;
   if(unsavedFailure)setStreamInterrupted(true);
   if(!controller.signal.aborted&&view.current===id){
    setError(translateError((e as Error).message));
    // A failed stream can follow a persisted owner message; do not fabricate
    // a successful send or automatically retry an uncertain paid request.
    if(unsavedFailure)setPendingOwner(previous=>previous&&previous.scope===id?{...previous,failed:true}:previous);
   }
  }finally{
   if(stream.current===controller){
    stream.current=null;activeTurnSession.current=undefined;
    refresh('/api/sessions',...(id?['/api/sessions/'+encodeURIComponent(id)+'/messages']:[]));
    setBusy(false);setLiveApproval(null);setToolStatus('');
    if(!saved&&!unsavedFailure){setLive('');setThought('');}
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
 const columnLayout=showHistory
  ?showInspector?'md:grid-cols-[232px_minmax(0,1fr)] xl:grid-cols-[232px_minmax(0,1fr)_300px]':'md:grid-cols-[232px_minmax(0,1fr)]'
  :showInspector?'xl:grid-cols-[minmax(0,1fr)_300px]':''
 return <div className={'relative grid h-full min-h-0 min-w-0 grid-cols-1 grid-rows-[minmax(0,1fr)] bg-canvas '+columnLayout}>
  {showHistory&&<Button type="button" aria-label={t('chat.listCollapse')} className="absolute inset-0 z-20 !h-auto !w-auto cursor-default !rounded-none !border-0 !bg-slate-900/30 md:hidden" onClick={()=>setShowHistory(false)}/>}
  <aside id="chat-history" aria-label={t('ui.label.conversations')} className={'flex min-h-0 flex-col overflow-hidden border-r border-divider bg-surface '+(showHistory?'max-md:absolute max-md:inset-y-0 max-md:left-0 max-md:z-30 max-md:w-[232px] max-md:shadow-2xl':'hidden')}>
   <div className="flex items-center justify-between gap-3 px-4 pb-3 pt-2"><div className="min-w-0"><span className="block text-[10px] font-extrabold uppercase tracking-widest text-brand">{t('ui.label.conversations')}</span><strong className="mt-1 block text-sm text-ink">{t(showArchived?'chat.archived':'chat.recent')}</strong></div><Button size="icon" variant="ghost" onClick={resetLocal} title={t('chat.newChat')}><Plus size={19}/></Button></div>
   <Button className="mx-3 mb-4 inline-flex items-center justify-center gap-2 rounded-xl border border-divider bg-brand-soft px-4 py-2.5 text-xs font-bold text-brand-hover hover:bg-brand-soft" onClick={resetLocal}><Plus size={17}/> {t('chat.newChat')}</Button>
   <Button variant="ghost" size="sm" aria-pressed={showArchived} onClick={()=>setShowArchived(value=>!value)} className="mx-3 mb-3 !justify-start gap-2 !text-xs"><Archive size={14}/>{t(showArchived?'chat.showRecent':'chat.showArchived')}</Button>
   <div className="px-3 pb-3"><TextInput aria-label={t('chat.historySearch')} placeholder={t('chat.historySearch')} value={historyQuery} onChange={event=>setHistoryQuery(event.target.value)} className="w-full text-xs"/></div>
   <div className="min-h-0 flex-1 overflow-y-auto px-2">{historyEntries.map(s=>{const actionLabel=t(s.archived?'chat.restoreSession':'chat.archiveSession');const running=current===s.id&&(busy||stopping);return <div key={s.id} className={'chat-history-entry group flex items-center gap-1 rounded-xl pr-1 hover:bg-brand-soft '+(current===s.id?'bg-brand-soft':'')}><Button variant="bare" onClick={()=>pickSession(s)} title={s.title} className={'flex min-w-0 flex-1 items-center gap-3 rounded-xl px-3 py-2.5 text-left text-xs text-muted '+(current===s.id?'font-bold text-brand-hover':'')}><MessageCircle size={15} className="shrink-0"/><span className="truncate">{s.title}</span></Button><Button size="icon" variant="ghost" disabled={Boolean(archiveBusy)||running} onClick={()=>void toggleArchive(s)} aria-label={actionLabel} title={running?t('chat.archiveWait'):actionLabel} className="size-8 opacity-60 hover:opacity-100">{s.archived?<RotateCcw size={14}/>:<Archive size={14}/>}</Button></div>})}{!historyEntries.length&&<div className="p-5 text-center text-xs text-muted">{historyQuery.trim()?t('chat.historyNoResults'):t(showArchived?'chat.noArchived':'chat.first')}</div>}</div>
   <div className="shrink-0 border-t border-divider p-2"><Button variant="ghost" onClick={openSettings} className="w-full !justify-start gap-2 !text-xs" aria-label={t('nav.settings')}><Settings size={17}/>{t('nav.settings')}</Button></div>
  </aside>
  <div className="flex min-h-0 min-w-0 flex-col bg-canvas">
   <header className="chat-header relative z-20 grid shrink-0 grid-cols-[2rem_2rem_minmax(0,1fr)_auto] grid-rows-[2.5rem_2.25rem] items-center gap-x-1 border-b border-divider bg-surface px-2 py-1 sm:flex sm:h-14 sm:gap-2 sm:px-4">
    <Button size="icon" variant="ghost" onClick={()=>setShowHistory(x=>!x)} title={showHistory?t('chat.listCollapse'):t('chat.listExpand')} aria-label={showHistory?t('chat.listCollapse'):t('chat.listExpand')} aria-expanded={showHistory} aria-controls="chat-history"><Menu size={18}/></Button>
    <Button size="icon" variant="ghost" className="chat-new-session" onClick={resetLocal} title={t('chat.newChat')} aria-label={t('chat.newChat')}><Plus size={18}/></Button>
    <div className="col-span-4 row-start-2 flex min-w-0 items-center gap-1.5 border-t border-divider sm:flex-1 sm:border-0">
     <div className="chat-model-controls relative flex min-w-0 items-center gap-1">
      <SelectInput aria-label={t('chat.selectProvider')} title={selectedProvider?.name||t('chat.autoProvider')} value={manual?.providerId??''} onChange={e=>{if(e.target.value)setRoute(e.target.value,'');else{setOverride(undefined);if(initialProvider)onSession(undefined)}}} className="!w-[124px] !pl-1.5 !border-0 !bg-transparent !py-1 !pr-6 !text-[12px]"><option value="">{t('chat.autoProvider')}</option>{providers.value.filter(p=>p.enabled).map(p=><option value={p.id} key={p.id}>{p.name}</option>)}</SelectInput>
      <span className="text-[#B8AFA2]">/</span>
      <ModelPicker providerId={providerId} value={model} catalogue={catalogue} onChange={value=>setRoute(providerId,value)} placeholder={providerId?t('chat.modelPick'):t('chat.connectApi')} className="!h-7 !w-[220px] !border-0 !bg-transparent !py-1 !pl-1.5 !pr-2 !text-[11px] !font-semibold" ariaLabel={t('chat.modelPick')}/>
      <Button variant="ghost" size="sm" ref={modelPanel.trigger} onClick={openModelMenu} title={t('chat.localModel')} className="ml-auto !hidden sm:!inline-flex !border !border-divider !bg-white !text-muted hover:!bg-canvas" aria-label={t('chat.localModel')} aria-expanded={modelMenu}><HardDrive size={14}/><span className="hidden lg:inline">{t('chat.localModel')}</span><ChevronDown size={12}/></Button>
      <Button variant="ghost" size="sm" title={t('chat.unload')} aria-label={t('chat.unload')} onClick={()=>void unloadModel()} disabled={unloading||runtime.status!=='ready'} className="!hidden sm:!inline-flex !border !border-divider !bg-white !text-muted hover:!bg-canvas disabled:opacity-40"><Power size={14}/><span className="hidden xl:inline">{unloading?t('chat.unloading'):t('chat.unload')}</span></Button>
      {modelMenu&&modelPanel.position&&createPortal(
       <div ref={modelPanel.panel} style={modelPanel.position} className="z-[80] overflow-y-auto overscroll-contain rounded-2xl border border-divider bg-surface shadow-[0_18px_50px_rgba(36,33,29,.14)]">
        <div className="flex items-center justify-between gap-2 border-b border-divider px-3 py-2.5"><div className="min-w-0"><strong className="text-[12px] text-ink">{t('chat.localModel')}</strong><p className="truncate text-[10px] text-muted">{runtime.status==='ready'?t('chat.running')+' · '+(runtime.name||''):runtime.status==='loading'?t('chat.loadingModel'):t('chat.noLocal')}</p></div><Button size="icon" variant="ghost" title={t('models.refresh')} onClick={()=>void loadModelMenu()}><RefreshCw size={15}/></Button></div>
        {menuError&&<div className="border-b border-rose-100 bg-rose-50 px-3 py-2 text-[11px] text-rose-700">{menuError}</div>}
        <div className="max-h-[320px] overflow-y-auto p-1.5">{modelsLoading&&<div className="flex items-center justify-center gap-2 p-5 text-[11px] text-muted"><LoaderCircle size={14} className="animate-spin"/> {t('chat.loadingModel')}</div>}{!modelsLoading&&installed.map(m=><Button type="button" variant="ghost" key={m.id} disabled={Boolean(modelBusy)} onClick={()=>void switchLocalModel(m)} className={'flex w-full items-center gap-3 !justify-start !border-0 rounded-lg px-2.5 py-2.5 text-left hover:bg-canvas disabled:opacity-60 '+(runtime.status==='ready'&&runtime.id===m.id?'bg-emerald-50/60':'')}>
         <span className="grid size-8 shrink-0 place-items-center rounded-lg bg-canvas text-muted"><HardDrive size={15}/></span>
         <span className="min-w-0 flex-1"><strong className="block truncate text-[12px] text-ink">{m.repo}</strong><small className="text-[10px] text-muted">{m.label} · {m.format.toUpperCase()} · {formatBytes(m.bytes)}</small></span>
         {runtime.status==='ready'&&runtime.id===m.id?<span className="shrink-0 rounded bg-emerald-100 px-2 py-1 text-[10px] font-semibold text-emerald-700">{t('chat.inUse')}</span>:modelBusy===m.id?<span className="shrink-0 text-[10px] font-semibold text-brand">{t('chat.loadingShort')}</span>:<span className="shrink-0 text-[10px] font-semibold text-brand">{t('chat.runShort')}</span>}
        </Button>)}{!modelsLoading&&!installed.length&&<div className="p-5 text-center text-[11px] text-muted">{t('chat.noInstalled')}</div>}</div>
        <Button type="button" variant="ghost" onClick={()=>{setModelMenu(false);openModels()}} className="flex w-full items-center justify-between !rounded-none border-t border-divider px-3 py-2.5 text-[11px] font-semibold text-muted hover:bg-canvas">{t('chat.openModels')} <ArrowRight size={13}/></Button>
       </div>
      ,document.body)}
     </div>
    </div>
    <div className="relative z-[2] col-start-4 row-start-1 flex shrink-0 items-center gap-0.5 sm:gap-1">
     {sessionTokens>0&&<span title={usageDetails} className="hidden rounded-lg border border-divider bg-canvas px-2 py-1 text-[10px] font-semibold text-[#8A775F] sm:inline-flex">{t('usage.sessionTotal',{count:sessionTokens.toLocaleString()})}{sessionCalls>0&&<span className="ml-1">{' · '}{t('usage.calls',{count:sessionCalls.toLocaleString()})}</span>}</span>}

     
     <Button size="icon" variant="ghost" aria-expanded={showInspector} aria-controls="chat-session-inspector"
      title={loc==='en'?'Session details':'대화 정보 패널'} aria-label={loc==='en'?'Session details':'대화 정보 패널'}
      onClick={()=>setShowInspector(value=>!value)}><PanelRight size={18}/></Button>
     <Button size="icon" variant="ghost" className="!hidden sm:!inline-flex" title={t('chat.modelManage')} aria-label={t('chat.modelManage')} onClick={openModels}><Cpu size={18}/></Button>
     <Button size="icon" variant="ghost" className="conversation-settings !hidden sm:!inline-flex" title={t('nav.settings')} aria-label={t('nav.settings')} onClick={openSettings}><Settings size={18}/></Button>
    </div>
   </header>
   <div ref={scrollRef} onScroll={onScroll} data-chat-scroll className="min-h-0 flex-1 overflow-y-auto overscroll-contain">{!messages.length&&!(busy&&viewingTurn)?<div className="mx-auto flex min-h-full max-w-xl flex-col justify-center px-5 py-8">
    <div className="chat-welcome my-auto flex w-full flex-col items-start gap-2 py-5 text-ink">
     <span className="chat-welcome-mark mb-3 grid size-11 place-items-center rounded-xl border border-divider bg-surface text-muted" aria-hidden="true"><Sparkles size={22}/></span>
     <h1 className="text-2xl font-semibold tracking-tight text-ink">{t('chat.newChatHeading')}</h1>
     <p className="text-sm text-muted">{t('chat.newChatDesc')}</p>
     <details className="chat-welcome-examples mt-5 w-full border-t border-divider pt-3">
      <summary className="flex cursor-pointer items-center gap-2 py-2 text-xs text-muted">{t('chat.examples')} <ChevronDown size={14}/></summary>
      <div className="grid gap-1">{[t('chat.example1'),t('chat.example2'),t('chat.example3')].map(example=><Button type="button" key={example} onClick={()=>{setInput(example);inputRef.current?.focus({preventScroll:true})}} className="chat-welcome-example flex w-full items-center justify-between rounded-lg border border-transparent px-3 py-2 text-left text-xs text-muted hover:border-divider hover:bg-surface">{example}<ArrowRight size={14}/></Button>)}</div>
     </details>
    </div>
    {!model&&<Button type="button" onClick={openModels} className="mt-4 inline-flex items-center gap-2 self-start text-[12px] font-semibold text-brand"><Compass size={15}/> {t('chat.connectFirst')} <ArrowRight size={14}/></Button>}
   </div>:<div className="chat-transcript mx-auto flex min-h-full w-full max-w-[920px] flex-col gap-7 px-4 py-8 sm:px-7 md:px-9">{messages.filter(m=>m.role!=='tool').map(m=><ChatMessageRow key={m.id} message={m} agentName={agents.value.find(a=>a.id===m.agent_id)?.name} showReasoning={showReasoning} />)}{pendingVisible&&<div className="chat-message chat-message--user flex min-w-0 items-start justify-end gap-3" data-message data-role="user" data-pending="true"><div className="chat-message-inner relative min-w-0"><div className="chat-message-text chat-message-text--user min-w-0 whitespace-pre-wrap break-words">{pendingOwner?.text}</div>{pendingOwner?.failed&&<div role="status" className="mt-1 text-right text-xs text-amber-700">전송 상태를 확인하세요. 자동으로 재전송하지 않았습니다.</div>}</div></div>}{viewingTurn&&(live||thought||busy)&&<div className="chat-message chat-message--assistant flex min-w-0 items-start gap-3"><div className="grid size-9 shrink-0 place-items-center rounded-xl bg-canvas text-ink ai"><Sparkles size={17}/></div><div className="chat-message-inner relative min-w-0 flex-1"><div className="chat-message-label mb-2 flex items-center gap-2 font-bold text-ink">DOMINION <span>{streamInterrupted?'미완료 · 서버 저장 확인 필요':t('ui.label.live')}</span>{busy&&thought&&!showReasoning&&<span className="inline-flex items-center gap-1 rounded bg-canvas px-1.5 py-0.5 text-[10px] font-medium text-muted"><BrainCircuit size={11}/> {t('chat.thinkingNow')}</span>}</div>{showReasoning&&thought&&<details className="mb-3 max-h-56 overflow-y-auto rounded-xl border border-divider bg-canvas p-3 text-[11px] leading-5 text-muted" open={thinking}><summary className="cursor-pointer select-none" onClick={e=>{e.preventDefault();setThinking(x=>!x)}}><BrainCircuit size={14} className="inline"/> {t('chat.reasoningProcess',{count:thought.length.toLocaleString()} )} <ChevronRight size={13} className="inline"/></summary><div className="mt-2 whitespace-pre-wrap">{thought}</div></details>}{live&&<div className="chat-message-text chat-message-text--assistant min-w-0 break-words text-ink"><ChatRichText content={live}/></div>}{busy&&!live&&<div className="flex items-center gap-2 text-xs text-muted"><i/><i/><i/></div>}</div></div>}{viewingTurn&&toolStatus&&<div className="inline-flex items-center gap-2 rounded-lg bg-canvas px-3 py-1.5 text-xs text-muted"><TerminalSquare size={15}/>{toolStatus}</div>}</div>}</div>
   <div className="conversation-composer-wrap sticky bottom-0 z-10"><div className="mx-auto w-full max-w-[860px]">{approval&&<div className="mb-3 flex flex-wrap items-center justify-between gap-3 rounded-xl border border-amber-200 bg-amber-50 p-3 text-xs text-amber-900"><div><ShieldAlert size={20}/><span><strong>{t('chat.approval')}</strong><small>{approval.tool} · {JSON.stringify(approval.args??{}).slice(0,150)}</small></span></div><div><Action variant="secondary" disabled={approvalBusy} onClick={()=>approve(false)}>{t('chat.deny')}</Action><Action disabled={approvalBusy} onClick={()=>approve(true)}><Check size={15}/> {t('chat.approve')}</Action></div></div>}
    {(error||transcript.error||job.error)&&<div className="mb-3 rounded-xl border border-rose-200 bg-rose-50 p-3 text-xs text-rose-700"><CircleHelp size={16}/>{error||transcript.error||translateError(job.error||'')}<Button onClick={()=>setError('')}><X size={14}/></Button></div>}
    {modelError&&!model&&<div className="mb-3 text-xs text-muted">{t('chat.noModel')}: {modelError} <Button onClick={openModels}>{t('chat.manage')} <ArrowRight size={13}/></Button></div>}
    {commandInfo&&<div role="status" className="mb-3 max-h-36 overflow-auto whitespace-pre-wrap rounded-xl border border-divider bg-brand-soft p-3 text-xs leading-6 text-ink">{commandInfo}</div>}
    <div className="relative">
    {slashOpen&&<SlashMenu items={slashItems} active={slashActive} onChoose={chooseSlash}/>}
    <form onSubmit={send} className="conversation-composer">
     <TextArea ref={inputRef} rows={1} placeholder={t("chat.inputPlaceholder")} value={input} onChange={e=>{const next=e.target.value;setInput(next);setSlashOpen(next.startsWith('/')&&!next.includes('\n'));setSlashActive(0);const el=e.currentTarget;el.style.height='auto';el.style.height=Math.min(el.scrollHeight,180)+'px'}} onKeyDown={appendKey} aria-controls="dominion-slash-menu" aria-expanded={slashOpen} className="conversation-composer-input"/>
     {draftFiles.length>0&&<div className="flex max-h-28 flex-wrap gap-1.5 overflow-auto px-3 pb-2">
      {draftFiles.map((entry,index)=><div key={entry.file.name+'-'+entry.file.lastModified}
       className="inline-flex min-w-0 max-w-full items-center gap-1.5 rounded-lg border border-divider bg-canvas px-2 py-1 text-[11px] text-muted">
       <FileText size={13} className="shrink-0"/><span className="max-w-[190px] truncate">{entry.file.name}</span>
       <button type="button" aria-label={t('chat.removeAttachment',{name:entry.file.name})}
        disabled={busy} onClick={()=>setFileSelection(previous=>previous&&previous.scope===current?
         {...previous,files:previous.files.filter((_,i)=>i!==index)}:previous)}
        className="rounded p-0.5 hover:bg-[#EEE3D5]"><X size={12}/></button>
      </div>)}
     </div>}
     <div className="chat-composer-controls conversation-composer-controls">
      <div className="chat-composer-options conversation-composer-options">
       <label className="inline-flex h-7 shrink-0 cursor-pointer items-center gap-1 rounded-md px-1.5 text-[11px] font-semibold text-muted hover:bg-canvas" title={t('chat.attachFiles')}>
        <Paperclip size={16}/><span className="hidden sm:inline">{t('chat.attachFiles')}</span>
        <input ref={fileInputRef} type="file" multiple className="sr-only" aria-label={t('chat.attachFiles')}
         disabled={busy||stopping||Boolean(boundSession)} onChange={event=>chooseFiles(event.target.files)}/>
       </label>
       <SelectInput aria-label="대화 실행 모드" title={mode==='chat'?'모델과 직접 대화':'도구를 사용하는 에이전트 실행'}
        value={mode} disabled={busy||stopping} onChange={e=>{
         const next=e.target.value as ChatMode;setModeChoice({scope:current,mode:next});
         setPermissionOpen(false);setShowSettings(false);setError('')
        }} className="chat-mode !h-7 !w-[102px] !shrink-0 !border-0 !bg-transparent !py-1 !pl-1 !pr-5 !text-[11px] !font-semibold !text-muted">
        <option value="chat">일반 채팅</option><option value="agent">에이전트</option>
       </SelectInput>
       {agentMode&&<ExecutionPermissions active={active} open={permissionOpen} onOpenChange={open=>{setPermissionOpen(open);if(open){setShowSettings(false);setModelMenu(false)}}} onError={setError} onChanged={label=>setCommandInfo(t('chat.perm.changed',{label}))}/>}
       <ReasoningSelect value={reasoningEffort} onChange={effort=>{setReasoningEffort(effort);setThinkingEnabled(effort!=='none')}} model={selectedModel} requireCatalog={selectedProvider?.kind==='codex'} defaultLabel={activeAgent?.reasoningEffort?`에이전트 설정 (${activeAgent.reasoningEffort})`:t('chat.reasoningAuto')} className="chat-reasoning !h-7 !w-auto !border-0 !bg-transparent !py-1 !pl-1.5 !pr-6 !text-[11px] !font-semibold !text-muted"/>
       {agentMode&&<SelectInput aria-label={t('chat.agentAria')} value={agentId} onChange={e=>chooseAgent(e.target.value)} className="chat-agent !h-7 !w-auto !max-w-[140px] !border-0 !bg-transparent !py-1 !pl-1.5 !pr-6 !text-[11px] !font-semibold !text-muted">{agents.value.map(a=><option key={a.id} value={a.id}>{a.name}</option>)}</SelectInput>}
       <ConversationSettings agentMode={agentMode} open={showSettings} onOpenChange={open=>{setShowSettings(open);if(open){setPermissionOpen(false);setModelMenu(false)}}} showReasoning={showReasoning} setShowReasoning={setShowReasoning} reasoningKo={reasoningKo} setReasoningKo={setReasoningKo} thinkingEnabled={thinkingEnabled} setThinkingEnabled={setThinkingEnabled} canDisableThinking={canDisableThinking} reasoningEffort={reasoningEffort} setReasoningEffort={setReasoningEffort} toolsEnabled={agentMode&&toolsEnabled} setToolsEnabled={setToolsEnabled} autoLearn={agentMode&&autoLearn} setAutoLearn={setAutoLearn} learning={learning} learnDisabled={!agentMode||!current||Boolean(boundSession)||busy} onLearn={learnSession}/>
      </div>
      <div className="ml-auto flex items-center gap-2">
       <span className="hidden text-[10px] text-muted sm:inline">{t('chat.enterHint')}</span>
       {busy?<Button className="conversation-composer-send !bg-rose-600 hover:!bg-rose-700" type="button" onClick={()=>void stopChat()} disabled={stopping} title={stopping?t('chat.stopping'):t('chat.stop')} aria-label={stopping?t('chat.stopping'):t('chat.stop')}>{stopping?<LoaderCircle size={18} className="animate-spin"/>:<Square size={18} fill="currentColor"/>}</Button>
       :<Button className="conversation-composer-send" disabled={stopping||Boolean(boundSession)||(Boolean(current)&&sessions.loading)||(!input.trim()&&!draftFiles.length)} type="submit" title={t('chat.send')}><ArrowUp size={19}/></Button>}
      </div>
     </div>
    </form></div>
   </div></div>
  </div>
  {showInspector&&<button type="button" className="chat-inspector-scrim" aria-label={loc==='en'?'Close session details':'대화 정보 닫기'} onClick={()=>setShowInspector(false)}/>}
  {showInspector&&<ChatSessionInspector
   locale={loc} onClose={()=>setShowInspector(false)}
   title={selectedSession?.title} sessionId={current}
   agentMode={agentMode} providerName={selectedProvider?.name} model={model}
   messageCount={messages.length} toolsEnabled={toolsEnabled}
   reasoningEffort={reasoningEffort} totalTokens={sessionTokens}
   calls={sessionCalls} cachedTokens={Number(serverUsage.cachedPromptTokensKnown)||0}
   pendingTool={approval?.tool} streamInterrupted={streamInterrupted}/>}
 </div>
}