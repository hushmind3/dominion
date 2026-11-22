import {ConversationRail} from '../components/chat/ConversationRail'
import {useConversationSidebar} from '../hooks/useConversationSidebar'
import type {CSSProperties} from 'react'
import {ChatWelcome} from '../components/chat/ChatWelcome'
import {ChatHistory} from '../components/chat/ChatHistory'
import {ConversationModelPicker} from '../components/chat/ConversationModelPicker'
import {ConversationSettings} from '../components/ConversationSettings'
import {ChatRichText} from '../components/ChatRichText'
import {usePersistentConversationToggle,usePersistentReasoningEffort,usePersistentKoreanReasoning} from '../hooks/usePersistentConversationPreference'
import {compatibleConversationEffort} from '../conversationPreferences'
import {ExecutionPermissionsSetting} from '../components/ExecutionPermissions'
import {MessageInfo} from '../components/chat/MessageInfo'
import {ExecutionInsights} from '../components/ExecutionInsights'
import {createPortal} from 'react-dom'
import {Button,TextArea,SelectInput} from '../components/Primitives'
import {FormEvent,useEffect,useLayoutEffect,useRef,useState,type ChangeEvent,type KeyboardEvent} from 'react'
import {ArrowDown,ArrowRight,ArrowUp,BrainCircuit,Check,ChevronRight,CircleHelp,Copy,Menu,LoaderCircle,Paperclip,ShieldAlert,Sparkles,Square,TerminalSquare,X} from 'lucide-react'
import {Agent,Approval,Attachment,ChatMessage,ChatSession,Provider,get,post,resolveApproval,streamChat,uploadAttachment} from '../api'
import {Action,refresh,useRemote} from '../components/UI'
import {SlashMenu,slashCandidates,slashCommandLabel,slashCommands,type SlashChoice} from '../components/SlashMenu'
import {ReasoningSelect,reasoningOptions} from '../components/ReasoningSelect'
import {useProviderModels} from '../hooks/useProviderModels'
import {useTranscriptScroll} from '../hooks/useTranscriptScroll'
import {useChatDraft} from '../hooks/useChatDraft'
import {useJob,jobFinished,type JobState} from '../hooks/useJob'
import {t,useLocale} from '../i18n'
import {translateError} from '../i18n/errors'

