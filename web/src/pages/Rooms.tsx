import {RoomWelcome, RoomFirstMessage} from '../components/rooms/RoomWelcome'
import {useChatDraft} from '../hooks/useChatDraft'
import {ConversationComposer, ComposerSendButton} from '../components/chat/ConversationComposer'
import {ExecutionPermissionsSetting} from '../components/ExecutionPermissions'
import {RoomActions} from '../components/rooms/RoomActions'
import {RoomAvatar as Avatar} from '../components/rooms/RoomAvatar'
import {RoomMessage as TranscriptMessage, RoomMessageBody as MessageBody, RoomMessageFrame, RoomTyping} from '../components/rooms/RoomMessage'
import {ConversationSettings} from '../components/ConversationSettings'
import {usePersistentConversationToggle, usePersistentReasoningEffort, usePersistentKoreanReasoning} from '../hooks/usePersistentConversationPreference'
import {matchesRoomRefresh, type RefreshScope} from '../refreshEvents'
import {createPortal} from 'react-dom'
import {useAnchoredPanel} from '../hooks/useAnchoredPanel'
import {TeamWorkflowPanel} from '../components/TeamWorkflowPanel'
import {RoomTeamLauncher} from '../components/RoomTeamLauncher'
import {RunEventTimeline} from '../components/RunEventTimeline'
import {useTranscriptScroll} from '../hooks/useTranscriptScroll'
import {useCallback, useEffect, useMemo, useRef, useState, type ChangeEvent, type FormEvent} from 'react'
import {Activity, ArrowDown, ArrowRight, ArrowUp, Workflow, BookOpenText, Check, ChevronDown, LoaderCircle, Paperclip, Plus, RefreshCw, RotateCcw, ShieldCheck, Square, TerminalSquare, Trash2, UsersRound, X} from 'lucide-react'
import {get, post, del, resolveApproval, uploadAttachment, type Agent, type ChatMessage, type ModelOption, type Provider} from '../api'
import {Button, TextArea, TextInput, SelectInput} from '../components/Primitives'
import {DialogBackdrop, DialogPanel} from '../components/Dialog'
import {FormField, Notice, refresh, useRemote} from '../components/UI'
import {ReasoningSelect} from '../components/ReasoningSelect'
import './Rooms.css'
import '../styles/group-workspace.css'
import {t, useLocale, getLocale, type LocaleKey} from '../i18n'
import {translateError} from '../i18n/errors'

const MAX_ROOM_PARTICIPANTS=32

type RoutingMode='discussion'|'round_robin'|'mentioned'
type RoomModel={providerId:string;model:string;reasoningEffort?:string}
type RoomConfig={configRevision?:number;id:string;name:string;topic?:string;description?:string;ownerName?:string;agentIds:string[];routingMode?:RoutingMode;rounds?:number;maxTurns?:number;tokenBudget?:number;contextBudgetTokens?:number;toolsEnabled?:boolean;autoLearning?:boolean;autoModels?:boolean;models?:RoomModel[];reasoningEffort?:string;sessionId?:string;status?:string;activeAgentId?:string;pauseReason?:string;updatedAt?:string}
type RoomDraft={expectedConfigRevision?:number;id?:string;name:string;topic:string;ownerName:string;agentIds:string[];routingMode:RoutingMode;rounds:number;maxTurns:number;tokenBudget:number;contextBudgetTokens:number;autoModels:boolean;toolsEnabled:boolean;autoLearning:boolean;models:RoomModel[];reasoningEffort:string}
type Job={id:string;kind:string;status:string;stepIndex?:number;stepTotal?:number;error?:string;createdAt?:string;input?:{roomId?:string;sessionId?:string};result?:{memories?:number;status?:string};roomState?:{activeAgentId?:string;inFlight?:boolean}}
type Execution={id:string;status:string;agentId?:string;sessionId?:string;toolCalls?:number;tokens?:number;reason?:string}
type Operation={id:string;runId?:string;tool?:string;toolName?:string;name?:string;status?:string;result?:unknown;error?:string;arguments?:unknown}
type Approval={id:string;tool:string;args?:unknown;status:string}
type TokenUsage={promptTokens?:number;completionTokens?:number;totalTokens?:number;calls?:number;estimatedCalls?:number}
type FeedState={room?:RoomConfig;sessionId?:string;jobs:Job[];executions:Execution[];learningJobs:Job[];operations:Operation[];approvals:Approval[];activeAgentId?:string;status?:string;queueDepth?:number;usage?:TokenUsage}
type LiveDraft={turnId:string;agentId:string;text:string;reasoning:string;tool:string;error:string;done:boolean;messageId?:string}
type RoomEvent={roomId:string;agentId:string;turnId?:string;event:{type:string;text?:string;name?:string;message?:string;messageId?:string;terminal?:boolean;memories?:number;skill?:boolean;inserted?:number;updated?:number;promoted?:number}}
type PendingRoomImage={key:string;file:File;preview:string;attachmentId?:string}
const ROOM_ATTACH_ALLOWED=['image/png','image/jpeg','image/webp','image/gif']
const ROOM_ATTACH_LIMIT=8
const emptyLive=(turnId:string,agentId:string):LiveDraft=>({turnId,agentId,text:'',reasoning:'',tool:'',error:'',done:false})
const initialFeed=():FeedState=>({jobs:[],executions:[],learningJobs:[],operations:[],approvals:[]})
const labels:Record<string,LocaleKey>={queued:'rooms.st.waiting',running:'rooms.st.running',completed:'rooms.st.completed',idle:'rooms.st.idle',paused:'rooms.st.stopped',stopped:'rooms.st.stopped',cancelled:'rooms.st.stopped',failed:'rooms.st.failed',needs_review:'rooms.st.needsReview',interrupted:'rooms.st.interrupted'}
const modes:Record<RoutingMode,{label:LocaleKey;description:LocaleKey}>={discussion:{label:'rooms.mode.discussion.label',description:'rooms.mode.discussion.desc'},round_robin:{label:'rooms.mode.round_robin.label',description:'rooms.mode.round_robin.desc'},mentioned:{label:'rooms.mode.mentioned.label',description:'rooms.mode.mentioned.desc'}}
const active=(status?:string)=>status==='queued'||status==='running'
const stamp=(value?:string)=>{if(!value)return '';const normalized=value.includes('T')?value:value.replace(' ','T')+'Z';const date=new Date(normalized);return Number.isNaN(date.getTime())?'':date.toLocaleTimeString(getLocale()==='en'?'en-US':'ko-KR',{hour:'2-digit',minute:'2-digit'})}
const draftOf=(room?:RoomConfig):RoomDraft=>({expectedConfigRevision:room?.configRevision??0,id:room?.id,name:room?.name||'',topic:room?.topic||room?.description||'',ownerName:room?.ownerName||t('rooms.me'),agentIds:room?.agentIds||[],routingMode:room?.routingMode||'discussion',rounds:room?.rounds||1,maxTurns:room?.maxTurns??0,tokenBudget:room?.tokenBudget??0,contextBudgetTokens:room?.contextBudgetTokens??2000,toolsEnabled:room?.toolsEnabled===true,autoLearning:room?.autoLearning===true,autoModels:room?.autoModels!==false,models:room?.models||[],reasoningEffort:room?.reasoningEffort||''})
export function RoomsPage({openSession,settingsTarget,active:visibleView=true,onOpenNavigation,selectedRoom,onSelectRoom,newRoomRevision=0}:{openSession:(id:string)=>void;settingsTarget?:HTMLElement|null;active?:boolean;onOpenNavigation?:()=>void;selectedRoom?:string;onSelectRoom?:(id:string)=>void;newRoomRevision?:number}){
 useLocale()
 const rooms=useRemote<RoomConfig[]>('/api/rooms',[],visibleView)
 const agents=useRemote<Agent[]>('/api/agents',[],visibleView)
 const providers=useRemote<Provider[]>('/api/providers',[],visibleView)
 const selected=selectedRoom||''
 const setSelected=(id:string)=>onSelectRoom?.(id)
 const [creating,setCreating]=useState<RoomDraft|null>(null)
 const lastNewRoom=useRef(0)
 useEffect(()=>{if(newRoomRevision!==lastNewRoom.current){lastNewRoom.current=newRoomRevision;setCreating(draftOf())}},[newRoomRevision])
 const [saving,setSaving]=useState(false),[error,setError]=useState('')
 const current=rooms.value.find(room=>room.id===selected)
 useEffect(()=>{if(rooms.loading)return;if(!rooms.value.some(room=>room.id===selected))setSelected(rooms.value[0]?.id||'')},[rooms.value,rooms.loading,selected])
 useEffect(()=>{try{if(selected)localStorage.setItem('dominion.room.selected',selected)}catch{}},[selected])
 const createRoom=async()=>{
  if(!creating||saving)return;setSaving(true);setError('')
  try{const created=await post<RoomConfig>('/api/rooms',{...creating,name:creating.name.trim()||t('rooms.defaultName')});await rooms.reload();setSelected(created.id);setCreating(null);refresh()}
  catch(e){setError((e as Error).message)}finally{setSaving(false)}
 }
 return <div className="rooms-app room-workspace">
  {current?<RoomConversation settingsTarget={settingsTarget} key={current.id} room={current} visibleView={visibleView} agents={agents.value} providers={providers.value} openSession={openSession} onSaved={async()=>{await rooms.reload();refresh()}} onRemoved={async()=>{setSelected('');await rooms.reload()}}/>:rooms.loading?<div className="room-feed-loading" role="status"><LoaderCircle size={20} className="animate-spin"/>{t('rooms.loading')}</div>:<RoomWelcome hasAgents={agents.value.length>0} onCreate={()=>{setCreating(draftOf());setError('')}}/>}
  {(error||rooms.error||agents.error)&&!creating&&<div className="room-global-error"><Notice message={error||rooms.error||agents.error||providers.error} retrying={rooms.refreshing||agents.refreshing||providers.refreshing} onRetry={!error?()=>{void Promise.all([rooms.reload(),agents.reload(),providers.reload()])}:undefined}/></div>}
  {visibleView&&creating&&<DialogBackdrop onDismiss={()=>{if(!saving)setCreating(null)}}><DialogPanel size="wide" className="room-settings-dialog" aria-label={t('rooms.newTitle')}><div className="room-dialog-header"><div><span className="room-eyebrow">{t('ui.label.startConversation')}</span><h2>{t('rooms.who')}</h2></div><Button size="icon" variant="ghost" aria-label={t('rooms.close')} onClick={()=>setCreating(null)} disabled={saving}><X size={18}/></Button></div><div className="room-dialog-body">{!agents.loading&&!agents.value.length&&<div className="room-setup-next-step"><strong>{t('rooms.setupAssistants')}</strong><p>{t('rooms.setupAssistantsDescription')}</p><Button variant="primary" onClick={()=>{window.location.hash='#/agents'}}><Plus size={15}/>{t('rooms.setupAssistantsAction')}</Button></div>}<RoomSettings value={creating} onChange={setCreating} agents={agents.value} providers={providers.value} disabled={saving}/><Notice message={error}/></div><div className="room-dialog-footer"><span>{t('rooms.createHint')}</span><Button variant="primary" onClick={()=>void createRoom()} disabled={saving||!creating.agentIds.length}>{saving?<LoaderCircle size={15} className="animate-spin"/>:<Plus size={15}/>} {t('rooms.create')}</Button></div></DialogPanel></DialogBackdrop>}
 </div>
}

