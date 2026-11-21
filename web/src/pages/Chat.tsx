import {ConversationSettings} from '../components/ConversationSettings'
import {ChatRichText} from '../components/ChatRichText'
import {usePersistentConversationToggle,usePersistentReasoningEffort,usePersistentKoreanReasoning} from '../hooks/usePersistentConversationPreference'
import {compatibleConversationEffort} from '../conversationPreferences'
import {ExecutionPermissions} from '../components/ExecutionPermissions'
import {ConversationSwitch} from '../components/WorkspaceShell'
import {TokenUsage} from '../components/TokenUsage'
import {ExecutionInsights} from '../components/ExecutionInsights'
import {createPortal} from 'react-dom'
import {Button,TextInput,TextArea,SelectInput} from '../components/Primitives'
import {FormEvent,useEffect,useLayoutEffect,useMemo,useRef,useState,type ChangeEvent,type KeyboardEvent} from 'react'
import {ArrowDown,ArrowRight,ArrowUp,BrainCircuit,Check,ChevronDown,ChevronRight,CircleHelp,Clock3,Code2,Command,Compass,Copy,Cpu,CornerDownLeft,HardDrive,Menu,Settings,List,LoaderCircle,MessageCircle,MoreHorizontal,Paperclip,Plus,Power,RefreshCw,ShieldAlert,ShieldCheck,Sparkles,Square,TerminalSquare,Trash2,X} from 'lucide-react'
import {Agent,Approval,Attachment,ChatMessage,ChatSession,Provider,get,post,resolveApproval,streamChat,uploadAttachment} from '../api'
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