type LearningResult={memories:number;skill:boolean}
type LearningJob=JobState & {result?:LearningResult}
type ModelOverride={scope?:string;providerId:string;model:string}
type PendingImage={key:string;file:File;preview:string}
type OptimisticOwner={sessionId:string|undefined;content:string;lastSeenMessageId:string|undefined;persistedId?:string}
const ATTACH_ALLOWED=['image/png','image/jpeg','image/webp','image/gif']
const ATTACH_LIMIT=8
export function ChatPage({sessionId,initialAgent,initialProvider,initialModel,onSession,openModels,openSettings,switchMode,settingsTarget,active=true}:{sessionId?:string;initialAgent?:string;initialProvider?:string;initialModel?:string;onSession:(x:string|undefined)=>void;openModels:()=>void;openSettings:()=>void;switchMode:(mode:'chat'|'rooms')=>void;settingsTarget?:HTMLElement|null;active?:boolean}){
 const loc=useLocale()
 const sessions=useRemote<ChatSession[]>('/api/sessions',[],active);
 const agents=useRemote<Agent[]>('/api/agents',[],active);
 const providers=useRemote<Provider[]>('/api/providers',[],active);
 const current=sessionId;
 const selectedSession=sessions.value.find(session=>session.id===current);
 const transcript=useRemote<ChatMessage[]>(current?'/api/sessions/'+encodeURIComponent(current)+'/messages':null,[],active);
 const messages=transcript.value
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
 const recentChat=sessions.value.find(session=>!session.roomId&&session.chatMode==='chat'&&session.model&&
  enabledProviders.some(provider=>provider.id===session.providerId));
 const defaultProvider=enabledProviders.find(provider=>provider.id===recentChat?.providerId)||
  enabledProviders.find(provider=>provider.model&&(provider.hasKey||provider.local||provider.kind==='cli'||provider.kind==='ollama'))||enabledProviders[0];
 // Wait for stored routes instead of racing an arbitrary provider's catalog.
 const restoringRoute=sessions.loading&&!manual&&!activeAgent;
 const providerId=manual?.providerId??(activeAgent?.providerId||sessionProvider?.id||(!restoringRoute?defaultProvider?.id:'')||'');
 const selectedProvider=enabledProviders.find(provider=>provider.id===providerId);
 const model=manual?.model||((activeAgent?.providerId===providerId?activeAgent.model:'')||
  (selectedSession?.providerId===providerId&&(!agentMode||selectedSession.agentId===agentId)?selectedSession.model:'')||
  (!current&&recentChat?.providerId===providerId?recentChat.model:'')||selectedProvider?.model||'');
 const catalogue=useProviderModels(providerId,active);
 const {value:models,error:modelError}=catalogue;
 const selectedModel=models.find(item=>item.id===model);
 useEffect(()=>{
  if(!model&&!catalogue.loading&&!catalogue.error&&models.length){
   const choice=models.find(item=>item.isDefault)||models[0];
   setOverride({scope:current,providerId,model:choice.id});
  }
 },[model,catalogue.loading,catalogue.error,models,providerId,current]);
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
 const [dismissedError,setDismissedError]=useState<{session?:string;message:string}>();
 const [error,setError]=useState(''),[busy,setBusy]=useState(false),[live,setLive]=useState(''),[thought,setThought]=useState(''),[thinking,setThinking]=useState(false),[toolStatus,setToolStatus]=useState('');
 const {open:showHistory,setOpen:setShowHistory,width:sidebarWidth,resize:resizeSidebar,mobile:sidebarMobile,panel:sidebarPanel}=useConversationSidebar('dominion.chat.sidebar',active);

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
 const [learning,setLearning]=useState(false);
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
 // Keep the same logical turn key when a disconnected request is retried.
 // The Rust session lease then refuses a duplicate owner write or tool effect.
 const pendingTurn=useRef<{scope?:string;text:string;id:string}|null>(null);
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
  optimisticOwner.persistedId
   ?message.id===optimisticOwner.persistedId
   :index>lastSeenOwnerIndex&&message.role==='user'&&message.content===optimisticOwner.content));
 const showOptimisticOwner=Boolean(optimisticOwner?.sessionId===current&&!ownerReceived&&(busy||optimisticOwner?.persistedId||savedMessageId));
 const slashItems=slashOpen?slashCandidates(input,agents.value,models):[];
 const transcriptRevision=JSON.stringify([messages.at(-1)?.id,messages.length,showOptimisticOwner,busy&&viewingTurn,viewingTurn?live.length:0,viewingTurn?thought.length:0,viewingTurn?toolStatus:'',approval?.id]);
 const alertMessage=error||transcript.error||translateError(job.error||'');
 const visibleAlert=alertMessage&&!(dismissedError?.session===current&&dismissedError?.message===alertMessage)?alertMessage:'';
 const [moreBelow,setMoreBelow]=useState(false);
 const {scrollRef,onScroll,jumpDown}=useTranscriptScroll(current,transcriptRevision,setMoreBelow);
 useEffect(()=>()=>{stream.current?.abort();clearStreamBuffer()},[]);
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
  setDismissedError(undefined);setError('');setCommandInfo('');setSlashOpen(false);
  if(window.innerWidth<768)setShowHistory(false);
 };
 const resetLocal=()=>{
  onSession(undefined);setAgentChoice(undefined);setChatModeOverride(undefined);setOverride(undefined);
  discardPendingImages();
  if(!current)setInput('');
  setDismissedError(undefined);setError('');setCommandInfo('');setSlashOpen(false);
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
    // No session exists yet, so the owner prompt cannot be recovered from
    // SQLite. Return it to the draft before dismissing the optimistic bubble.
    if(optimisticOwner&&optimisticOwner.sessionId===undefined)
     setInput(previous=>previous.trim()?previous:optimisticOwner.content);
    setOptimisticOwner(undefined);
    pendingTurn.current=null;
    setBusy(false);setLive('');setThought('');setToolStatus('');setTurnSession(undefined);
    setCommandInfo(t('chat.stopped'));
   }else setError(t('chat.cmd.noSessionStop'));
   return;
  }
  stoppingRef.current=true;setStopping(true);setError('');
  try{
   const outcome=await post<{cancelled:boolean;stopped:boolean}>('/api/sessions/'+encodeURIComponent(id)+'/cancel',{requestId:pendingTurn.current?.id});
   if(!outcome.stopped)throw Error(t('chat.stopPending'));
   // A STOP must preserve already streamed text, even when the native
   // execution was cancelled before the assistant answer was saved.
   if(controller)flushStreamBuffer();
   // STOP during attachment upload or before the owner-save acknowledgment
   // must keep the unsent prompt available. A retry reuses clientTurnId, so
   // an already-persisted operation still cannot run twice.
   if(optimisticOwner?.sessionId===id&&!optimisticOwner.persistedId)
    setInput(previous=>previous.trim()?previous:optimisticOwner.content);
   controller?.abort();
   if(stream.current===controller){
    stream.current=null;activeTurnSession.current=undefined;
    setBusy(false);setLiveApproval(null);setToolStatus('');
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
  if(!providerId){openModels();return;}
  if(!model){setError(t('chat.err.pickModel'));return;}
  const previousTurn=pendingTurn.current;
  const clientTurnId=previousTurn&&previousTurn.scope===current&&previousTurn.text===text
   ?previousTurn.id
   :typeof crypto!=='undefined'?crypto.randomUUID?.():undefined;
  if(clientTurnId)pendingTurn.current={scope:current,text,id:clientTurnId};
  const controller=new AbortController();
  clearStreamBuffer();
  stream.current=controller;
  activeTurnSession.current=current;
  setTurnSession(current);setSavedMessageId(undefined);
  setOptimisticOwner({sessionId:current,content:text,lastSeenMessageId:messages.at(-1)?.id});
  setBusy(true);setDismissedError(undefined);setError('');setCommandInfo('');setLive('');setThought('');setSlashOpen(false);setToolStatus('');
  jumpDown();
  let id=current;
  let streamStarted=false;
  let serverTerminal=false;
  let ownerSaved=false;
  // Retain local image previews until the owner turn is durably recorded.
  // Failed uploads and rejected requests must be retryable without reselecting files.
  let releasedSentImages=false;
  const releaseSentImages=()=>{
   if(releasedSentImages||!pendingImages.length)return;
   releasedSentImages=true;
   const sentKeys=new Set(pendingImages.map(item=>item.key));
   pendingImages.forEach(item=>URL.revokeObjectURL(item.preview));
   setPendingImages(previous=>previous.filter(item=>!sentKeys.has(item.key)));
  };
  try{
   if(!id){
    const session=await post<ChatSession>('/api/sessions',{title:t('chat.sessionTitle'),agentId:agentMode?agentId:undefined,
     providerId:agentMode?manual?.providerId:providerId,model:agentMode?manual?.model:model});
    if(controller.signal.aborted)return;
    id=session.id;
    if(clientTurnId&&pendingTurn.current?.id===clientTurnId)pendingTurn.current.scope=id;
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
     const uploaded=await Promise.all(pendingImages.map(item=>uploadAttachment({sessionId:id!},item.file,controller.signal)));
     if(controller.signal.aborted)return;
     attachmentIds=uploaded.map(item=>item.id);
    }finally{
     setUploading(false);
    }
   }
   streamStarted=true;
   await streamChat({
    sessionId:id,clientTurnId,requestId:clientTurnId,
    conversationMode:agentMode?'agent':'chat',
    agentId:agentMode?agentId:undefined,directChat:!agentMode,
    providerId:agentMode?manual?.providerId:providerId,model:agentMode?manual?.model:model,input:text,
    toolsEnabled:agentMode&&toolsEnabled,toolsExecutionEnabled:agentMode&&toolsEnabled,autoLearning:agentMode&&autoLearn,attachmentIds,
    reasoningLanguage:reasoningKo?'ko':'original',reasoningEffort:effectiveReasoningEffort||undefined,thinking:selectedProvider?.kind==='codex'?(effectiveReasoningEffort==='none'?false:canDisableThinking?effectiveThinkingEnabled:undefined):effectiveThinkingEnabled,
   },event=>{
    if(controller.signal.aborted||stoppingRef.current||stream.current!==controller)return;
    if(event.type==='owner_saved'){
     ownerSaved=true;
     if(pendingTurn.current?.id===clientTurnId)pendingTurn.current=null;
     releaseSentImages();
     setOptimisticOwner(previous=>previous&&previous.sessionId===id?{...previous,persistedId:String(event.messageId)}:previous);
     refresh('/api/sessions/'+encodeURIComponent(id!)+'/messages');
    }
    if(event.type==='assistant_saved'){
     if(pendingTurn.current?.id===clientTurnId)pendingTurn.current=null;
     releaseSentImages();flushStreamBuffer();setSavedMessageId(event.messageId);refresh('/api/sessions',...(id?['/api/sessions/'+encodeURIComponent(id)+'/messages']:[]));}
    if(event.type==='text')queueStreamDelta('text',event.text);
    if(event.type==='reasoning')queueStreamDelta('reasoning',event.text);
    if(event.type==='approval'){setLiveApproval({id:event.id,tool:event.tool,args:event.args,sessionId:id!,status:'pending'});void approvals.reload();}
    if(event.type==='tool_start'){setLiveApproval(null);setToolStatus(t('chat.tool.running',{name:event.name}));}
    if(event.type==='tool_end')setToolStatus(t('chat.tool.done',{name:event.name}));
    if(event.type==='tool_error')setToolStatus(t('chat.tool.error',{error:translateError(event.error)}));
    if(event.type==='goal_progress')setToolStatus(t('chat.tool.goal',{event:event.event,status:event.status}));
    if(event.type==='done')serverTerminal=true;
    if(event.type==='error'){serverTerminal=true;throw Error(event.message);}
   },controller.signal);
  }catch(e){
   if(!controller.signal.aborted&&view.current===id){
    let notice=translateError((e as Error).message);
    // A failed upload or pre-stream request cannot have persisted the owner
    // input. After SSE begins, reconcile against SQLite before offering retry:
    // blindly restoring it can duplicate an effectful agent instruction.
    if(!streamStarted){
     setInput(previous=>previous.trim()?previous:text);
     setOptimisticOwner(previous=>previous?.sessionId===id?undefined:previous);
    }else if(!ownerSaved&&id){
     let persisted:ChatMessage|undefined;
     let lookupFailed=false;
     try{
      const rows=await get<ChatMessage[]>('/api/sessions/'+encodeURIComponent(id)+'/messages');
      const oldId=messages.at(-1)?.id;
      const lastIndex=oldId?rows.findIndex(row=>row.id===oldId):-1;
      persisted=rows.find((row,index)=>row.role==='user'&&(
       clientTurnId&&row.metadata?.clientTurnId===clientTurnId||
       index>lastIndex&&row.metadata?.clientTurnId==null&&row.content===text
      ));
     }catch{lookupFailed=true;}
     if(view.current===id){
      if(persisted){
       if(pendingTurn.current?.id===clientTurnId)pendingTurn.current=null;
       releaseSentImages();
       setOptimisticOwner(previous=>previous&&previous.sessionId===id?{...previous,persistedId:persisted!.id}:previous);
      }else{
       setInput(previous=>previous.trim()?previous:text);
       setOptimisticOwner(previous=>previous?.sessionId===id?undefined:previous);
       if(lookupFailed||!serverTerminal)
        notice+=' · 실행 종료 여부를 확인하지 못했습니다. 재전송 전 대화 기록과 실행 상태를 확인하세요.';
      }
     }
    }
    if(view.current===id)setError(notice);
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
 useEffect(()=>{if(!active){setSlashOpen(false)}},[active]);
 return <div style={{'--conversation-sidebar-width':sidebarWidth+'px'} as CSSProperties} className={'relative grid h-dvh min-h-0 grid-cols-1 grid-rows-[minmax(0,1fr)] bg-surface '+(showHistory?'md:grid-cols-[var(--conversation-sidebar-width)_minmax(0,1fr)]':'md:grid-cols-[52px_minmax(0,1fr)]')}>
  {showHistory&&<Button type="button" aria-label={t('chat.listCollapse')} className="absolute inset-0 z-20 !h-auto !w-auto cursor-default !rounded-none !border-0 !bg-slate-900/30 md:hidden" onClick={()=>setShowHistory(false)}/>}
  <aside ref={sidebarPanel} role={sidebarMobile&&showHistory?'dialog':undefined} aria-modal={sidebarMobile&&showHistory?true:undefined} aria-label={t('chat.recent')} tabIndex={-1} id="conversation-sidebar" className={'chat-history md:relative flex min-h-0 flex-col overflow-hidden border-r border-divider '+(showHistory?'max-md:absolute max-md:inset-y-0 max-md:left-0 max-md:z-30 max-md:w-[min(300px,85vw)] max-md:shadow-2xl':'hidden')}>
   <ChatHistory active={active} sessions={sessions.value} selected={current} loading={sessions.loading} error={sessions.error}
    onNew={resetLocal} onSelect={pickSession} onSettings={openSettings} onSwitchMode={switchMode} onClose={()=>setShowHistory(false)} onRetry={()=>void sessions.reload()}/>

   <div {...resizeSidebar} className="chat-history-resize" aria-label={t('history.resize')} title={t('history.resizeHint')}/>
  </aside>
  {!showHistory&&<ConversationRail onExpand={()=>setShowHistory(true)} onNew={resetLocal} onSwitchMode={switchMode} onSettings={openSettings}/>}
  <div inert={showHistory&&sidebarMobile} aria-hidden={showHistory&&sidebarMobile||undefined} className="relative flex min-h-0 min-w-0 flex-col">

   <div ref={scrollRef} onScroll={onScroll} data-chat-scroll aria-label={t('ui.label.conversations')} className="min-h-0 flex-1 overflow-y-auto overscroll-contain">{current&&transcript.loading?<div className="chat-transcript-loading" role="status"><LoaderCircle size={20} className="animate-spin"/>{t('history.loading')}</div>:!messages.length&&!showOptimisticOwner&&!(busy&&viewingTurn)?<ChatWelcome ready={Boolean(providerId&&model)} loading={sessions.loading||providers.loading||catalogue.loading} error={providers.error} onRetry={()=>{void providers.reload();void sessions.reload()}}
    onConnect={openModels} onPrompt={prompt=>{setInput(prompt);inputRef.current?.focus({preventScroll:true})}}/>
   :<div className={'chat-transcript mx-auto flex min-h-full w-full max-w-[860px] flex-col gap-7 px-4 py-7 sm:px-7 '+(busy&&viewingTurn?'chat-transcript--responding':'')}>{messages.filter(m=>m.role!=='tool').map(m=><Message key={m.id} active={active} message={m} agentName={agents.value.find(a=>a.id===m.agent_id)?.name} showReasoning={showReasoning} />)}{showOptimisticOwner&&<Message key="optimistic-owner" message={{id:'optimistic-owner',session_id:current||'',role:'user',content:optimisticOwner!.content,created_at:''}}/>}{viewingTurn&&(live||thought||busy)&&<div className="chat-message chat-message--assistant flex min-w-0 items-start gap-3 justify-start assistant" data-message data-message-role="assistant"><div className="grid size-9 shrink-0 place-items-center rounded-xl bg-[#F3EEE5] text-[#4A443C] ai"><Sparkles size={17}/></div><div className="chat-message-body relative min-w-0 flex-1"><div className="mb-2 flex items-center gap-2 text-xs font-bold text-[#4A443C]">DOMINION <span>{busy?t('ui.label.live'):savedMessageId?'대화 내역 동기화 중':'중단된 응답 · 미저장'}</span>{busy&&thought&&!showReasoning&&<span className="inline-flex items-center gap-1 rounded bg-[#F3EEE5] px-1.5 py-0.5 text-[10px] font-medium text-muted"><BrainCircuit size={11}/> {t('chat.thinkingNow')}</span>}</div>{showReasoning&&thought&&<details className="mb-3 max-h-56 overflow-y-auto rounded-xl border border-divider bg-[#FAF7F1] p-3 text-[11px] leading-5 text-muted" open={thinking}><summary className="cursor-pointer select-none" onClick={e=>{e.preventDefault();setThinking(x=>!x)}}><BrainCircuit size={14} className="inline"/> {t('chat.reasoningProcess',{count:thought.length.toLocaleString()} )} <ChevronRight size={13} className="inline"/></summary><div className="mt-2 whitespace-pre-wrap">{thought}</div></details>}{live&&<div className="chat-message-content chat-message-content--assistant min-w-0 break-words text-[#3A342D] [&_p]:my-2 [&_pre]:overflow-x-auto [&_pre]:rounded-xl [&_pre]:bg-slate-950 [&_pre]:p-4 [&_pre]:text-slate-100 [&_code]:break-words [&_ul]:list-disc [&_ul]:pl-6"><ChatRichText text={live}/></div>}{!busy&&!savedMessageId&&live&&<Button type="button" variant="ghost" aria-label="중단된 답변 복사" onClick={()=>void navigator.clipboard.writeText(live)} className="mt-2 inline-flex items-center gap-1.5 rounded-lg px-2 py-1 text-[11px] text-muted hover:bg-[#F3EEE5]"><Copy size={14}/>내용 복사</Button>}{busy&&!live&&<div className="flex items-center gap-2 text-xs text-[#9C9388]"><i/><i/><i/></div>}</div></div>}{viewingTurn&&toolStatus&&<div className="inline-flex items-center gap-2 rounded-lg bg-[#FAF7F1] px-3 py-1.5 text-xs text-muted"><TerminalSquare size={15}/>{toolStatus}</div>}</div>}</div>
   {moreBelow&&<Button type="button" variant="ghost" onClick={jumpDown} aria-label="최신 메시지로 이동" className="chat-scroll-to-bottom absolute bottom-[132px] left-1/2 z-20 -translate-x-1/2 inline-flex items-center gap-2 rounded-full border border-divider bg-white px-3 py-2 text-[12px] font-semibold text-[#5C544A] shadow-md hover:bg-[#F7F1E6]"><ArrowDown size={16}/>최신 메시지</Button>}
   <div className="conversation-composer-wrap sticky bottom-0 z-10"><div className="mx-auto w-full max-w-[860px]">{approval&&<div className="mb-3 flex flex-wrap items-center justify-between gap-3 rounded-xl border border-amber-200 bg-amber-50 p-3 text-xs text-amber-900"><div><ShieldAlert size={20}/><span><strong>{t('chat.approval')}</strong><small>{approval.tool} · {JSON.stringify(approval.args??{}).slice(0,150)}</small></span></div><div><Action variant="secondary" disabled={approvalBusy} onClick={()=>approve(false)}>{t('chat.deny')}</Action><Action disabled={approvalBusy} onClick={()=>approve(true)}><Check size={15}/> {t('chat.approve')}</Action></div></div>}
    {visibleAlert&&<div role="alert" className="mb-3 rounded-xl border border-rose-200 bg-rose-50 p-3 text-xs text-rose-700"><CircleHelp size={16}/>{visibleAlert}<Button type="button" aria-label="오류 닫기" onClick={()=>setDismissedError({session:current,message:visibleAlert})}><X size={14}/></Button></div>}
    {modelError&&!model&&<div className="mb-3 text-xs text-muted">{t('welcome.modelError')} <Button onClick={openModels}>{t('chat.manage')} <ArrowRight size={13}/></Button></div>}
    {commandInfo&&<div role="status" className="mb-3 max-h-36 overflow-auto whitespace-pre-wrap rounded-xl border border-[#EED9CC] bg-[#F7E8E1] p-3 text-xs leading-6 text-[#4A443C]">{commandInfo}</div>}
    <div className="relative">
    {pendingImages.length>0&&<div className="mb-2 flex flex-wrap items-center gap-2">
     {pendingImages.map(item=><div key={item.key} className="relative h-16 w-16 overflow-hidden rounded-lg border border-divider bg-[#FAF7F1]">
      <img src={item.preview} alt={item.file.name} className="h-full w-full object-cover"/>
      <button type="button" onClick={()=>removeImage(item.key)} aria-label={t('chat.attachRemove')} title={t('chat.attachRemove')} className="absolute right-0.5 top-0.5 grid size-5 place-items-center rounded-full bg-black/60 text-white hover:bg-black/80"><X size={12}/></button>
     </div>)}
    </div>}
    {slashOpen&&<SlashMenu items={slashItems} active={slashActive} onChoose={chooseSlash}/>}
    <form onSubmit={send} className="conversation-composer">
     <TextArea ref={inputRef} rows={1} placeholder={t("chat.inputPlaceholder")} value={input} onChange={e=>{const next=e.target.value;setInput(next);setSlashOpen(next.startsWith('/')&&!next.includes('\n'));setSlashActive(0);const el=e.currentTarget;el.style.height='auto';el.style.height=Math.min(el.scrollHeight,180)+'px'}} onKeyDown={appendKey} aria-controls="dominion-slash-menu" aria-expanded={slashOpen} className="conversation-composer-input"/>
     <div className="chat-composer-controls conversation-composer-controls">
      <div className="chat-composer-options conversation-composer-options">
       <Button variant="ghost" size="icon" className="!h-7 !w-7 md:hidden" onClick={()=>setShowHistory(value=>!value)} aria-label={showHistory?t('chat.listCollapse'):t('chat.listExpand')} title={t('chat.recent')}><Menu size={17}/></Button>
       <ConversationModelPicker active={active} providers={enabledProviders} providerId={providerId} model={model} catalogue={catalogue} onChange={setRoute} onManage={openModels}/>

       <Button type="button" variant="ghost" size="icon" className="!h-7 !w-7 !border-0 !bg-transparent !text-[#5C544A]" title={t('chat.attach')} aria-label={t('chat.attach')} onClick={()=>imageInputRef.current?.click()} disabled={busy||uploading||Boolean(selectedSession?.roomId)}>{uploading?<LoaderCircle size={16} className="animate-spin"/>:<Paperclip size={16}/>}</Button>
       <input ref={imageInputRef} type="file" accept={ATTACH_ALLOWED.join(',')} multiple className="sr-only" aria-label={t('chat.attach')} onChange={onPickImages}/>


       <SelectInput aria-label="대화 모드 및 에이전트 선택" value={agentMode?agentId:''} onChange={e=>{if(e.target.value)chooseAgent(e.target.value);else{setChatModeOverride({scope:current,mode:'chat'})}}} className="chat-agent !h-7 !w-auto !max-w-[140px] !border-0 !bg-transparent !py-1 !pl-1.5 !pr-6 !text-[11px] !font-semibold !text-[#5C544A]"><option value="">{t('welcome.chatMode')}</option>{agents.value.map(a=><option key={a.id} value={a.id}>{t('welcome.agentMode')+' · '+a.name}</option>)}</SelectInput>

      </div>
      <div className="ml-auto flex items-center gap-2">
       {busy?<Button className="conversation-composer-send !bg-rose-600 hover:!bg-rose-700" type="button" onClick={()=>void stopChat()} disabled={stopping} title={stopping?t('chat.stopping'):t('chat.stop')} aria-label={stopping?t('chat.stopping'):t('chat.stop')}>{stopping?<LoaderCircle size={18} className="animate-spin"/>:<Square size={18} fill="currentColor"/>}</Button>
       :<Button className="conversation-composer-send" disabled={stopping||uploading||Boolean(selectedSession?.roomId)||(Boolean(current)&&sessions.loading)||(!input.trim()&&!pendingImages.length)} type="submit" title={t('chat.send')}><ArrowUp size={19}/></Button>}
      </div>
     </div>
    </form></div>
   </div></div>
  </div>
  {settingsTarget&&createPortal(<ConversationSettings permissionControl={agentMode?<ExecutionPermissionsSetting/>:undefined} reasoningControl={<ReasoningSelect value={effectiveReasoningEffort} onChange={effort=>{setReasoningEffort(effort);setThinkingEnabled(effort!=='none')}} model={selectedModel} requireCatalog={selectedProvider?.kind==='codex'} className="w-full"/>} showReasoning={showReasoning} setShowReasoning={setShowReasoning} reasoningKo={reasoningKo} setReasoningKo={setReasoningKo} thinkingEnabled={effectiveThinkingEnabled} setThinkingEnabled={setThinkingEnabled} canDisableThinking={canDisableThinking} reasoningEffort={effectiveReasoningEffort} setReasoningEffort={setReasoningEffort} toolsEnabled={agentMode&&toolsEnabled} setToolsEnabled={setToolsEnabled} autoLearn={agentMode&&autoLearn} setAutoLearn={setAutoLearn} optionsBusy={!agentMode} learning={learning} learnDisabled={!agentMode||!current||Boolean(selectedSession?.roomId)||busy} onLearn={learnSession}/>,settingsTarget)}
 </div>
}
function Message({message,agentName,showReasoning,active=true}:{message:ChatMessage;agentName?:string;showReasoning?:boolean;active?:boolean}){
 const [open,setOpen]=useState(true);const human=message.role==='user';
 const attachments=Array.isArray(message.metadata?.attachments)?message.metadata.attachments as Attachment[]:[];
 return <div className={'chat-message flex min-w-0 items-start gap-3 '+(human?'chat-message--user justify-end':'chat-message--assistant justify-start')} data-message data-message-role={human?'user':'assistant'}>
  {!human&&<div className="grid size-9 shrink-0 place-items-center rounded-xl bg-[#F7E8E1] text-brand-hover"><Sparkles size={17}/></div>}
  <div className={'chat-message-body relative min-w-0 '+(human?'':'flex-1')}>
   <div className={'chat-message-author mb-2 flex items-center gap-2 text-xs font-bold text-[#4A443C] '+(human?'justify-end':'')}>{human?t('ui.you'):agentName||'DOMINION'}</div>
   {showReasoning&&message.reasoning&&<details className="mb-3 max-h-56 overflow-y-auto rounded-xl border border-divider bg-[#FAF7F1] p-3 text-[11px] leading-5 text-muted" open={open}><summary className="cursor-pointer select-none" onClick={e=>{e.preventDefault();setOpen(v=>!v)}}><BrainCircuit size={14} className="inline"/> {t('chat.reasoningProcess',{count:message.reasoning.length.toLocaleString()})} <ChevronRight size={13} className="inline"/></summary><div className="mt-2 whitespace-pre-wrap">{message.reasoning}</div></details>}
   <div className={'chat-message-content min-w-0 break-words text-[#3A342D] [&_p]:my-2 [&_pre]:overflow-x-auto [&_pre]:rounded-xl [&_pre]:bg-slate-950 [&_pre]:p-4 [&_pre]:text-slate-100 [&_code]:break-words [&_ul]:list-disc [&_ul]:pl-6 '+(human?'chat-message-content--user':'chat-message-content--assistant')}>{human?message.content:<ChatRichText text={message.content||(!human&&message.tool_calls?.length?t('chat.toolRequest'):'')}/>}</div>
   {human&&attachments.length>0&&<div className="mt-2 flex flex-wrap gap-2">
    {attachments.map(item=>item.isImage
     ?<a key={item.id} href={item.url} target="_blank" rel="noreferrer" title={item.name} className="block overflow-hidden rounded-lg border border-divider"><img src={item.url} alt={item.name} className="h-24 w-24 object-cover"/></a>
     :<a key={item.id} href={item.url} target="_blank" rel="noreferrer" className="rounded-lg border border-divider bg-white px-2 py-1 text-[11px] text-[#5C544A] hover:bg-[#FAF7F1]">{item.name}</a>)}
   </div>}
   {!human&&message.content&&<Button className="inline-flex items-center gap-1.5 rounded-lg px-2 py-1 text-[11px] text-muted hover:bg-[#F3EEE5]" onClick={()=>navigator.clipboard.writeText(message.content)} title={t('chat.copy')}><Copy size={14}/></Button>}
   {!human&&<MessageInfo message={message} active={active}>{typeof message.metadata?.executionId==='string'&&<ExecutionInsights executionId={message.metadata.executionId} compact/>}</MessageInfo>}
  </div>
 </div>
}