function RoomSettings({value,onChange,agents,providers,disabled=false}:{value:RoomDraft;onChange:(value:RoomDraft)=>void;agents:Agent[];providers:Provider[];disabled?:boolean}){
 useLocale()
 const [advanced,setAdvanced]=useState(false)
 const toggle=(id:string)=>onChange({...value,agentIds:value.agentIds.includes(id)?value.agentIds.filter(item=>item!==id):[...value.agentIds,id]})
 return <div className="room-settings-form">
  <FormField label={t('rooms.name')}><TextInput value={value.name} onChange={e=>onChange({...value,name:e.target.value})} placeholder={t('rooms.namePlaceholder')} disabled={disabled}/></FormField>
  <FormField label={t('rooms.topic')}><TextArea rows={2} value={value.topic} onChange={e=>onChange({...value,topic:e.target.value})} placeholder={t('rooms.topicPlaceholder')} disabled={disabled}/></FormField>

  <div className="room-roster-heading"><strong>{t('rooms.roster')}</strong><span>{t('rooms.rosterCount',{count:value.agentIds.length,max:MAX_ROOM_PARTICIPANTS})}</span></div>
  <div className="room-roster-options">{agents.map((agent,index)=>{const checked=value.agentIds.includes(agent.id);const unavailable=disabled||(!checked&&value.agentIds.length>=MAX_ROOM_PARTICIPANTS);return <label key={agent.id} title={agent.name} className={'room-roster-option '+(checked?'checked':'')}><input type="checkbox" checked={checked} disabled={unavailable} onChange={()=>toggle(agent.id)}/><Avatar agent={agent} size="small" index={index}/><span><strong>{agent.name}</strong></span></label>})}{!agents.length&&<p className="room-note">{t('rooms.noAgents')}</p>}</div>
  <FormField label={t('rooms.mode')}><SelectInput value={value.routingMode} onChange={e=>onChange({...value,routingMode:e.target.value as RoutingMode})} disabled={disabled}>{Object.entries(modes).map(([key,item])=><option key={key} value={key}>{t(item.label)}</option>)}</SelectInput><small className="room-note">{t(modes[value.routingMode].description)}</small></FormField>
  <details className="room-advanced-settings" onToggle={event=>setAdvanced(event.currentTarget.open)}><summary>{t('rooms.advancedSettings')}</summary>{advanced&&<div>
  <FormField label={t('rooms.ownerName')}><TextInput value={value.ownerName} onChange={e=>onChange({...value,ownerName:e.target.value})} placeholder={t('rooms.me')} disabled={disabled}/></FormField>
  <div className="room-settings-numbers"><FormField label={t('rooms.rounds')} hint={t('rooms.roundsHint')}><TextInput type="number" min={1} max={100000} value={value.rounds} onChange={e=>onChange({...value,rounds:Math.min(100000,Math.max(1,Number(e.target.value)||1))})} disabled={disabled||value.routingMode==='round_robin'}/></FormField><FormField label={t('rooms.maxTurns')} hint={t('rooms.maxTurnsHint')}><TextInput type="number" min={0} max={100000} value={value.maxTurns} onChange={e=>onChange({...value,maxTurns:Math.min(100000,Math.max(0,Number(e.target.value)||0))})} disabled={disabled}/></FormField></div>
  <FormField label={t('rooms.tokenBudget')} hint={t('rooms.tokenBudgetHint')}><TextInput type="number" min={0} max={10000000} value={value.tokenBudget} onChange={e=>onChange({...value,tokenBudget:Math.min(10000000,Math.max(0,Number(e.target.value)||0))})} disabled={disabled}/></FormField>
  <FormField label="대화 문맥 목표 (토큰)" hint="동적 대화·기억 선택의 목표값입니다. 페르소나·공통 지시는 압축하지 않으며 이 값보다 커질 수 있습니다."><TextInput type="number" min={2000} max={1000000} value={value.contextBudgetTokens} onChange={e=>onChange({...value,contextBudgetTokens:Math.min(1000000,Math.max(2000,Number(e.target.value)||2000))})} disabled={disabled}/></FormField>
  <FormField label={t('rooms.reasoning')} hint={t('rooms.reasoningHint')}><ReasoningSelect value={value.reasoningEffort} onChange={reasoningEffort=>onChange({...value,reasoningEffort})} defaultLabel={t('rooms.reasoningDefault')} disabled={disabled}/></FormField>
  <label className="room-setting-check"><input type="checkbox" checked={value.autoLearning} onChange={e=>onChange({...value,autoLearning:e.target.checked})} disabled={disabled}/><span><strong>{t('rooms.autoLearn')}</strong><small>{t('rooms.autoLearnDesc')}</small></span></label>
  <RoomModelPicker value={value.models} onChange={models=>onChange({...value,models})} providers={providers} disabled={disabled}/>
  <label className="room-setting-check"><input type="checkbox" checked={value.autoModels} onChange={e=>onChange({...value,autoModels:e.target.checked})} disabled={disabled}/><span><strong>{t('rooms.autoModels')}</strong><small>{t('rooms.autoModelsDesc')}</small></span></label>
  <label className="room-setting-check"><input type="checkbox" checked={value.toolsEnabled} onChange={e=>onChange({...value,toolsEnabled:e.target.checked})} disabled={disabled}/><span><strong>{t('rooms.tools')}</strong><small>{t('rooms.toolsDesc')}</small></span></label>
  </div>}</details>
 </div>
}