type LearningResult={memories:number;skill:boolean}
type LearningJob=JobState & {result?:LearningResult}
type ModelOverride={scope?:string;providerId:string;model:string}
type InstalledModel={id:string;repo:string;label:string;format:string;bytes:number;source?:string}
type Runtime={status:string;id?:string;name?:string;model?:string;error?:string}
type PendingImage={key:string;file:File;preview:string}
type OptimisticOwner={sessionId:string|undefined;content:string;lastSeenMessageId:string|undefined}
const ATTACH_ALLOWED=['image/png','image/jpeg','image/webp','image/gif']
const ATTACH_LIMIT=8
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
 const approvals=useRemote<Approval[]>('/api/approvals',[],active);
 const [input,setInput,adoptNewSession]=useChatDraft(current);
 const [agentChoice,setAgentChoice]=useState<{scope?:string;id:string}>();
 const [chatModeOverride,setChatModeOverride]=useState<{scope?:string;mode:'chat'|'agent'}>();
 const [override,setOverride]=useState<ModelOverride>();
 const defaultAgent=agents.value.find(agent=>agent.id==='default')||agents.value.find(agent=>agent.name==='기본값')||agents.value.find(agent=>agent.id==='general');
 const agentMode=(chatModeOverride&&chatModeOverride.scope===current?chatModeOverride.mode:
  current?(selectedSession?.chatMode??(selectedSession?.agentId?'agent':'chat')):(initialAgent?'agent':'chat'))==='agent';
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
 useEffect(()=>{if(!current){setOverride(undefined);setAgentChoice(undefined)}},[initialAgent,initialProvider,initialModel]);
 const [turnSession,setTurnSession]=useState<string>();
 const [savedMessageId,setSavedMessageId]=useState<string>();
 const [optimisticOwner,setOptimisticOwner]=useState<OptimisticOwner>();
 const [approvalBusy,setApprovalBusy]=useState(false);
 const [liveApproval,setLiveApproval]=useState<Approval|null>(null);
 const [learningJob,setLearningJob]=useState<{id:string;scope:string}>();
 const job=useJob<LearningJob>(learningJob?'/api/jobs/'+encodeURIComponent(learningJob.id):null);
 const approval=(liveApproval?.sessionId===current?liveApproval:null)||
  approvals.value.find(item=>item.sessionId===current&&item.status==='pending');
 const [error,setError]=useState(''),[busy,setBusy]=useState(false),[live,setLive]=useState(''),[thought,setThought]=useState(''),[thinking,setThinking]=useState(false),[showSettings,setShowSettings]=useState(false),[showHistory,setShowHistory]=useState(()=>{try{if(window.innerWidth<768)return false;return localStorage.getItem('dominion.chat.sidebar')!=='0'}catch{return true}}),[toolStatus,setToolStatus]=useState('');
 const [reasoningKo,setReasoningKo]=usePersistentKoreanReasoning();
 const [showReasoning,setShowReasoning]=usePersistentConversationToggle('showReasoning');
 const [toolsEnabled,setToolsEnabled]=usePersistentConversationToggle('toolsEnabled');
 const [thinkingEnabled,setThinkingEnabled]=usePersistentConversationToggle('thinkingEnabled');
 const [autoLearn,setAutoLearn]=usePersistentConversationToggle('autoLearn');
 const [reasoningEffort,setReasoningEffort]=usePersistentReasoningEffort();
 // A model without the 'none' tier cannot honor the off switch. Keep the
 // owner's stored preference so switching back does not silently erase it.
 const effectiveReasoningEffort=compatibleConversationEffort(
  reasoningEffort,reasoningOptions(selectedModel,selectedProvider?.kind==='codex')
 );
 const effectiveThinkingEnabled=!canDisableThinking||thinkingEnabled;
 const [permissionOpen,setPermissionOpen]=useState(false);
 const [modelMenu,setModelMenu]=useState(false),[installed,setInstalled]=useState<InstalledModel[]>([]),[modelsLoading,setModelsLoading]=useState(false),[runtime,setRuntime]=useState<Runtime>({status:'idle'}),[modelBusy,setModelBusy]=useState(''),[unloading,setUnloading]=useState(false),[learning,setLearning]=useState(false),[menuError,setMenuError]=useState('');
 const [slashOpen,setSlashOpen]=useState(false),[slashActive,setSlashActive]=useState(0),[commandInfo,setCommandInfo]=useState('');
 const inputRef=useRef<HTMLTextAreaElement>(null);
 // Refit restored and newly typed drafts; a session switch does not emit an
 // input event and can otherwise leave multiline drafts visually clipped.
 useLayoutEffect(()=>{
  const el=inputRef.current;
  if(!el)return;
  el.style.height='auto';
  el.style.height=Math.min(el.scrollHeight,180)+'px';
 },[current,input]);
 const imageInputRef=useRef<HTMLInputElement>(null);
 const [pendingImages,setPendingImages]=useState<PendingImage[]>([]);
 const [uploading,setUploading]=useState(false);
 const pendingImagesRef=useRef<PendingImage[]>([]);
 pendingImagesRef.current=pendingImages;
 useEffect(()=>()=>{pendingImagesRef.current.forEach(item=>URL.revokeObjectURL(item.preview))},[]);
 const onPickImages=(event:ChangeEvent<HTMLInputElement>)=>{
  const files=Array.from(event.target.files??[]);
  event.target.value='';
  if(!files.length)return;
  if(files.some(file=>!ATTACH_ALLOWED.includes(file.type))){setError(t('chat.attachOnlyImages'));return}
  if(pendingImages.length+files.length>ATTACH_LIMIT){setError(t('chat.attachLimit'));return}
  setPendingImages(previous=>[...previous,...files.map(file=>({
   key:`${file.name}:${file.lastModified}:${Math.random().toString(36).slice(2)}`,
   file,preview:URL.createObjectURL(file),
  }))]);
 };
 const discardPendingImages=()=>setPendingImages(previous=>{
  previous.forEach(item=>URL.revokeObjectURL(item.preview));
  return [];
 });
 const removeImage=(key:string)=>{
  setPendingImages(previous=>{
   const target=previous.find(item=>item.key===key);
   if(target)URL.revokeObjectURL(target.preview);
   return previous.filter(item=>item.key!==key);
  });
 };
 const stream=useRef<AbortController|null>(null);
 const streamBuffer=useRef({text:'',reasoning:'',frame:0});
 const clearStreamBuffer=()=>{
  const pending=streamBuffer.current;
  if(pending.frame)cancelAnimationFrame(pending.frame);
  pending.frame=0;pending.text='';pending.reasoning='';
 };
 const flushStreamBuffer=()=>{
  const pending=streamBuffer.current;
  if(pending.frame)cancelAnimationFrame(pending.frame);
  pending.frame=0;
  const text=pending.text,reasoning=pending.reasoning;
  pending.text='';pending.reasoning='';
  if(text)setLive(previous=>previous+text);
  if(reasoning)setThought(previous=>previous+reasoning);
 };
 const queueStreamDelta=(kind:'text'|'reasoning',delta:string)=>{
  if(!delta)return;
  const pending=streamBuffer.current;
  pending[kind]+=delta;
  if(!pending.frame)pending.frame=requestAnimationFrame(flushStreamBuffer);
 };
 const activeTurnSession=useRef<string|undefined>(undefined);
 const stoppingRef=useRef(false);
 const [stopping,setStopping]=useState(false);
 // Pending attachments must never follow a sidebar/browser-history switch to
 // another conversation. The first send adopting a new session is not a switch.
 const attachmentScope=useRef(current);
 useLayoutEffect(()=>{
  if(attachmentScope.current===current)return;
  const previous=attachmentScope.current;
  attachmentScope.current=current;
  if(previous===undefined&&activeTurnSession.current===current)return;
  discardPendingImages();
 },[current]);
 const view=useRef(current);
 view.current=current;
 const viewingTurn=turnSession===current&&(!savedMessageId||!messages.some(message=>message.id===savedMessageId));
 const lastSeenOwnerIndex=optimisticOwner?.lastSeenMessageId
  ?messages.findIndex(message=>message.id===optimisticOwner.lastSeenMessageId):-1;
 const ownerReceived=Boolean(optimisticOwner&&messages.some((message,index)=>
  index>lastSeenOwnerIndex&&message.role==='user'&&message.content===optimisticOwner.content));
 const showOptimisticOwner=Boolean(optimisticOwner?.sessionId===current&&!ownerReceived&&(busy||savedMessageId));
 const slashItems=slashOpen?slashCandidates(input,agents.value,models):[];
 const transcriptRevision=JSON.stringify([messages.at(-1)?.id,messages.length,showOptimisticOwner,busy&&viewingTurn,viewingTurn?live.length:0,viewingTurn?thought.length:0,viewingTurn?toolStatus:'',approval?.id]);
 const usageRevision=JSON.stringify([messages.at(-1)?.id,messages.length]);
 useEffect(()=>{if(!current){setServerUsage(0);return}let alive=true;get<{totalTokens?:number}>('/api/sessions/'+encodeURIComponent(current)+'/usage').then(usage=>{if(alive)setServerUsage(Number(usage.totalTokens)||0)}).catch(()=>{});return()=>{alive=false}},[current,usageRevision])
 const [moreBelow,setMoreBelow]=useState(false);
 const {scrollRef,onScroll,jumpDown}=useTranscriptScroll(current,transcriptRevision,setMoreBelow);
 const modelPanel=useAnchoredPanel(modelMenu,()=>setModelMenu(false),380);
 useEffect(()=>()=>{stream.current?.abort();clearStreamBuffer()},[]);
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
 const chooseAgent=(id:string)=>{setAgentChoice({scope:current,id});setChatModeOverride({scope:current,mode:'agent'});setOverride(undefined)};
 const pickSession=(s:ChatSession)=>{
  onSession(s.id);setAgentChoice(undefined);setChatModeOverride(undefined);setOverride(undefined);
  setError('');setCommandInfo('');setSlashOpen(false);
  if(window.innerWidth<768)setShowHistory(false);
 };
 const resetLocal=()=>{
  onSession(undefined);setAgentChoice(undefined);setChatModeOverride(undefined);setOverride(undefined);
  discardPendingImages();
  if(!current)setInput('');
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
    controller.abort();stream.current=null;clearStreamBuffer();
    setBusy(false);setLive('');setThought('');setToolStatus('');setTurnSession(undefined);
    setCommandInfo(t('chat.stopped'));
   }else setError(t('chat.cmd.noSessionStop'));
   return;
  }
  stoppingRef.current=true;setStopping(true);setError('');
  try{
   const outcome=await post<{cancelled:boolean;stopped:boolean}>('/api/sessions/'+encodeURIComponent(id)+'/cancel',{});
   if(!outcome.stopped)throw Error(t('chat.stopPending'));
   controller?.abort();clearStreamBuffer();
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
  e?.preventDefault();const text=input.trim();
  if(!text&&!pendingImages.length)return;
  if(text.startsWith('/')){
   if(busy&&text!=='/stop')return;
   await executeCommand(text);return;
  }
  if(stream.current||stoppingRef.current||selectedSession?.roomId||(current&&sessions.loading))return;
  if(providerId&&!selectedProvider){setError(t('chat.err.agentApi'));return;}
  if(!providerId){setError(t('chat.err.connectApi'));return;}
  if(!model){setError(t('chat.err.pickModel'));return;}
  const controller=new AbortController();
  clearStreamBuffer();
  stream.current=controller;
  activeTurnSession.current=current;
  setTurnSession(current);setSavedMessageId(undefined);
  setOptimisticOwner({sessionId:current,content:text,lastSeenMessageId:messages.at(-1)?.id});
  setBusy(true);setError('');setCommandInfo('');setLive('');setThought('');setSlashOpen(false);setToolStatus('');
  jumpDown();
  let id=current;
  let streamStarted=false;
  try{
   if(!id){
    const session=await post<ChatSession>('/api/sessions',{title:t('chat.sessionTitle'),agentId:agentMode?agentId:undefined,
     providerId:agentMode?manual?.providerId:providerId,model:agentMode?manual?.model:model});
    if(controller.signal.aborted)return;
    id=session.id;
    activeTurnSession.current=id;
    if(view.current===current){
     adoptNewSession(id,text);
     onSession(id);setTurnSession(id);
     setOptimisticOwner(previous=>previous?.content===text?{...previous,sessionId:id}:previous);
     setChatModeOverride({scope:id,mode:agentMode?'agent':'chat'});
     if(agentMode&&agentId)setAgentChoice({id:agentId,scope:id});
     if(manual)setOverride({...manual,scope:id});
    }
    void sessions.reload();
   }
   // A background turn must never clear the draft of another selected chat.
   if(view.current===current||view.current===id){
    setInput(previous=>previous.trim()===text?'':previous);
   }
   if(inputRef.current)inputRef.current.style.height='';
   let attachmentIds:string[]|undefined;
   if(pendingImages.length){
    setUploading(true);
    try{
     const uploaded=await Promise.all(pendingImages.map(item=>uploadAttachment({sessionId:id!},item.file)));
     if(controller.signal.aborted)return;
     attachmentIds=uploaded.map(item=>item.id);
     pendingImages.forEach(item=>URL.revokeObjectURL(item.preview));
     setPendingImages([]);
    }finally{
     setUploading(false);
    }
   }
   streamStarted=true;
   await streamChat({
    sessionId:id,agentId:agentMode?agentId:undefined,directChat:!agentMode,
    providerId:agentMode?manual?.providerId:providerId,model:agentMode?manual?.model:model,input:text,
    toolsEnabled:agentMode&&toolsEnabled,toolsExecutionEnabled:agentMode&&toolsEnabled,autoLearning:agentMode&&autoLearn,attachmentIds,
    reasoningLanguage:reasoningKo?'ko':'original',reasoningEffort:effectiveReasoningEffort||undefined,thinking:selectedProvider?.kind==='codex'?(effectiveReasoningEffort==='none'?false:canDisableThinking?effectiveThinkingEnabled:undefined):effectiveThinkingEnabled,
   },event=>{
    if(controller.signal.aborted||stoppingRef.current||stream.current!==controller)return;
    if(event.type==='assistant_saved'){flushStreamBuffer();setSavedMessageId(event.messageId);refresh('/api/sessions',...(id?['/api/sessions/'+encodeURIComponent(id)+'/messages']:[]));}
    if(event.type==='text')queueStreamDelta('text',event.text);
    if(event.type==='reasoning')queueStreamDelta('reasoning',event.text);
    if(event.type==='approval'){setLiveApproval({id:event.id,tool:event.tool,args:event.args,sessionId:id!,status:'pending'});void approvals.reload();}
    if(event.type==='tool_start'){setLiveApproval(null);setToolStatus(t('chat.tool.running',{name:event.name}));}
    if(event.type==='tool_end')setToolStatus(t('chat.tool.done',{name:event.name}));
    if(event.type==='tool_error')setToolStatus(t('chat.tool.error',{error:translateError(event.error)}));
    if(event.type==='goal_progress')setToolStatus(t('chat.tool.goal',{event:event.event,status:event.status}));
    if(event.type==='error')throw Error(event.message);
   },controller.signal);
  }catch(e){
   if(!controller.signal.aborted&&view.current===id){
    // Failed uploads or request preparation never wrote the owner turn.
    // Restore it without overwriting anything typed while the upload ran.
    // Once the stream starts, its accepted/persisted state is unknown and
    // blind restoration would invite duplicate tool actions on retry.
    if(!streamStarted)setInput(previous=>previous.trim()?previous:text);
    setError(translateError((e as Error).message));
   }
  }finally{
   if(stream.current===controller){
    flushStreamBuffer();
    stream.current=null;activeTurnSession.current=undefined;
    refresh('/api/sessions',...(id?['/api/sessions/'+encodeURIComponent(id)+'/messages']:[]));
    setBusy(false);setLiveApproval(null);setToolStatus('');
    // Keep an unsaved partial answer visible after a provider/network failure.
    // The next send clears it; a successful save is replaced by the transcript.
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
 useEffect(()=>{if(!active){setModelMenu(false);setPermissionOpen(false);setShowSettings(false);setSlashOpen(false)}},[active]);
 return <div className={'relative grid h-dvh min-h-0 grid-cols-1 grid-rows-[minmax(0,1fr)] bg-[#FFFDF9] '+(showHistory?'md:grid-cols-[232px_minmax(0,1fr)]':'')}>
  {showHistory&&<Button type="button" aria-label={t('chat.listCollapse')} className="absolute inset-0 z-20 !h-auto !w-auto cursor-default !rounded-none !border-0 !bg-slate-900/30 md:hidden" onClick={()=>setShowHistory(false)}/>}
  <aside className={'flex min-h-0 flex-col overflow-hidden border-r border-[#E7E0D5] bg-[#FAF7F1] '+(showHistory?'max-md:absolute max-md:inset-y-0 max-md:left-0 max-md:z-30 max-md:w-[232px] max-md:shadow-2xl':'hidden')}>
   <div className="flex items-center justify-between gap-3 px-4 pb-3 pt-2"><div className="min-w-0"><span className="block text-[10px] font-extrabold uppercase tracking-widest text-[#C96442]">{t('ui.label.conversations')}</span><strong className="mt-1 block text-sm text-[#3A342D]">{t('chat.recent')}</strong></div><Button size="icon" variant="ghost" onClick={resetLocal} title={t('chat.newChat')}><Plus size={19}/></Button></div>
   <Button className="mx-3 mb-4 inline-flex items-center justify-center gap-2 rounded-xl border border-[#EED9CC] bg-[#F7E8E1] px-4 py-2.5 text-xs font-bold text-[#A94E30] hover:bg-[#F1DCCF]" onClick={resetLocal}><Plus size={17}/> {t('chat.newChat')}</Button>
   <div className="min-h-0 flex-1 overflow-y-auto px-2">{sessions.value.filter(s=>!s.roomId).map(s=><Button variant="bare" onClick={()=>pickSession(s)} key={s.id} title={s.title} className={'flex w-full items-center gap-3 rounded-xl px-3 py-2.5 text-left text-xs text-[#7C7469] hover:bg-[#F7E8E1] '+(current===s.id?'bg-[#F7E8E1] font-bold text-[#A94E30]':'')}><MessageCircle size={15} className="shrink-0"/><span className="truncate">{s.title}</span></Button>)}{!sessions.value.length&&<div className="p-5 text-center text-xs text-[#9C9388]">{t('chat.first')}</div>}</div>
   <div className="shrink-0 border-t border-[#E7E0D5] p-2"><Button variant="ghost" onClick={openSettings} className="w-full !justify-start gap-2 !text-xs" aria-label={t('nav.settings')}><Settings size={17}/>{t('nav.settings')}</Button></div>
  </aside>
  <div className="relative flex min-h-0 min-w-0 flex-col">
   <header className="chat-header relative flex h-13 shrink-0 items-center justify-between gap-2 border-b border-[#E7E0D5] bg-[#FFFDF9] px-4">
    <Button size="icon" variant="ghost" onClick={()=>setShowHistory(x=>!x)} title={showHistory?t('chat.listCollapse'):t('chat.listExpand')} aria-label={showHistory?t('chat.listCollapse'):t('chat.listExpand')}><Menu size={18}/></Button>
    <ConversationSwitch mode="chat" onChange={switchMode}/>
    <div className="flex min-w-0 flex-1 items-center gap-1.5">
     <div className="chat-model-controls relative flex min-w-0 items-center gap-1">
      <SelectInput aria-label={t('chat.selectProvider')} title={selectedProvider?.name||t('chat.autoProvider')} value={manual?.providerId??''} onChange={e=>{if(e.target.value)setRoute(e.target.value,'');else{setOverride(undefined);if(initialProvider)onSession(undefined)}}} className="!w-[124px] !pl-1.5 !border-0 !bg-transparent !py-1 !pr-6 !text-[12px]"><option value="">{t('chat.autoProvider')}</option>{providers.value.filter(p=>p.enabled).map(p=><option value={p.id} key={p.id}>{p.name}</option>)}</SelectInput>
      <span className="text-[#B8AFA2]">/</span>
      <ModelPicker providerId={providerId} value={model} catalogue={catalogue} onChange={value=>setRoute(providerId,value)} placeholder={providerId?t('chat.modelPick'):t('chat.connectApi')} className="!h-7 !w-[220px] !border-0 !bg-transparent !py-1 !pl-1.5 !pr-2 !text-[11px] !font-semibold" ariaLabel={t('chat.modelPick')}/>
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
     {(serverUsage||messageUsage)>0&&<span className="hidden rounded-lg border border-[#E7DDCC] bg-[#F7F1E6] px-2 py-1 text-[10px] font-semibold text-[#8A775F] sm:inline-flex">{t('usage.sessionTotal',{count:(serverUsage||messageUsage).toLocaleString()})}</span>}

     
     <Button size="icon" variant="ghost" title={t('chat.modelManage')} aria-label={t('chat.modelManage')} onClick={openModels}><Cpu size={18}/></Button>
     <Button size="icon" variant="ghost" className="conversation-settings" title={t('nav.settings')} aria-label={t('nav.settings')} onClick={openSettings}><Settings size={18}/></Button>
    </div>
   </header>
   <div ref={scrollRef} onScroll={onScroll} data-chat-scroll aria-label={t('ui.label.conversations')} className="min-h-0 flex-1 overflow-y-auto overscroll-contain">{!messages.length&&!(busy&&viewingTurn)?<div className="mx-auto flex min-h-full max-w-xl flex-col justify-center px-5 py-8">
    <div className="mb-6 flex items-center gap-3"><span className="grid size-11 place-items-center rounded-2xl bg-[#F7E8E1] text-[#C96442]"><Sparkles size={22}/></span><div><h1 className="text-[24px] font-bold tracking-[-.03em] text-[#24211D]">{t('chat.newChatHeading')}</h1><p className="mt-1 text-[13px] text-[#8A8176]">{t('chat.newChatDesc')}</p></div></div>
    <div className="rounded-2xl border border-[#E7E0D5] bg-[#FFFDF9] p-5"><div className="mb-2 text-[11px] font-semibold text-[#8A8176]">{t('chat.examples')}</div>{[t('chat.example1'),t('chat.example2'),t('chat.example3')].map(example=><Button type="button" key={example} onClick={()=>{setInput(example);inputRef.current?.focus({preventScroll:true})}} className="flex w-full items-center justify-between border-t border-[#EFE9DF] px-2 py-3 text-left text-[12px] text-[#5C544A] hover:bg-[#FFFDF9]">{example}<ArrowRight size={14} className="shrink-0 text-[#9C9388]"/></Button>)}</div>
    {!model&&<Button type="button" onClick={openModels} className="mt-4 inline-flex items-center gap-2 self-start text-[12px] font-semibold text-[#C96442]"><Compass size={15}/> {t('chat.connectFirst')} <ArrowRight size={14}/></Button>}
   </div>:<div className={'chat-transcript mx-auto flex min-h-full w-full max-w-[860px] flex-col gap-7 px-4 py-7 sm:px-7 '+(busy&&viewingTurn?'chat-transcript--responding':'')}>{messages.filter(m=>m.role!=='tool').map(m=><Message key={m.id} message={m} agentName={agents.value.find(a=>a.id===m.agent_id)?.name} showReasoning={showReasoning} />)}{showOptimisticOwner&&<Message key="optimistic-owner" message={{id:'optimistic-owner',session_id:current||'',role:'user',content:optimisticOwner!.content,created_at:''}}/>}{viewingTurn&&(live||thought||busy)&&<div className="chat-message chat-message--assistant flex min-w-0 items-start gap-3 justify-start assistant" data-message-role="assistant"><div className="grid size-9 shrink-0 place-items-center rounded-xl bg-[#F3EEE5] text-[#4A443C] ai"><Sparkles size={17}/></div><div className="relative min-w-0 flex-1"><div className="mb-2 flex items-center gap-2 text-xs font-bold text-[#4A443C]">DOMINION <span>{busy?t('ui.label.live'):savedMessageId?'대화 내역 동기화 중':'중단된 응답 · 미저장'}</span>{busy&&thought&&!showReasoning&&<span className="inline-flex items-center gap-1 rounded bg-[#F3EEE5] px-1.5 py-0.5 text-[10px] font-medium text-[#7C7469]"><BrainCircuit size={11}/> {t('chat.thinkingNow')}</span>}</div>{showReasoning&&thought&&<details className="mb-3 max-h-56 overflow-y-auto rounded-xl border border-[#E7E0D5] bg-[#FAF7F1] p-3 text-[11px] leading-5 text-[#7C7469]" open={thinking}><summary className="cursor-pointer select-none" onClick={e=>{e.preventDefault();setThinking(x=>!x)}}><BrainCircuit size={14} className="inline"/> {t('chat.reasoningProcess',{count:thought.length.toLocaleString()} )} <ChevronRight size={13} className="inline"/></summary><div className="mt-2 whitespace-pre-wrap">{thought}</div></details>}{live&&<div className="chat-message-content chat-message-content--assistant min-w-0 break-words text-[#3A342D] [&_p]:my-2 [&_pre]:overflow-x-auto [&_pre]:rounded-xl [&_pre]:bg-slate-950 [&_pre]:p-4 [&_pre]:text-slate-100 [&_code]:break-words [&_ul]:list-disc [&_ul]:pl-6"><ChatRichText text={live}/></div>}{busy&&!live&&<div className="flex items-center gap-2 text-xs text-[#9C9388]"><i/><i/><i/></div>}</div></div>}{viewingTurn&&toolStatus&&<div className="inline-flex items-center gap-2 rounded-lg bg-[#FAF7F1] px-3 py-1.5 text-xs text-[#7C7469]"><TerminalSquare size={15}/>{toolStatus}</div>}</div>}</div>
   {moreBelow&&<Button type="button" variant="ghost" onClick={jumpDown} aria-label="최신 메시지로 이동" className="chat-scroll-to-bottom absolute bottom-[132px] left-1/2 z-20 -translate-x-1/2 inline-flex items-center gap-2 rounded-full border border-[#E7E0D5] bg-white px-3 py-2 text-[12px] font-semibold text-[#5C544A] shadow-md hover:bg-[#F7F1E6]"><ArrowDown size={16}/>최신 메시지</Button>}
   <div className="conversation-composer-wrap sticky bottom-0 z-10"><div className="mx-auto w-full max-w-[860px]">{approval&&<div className="mb-3 flex flex-wrap items-center justify-between gap-3 rounded-xl border border-amber-200 bg-amber-50 p-3 text-xs text-amber-900"><div><ShieldAlert size={20}/><span><strong>{t('chat.approval')}</strong><small>{approval.tool} · {JSON.stringify(approval.args??{}).slice(0,150)}</small></span></div><div><Action variant="secondary" disabled={approvalBusy} onClick={()=>approve(false)}>{t('chat.deny')}</Action><Action disabled={approvalBusy} onClick={()=>approve(true)}><Check size={15}/> {t('chat.approve')}</Action></div></div>}
    {(error||transcript.error||job.error)&&<div className="mb-3 rounded-xl border border-rose-200 bg-rose-50 p-3 text-xs text-rose-700"><CircleHelp size={16}/>{error||transcript.error||translateError(job.error||'')}<Button onClick={()=>setError('')}><X size={14}/></Button></div>}
    {modelError&&!model&&<div className="mb-3 text-xs text-[#7C7469]">{t('chat.noModel')}: {modelError} <Button onClick={openModels}>{t('chat.manage')} <ArrowRight size={13}/></Button></div>}
    {commandInfo&&<div role="status" className="mb-3 max-h-36 overflow-auto whitespace-pre-wrap rounded-xl border border-[#EED9CC] bg-[#F7E8E1] p-3 text-xs leading-6 text-[#4A443C]">{commandInfo}</div>}
    <div className="relative">
    {pendingImages.length>0&&<div className="mb-2 flex flex-wrap items-center gap-2">
     {pendingImages.map(item=><div key={item.key} className="relative h-16 w-16 overflow-hidden rounded-lg border border-[#E7E0D5] bg-[#FAF7F1]">
      <img src={item.preview} alt={item.file.name} className="h-full w-full object-cover"/>
      <button type="button" onClick={()=>removeImage(item.key)} aria-label={t('chat.attachRemove')} title={t('chat.attachRemove')} className="absolute right-0.5 top-0.5 grid size-5 place-items-center rounded-full bg-black/60 text-white hover:bg-black/80"><X size={12}/></button>
     </div>)}
    </div>}
    {slashOpen&&<SlashMenu items={slashItems} active={slashActive} onChoose={chooseSlash}/>}
    <form onSubmit={send} className="conversation-composer">
     <TextArea ref={inputRef} rows={1} placeholder={t("chat.inputPlaceholder")} value={input} onChange={e=>{const next=e.target.value;setInput(next);setSlashOpen(next.startsWith('/')&&!next.includes('\n'));setSlashActive(0);const el=e.currentTarget;el.style.height='auto';el.style.height=Math.min(el.scrollHeight,180)+'px'}} onKeyDown={appendKey} aria-controls="dominion-slash-menu" aria-expanded={slashOpen} className="conversation-composer-input"/>
     <div className="chat-composer-controls conversation-composer-controls">
      <div className="chat-composer-options conversation-composer-options">
       <Button type="button" variant="ghost" size="icon" className="!h-7 !w-7 !border-0 !bg-transparent !text-[#5C544A]" title={t('chat.attach')} aria-label={t('chat.attach')} onClick={()=>imageInputRef.current?.click()} disabled={busy||uploading||Boolean(selectedSession?.roomId)}>{uploading?<LoaderCircle size={16} className="animate-spin"/>:<Paperclip size={16}/>}</Button>
       <input ref={imageInputRef} type="file" accept={ATTACH_ALLOWED.join(',')} multiple className="sr-only" aria-label={t('chat.attach')} onChange={onPickImages}/>
       {agentMode&&<ExecutionPermissions active={active} open={permissionOpen} onOpenChange={open=>{setPermissionOpen(open);if(open){setShowSettings(false);setModelMenu(false)}}} onError={setError} onChanged={label=>setCommandInfo(t('chat.perm.changed',{label}))}/>}

       <ReasoningSelect value={effectiveReasoningEffort} onChange={effort=>{setReasoningEffort(effort);setThinkingEnabled(effort!=='none')}} model={selectedModel} requireCatalog={selectedProvider?.kind==='codex'} defaultLabel={activeAgent?.reasoningEffort?`에이전트 설정 (${activeAgent.reasoningEffort})`:t('chat.reasoningAuto')} className="chat-reasoning !h-7 !w-auto !border-0 !bg-transparent !py-1 !pl-1.5 !pr-6 !text-[11px] !font-semibold !text-[#5C544A]"/>
       <SelectInput aria-label="대화 모드 및 에이전트 선택" value={agentMode?agentId:''} onChange={e=>{if(e.target.value)chooseAgent(e.target.value);else{setChatModeOverride({scope:current,mode:'chat'});setPermissionOpen(false)}}} className="chat-agent !h-7 !w-auto !max-w-[140px] !border-0 !bg-transparent !py-1 !pl-1.5 !pr-6 !text-[11px] !font-semibold !text-[#5C544A]"><option value="">일반 채팅</option>{agents.value.map(a=><option key={a.id} value={a.id}>{'에이전트 · '+a.name}</option>)}</SelectInput>}
       <ConversationSettings open={showSettings} onOpenChange={open=>{setShowSettings(open);if(open){setPermissionOpen(false);setModelMenu(false)}}} showReasoning={showReasoning} setShowReasoning={setShowReasoning} reasoningKo={reasoningKo} setReasoningKo={setReasoningKo} thinkingEnabled={effectiveThinkingEnabled} setThinkingEnabled={setThinkingEnabled} canDisableThinking={canDisableThinking} reasoningEffort={effectiveReasoningEffort} setReasoningEffort={setReasoningEffort} toolsEnabled={agentMode&&toolsEnabled} setToolsEnabled={setToolsEnabled} autoLearn={agentMode&&autoLearn} setAutoLearn={setAutoLearn} optionsBusy={!agentMode} learning={learning} learnDisabled={!agentMode||!current||Boolean(selectedSession?.roomId)||busy} onLearn={learnSession}/>
      </div>
      <div className="ml-auto flex items-center gap-2">
       <span className="hidden text-[10px] text-[#9C9388] sm:inline">{t('chat.enterHint')}</span>
       {busy?<Button className="conversation-composer-send !bg-rose-600 hover:!bg-rose-700" type="button" onClick={()=>void stopChat()} disabled={stopping} title={stopping?t('chat.stopping'):t('chat.stop')} aria-label={stopping?t('chat.stopping'):t('chat.stop')}>{stopping?<LoaderCircle size={18} className="animate-spin"/>:<Square size={18} fill="currentColor"/>}</Button>
       :<Button className="conversation-composer-send" disabled={stopping||uploading||Boolean(selectedSession?.roomId)||(Boolean(current)&&sessions.loading)||(!input.trim()&&!pendingImages.length)} type="submit" title={t('chat.send')}><ArrowUp size={19}/></Button>}
      </div>
     </div>
    </form></div>
   </div></div>
  </div>
 </div>
}
function Message({message,agentName,showReasoning}:{message:ChatMessage;agentName?:string;showReasoning?:boolean}){
 const [open,setOpen]=useState(true);const human=message.role==='user';
 const attachments=Array.isArray(message.metadata?.attachments)?message.metadata.attachments as Attachment[]:[];
 return <div className={'chat-message flex min-w-0 items-start gap-3 '+(human?'chat-message--user justify-end':'chat-message--assistant justify-start')} data-message data-message-role={human?'user':'assistant'}>
  {!human&&<div className="grid size-9 shrink-0 place-items-center rounded-xl bg-[#F7E8E1] text-[#A94E30]"><Sparkles size={17}/></div>}
  <div className={'chat-message-body relative min-w-0 '+(human?'':'flex-1')}>
   <div className={'chat-message-author mb-2 flex items-center gap-2 text-xs font-bold text-[#4A443C] '+(human?'justify-end':'')}>{human?t('ui.you'):agentName||'DOMINION'}{!human&&<TokenUsage usage={message.metadata?.usage} className="ml-auto rounded border border-[#E7DDCC] bg-[#F7F1E6] px-1.5 py-0.5 text-[9px] font-medium text-[#8A775F]"/>}</div>
   {showReasoning&&message.reasoning&&<details className="mb-3 max-h-56 overflow-y-auto rounded-xl border border-[#E7E0D5] bg-[#FAF7F1] p-3 text-[11px] leading-5 text-[#7C7469]" open={open}><summary className="cursor-pointer select-none" onClick={e=>{e.preventDefault();setOpen(v=>!v)}}><BrainCircuit size={14} className="inline"/> {t('chat.reasoningProcess',{count:message.reasoning.length.toLocaleString()})} <ChevronRight size={13} className="inline"/></summary><div className="mt-2 whitespace-pre-wrap">{message.reasoning}</div></details>}
   <div className={'chat-message-content min-w-0 break-words text-[#3A342D] [&_p]:my-2 [&_pre]:overflow-x-auto [&_pre]:rounded-xl [&_pre]:bg-slate-950 [&_pre]:p-4 [&_pre]:text-slate-100 [&_code]:break-words [&_ul]:list-disc [&_ul]:pl-6 '+(human?'chat-message-content--user':'chat-message-content--assistant')}>{human?message.content:<ChatRichText text={message.content||(!human&&message.tool_calls?.length?t('chat.toolRequest'):'')}/>}</div>
   {human&&attachments.length>0&&<div className="mt-2 flex flex-wrap gap-2">
    {attachments.map(item=>item.isImage
     ?<a key={item.id} href={item.url} target="_blank" rel="noreferrer" title={item.name} className="block overflow-hidden rounded-lg border border-[#E7E0D5]"><img src={item.url} alt={item.name} className="h-24 w-24 object-cover"/></a>
     :<a key={item.id} href={item.url} target="_blank" rel="noreferrer" className="rounded-lg border border-[#E7E0D5] bg-white px-2 py-1 text-[11px] text-[#5C544A] hover:bg-[#FAF7F1]">{item.name}</a>)}
   </div>}
   {!human&&message.content&&<Button className="inline-flex items-center gap-1.5 rounded-lg px-2 py-1 text-[11px] text-[#7C7469] hover:bg-[#F3EEE5]" onClick={()=>navigator.clipboard.writeText(message.content)} title={t('chat.copy')}><Copy size={14}/></Button>}
   {!human&&typeof message.metadata?.executionId==='string'&&
    <ExecutionInsights executionId={message.metadata.executionId} compact/>}
  </div>
 </div>
}
