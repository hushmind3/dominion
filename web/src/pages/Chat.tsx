import {resolveConversationRoute} from '../features/conversations/resolveRoute'
import {ConversationComposer,ComposerSendButton} from '../components/chat/ConversationComposer'
import {ChatWelcome} from '../components/chat/ChatWelcome'
import {McpQuickConnect} from '../components/chat/McpQuickConnect'
import {ConversationTargetPicker} from '../components/chat/ConversationTargetPicker'
import {ConversationSettings} from '../components/ConversationSettings'
import {ChatRichText} from '../components/ChatRichText'
import {usePersistentConversationToggle,usePersistentReasoningEffort,usePersistentKoreanReasoning} from '../hooks/usePersistentConversationPreference'
import {compatibleConversationEffort} from '../conversationPreferences'
import {ExecutionPermissionsSetting} from '../components/ExecutionPermissions'
import {MessageInfo} from '../components/chat/MessageInfo'
import {ExecutionInsights} from '../components/ExecutionInsights'
import {createPortal} from 'react-dom'
import {Button} from '../components/Primitives'
import {FormEvent,useEffect,useLayoutEffect,useRef,useState,type ChangeEvent,type KeyboardEvent} from 'react'
import {ArrowDown,ArrowRight,ArrowUp,BrainCircuit,Check,ChevronRight,CircleHelp,Copy,Menu,LoaderCircle,Paperclip,ShieldAlert,Sparkles,Square,TerminalSquare,Wrench,X} from 'lucide-react'
import {Agent,Approval,Attachment,ChatMessage,ChatSession,Provider,del,get,post,resolveApproval,streamChat,uploadAttachment} from '../api'
import {Action,refresh,useRemote} from '../components/UI'
import {SlashMenu,slashCandidates,slashCommandLabel,slashCommands,type SlashChoice} from '../components/SlashMenu'
import {parseMcpQuickCommand,findInstalledMcp,validRemoteMcpUrl,type InstalledMcp,type RegistryMcp,type McpAvailableTool} from '../features/mcp/quickCommands'
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
export function ChatPage({sessionId,initialAgent,initialProvider,initialModel,onSession,openModels,openSettings,openExtensions,switchMode,settingsTarget,active=true,onOpenNavigation,newConversationRevision=0}:{sessionId?:string;initialAgent?:string;initialProvider?:string;initialModel?:string;onSession:(x:string|undefined)=>void;openModels:()=>void;openSettings:()=>void;openExtensions:()=>void;switchMode:(mode:'chat'|'rooms')=>void;settingsTarget?:HTMLElement|null;active?:boolean;onOpenNavigation?:()=>void;newConversationRevision?:number}){
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
 const [pickerRevision,setPickerRevision]=useState(0);
 const chooseAI=()=>setPickerRevision(value=>value+1);
 const [agentChoice,setAgentChoice]=useState<{scope?:string;id:string}>();
 const [chatModeOverride,setChatModeOverride]=useState<{scope?:string;mode:'chat'|'agent'}>();
 const [override,setOverride]=useState<ModelOverride>();
 const defaultAgent=agents.value.find(agent=>agent.id==='default')||agents.value.find(agent=>agent.name==='기본값')||agents.value.find(agent=>agent.id==='general');
 const agentMode=(chatModeOverride&&chatModeOverride.scope===current?chatModeOverride.mode:
  current?(selectedSession?.agentId?'agent':'chat'):(initialAgent?'agent':'chat'))==='agent';
 const agentId=agentMode?((agentChoice?.scope===current?agentChoice?.id:undefined)??selectedSession?.agentId??(!current?initialAgent:undefined)??defaultAgent?.id??''):'';
 const activeAgent=agents.value.find(agent=>agent.id===agentId);
 const manual=override&&override.scope===current?override:
  !current&&initialProvider?{providerId:initialProvider,model:initialModel??''}:undefined;
 const restoringRoute=sessions.loading&&!manual&&!activeAgent;
 const route=resolveConversationRoute({providers:providers.value,sessions:sessions.value,session:selectedSession,
  agent:activeAgent,manual,restoring:restoringRoute});
 const {providerId,model,provider:selectedProvider,providers:enabledProviders}=route;
 const catalogue=useProviderModels(route.configured?providerId:'',active);
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
 const [mcpQuickOpen,setMcpQuickOpen]=useState(false);
 const [mcpInstalled,setMcpInstalled]=useState<InstalledMcp[]>([]);
 const [mcpResults,setMcpResults]=useState<RegistryMcp[]>([]);
 const [mcpQuickBusy,setMcpQuickBusy]=useState(false);
 const [mcpNotice,setMcpNotice]=useState('');
 const [mcpLastQuery,setMcpLastQuery]=useState('');
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
 };
 const resetLocal=()=>{
  onSession(undefined);setAgentChoice(undefined);setChatModeOverride(undefined);setOverride(undefined);
  discardPendingImages();
  if(!current)setInput('');
  setDismissedError(undefined);setError('');setCommandInfo('');setSlashOpen(false);
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
   const outcome=await post<{cancelled:boolean;stopped:boolean}>('/api/sessions/'+encodeURIComponent(id)+'/cancel',{});
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
 // MCP management stays in the native MCP registry/API rather than creating
 // a second plugin runtime. Tool discovery proves a connection; saving alone does not.
 const loadQuickMcp=async()=>{
  const items=await get<InstalledMcp[]>('/api/mcp');
  setMcpInstalled(items);
  return items;
 };
 const verifyQuickMcp=async(record:InstalledMcp)=>{
  try{
   const discovered=await get<McpAvailableTool[]>('/api/mcp/discover');
   const own=discovered.filter(tool=>tool.mcpId===record.id&&!tool.error);
   setMcpNotice(own.length
    ?record.name+' · 연결 성공 · '+own.length+'개 도구 사용 가능'
    :record.name+' · 등록 완료. 연결 확인이 필요합니다. 인증 또는 서버 설정을 확인하세요.');
  }catch(e){setMcpNotice(record.name+' · 등록 완료. 연결 확인 실패: '+(e as Error).message);}
 };
 const registerQuickMcp=async(entry:RegistryMcp)=>{
  if(mcpQuickBusy)return;
  setMcpQuickOpen(true);setMcpQuickBusy(true);setMcpNotice('MCP 연결 정보를 확인하는 중…');
  try{
   const existing=await loadQuickMcp();
   if(existing.some(record=>record.name.toLocaleLowerCase()===entry.name.toLocaleLowerCase()))
    throw Error('이미 등록된 MCP입니다: '+entry.name);
   const remote=Boolean(entry.url&&validRemoteMcpUrl(entry.url));
   if(entry.url&&!remote)throw Error('원격 MCP는 HTTPS 주소와 인증정보가 없는 URL만 연결할 수 있습니다.');
   if(!remote&&!entry.packageName)throw Error('이 항목에는 연결 가능한 MCP 주소 또는 패키지가 없습니다.');
   if(!remote){
    if(!/^(@[a-z0-9_.-]+\/)?[a-z0-9_.-]+(?:@[a-z0-9_.-]+)?$/i.test(entry.packageName||''))
     throw Error('MCP 패키지 이름을 확인하세요.');
    if(!window.confirm(entry.name+' 설치 시 외부 npm 프로그램이 실행됩니다. 설치하시겠습니까?')){
     setMcpNotice('설치를 취소했습니다.');return;
    }
   }
   const payload=remote
    ?{name:entry.name,transport:'http',url:entry.url,enabled:true}
    :{name:entry.name,transport:'stdio',command:'npx',args:['-y',entry.packageName],enabled:true};
   const saved=await post<InstalledMcp>('/api/mcp',payload);
   await loadQuickMcp();
   refresh('/api/mcp','/api/tools');
   setMcpNotice(saved.name+' · 등록 완료. 실제 연결을 확인하는 중…');
   await verifyQuickMcp(saved);
  }catch(e){setMcpNotice('MCP 연결 실패: '+(e as Error).message);}
  finally{setMcpQuickBusy(false);}
 };
 const toggleQuickMcp=async(record:InstalledMcp,enabled:boolean)=>{
  if(mcpQuickBusy)return;
  setMcpQuickBusy(true);
  try{
   await post('/api/mcp',{...record,enabled});
   await loadQuickMcp();refresh('/api/mcp','/api/tools');
   setMcpNotice(record.name+' · '+(enabled?'활성화했습니다.':'비활성화했습니다.'));
  }catch(e){setMcpNotice('변경 실패: '+(e as Error).message);}
  finally{setMcpQuickBusy(false);}
 };
 const removeQuickMcp=async(record:InstalledMcp)=>{
  if(mcpQuickBusy||!window.confirm(record.name+' MCP 연결을 삭제하시겠습니까?'))return;
  setMcpQuickBusy(true);
  try{
   await del('/api/mcp/'+encodeURIComponent(record.id));
   await loadQuickMcp();refresh('/api/mcp','/api/tools');
   setMcpNotice(record.name+' · 연결을 삭제했습니다.');
  }catch(e){setMcpNotice('삭제 실패: '+(e as Error).message);}
  finally{setMcpQuickBusy(false);}
 };
 const executeMcpCommand=async(input:string)=>{
  const action=parseMcpQuickCommand(input);
  if(action.kind==='invalid')throw Error(action.message);
  setMcpQuickOpen(true);
  if(action.kind==='add'){await registerQuickMcp({name:action.name,url:action.url});return;}
  setMcpQuickBusy(true);
  try{
   const items=await loadQuickMcp();
   if(action.kind==='open'||action.kind==='list'){
    setMcpNotice('등록된 MCP '+items.length+'개 · 검색으로 새 서버를 연결할 수 있습니다.');return;
   }
   if(action.kind==='help'){
    setMcpNotice('/mcp search 검색어 · /mcp add 이름 https://주소 · /mcp list · /mcp enable 이름 · /mcp disable 이름 · /mcp remove 이름');return;
   }
   if(action.kind==='search'){
    setMcpLastQuery(action.query);
    setMcpResults([]);
    setMcpNotice('공식 MCP 레지스트리에서 검색 중…');
    const results=await get<RegistryMcp[]>('/api/mcp/catalog?q='+encodeURIComponent(action.query));
    setMcpResults(results);
    setMcpNotice(results.length?results.length+'개 검색됨 · 연결할 MCP를 선택하세요.':'검색 결과가 없습니다.');
    return;
   }
   const target=findInstalledMcp(items,action.name);
   if(!target)throw Error('등록된 MCP를 찾을 수 없습니다: '+action.name);
   if(action.kind==='remove'){
    if(!window.confirm(target.name+' MCP 연결을 삭제하시겠습니까?')){setMcpNotice('삭제를 취소했습니다.');return;}
    await del('/api/mcp/'+encodeURIComponent(target.id));
    await loadQuickMcp();refresh('/api/mcp','/api/tools');
    setMcpNotice(target.name+' · 연결을 삭제했습니다.');return;
   }
   await post('/api/mcp',{...target,enabled:action.kind==='enable'});
   await loadQuickMcp();refresh('/api/mcp','/api/tools');
   setMcpNotice(target.name+' · '+(action.kind==='enable'?'활성화했습니다.':'비활성화했습니다.'));
  }catch(e){setMcpNotice('MCP 명령 실패: '+(e as Error).message);}
  finally{setMcpQuickBusy(false);}
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
   if(name==='history'){onOpenNavigation?.();await sessions.reload();setCommandInfo(t('chat.cmd.history'));return;}
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
  if(!providerId||!route.configured){chooseAI();return;}
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
    sessionId:id,clientTurnId,agentId:agentMode?agentId:undefined,mode:agentMode||toolsEnabled?'agent':'chat',
    providerId:agentMode?manual?.providerId:providerId,model:agentMode?manual?.model:model,input:text,
    toolsEnabled,toolsExecutionEnabled:toolsEnabled,autoLearning:agentMode&&autoLearn,attachmentIds,
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
 const emptyConversation=!messages.length&&!showOptimisticOwner&&!(busy&&viewingTurn);
 useEffect(()=>{if(!active){setSlashOpen(false)}},[active]);
 const lastNewConversation=useRef(0);
 useEffect(()=>{if(lastNewConversation.current!==newConversationRevision){lastNewConversation.current=newConversationRevision;resetLocal()}},[newConversationRevision]);
 return <div className="conversation-page flex h-full min-h-0">
  <div className="conversation-view relative flex h-full min-h-0 min-w-0 flex-1 flex-col">

   <div ref={scrollRef} onScroll={onScroll} data-chat-scroll aria-label={t('ui.label.conversations')} className="min-h-0 flex-1 overflow-y-auto overscroll-contain">{current&&transcript.loading?<div className="flex h-full min-h-52 items-center justify-center gap-3 text-sm text-muted" role="status"><LoaderCircle size={20} className="animate-spin"/>{t('history.loading')}</div>:!messages.length&&!showOptimisticOwner&&!(busy&&viewingTurn)?<ChatWelcome loading={sessions.loading||providers.loading||catalogue.loading} error={providers.error} onRetry={()=>{void providers.reload();void sessions.reload()}}
    />
   :<div className={'chat-transcript mx-auto flex min-h-full w-full max-w-conversation flex-col gap-7 px-4 py-7 sm:px-7 '+(busy&&viewingTurn?'chat-transcript--responding':'')}>{messages.filter(m=>m.role!=='tool').map(m=><Message key={m.id} active={active} message={m} agentName={agents.value.find(a=>a.id===m.agent_id)?.name} showReasoning={showReasoning} />)}{showOptimisticOwner&&<Message key="optimistic-owner" message={{id:'optimistic-owner',session_id:current||'',role:'user',content:optimisticOwner!.content,created_at:''}}/>}{viewingTurn&&(live||thought||busy)&&<div className="chat-message chat-message--assistant flex min-w-0 items-start gap-3 justify-start assistant" data-message data-message-role="assistant"><div className="grid size-9 shrink-0 place-items-center rounded-xl bg-canvas text-ink ai"><Sparkles size={17}/></div><div className="chat-message-body relative min-w-0 flex-1"><div className="mb-2 flex items-center gap-2 text-xs font-bold text-ink">DOMINION <span>{busy?t('ui.label.live'):savedMessageId?'대화 내역 동기화 중':'중단된 응답 · 미저장'}</span>{busy&&thought&&!showReasoning&&<span className="inline-flex items-center gap-1 rounded bg-canvas px-1.5 py-0.5 text-[10px] font-medium text-muted"><BrainCircuit size={11}/> {t('chat.thinkingNow')}</span>}</div>{showReasoning&&thought&&<details className="mb-3 max-h-56 overflow-y-auto rounded-xl border border-divider bg-canvas p-3 text-[11px] leading-5 text-muted" open={thinking}><summary className="cursor-pointer select-none" onClick={e=>{e.preventDefault();setThinking(x=>!x)}}><BrainCircuit size={14} className="inline"/> {t('chat.reasoningProcess',{count:thought.length.toLocaleString()} )} <ChevronRight size={13} className="inline"/></summary><div className="mt-2 whitespace-pre-wrap">{thought}</div></details>}{live&&<div className="chat-message-content min-w-0 break-words text-base leading-7 text-ink [&_p]:my-2 [&_pre]:overflow-x-auto [&_pre]:rounded-xl [&_pre]:bg-ink [&_pre]:p-4 [&_pre]:text-surface [&_code]:break-words [&_ul]:list-disc [&_ul]:pl-6"><ChatRichText text={live}/></div>}{!busy&&!savedMessageId&&live&&<Button type="button" variant="ghost" aria-label="중단된 답변 복사" onClick={()=>void navigator.clipboard.writeText(live)} className="mt-2 inline-flex items-center gap-1.5 rounded-lg px-2 py-1 text-[11px] text-muted hover:bg-canvas"><Copy size={14}/>내용 복사</Button>}{busy&&!live&&<div className="flex items-center gap-2 text-xs text-muted"><i/><i/><i/></div>}</div></div>}{viewingTurn&&toolStatus&&<div className="inline-flex items-center gap-2 rounded-lg bg-canvas px-3 py-1.5 text-xs text-muted"><TerminalSquare size={15}/>{toolStatus}</div>}</div>}</div>
   {moreBelow&&<Button type="button" variant="ghost" onClick={jumpDown} aria-label="최신 메시지로 이동" className="chat-scroll-to-bottom absolute bottom-[132px] left-1/2 z-20 -translate-x-1/2 inline-flex items-center gap-2 rounded-full border border-divider bg-surface px-3 py-2 text-[12px] font-semibold text-muted shadow-md hover:bg-canvas"><ArrowDown size={16}/>최신 메시지</Button>}
   <div className="conversation-composer-wrap z-10 mt-auto w-full shrink-0 bg-surface px-3 pb-3 pt-2 sm:px-6 sm:pb-6"><div className="mx-auto w-full max-w-conversation">{approval&&<div className="mb-3 flex flex-wrap items-center justify-between gap-3 rounded-xl border border-warning/20 bg-warning-soft p-3 text-xs text-warning"><div><ShieldAlert size={20}/><span><strong>{t('chat.approval')}</strong><small>{approval.tool} · {JSON.stringify(approval.args??{}).slice(0,150)}</small></span></div><div><Action variant="secondary" disabled={approvalBusy} onClick={()=>approve(false)}>{t('chat.deny')}</Action><Action disabled={approvalBusy} onClick={()=>approve(true)}><Check size={15}/> {t('chat.approve')}</Action></div></div>}
    {visibleAlert&&<div role="alert" className="mb-3 rounded-xl border border-danger/20 bg-danger-soft p-3 text-xs text-danger"><CircleHelp size={16}/>{visibleAlert}<Button type="button" aria-label="오류 닫기" onClick={()=>setDismissedError({session:current,message:visibleAlert})}><X size={14}/></Button></div>}
    {modelError&&!model&&<div className="mb-3 text-xs text-muted">{t('welcome.modelError')} <Button onClick={chooseAI}>{t('chat.modelPick')} <ArrowRight size={13}/></Button></div>}
    {commandInfo&&<div role="status" className="mb-3 max-h-36 overflow-auto whitespace-pre-wrap rounded-xl border border-divider bg-brand-soft p-3 text-xs leading-6 text-ink">{commandInfo}</div>}
    {toolsEnabled&&selectedProvider&&(((selectedProvider.local||selectedProvider.kind==='ollama')&&selectedProvider.toolMode==='compat')||selectedProvider.capabilities?.toolCalling===false||selectedProvider.modelCapabilities?.[model]?.toolCalling===false)&&<div role="status" className="mb-2 flex flex-wrap items-center gap-2 rounded-lg border border-divider bg-canvas px-3 py-2 text-xs text-muted">
      <Wrench size={15} className="shrink-0"/>
      <span className="min-w-0 flex-1">{selectedProvider.capabilities?.toolCalling===false||selectedProvider.modelCapabilities?.[model]?.toolCalling===false
       ?(loc==='en'?'This connection declares tool calling unavailable.':'연결 설정에서 이 모델의 도구 호출이 미지원으로 지정돼 있습니다.')
       :(loc==='en'?'Legacy text-based tools are active. Native tool calling can be configured in AI connections.':'이 연결은 구형 텍스트 도구 방식입니다. 정식 Tool Calling은 AI 연결에서 설정할 수 있습니다.')}</span>
      <Button type="button" variant="ghost" size="sm" className="!min-h-8 !shrink-0 !text-xs" onClick={openModels}>{loc==='en'?'Connections':'연결 설정'}</Button>
    </div>}
    <div className="relative">
    {pendingImages.length>0&&<div className="mb-2 flex flex-wrap items-center gap-2">
     {pendingImages.map(item=><div key={item.key} className="relative h-16 w-16 overflow-hidden rounded-lg border border-divider bg-canvas">
      <img src={item.preview} alt={item.file.name} className="h-full w-full object-cover"/>
      <button type="button" onClick={()=>removeImage(item.key)} aria-label={t('chat.attachRemove')} title={t('chat.attachRemove')} className="absolute right-0.5 top-0.5 grid size-5 place-items-center rounded-full bg-black/60 text-white hover:bg-black/80"><X size={12}/></button>
     </div>)}
    </div>}
    {slashOpen&&<SlashMenu items={slashItems} active={slashActive} onChoose={chooseSlash}/>}
    <ConversationComposer value={input} onChange={next=>{setInput(next);setSlashOpen(next.startsWith('/')&&!next.includes('\n'));setSlashActive(0)}}
     onSend={send} inputRef={inputRef} label={t('chat.messageAria')} placeholder={t('chat.inputPlaceholder')} onKeyDown={appendKey} slashOpen={slashOpen}
     options={<>
       <Button variant="ghost" size="icon" className="!h-7 !w-7 md:hidden" onClick={onOpenNavigation} aria-label={t('chat.listExpand')} title={t('chat.recent')}><Menu size={17}/></Button>
       <ConversationTargetPicker openRevision={pickerRevision} active={active} providers={enabledProviders} agents={agents.value} providerId={providerId} model={model} agentId={agentMode?agentId:''} catalogue={catalogue}
        onModel={(provider,chosenModel)=>{setChatModeOverride({scope:current,mode:agentMode||Boolean(selectedSession?.agentId)?'agent':'chat'});if(!agentMode&&!selectedSession?.agentId)setAgentChoice(undefined);setRoute(provider,chosenModel)}} onAgent={chooseAgent}/>
       <Button type="button" variant="ghost" size="sm" aria-pressed={toolsEnabled}
        aria-label={loc==='en'?'Use built-in tools':'기본 도구 사용'}
        title={toolsEnabled?(loc==='en'?'Built-in search, files and code tools are ready. Connected MCP plugins are managed separately.':'검색·파일·코드 등 내장 도구를 사용할 수 있습니다. MCP는 확장 기능에서 별도로 관리합니다.'):(loc==='en'?'Enable built-in tools':'검색·파일·코드 도구 켜기')}
        onClick={()=>setToolsEnabled(!toolsEnabled)} disabled={busy}
        className={'!min-h-8 !gap-1.5 !rounded-lg !px-2 !text-xs '+(toolsEnabled?'!bg-canvas !font-medium !text-ink':'!text-muted')}>
        <Wrench size={15}/><span>{loc==='en'?(toolsEnabled?'Tools on':'Tools off'):(toolsEnabled?'도구 켜짐':'도구 꺼짐')}</span>
       </Button>

       <Button type="button" variant="ghost" size="icon" className="!h-7 !w-7 !border-0 !bg-transparent !text-muted" title={t('chat.attach')} aria-label={t('chat.attach')} onClick={()=>imageInputRef.current?.click()} disabled={busy||uploading||Boolean(selectedSession?.roomId)}>{uploading?<LoaderCircle size={16} className="animate-spin"/>:<Paperclip size={16}/>}</Button>
       <input ref={imageInputRef} type="file" accept={ATTACH_ALLOWED.join(',')} multiple className="sr-only" aria-label={t('chat.attach')} onChange={onPickImages}/>



</>} actions={<>
       {busy?<ComposerSendButton variant="danger" type="button" onClick={()=>void stopChat()} disabled={stopping} title={stopping?t('chat.stopping'):t('chat.stop')} aria-label={stopping?t('chat.stopping'):t('chat.stop')}>{stopping?<LoaderCircle size={18} className="animate-spin"/>:<Square size={18} fill="currentColor"/>}</ComposerSendButton>
       :<ComposerSendButton disabled={stopping||uploading||Boolean(selectedSession?.roomId)||(Boolean(current)&&sessions.loading)||(!input.trim()&&!pendingImages.length)} type="submit" title={t('chat.send')}><ArrowUp size={19}/></ComposerSendButton>}</>}/>
</div>
   </div></div>
  </div>
  {settingsTarget&&createPortal(<ConversationSettings permissionControl={agentMode||toolsEnabled?<ExecutionPermissionsSetting/>:undefined} reasoningControl={<ReasoningSelect value={effectiveReasoningEffort} onChange={effort=>{setReasoningEffort(effort);setThinkingEnabled(effort!=='none')}} model={selectedModel} requireCatalog={selectedProvider?.kind==='codex'} className="w-full"/>} showReasoning={showReasoning} setShowReasoning={setShowReasoning} reasoningKo={reasoningKo} setReasoningKo={setReasoningKo} thinkingEnabled={effectiveThinkingEnabled} setThinkingEnabled={setThinkingEnabled} canDisableThinking={canDisableThinking} reasoningEffort={effectiveReasoningEffort} setReasoningEffort={setReasoningEffort} toolsEnabled={toolsEnabled} setToolsEnabled={setToolsEnabled} autoLearn={agentMode&&autoLearn} setAutoLearn={setAutoLearn} optionsBusy={busy} onManageExtensions={openExtensions} learning={learning} learnDisabled={!agentMode||!current||Boolean(selectedSession?.roomId)||busy} onLearn={learnSession}/>,settingsTarget)}
 </div>
}
function Message({message,agentName,showReasoning,active=true}:{message:ChatMessage;agentName?:string;showReasoning?:boolean;active?:boolean}){
 const [open,setOpen]=useState(true);const human=message.role==='user';
 const attachments=Array.isArray(message.metadata?.attachments)?message.metadata.attachments as Attachment[]:[];
 return <div className={'chat-message flex w-full min-w-0 items-start gap-3 '+(human?'chat-message--user justify-end':'chat-message--assistant justify-start')} data-message data-message-role={human?'user':'assistant'}>
  {!human&&<div className="grid size-9 shrink-0 place-items-center rounded-xl bg-brand-soft text-brand-hover"><Sparkles size={17}/></div>}
  <div className={'chat-message-body relative min-w-0 '+(human?'flex max-w-[88%] flex-col items-end sm:max-w-[78%]':'flex-1')}>
   <div className={'chat-message-author mb-2 flex items-center gap-2 text-xs font-bold text-ink '+(human?'justify-end':'')}>{human?t('ui.you'):agentName||'DOMINION'}</div>
   {showReasoning&&message.reasoning&&<details className="mb-3 max-h-56 overflow-y-auto rounded-xl border border-divider bg-canvas p-3 text-[11px] leading-5 text-muted" open={open}><summary className="cursor-pointer select-none" onClick={e=>{e.preventDefault();setOpen(v=>!v)}}><BrainCircuit size={14} className="inline"/> {t('chat.reasoningProcess',{count:message.reasoning.length.toLocaleString()})} <ChevronRight size={13} className="inline"/></summary><div className="mt-2 whitespace-pre-wrap">{message.reasoning}</div></details>}
   <div className={'chat-message-content min-w-0 break-words text-base leading-7 text-ink [&_p]:my-2 [&_pre]:overflow-x-auto [&_pre]:rounded-xl [&_pre]:bg-ink [&_pre]:p-4 [&_pre]:text-surface [&_code]:break-words [&_ul]:list-disc [&_ul]:pl-6 '+(human?'max-w-full whitespace-pre-wrap rounded-2xl rounded-br-md border border-divider bg-canvas px-4 py-3':'w-full whitespace-normal')}>{human?message.content:<ChatRichText text={message.content||(!human&&message.tool_calls?.length?t('chat.toolRequest'):'')}/>}</div>
   {human&&attachments.length>0&&<div className="mt-2 flex flex-wrap gap-2">
    {attachments.map(item=>item.isImage
     ?<a key={item.id} href={item.url} target="_blank" rel="noreferrer" title={item.name} className="block overflow-hidden rounded-lg border border-divider"><img src={item.url} alt={item.name} className="h-24 w-24 object-cover"/></a>
     :<a key={item.id} href={item.url} target="_blank" rel="noreferrer" className="rounded-lg border border-divider bg-surface px-2 py-1 text-[11px] text-muted hover:bg-canvas">{item.name}</a>)}
   </div>}
   {!human&&message.content&&<Button className="inline-flex items-center gap-1.5 rounded-lg px-2 py-1 text-[11px] text-muted hover:bg-canvas" onClick={()=>navigator.clipboard.writeText(message.content)} title={t('chat.copy')}><Copy size={14}/></Button>}
   {!human&&<MessageInfo message={message} active={active}>{typeof message.metadata?.executionId==='string'&&<ExecutionInsights executionId={message.metadata.executionId} compact/>}</MessageInfo>}
  </div>
 </div>
}