function RoomModelPicker({value,onChange,providers,disabled=false}:{value:RoomModel[];onChange:(models:RoomModel[])=>void;providers:Provider[];disabled?:boolean}){
 useLocale()
 const available=providers.filter(provider=>provider.enabled!==false)
 const [providerId,setProviderId]=useState(()=>available[0]?.id||'')
 const [catalog,setCatalog]=useState<Record<string,ModelOption[]>>({})
 const [loading,setLoading]=useState(false),[error,setError]=useState('')
 useEffect(()=>{
  if(!providerId||catalog[providerId])return
  let alive=true
  setLoading(true);setError('')
  get<ModelOption[]>('/api/providers/'+providerId+'/models')
   .then(models=>{if(alive)setCatalog(previous=>({...previous,[providerId]:models}))})
   .catch(reason=>{if(alive)setError((reason as Error).message)})
   .finally(()=>{if(alive)setLoading(false)})
  return()=>{alive=false}
 },[providerId,catalog])
 const has=(provider:string,model:string)=>value.some(item=>item.providerId===provider&&item.model===model)
 const effortOf=(model:string)=>value.find(item=>item.providerId===providerId&&item.model===model)?.reasoningEffort||''
 const setEffort=(model:string,effort:string)=>onChange(value.map(item=>item.providerId===providerId&&item.model===model?{...item,reasoningEffort:effort||undefined}:item))
 const toggle=(model:string)=>onChange(has(providerId,model)?value.filter(item=>!(item.providerId===providerId&&item.model===model)):[...value,{providerId,model}])
 const models=(catalog[providerId]||[]).filter(item=>item.id)
 const picked=value.filter(item=>item.providerId===providerId).length
 return <div className="room-model-picker">
  <div className="room-roster-heading"><strong>{t('rooms.models')}</strong><span>{t('rooms.modelsSelected',{count:value.length})}</span></div>
  <small className="room-note">{t('rooms.modelsDesc')}</small>
  <div className="room-model-picker-controls">
   <SelectInput value={providerId} onChange={e=>setProviderId(e.target.value)} disabled={disabled||!available.length}>{available.map(provider=><option key={provider.id} value={provider.id}>{provider.name}</option>)}</SelectInput>
   <Button size="sm" variant="ghost" onClick={()=>onChange([...value,...models.filter(item=>!has(providerId,item.id)).map(item=>({providerId,model:item.id}))])} disabled={disabled||!models.length}>{t('rooms.modelsAll')}</Button>
   <Button size="sm" variant="ghost" onClick={()=>onChange(value.filter(item=>item.providerId!==providerId))} disabled={disabled||!picked}>{t('rooms.modelsNone')}</Button>
  </div>
  {loading&&<p className="room-note">{t('rooms.modelsLoading')}</p>}
  {error&&<p className="room-message-error">{translateError(error)}</p>}
  {!loading&&!error&&<div className="room-model-list">{models.map(item=>{const checked=has(providerId,item.id);return <div key={item.id} className={'room-model-option '+(checked?'checked':'')}><label className="room-model-check"><input type="checkbox" checked={checked} onChange={()=>toggle(item.id)} disabled={disabled}/><span>{item.name||item.id}</span></label>{checked&&<ReasoningSelect className="room-model-effort" value={effortOf(item.id)} onChange={effort=>setEffort(item.id,effort)} model={item} requireCatalog={available.find(provider=>provider.id===providerId)?.kind==='codex'} defaultLabel={t('rooms.models.effortDefault')} disabled={disabled}/>}{checked&&<Check size={13}/>}</div>})}{!models.length&&<p className="room-note">{t('rooms.modelsEmpty')}</p>}</div>}
  <small className="room-note">{t('rooms.modelsHint')}</small>
 </div>
}

function RoomConversation({settingsTarget,room,agents,providers,openSession,onSaved,onRemoved,visibleView}:{settingsTarget?:HTMLElement|null;visibleView:boolean;room:RoomConfig;agents:Agent[];providers:Provider[];openSession:(id:string)=>void;onSaved:()=>Promise<void>;onRemoved:()=>Promise<void>}){
 useLocale()
 const [messages,setMessages]=useState<ChatMessage[]>([]),[feed,setFeed]=useState<FeedState>(initialFeed)
 const [loading,setLoading]=useState(true),[loadError,setLoadError]=useState(''),[error,setError]=useState('')
 const [optionsBusy,setOptionsBusy]=useState(false)
 const [reasoningKo,setReasoningKo]=usePersistentKoreanReasoning()
 const [showReasoning,setShowReasoning]=usePersistentConversationToggle('showReasoning')
 const [thinkingEnabled,setThinkingEnabled]=usePersistentConversationToggle('thinkingEnabled')
 const [reasoningEffort,setReasoningEffort]=usePersistentReasoningEffort()
 const [recipientsOpen,setRecipientsOpen]=useState(false)
 const recipientsPanel=useAnchoredPanel(recipientsOpen,()=>setRecipientsOpen(false),280,'top')
 useEffect(()=>{if(!visibleView){setRecipientsOpen(false)}},[visibleView])
 const [input,setInput]=useChatDraft('room:'+room.id)
 const [targets,setTargets]=useState<string[]>([]),[sending,setSending]=useState(false),[stopping,setStopping]=useState(false)
 const [inspector,setInspector]=useState(false),[draft,setDraft]=useState<RoomDraft>(()=>draftOf(room)),[dirty,setDirty]=useState(false),[saving,setSaving]=useState(false),[saved,setSaved]=useState(false)
 const [knowledgeOpen,setKnowledgeOpen]=useState(false),[knowledge,setKnowledge]=useState<any[]>([]),[knowledgeBusy,setKnowledgeBusy]=useState(false),[knowledgeNotice,setKnowledgeNotice]=useState('')
 const [approvalBusy,setApprovalBusy]=useState('')
 const [learningNotice,setLearningNotice]=useState('')
 const [teamOpen,setTeamOpen]=useState(false)
 const [showActivity,setShowActivity]=useState(false),[live,setLive]=useState<LiveDraft|null>(null),[moreBelow,setMoreBelow]=useState(false)
 const inputRef=useRef<HTMLTextAreaElement>(null),approvalRef=useRef<HTMLDivElement>(null),requestInFlight=useRef(false),reloadRequested=useRef(false),dataVersion=useRef(0),mounted=useRef(true),sendAttempt=useRef<{input:string;targets:string[];attachmentIds:string[];id:string;reasoningLanguage:'ko'|'default';reasoningEffort?:string;thinking:boolean}|null>(null)
 const activeTurnId=useRef<string|null>(null),retiredTurnIds=useRef(new Set<string>()),terminalTurnIds=useRef(new Set<string>())
 const imageInputRef=useRef<HTMLInputElement>(null),pendingImagesRef=useRef<PendingRoomImage[]>([])
 const [pendingImages,setPendingImages]=useState<PendingRoomImage[]>([])
 const [uploading,setUploading]=useState(false)
 pendingImagesRef.current=pendingImages
 useEffect(()=>()=>{pendingImagesRef.current.forEach(item=>URL.revokeObjectURL(item.preview))},[])
 const onPickImages=(event:ChangeEvent<HTMLInputElement>)=>{
  const files=Array.from(event.target.files??[])
  event.target.value=''
  if(!files.length)return
  if(files.some(file=>!ROOM_ATTACH_ALLOWED.includes(file.type))){setError(t('chat.attachOnlyImages'));return}
  if(pendingImages.length+files.length>ROOM_ATTACH_LIMIT){setError(t('chat.attachLimit'));return}
  setPendingImages(previous=>[...previous,...files.map(file=>({
   key:`${file.name}:${file.lastModified}:${Math.random().toString(36).slice(2)}`,
   file,preview:URL.createObjectURL(file),
  }))])
 }
 const removeImage=(key:string)=>{
  setPendingImages(previous=>{
   const target=previous.find(item=>item.key===key)
   if(target)URL.revokeObjectURL(target.preview)
   return previous.filter(item=>item.key!==key)
  })
 }
 const viewActive=useRef(visibleView);viewActive.current=visibleView
 const current=feed.room||room
 const roster=useMemo(()=>current.agentIds.map(id=>agents.find(agent=>agent.id===id)).filter((a):a is Agent=>Boolean(a)),[current.agentIds,agents])
 const runningJobs=feed.jobs.filter(job=>active(job.status))
 const running=runningJobs.length>0||current.status==='running'||feed.status==='running'
 const activeAgentId=live?.agentId||feed.activeAgentId||current.activeAgentId||runningJobs.find(job=>job.roomState?.activeAgentId)?.roomState?.activeAgentId
 const speaker=agents.find(agent=>agent.id===activeAgentId)
 const latestJob=feed.jobs[0]
 const pendingApprovals=feed.approvals.filter(approval=>approval.status==='pending')
 const mode=current.routingMode||'discussion'
 const sessionId=feed.sessionId||current.sessionId
 const sessionRef=useRef(sessionId);sessionRef.current=sessionId
 const visible=messages.filter(message=>(message.role==='user'&&!message.agent_id&&(!('metadata' in message)||(message as any).metadata?.authorKind!=='agent_context'))||(message.role==='assistant'&&Boolean(message.content.trim())))
 const learningActive=feed.learningJobs.filter(job=>active(job.status)).length
 const memoryCount=feed.learningJobs.filter(job=>job.status==='completed').reduce((sum,job)=>sum+(typeof job.result?.memories==='number'?job.result.memories:0),0)
 const load=useCallback(async():Promise<void>=>{
  if(requestInFlight.current){reloadRequested.current=true;return}
  requestInFlight.current=true
  const version=dataVersion.current
  try{
   const [rows,state]=await Promise.all([get<ChatMessage[]>('/api/rooms/'+room.id+'/messages'),get<FeedState>('/api/rooms/'+room.id+'/state')])
   if(!mounted.current)return
   // A GET begun before a confirmed write must not erase its owner message,
   // queued job or saved settings. Schedule one fresh read after it completes.
   if(version!==dataVersion.current){reloadRequested.current=true;return}
   const actualRows=Array.isArray(rows)?rows:[]
   setMessages(actualRows);setFeed({...initialFeed(),...state});setLoadError('')
   setLive(previous=>previous?.messageId&&actualRows.some(message=>message.role==='assistant'&&message.id===previous.messageId)?null:previous)
  }catch(e){if(mounted.current&&version===dataVersion.current)setLoadError((e as Error).message)}finally{
   requestInFlight.current=false
   if(mounted.current){setLoading(false);if(reloadRequested.current){reloadRequested.current=false;void load()}}
  }
 },[room.id])
 useEffect(()=>{
  mounted.current=true;void load();let debounce:ReturnType<typeof setTimeout>|undefined
  const update=()=>{if(!viewActive.current)return;clearTimeout(debounce);debounce=setTimeout(()=>void load(),160)}
  const stream=(event:Event)=>{
   const detail=(event as CustomEvent<RoomEvent>).detail;if(detail.roomId!==room.id)return
   const item=detail.event,turnId=detail.turnId||'legacy:'+detail.agentId
   if(retiredTurnIds.current.has(turnId))return
   if(item.type==='turn_start'){
    if(activeTurnId.current===turnId||terminalTurnIds.current.has(turnId))return
    if(activeTurnId.current){retiredTurnIds.current.add(activeTurnId.current);if(retiredTurnIds.current.size>100)retiredTurnIds.current.delete(retiredTurnIds.current.values().next().value!)}
    activeTurnId.current=turnId;setLive(emptyLive(turnId,detail.agentId));return
   }
   // Opening a tab halfway through a turn can miss turn_start. The first
   // observed invocation becomes current; other turns require their own start.
   if(activeTurnId.current===null)activeTurnId.current=turnId
   if(activeTurnId.current!==turnId)return
   if(['text','reasoning','tool_start','tool_end','tool_error'].includes(item.type)){
    if(terminalTurnIds.current.has(turnId))return
    setLive(previous=>{
     const base=previous?.turnId===turnId?previous:emptyLive(turnId,detail.agentId)
     if(base.done)return base
     return {...base,text:item.type==='text'?base.text+(item.text||''):base.text,reasoning:item.type==='reasoning'?base.reasoning+(item.text||''):base.reasoning,tool:item.type==='tool_start'?(item.name||t('rooms.toolFallback')):['tool_end','tool_error'].includes(item.type)?'':base.tool}
    })
   }
   if(item.type==='learning'){
    setLearningNotice(t('rooms.event.memory',{count:Number(item.memories||0)}))
    window.setTimeout(()=>setLearningNotice(''),5000)
    return
   }
   if(item.type==='knowledge'){
    const gained=Number(item.inserted||0)+Number(item.updated||0)
    if(gained>0){
     setLearningNotice(t('rooms.event.knowledge',{count:gained}))
     window.setTimeout(()=>setLearningNotice(''),5000)
    }
    return
   }
   if(item.type==='error'){
    terminalTurnIds.current.add(turnId)
    setLive(previous=>({...previous?.turnId===turnId?previous:emptyLive(turnId,detail.agentId),done:true,tool:'',error:item.message||t('chat.tool.error',{error:''})}));update()
   }
   if(item.type==='assistant_saved'){
    terminalTurnIds.current.add(turnId)
    setLive(previous=>previous?.turnId===turnId?{...previous,done:true,tool:'',messageId:item.messageId}:previous);update()
   }
   if(item.type==='done'){
    terminalTurnIds.current.add(turnId)
    setLive(previous=>previous?.turnId===turnId?(previous.error||previous.messageId?{...previous,done:true,tool:''}:null):previous);update()
   }
  }
  const scopedUpdate=(event:Event)=>{
   const scope=(event as CustomEvent<RefreshScope>).detail
   if(scope&&matchesRoomRefresh(scope,room.id,sessionRef.current))update()
  }
  window.addEventListener('dominion-refresh',update)
  window.addEventListener('dominion-data-change',scopedUpdate)
  window.addEventListener('dominion-room-event',stream)
  // SSE carries live events; this poll is only a fallback. Ten seconds and no
  // hidden-tab traffic removes the duplicate 2.5s loop.
  const timer=setInterval(()=>{if(!document.hidden&&viewActive.current)void load()},10000)
  const onVisible=()=>{if(!document.hidden&&viewActive.current)void load()}
  document.addEventListener('visibilitychange',onVisible)
  return()=>{mounted.current=false;clearTimeout(debounce);clearInterval(timer);document.removeEventListener('visibilitychange',onVisible);window.removeEventListener('dominion-refresh',update);window.removeEventListener('dominion-data-change',scopedUpdate);window.removeEventListener('dominion-room-event',stream)}
 },[room.id,load])
 useEffect(()=>{if(visibleView)void load()},[visibleView,load])
 useEffect(()=>{if(!dirty)setDraft(draftOf(current))},[current,dirty])
 useEffect(()=>{setTargets(previous=>previous.filter(id=>current.agentIds.includes(id)))},[current.agentIds])
 const transcriptRevision=JSON.stringify([messages.at(-1)?.id,messages.length,live?.turnId,live?.text.length,live?.reasoning.length,live?.tool,live?.done]);
 const {scrollRef,onScroll,jumpDown}=useTranscriptScroll(room.id,transcriptRevision,setMoreBelow);
 const updateDraft=(value:RoomDraft)=>{setDraft(value);setDirty(true);setSaved(false)}
 const save=async()=>{if(saving)return;setSaving(true);setError('');try{const savedRoom=await post<RoomConfig>('/api/rooms',draft);dataVersion.current+=1;setFeed(previous=>({...previous,room:savedRoom}));setDirty(false);setSaved(true);await load();await onSaved()}catch(e){setError((e as Error).message)}finally{setSaving(false)}}
 const changeRoomOption=async(field:'toolsEnabled'|'autoLearning',value:boolean)=>{
  if(optionsBusy)return
  setOptionsBusy(true);setError('')
  try{
   const updated=await post<RoomConfig>('/api/rooms',{id:room.id,[field]:value,expectedConfigRevision:current.configRevision??0})
   dataVersion.current+=1;setFeed(previous=>({...previous,room:updated}));setDraft(previous=>({...previous,[field]:value,expectedConfigRevision:updated.configRevision??0}))
   await onSaved()
  }catch(e){setError((e as Error).message)}finally{setOptionsBusy(false)}
 }
 const remove=async()=>{if(!confirm(t('rooms.deleteConfirm',{name:current.name})))return;setError('');try{await del('/api/rooms/'+room.id);await onRemoved()}catch(e){setError((e as Error).message)}}
 const reset=async()=>{if(!confirm(t('rooms.resetConfirm',{name:current.name})))return;setError('');setStopping(true);try{const result=await post<{sessionId:string}>('/api/rooms/'+room.id+'/reset',{});dataVersion.current+=1;if(activeTurnId.current)terminalTurnIds.current.add(activeTurnId.current);setMessages([]);setLive(null);setFeed(previous=>({...previous,sessionId:result.sessionId,jobs:[],executions:[],operations:[],approvals:[],status:'idle',activeAgentId:undefined}));setInspector(false);await load();await onSaved()}catch(e){setError((e as Error).message)}finally{setStopping(false)}}
 const loadKnowledge=async()=>{setKnowledgeBusy(true);try{setKnowledge(await get<any[]>('/api/rooms/'+room.id+'/knowledge'))}catch(e){setError((e as Error).message)}finally{setKnowledgeBusy(false)}}
 const openKnowledge=async()=>{setKnowledgeOpen(true);setKnowledgeNotice('');await loadKnowledge()}
 const learnKnowledge=async()=>{if(knowledgeBusy)return;setKnowledgeBusy(true);setKnowledgeNotice('');try{await post('/api/rooms/'+room.id+'/knowledge/learn',{});setKnowledgeNotice(t('rooms.knowledgeQueued'));setTimeout(()=>void loadKnowledge(),2500)}catch(e){setError((e as Error).message)}finally{setKnowledgeBusy(false)}}
 const send=async(event?:FormEvent)=>{
  event?.preventDefault()
  if((!input.trim()&&!pendingImages.length)||sending)return
  setSending(true);setError('');jumpDown()
  const contents=input.trim(),recipients=[...targets]
  let attachmentIds:string[]=[]
  try{
   if(pendingImages.length){
    setUploading(true)
    const uploaded=await Promise.allSettled(pendingImages.map(async item=>{
     if(item.attachmentId)return item.attachmentId
     const attachment=await uploadAttachment({roomId:room.id},item.file)
     setPendingImages(previous=>previous.map(image=>image.key===item.key?{...image,attachmentId:attachment.id}:image))
     return attachment.id
    }))
    const failed=uploaded.find(item=>item.status==='rejected')
    if(failed?.status==='rejected')throw failed.reason
    attachmentIds=uploaded.flatMap(item=>item.status==='fulfilled'?[item.value]:[])
   }
  }catch(e){setError((e as Error).message);setUploading(false);setSending(false);return}
  setUploading(false)
  if(!sendAttempt.current||sendAttempt.current.input!==contents||JSON.stringify(sendAttempt.current.targets)!==JSON.stringify(recipients)||JSON.stringify(sendAttempt.current.attachmentIds)!==JSON.stringify(attachmentIds)||sendAttempt.current.reasoningEffort!==(reasoningEffort||undefined)||sendAttempt.current.thinking!==(reasoningEffort==='none'?false:thinkingEnabled)||sendAttempt.current.reasoningLanguage!==(reasoningKo?'ko':'default'))sendAttempt.current={input:contents,targets:recipients,attachmentIds,id:crypto.randomUUID(),reasoningLanguage:reasoningKo?'ko':'default',reasoningEffort:reasoningEffort||undefined,thinking:reasoningEffort==='none'?false:thinkingEnabled}
  try{const result=await post<{room:RoomConfig;message:ChatMessage;job:Job;sessionId:string}>('/api/rooms/'+room.id+'/messages',{input:contents,...(attachmentIds.length?{attachmentIds}:{}),...(recipients.length?{agentIds:recipients}:{}),clientMessageId:sendAttempt.current.id,reasoningLanguage:sendAttempt.current.reasoningLanguage,reasoningEffort:sendAttempt.current.reasoningEffort,thinking:sendAttempt.current.thinking});dataVersion.current+=1;setMessages(previous=>previous.some(message=>message.id===result.message.id)?previous:[...previous,result.message]);setFeed(previous=>({...previous,room:result.room,sessionId:result.sessionId,jobs:[result.job,...previous.jobs.filter(job=>job.id!==result.job.id)]}));pendingImages.forEach(item=>URL.revokeObjectURL(item.preview));setPendingImages([]);setInput('');if(inputRef.current)inputRef.current.style.height='';setTargets([]);setRecipientsOpen(false);sendAttempt.current=null;await load();await onSaved();inputRef.current?.focus({preventScroll:true})}
  catch(e){setError((e as Error).message)}finally{setSending(false)}
 }
 const approve=async(id:string,allow:boolean)=>{if(approvalBusy)return;setApprovalBusy(id);setError('');try{await resolveApproval(id,allow);dataVersion.current+=1;setFeed(previous=>({...previous,approvals:previous.approvals.filter(approval=>approval.id!==id)}));refresh()}catch(e){setError((e as Error).message)}finally{await load();setApprovalBusy('')}}
 const stop=async()=>{if(stopping)return;setStopping(true);setError('');try{await post('/api/rooms/'+room.id+'/stop',{});dataVersion.current+=1;if(activeTurnId.current)terminalTurnIds.current.add(activeTurnId.current);setLive(null);await load();await onSaved()}catch(e){setError((e as Error).message)}finally{setStopping(false)}}
 const mention=(agent:Agent)=>setTargets(previous=>previous.includes(agent.id)?previous.filter(id=>id!==agent.id):[...previous,agent.id])
 return <div className="room-conversation">
  {settingsTarget&&createPortal(<ConversationSettings permissionControl={<ExecutionPermissionsSetting/>} reasoningControl={<ReasoningSelect value={reasoningEffort} onChange={effort=>{setReasoningEffort(effort);setThinkingEnabled(effort!=='none')}} defaultLabel={current.reasoningEffort?`${t('rooms.reasoningDefault')} (${current.reasoningEffort})`:t('chat.reasoningAuto')} className="chat-reasoning !h-7 !w-auto !border-0 !bg-transparent !py-1 !pl-1.5 !pr-6 !text-[11px] !font-semibold !text-muted"/>} showReasoning={showReasoning} setShowReasoning={setShowReasoning} reasoningKo={reasoningKo} setReasoningKo={setReasoningKo} thinkingEnabled={thinkingEnabled} setThinkingEnabled={setThinkingEnabled} reasoningEffort={reasoningEffort} setReasoningEffort={setReasoningEffort} toolsEnabled={current.toolsEnabled===true} setToolsEnabled={value=>void changeRoomOption('toolsEnabled',value)} autoLearn={current.autoLearning===true} setAutoLearn={value=>void changeRoomOption('autoLearning',value)} optionsBusy={optionsBusy} learning={knowledgeBusy} learnDisabled={!sessionId||running} onLearn={learnKnowledge} notice={knowledgeNotice}/>,settingsTarget)}
  <section className="room-main" aria-label={t('rooms.headerAria',{name:current.name})}>

   <div className="room-scroll min-h-0 flex-1 overflow-y-auto overscroll-contain [scrollbar-gutter:stable]" ref={scrollRef} onScroll={onScroll} role="log" aria-label={t('rooms.logAria')} aria-busy={loading}>
    <div className="mx-auto w-full max-w-conversation px-4 py-6 sm:px-6">
    {loading?<div className="room-feed-loading"><LoaderCircle size={20} className="animate-spin"/> {t('rooms.loading')}</div>:!visible.length?<RoomFirstMessage roster={roster}/>:<>{visible.map(message=>{const owner=message.role==='user';const shared=message.metadata?.kind==='learning_report'&&(message.metadata?.learningScope==='room'||message.metadata?.source==='knowledge');const agent=shared?undefined:agents.find(item=>item.id===message.agent_id);return <TranscriptMessage key={message.id} active={visibleView} message={message} showReasoning={showReasoning} agent={agent} name={shared?t('rooms.sharedKnowledge'):owner?(current.ownerName||t('rooms.me')):(agent?.name||message.agent_id||t('rooms.agentFallback'))} owner={owner} index={Math.max(0,current.agentIds.indexOf(message.agent_id||''))}/>})}</>}
    {live&&<RoomMessageFrame agent={agents.find(agent=>agent.id===live.agentId)} name={agents.find(agent=>agent.id===live.agentId)?.name||live.agentId} metadata={<span className="text-xs text-muted">{live.error?t('rooms.live.stopped'):live.done?t('rooms.live.saving'):t('rooms.live.responding')}</span>}>
     {showReasoning&&live.reasoning&&<details className="mb-3 text-xs text-muted"><summary className="cursor-pointer">{t('rooms.live.showReasoning')}</summary><pre className="mt-2 max-h-56 overflow-auto whitespace-pre-wrap leading-6">{live.reasoning}</pre></details>}
     {live.text?<MessageBody text={live.text}/>:!live.error&&<RoomTyping/>}{live.tool&&<p className="mt-2 flex items-center gap-1.5 text-xs text-muted"><TerminalSquare size={13}/>{t('rooms.live.toolRunning',{tool:live.tool})}</p>}{live.error&&<p role="alert" className="mt-2 text-sm text-danger">{translateError(live.error)}</p>}
    </RoomMessageFrame>}
    {running&&!live&&<RoomMessageFrame agent={speaker} name={speaker?.name||t('rooms.waitingSpeaker')} metadata={<span className="text-xs text-muted">{speaker?t('rooms.preparing'):t('rooms.queuedWait')}</span>}>{speaker&&<RoomTyping/>}</RoomMessageFrame>}
    {loadError&&<Notice message={loadError}/>}
    </div>
   </div>
   {moreBelow&&<Button variant="bare" className="room-new-messages" onClick={jumpDown}><ArrowDown size={14}/> {t('rooms.newMessages')}</Button>}
   <div className="conversation-composer-wrap mx-auto w-full max-w-conversation shrink-0 bg-surface px-3 pb-3 pt-2 sm:px-6 sm:pb-6">
    {pendingImages.length>0&&<div className="mb-2 flex flex-wrap items-center gap-2">
     {pendingImages.map(item=><div key={item.key} className="relative h-16 w-16 overflow-hidden rounded-lg border border-divider bg-canvas">
      <img src={item.preview} alt={item.file.name} className="h-full w-full object-cover"/>
      <button type="button" onClick={()=>removeImage(item.key)} aria-label={t('chat.attachRemove')} title={t('chat.attachRemove')} className="absolute right-0.5 top-0.5 grid size-5 place-items-center rounded-full bg-black/60 text-white hover:bg-black/80"><X size={12}/></button>
     </div>)}
    </div>}

    {pendingApprovals.length>0&&<div className="room-approvals-stack" ref={approvalRef} tabIndex={-1}>{pendingApprovals.map(approval=><div key={approval.id} className="room-approval" data-room-approval><div><ShieldCheck size={15}/><strong>{approval.tool}</strong><span>{t('rooms.approvalRequest')}</span></div><details><summary>{t('rooms.approvalShow')}</summary><pre>{JSON.stringify(approval.args,null,2)}</pre></details><div className="room-approval-actions"><Button size="sm" variant="ghost" disabled={Boolean(approvalBusy)} onClick={()=>void approve(approval.id,false)}>{t('rooms.deny')}</Button><Button size="sm" variant="primary" disabled={Boolean(approvalBusy)} onClick={()=>void approve(approval.id,true)}>{approvalBusy===approval.id?t('rooms.processing'):t('rooms.approveOnce')}</Button></div></div>)}</div>}
    {error&&<Notice message={error}/>}
    <ConversationComposer value={input} onChange={setInput} onSend={send} inputRef={inputRef} disabled={sending}
     label={t('rooms.messageAria')} placeholder={mode==='mentioned'?t('rooms.mentionPlaceholder'):t('rooms.composerPlaceholder',{name:current.name})}
     options={<>
       <Button variant="ghost" className="!min-h-9 !gap-1.5 !px-2 !py-1.5 !text-xs !font-medium" onClick={()=>{setTeamOpen(true);setInspector(false)}}><Workflow size={16}/>{t('rooms.taskAction')}</Button>
       <RoomActions active={visibleView} actions={[
        {label:t('rooms.editParticipants'),icon:<UsersRound size={16}/>,run:()=>{setInspector(true);setRecipientsOpen(false)}},
        {label:t('rooms.sharedKnowledge'),icon:<BookOpenText size={16}/>,run:()=>void openKnowledge()},
       ]}/>
       <Button type="button" variant="ghost" size="icon" className="!h-7 !w-7 !border-0 !bg-transparent !text-muted" title={t('chat.attach')} aria-label={t('chat.attach')} onClick={()=>imageInputRef.current?.click()} disabled={sending||uploading}>{uploading?<LoaderCircle size={16} className="animate-spin"/>:<Paperclip size={16}/>}</Button>
       <input ref={imageInputRef} type="file" accept={ROOM_ATTACH_ALLOWED.join(',')} multiple className="sr-only" aria-label={t('chat.attach')} onChange={onPickImages}/>

       <div className="relative">
        <Button variant="ghost" ref={recipientsPanel.trigger} onClick={()=>{setRecipientsOpen(value=>!value)}} className="conversation-recipients chat-agent !h-7 !min-h-0 !border-0 !px-2 !py-1 !text-[11px]" aria-label={t('rooms.mentionOptions')} aria-expanded={recipientsOpen} aria-haspopup="dialog"><UsersRound size={13}/><span className="truncate">{targets.length===roster.length&&targets.length?t('rooms.everyone'):targets.length===1?roster.find(agent=>agent.id===targets[0])?.name:targets.length?t('rooms.responders',{count:targets.length}):t('rooms.replyAuto')}</span><ChevronDown size={11}/></Button>
        {recipientsOpen&&recipientsPanel.position&&createPortal(<div ref={recipientsPanel.panel} style={recipientsPanel.position} role="dialog" aria-label={t('rooms.mentionOptions')} className="z-[80] overflow-y-auto rounded-xl border border-divider bg-surface p-2 shadow-xl">
         <Button variant="ghost" className="mb-1 w-full !justify-start !text-xs" aria-pressed={!targets.length} onClick={()=>setTargets([])}>{t('rooms.autoResponders')}{!targets.length&&<Check size={13}/>}</Button>
         <Button variant="ghost" className="mb-1 w-full !justify-start !text-xs" aria-pressed={targets.length===roster.length&&targets.length>0} onClick={()=>setTargets(roster.map(agent=>agent.id))}>{t('rooms.everyone')}{targets.length===roster.length&&targets.length>0&&<Check size={13}/>}</Button>
         {roster.map((agent,index)=><Button variant="ghost" key={agent.id} className="flex w-full !justify-start gap-2 !text-xs" aria-pressed={targets.includes(agent.id)} onClick={()=>mention(agent)}><Avatar agent={agent} size="small" index={index}/><span className="min-w-0 flex-1 truncate text-left">{agent.name}</span>{targets.includes(agent.id)&&<Check size={13}/>}</Button>)}
        </div>,document.body)}
       </div>
</>} actions={<>

       {running&&<Button size="sm" variant="ghost" disabled={stopping} onClick={()=>void stop()}><Square size={12} fill="currentColor"/>{stopping?t('rooms.stopping'):t('rooms.stop')}</Button>}
       <ComposerSendButton type="submit" aria-label={t('rooms.sendAria')} disabled={sending||uploading||(!input.trim()&&!pendingImages.length)||!current.agentIds.length}>{sending?<LoaderCircle size={18} className="animate-spin"/>:<ArrowUp size={19}/>}</ComposerSendButton></>}/>

   </div>
   {visibleView&&showActivity&&<DialogBackdrop onDismiss={()=>setShowActivity(false)}><DialogPanel size="wide" aria-label={t('rooms.activityTitle')}><RoomActivity jobs={feed.jobs} executions={feed.executions} learningJobs={feed.learningJobs} recordedOperations={feed.operations} sessionId={sessionId} openSession={openSession} onClose={()=>setShowActivity(false)}/></DialogPanel></DialogBackdrop>}
  </section>
  {visibleView&&teamOpen&&<DialogBackdrop onDismiss={()=>setTeamOpen(false)}>
   <DialogPanel size="wide" className="room-team-dialog" aria-label={getLocale()==='en'?'AI collaboration':'AI 협업 작업'}>
    <header className="room-team-dialog-header">
     <div className="min-w-0"><span className="room-team-eyebrow">{getLocale()==='en'?'GROUP COLLABORATION':'그룹 협업'}</span>
      <h2>{getLocale()==='en'?'Assign a team task':'AI에게 협업 작업 맡기기'}</h2>
      <p>{getLocale()==='en'?'Keep chatting normally. Plan, progress and outcomes are tracked here.':'그룹 대화는 그대로 두고, 작업 계획과 진행 결과는 이곳에서 확인합니다.'}</p>
     </div>
     <Button size="icon" variant="ghost" className="!size-10" aria-label={getLocale()==='en'?'Close collaboration':'협업 창 닫기'} onClick={()=>setTeamOpen(false)}><X size={20}/></Button>
    </header>
    <div className="room-team-dialog-content">
     <RoomTeamLauncher roomId={room.id} sessionId={sessionId} participantCount={roster.length} agents={roster.map(agent=>({id:agent.id,name:agent.name}))} toolsEnabled={current.toolsEnabled===true}
      onEnableTools={()=>changeRoomOption('toolsEnabled',true)} active={visibleView}/>
    </div>
   </DialogPanel>
  </DialogBackdrop>}
  {visibleView&&inspector&&<DialogBackdrop onDismiss={()=>setInspector(false)}><DialogPanel size="wide" className="room-settings-dialog" aria-label={t('rooms.groupSettings')}><div className="room-dialog-header"><h2>{t('rooms.groupSettings')}</h2><Button variant="ghost" size="icon" onClick={()=>setInspector(false)} aria-label={t('rooms.settingsClose')}><X size={18}/></Button></div><div className="room-dialog-body"><RoomSettings value={draft} onChange={updateDraft} agents={agents} providers={providers} disabled={saving}/>{saved&&<p className="room-saved"><Check size={13}/> {t('rooms.settingsSaved')}</p>}<Button variant="ghost" className="mt-3 w-full !justify-start !text-xs" onClick={()=>{setShowActivity(true);setInspector(false)}} aria-expanded={showActivity}><Activity size={14}/>{t('rooms.activity')}{feed.jobs.length>0&&' '+feed.jobs.length}</Button>{(learningNotice||learningActive>0||memoryCount>0)&&<p className="room-note">{learningNotice}{learningActive>0&&` · ${t('rooms.learning')} ${learningActive}`}{memoryCount>0&&` · ${t('rooms.memory',{count:memoryCount})}`}</p>}<Button variant="bare" className="room-reset" onClick={()=>void reset()} disabled={stopping}><RotateCcw size={13}/> {t('rooms.reset')}</Button><Button variant="bare" className="room-delete" onClick={()=>void remove()}><Trash2 size={13}/> {t('rooms.deleteRoom')}</Button></div><div className="room-dialog-footer"><Button variant="primary" disabled={!dirty||saving||!draft.name.trim()||!draft.agentIds.length} onClick={()=>void save()}>{saving?<LoaderCircle size={14} className="animate-spin"/>:<Check size={14}/>} {t('rooms.saveChanges')}</Button></div></DialogPanel></DialogBackdrop>}

{visibleView&&knowledgeOpen&&<DialogBackdrop onDismiss={()=>setKnowledgeOpen(false)}><DialogPanel size="wide" aria-label={t('rooms.sharedKnowledge')}><div className="room-dialog-header"><div><span className="room-eyebrow">{t('ui.label.sharedKnowledge')}</span><h2>{t('rooms.knowledgeOf',{name:current.name})}</h2></div><Button size="icon" variant="ghost" aria-label={t('rooms.close')} onClick={()=>setKnowledgeOpen(false)}><X size={18}/></Button></div><div className="room-dialog-body"><p className="room-note">{t('rooms.knowledgeDesc')}</p><div className="room-knowledge-list">{knowledgeBusy&&!knowledge.length&&<p className="room-note"><LoaderCircle size={14} className="animate-spin"/> {t('rooms.knowledgeLoading')}</p>}{!knowledgeBusy&&!knowledge.length&&<p className="room-note">{t('rooms.knowledgeEmpty')}</p>}{knowledge.map(item=>{const sources=(item.sources||[]).map((source:any)=>[source.agentName||source.agentId||'?',source.model||t('rooms.modelUnrecorded')].filter(Boolean).join(' · ')).filter(Boolean).join(', ');return <div className={'room-knowledge-item '+(item.status==='superseded'?'is-superseded':'')} key={item.id}><div className="room-knowledge-head"><strong>{item.type}</strong><span>{item.verification||'unverified'}</span><span>{item.status==='active'?t('rooms.knowledgeActive'):t('rooms.knowledgeSuperseded')}</span></div><p>{item.text}</p><small>{t('rooms.source')}: {sources||t('rooms.noRecord')}</small></div>})}</div>{knowledgeNotice&&<p className="room-saved"><Check size={13}/>{knowledgeNotice}</p>}</div><div className="room-dialog-footer"><span>{t('rooms.selectedKnowledge')}</span><div className="flex gap-2"><Button variant="secondary" disabled={knowledgeBusy} onClick={()=>void learnKnowledge()}>{knowledgeBusy?<LoaderCircle size={14} className="animate-spin"/>:<RefreshCw size={14}/>} {t('rooms.learnNow')}</Button><Button onClick={()=>setKnowledgeOpen(false)}>{t('rooms.close')}</Button></div></div></DialogPanel></DialogBackdrop>}
 </div>
}

function RoomActivity({jobs,executions,learningJobs,recordedOperations,sessionId,openSession,onClose}:{jobs:Job[];executions:Execution[];learningJobs:Job[];recordedOperations:Operation[];sessionId?:string;openSession:(id:string)=>void;onClose:()=>void}){
 useLocale()
 const [selected,setSelected]=useState(''),[operations,setOperations]=useState<Operation[]>([]),[error,setError]=useState(''),[loading,setLoading]=useState(false)
 useEffect(()=>{let valid=true;setOperations([]);setError('');if(!selected)return;setLoading(true);get<Operation[]>('/api/executions/'+selected+'/operations').then(rows=>{if(valid)setOperations(rows)}).catch(e=>{if(valid)setError(e.message)}).finally(()=>{if(valid)setLoading(false)});return()=>{valid=false}},[selected])
 return <div className="room-activity"><div className="room-activity-title"><span><Activity size={15}/> {t('rooms.activityTitle')}</span><div>{sessionId&&<Button size="sm" variant="ghost" onClick={()=>openSession(sessionId)}>{t('rooms.originalSession')} <ArrowRight size={12}/></Button>}<Button size="icon" variant="ghost" aria-label={t('rooms.activityClose')} onClick={onClose}><X size={15}/></Button></div></div><div className="room-activity-scroll">{!jobs.length&&!executions.length&&<p className="room-note">{t('rooms.noActivity')}</p>}{jobs.slice(0,8).map(job=><div key={job.id} className="room-job-row"><span className={'room-job-status '+(active(job.status)?'is-running':'')}>{labels[job.status]?t(labels[job.status]):job.status}</span><span>{t('rooms.turns',{current:job.stepIndex||0,total:job.stepTotal||0})}</span><time>{stamp(job.createdAt)}</time>{job.error&&<p>{translateError(job.error)}</p>}</div>)}{executions.length>0&&<div className="room-execution-buttons">{executions.slice(0,12).map(execution=><Button variant="bare" key={execution.id} className={selected===execution.id?'is-selected':''} onClick={()=>setSelected(selected===execution.id?'':execution.id)}><TerminalSquare size={12}/>{t('rooms.executionButton',{status:labels[execution.status]?t(labels[execution.status]):execution.status,tools:execution.toolCalls||0,tokens:execution.tokens||0})}</Button>)}</div>}{loading&&<span className="room-note">{t('rooms.loadingEvidence')}</span>}{selected&&!loading&&!operations.length&&!error&&<p className="room-note">{t('rooms.noToolCalls')}</p>}{(selected?operations:recordedOperations.slice(0,10)).map(operation=><details className="room-operation" key={operation.id}><summary><TerminalSquare size={12}/>{operation.tool||operation.toolName||operation.name||t('rooms.toolFallback')}<span>{operation.status?t(labels[operation.status]||'rooms.recordFallback'):t('rooms.recordFallback')}</span></summary><pre>{JSON.stringify(operation,null,2)}</pre></details>)}{learningJobs.length>0&&<div className="room-learning-log"><BookOpenText size={13}/><span>{t('rooms.learningJobs',{total:learningJobs.length,done:learningJobs.filter(job=>job.status==='completed').length})}</span></div>}{selected&&<RunEventTimeline executionId={selected}/>}<TeamWorkflowPanel filterSessionId={sessionId}/><Notice message={error}/></div></div>
}
