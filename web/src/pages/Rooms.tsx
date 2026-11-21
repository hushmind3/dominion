import {ConversationSettings} from '../components/ConversationSettings'
import {matchesRoomRefresh,type RefreshScope} from '../refreshEvents'
import {ExecutionPermissions} from '../components/ExecutionPermissions'
import {createPortal} from 'react-dom'
import {useAnchoredPanel} from '../hooks/useAnchoredPanel'
import {ConversationSwitch} from '../components/WorkspaceShell'
import {TokenUsage} from '../components/TokenUsage'
import {MarkdownMessage} from '../components/MarkdownMessage'
import {useOwnerProfile} from '../components/OwnerProfile'
import {useTranscriptScroll} from '../hooks/useTranscriptScroll'
import {useCallback,useEffect,useMemo,useRef,useState,type FormEvent,type SetStateAction} from 'react'
import {Activity,ArrowDown,ArrowRight,ArrowUp,AtSign,BookOpenText,Bot,Check,ChevronDown,ChevronLeft,ChevronRight,Clock3,Copy,FileText,Hash,LoaderCircle,MessageCircle,MoreHorizontal,Menu,PanelLeft,Paperclip,Settings,Network,Plus,RefreshCw,RotateCcw,Settings2,ShieldCheck,Square,TerminalSquare,Trash2,UserRound,UsersRound,X} from 'lucide-react'
import {get,post,del,resolveApproval,koreanReasoningEnabled,type Agent,type ChatMessage,type ModelOption,type Provider} from '../api'
import {Button,TextArea,TextInput,SelectInput} from '../components/Primitives'
import {DialogBackdrop,DialogPanel} from '../components/Dialog'
import {FormField,Notice,refresh,useRemote} from '../components/UI'
import {ReasoningSelect} from '../components/ReasoningSelect'
import './Rooms.css'
import {t,useLocale,getLocale,type LocaleKey} from '../i18n'
import {translateError} from '../i18n/errors'

const MAX_ROOM_PARTICIPANTS=32

type RoutingMode='discussion'|'round_robin'|'mentioned'
type RoomModel={providerId:string;model:string;reasoningEffort?:string}
type RoomConfig={id:string;name:string;topic?:string;description?:string;ownerName?:string;agentIds:string[];routingMode?:RoutingMode;rounds?:number;maxTurns?:number;tokenBudget?:number;contextBudgetTokens?:number;toolsEnabled?:boolean;autoLearning?:boolean;autoModels?:boolean;models?:RoomModel[];reasoningEffort?:string;sessionId?:string;status?:string;activeAgentId?:string;pauseReason?:string;updatedAt?:string}
type RoomDraft={id?:string;name:string;topic:string;ownerName:string;agentIds:string[];routingMode:RoutingMode;rounds:number;maxTurns:number;tokenBudget:number;contextBudgetTokens:number;autoModels:boolean;toolsEnabled:boolean;autoLearning:boolean;models:RoomModel[];reasoningEffort:string}
type Job={id:string;kind:string;status:string;stepIndex?:number;stepTotal?:number;error?:string;createdAt?:string;input?:{roomId?:string;sessionId?:string};result?:{memories?:number;status?:string};roomState?:{activeAgentId?:string;inFlight?:boolean}}
type Execution={id:string;status:string;agentId?:string;sessionId?:string;toolCalls?:number;tokens?:number;reason?:string}
type Operation={id:string;runId?:string;tool?:string;toolName?:string;name?:string;status?:string;result?:unknown;error?:string;arguments?:unknown}
type Approval={id:string;tool:string;args?:unknown;status:string}
type TokenUsage={promptTokens?:number;completionTokens?:number;totalTokens?:number;calls?:number;estimatedCalls?:number}
type FeedState={room?:RoomConfig;sessionId?:string;jobs:Job[];executions:Execution[];learningJobs:Job[];operations:Operation[];approvals:Approval[];activeAgentId?:string;status?:string;queueDepth?:number;usage?:TokenUsage}
type LiveDraft={turnId:string;agentId:string;text:string;reasoning:string;tool:string;error:string;done:boolean;messageId?:string}
type RoomEvent={roomId:string;agentId:string;turnId?:string;event:{type:string;text?:string;name?:string;message?:string;messageId?:string;terminal?:boolean;memories?:number;skill?:boolean;inserted?:number;updated?:number;promoted?:number}}
const emptyLive=(turnId:string,agentId:string):LiveDraft=>({turnId,agentId,text:'',reasoning:'',tool:'',error:'',done:false})
const initialFeed=():FeedState=>({jobs:[],executions:[],learningJobs:[],operations:[],approvals:[]})
const labels:Record<string,LocaleKey>={queued:'rooms.st.waiting',running:'rooms.st.running',completed:'rooms.st.completed',idle:'rooms.st.idle',paused:'rooms.st.stopped',stopped:'rooms.st.stopped',cancelled:'rooms.st.stopped',failed:'rooms.st.failed',needs_review:'rooms.st.needsReview',interrupted:'rooms.st.interrupted'}
const modes:Record<RoutingMode,{label:LocaleKey;description:LocaleKey}>={discussion:{label:'rooms.mode.discussion.label',description:'rooms.mode.discussion.desc'},round_robin:{label:'rooms.mode.round_robin.label',description:'rooms.mode.round_robin.desc'},mentioned:{label:'rooms.mode.mentioned.label',description:'rooms.mode.mentioned.desc'}}
const palette=['#668373','#9A745F','#6D7FA4','#967AA0','#B18C47','#6C9297','#AB7180','#7A8456','#8B7B69']
const active=(status?:string)=>status==='queued'||status==='running'
const stamp=(value?:string)=>{if(!value)return '';const normalized=value.includes('T')?value:value.replace(' ','T')+'Z';const date=new Date(normalized);return Number.isNaN(date.getTime())?'':date.toLocaleTimeString(getLocale()==='en'?'en-US':'ko-KR',{hour:'2-digit',minute:'2-digit'})}
/** Mirrors the host's stable roster-index model_pool routing. The display
 * projects configured routing; actual model evidence is shown per reply. */
function roomModelRoutes(config:{agentIds:string[];models?:RoomModel[];autoModels?:boolean},agents:Agent[]){
 const participants=config.agentIds.map(id=>agents.find(a=>a.id===id))
 const explicit=config.models||[]
 const seen=new Set<string>()
 const inherited=participants.flatMap(agent=>{
  if(!agent?.providerId||!agent.model)return []
  const key=agent.providerId+'|'+agent.model
  if(seen.has(key))return []
  seen.add(key)
  return [{providerId:agent.providerId,model:agent.model}]
 })
 const pool=explicit.length?explicit:config.autoModels!==false?inherited:[]
 return config.agentIds.map((id,index)=>{
  const agent=participants[index]
  const selected=pool.length?pool[index%pool.length]:agent
  return {id,name:agent?.name||id,providerId:selected?.providerId||'',
   model:selected?.model||''}
 })
}
const draftOf=(room?:RoomConfig):RoomDraft=>({id:room?.id,name:room?.name||'',topic:room?.topic||room?.description||'',ownerName:room?.ownerName||t('rooms.me'),agentIds:room?.agentIds||[],routingMode:room?.routingMode||'discussion',rounds:room?.rounds||3,maxTurns:room?.maxTurns??0,tokenBudget:room?.tokenBudget??0,contextBudgetTokens:room?.contextBudgetTokens??2000,toolsEnabled:room?.toolsEnabled===true,autoLearning:room?.autoLearning===true,autoModels:room?.autoModels!==false,models:room?.models||[],reasoningEffort:room?.reasoningEffort||''})
const agentColor=(agent?:Agent,index=0)=>/^#[0-9a-f]{3,8}$/i.test(agent?.color||'')?agent!.color!:palette[index%palette.length]

function Avatar({agent,name,owner=false,size='normal',index=0}:{agent?:Agent;name?:string;owner?:boolean;size?:'small'|'normal';index?:number}){
 const {profile}=useOwnerProfile()
 const source=owner?profile.avatar:String(agent?.avatar||agent?.avatarUrl||'')
 return <span title={owner?profile.name:agent?.name||name} className={'room-avatar '+(size==='small'?'room-avatar-small ':'')+(owner?'room-avatar-owner':'')} style={owner?undefined:{backgroundColor:agentColor(agent,index)}}>
  {source?<img src={source} alt=""/>:owner?<UserRound size={size==='small'?14:18}/>:<span>{(agent?.name||name||'?').slice(0,1)}</span>}
 </span>
}

export function RoomsPage({openSession,openSettings,switchMode,active:visibleView=true}:{openSession:(id:string)=>void;openSettings:()=>void;switchMode:(mode:'chat'|'rooms')=>void;active?:boolean}){
 useLocale()
 const rooms=useRemote<RoomConfig[]>('/api/rooms',[],visibleView)
 const agents=useRemote<Agent[]>('/api/agents',[],visibleView)
 const providers=useRemote<Provider[]>('/api/providers',[],visibleView)
 const [selected,setSelected]=useState(()=>{try{return localStorage.getItem('dominion.room.selected')||''}catch{return ''}})
 const [showList,setShowList]=useState(false)
 useEffect(()=>{if(!visibleView)setShowList(false)},[visibleView])
 const [creating,setCreating]=useState<RoomDraft|null>(null)
 const [saving,setSaving]=useState(false),[error,setError]=useState(''),[query,setQuery]=useState('')
 const current=rooms.value.find(room=>room.id===selected)
 useEffect(()=>{if(rooms.loading)return;if(!rooms.value.some(room=>room.id===selected))setSelected(rooms.value[0]?.id||'')},[rooms.value,rooms.loading,selected])
 useEffect(()=>{try{if(selected)localStorage.setItem('dominion.room.selected',selected)}catch{}},[selected])
 const selectRoom=(id:string)=>{setSelected(id);setError('');if(window.innerWidth<840)setShowList(false)}
 const createRoom=async()=>{
  if(!creating||saving)return;setSaving(true);setError('')
  try{const created=await post<RoomConfig>('/api/rooms',creating);await rooms.reload();setSelected(created.id);setCreating(null);if(window.innerWidth<840)setShowList(false);refresh()}
  catch(e){setError((e as Error).message)}finally{setSaving(false)}
 }
 const filtered=rooms.value.filter(room=>(room.name+' '+(room.topic||'')).toLowerCase().includes(query.toLowerCase()))
 return <div className="rooms-app">
  {showList&&<Button variant="bare" className="room-list-scrim" aria-label={t('rooms.listClose')} onClick={()=>setShowList(false)}/>}
  <aside className={'room-list '+(showList?'room-list-open':'room-list-closed')} aria-label={t('rooms.listAria')}>
   <div className="room-list-title"><div><span className="room-eyebrow">{t('ui.label.yourCircle')}</span><h2>{t('rooms.listTitle')}</h2></div><Button size="icon" variant="ghost" title={t('rooms.new')} aria-label={t('rooms.newAria')} onClick={()=>{setCreating(draftOf());setError('')}}><Plus size={19}/></Button></div>
   <div className="room-list-search"><TextInput aria-label={t('rooms.searchAria')} placeholder={t('rooms.search')} value={query} onChange={e=>setQuery(e.target.value)}/></div>
   <div className="room-list-items">{filtered.map(room=><Button variant="bare" key={room.id} className={'room-list-item '+(selected===room.id?'is-selected':'')} onClick={()=>selectRoom(room.id)} aria-current={selected===room.id?'true':undefined}><span className="room-list-icon"><Hash size={18}/></span><span className="room-list-copy"><strong>{room.name}</strong><small>{room.topic||t('rooms.space')}</small><span className="room-list-meta">{t('rooms.me')} + {room.agentIds?.length||0}{t('rooms.people')}{room.status==='running'&&<i title={t('rooms.st.running')}/>}</span></span></Button>)}
    {!rooms.loading&&!filtered.length&&<div className="room-list-empty">{query?t('rooms.noSearch'):t('rooms.empty')}</div>}
   </div>
   <div className="room-list-footer"><Button variant="ghost" onClick={openSettings} className="w-full !justify-start gap-2 !text-xs" aria-label={t('nav.settings')}><Settings size={17}/>{t('nav.settings')}</Button></div>
  </aside>
  {current?<RoomConversation key={current.id} room={current} openSettings={openSettings} visibleView={visibleView} switchMode={switchMode} agents={agents.value} providers={providers.value} openList={()=>setShowList(v=>!v)} showList={showList} openSession={openSession} onSaved={async()=>{await rooms.reload();refresh()}} onRemoved={async()=>{setSelected('');await rooms.reload()}}/>:<div className="room-welcome"><header className="room-empty-header"><Button variant="ghost" size="icon" aria-label={t('rooms.listExpand')} onClick={()=>setShowList(value=>!value)}><Menu size={18}/></Button><ConversationSwitch mode="rooms" onChange={switchMode}/><Button variant="ghost" size="icon" className="conversation-settings" aria-label={t('nav.settings')} onClick={openSettings}><Settings size={18}/></Button></header><div className="room-welcome-mark"><UsersRound size={40}/></div><span className="room-eyebrow">{t('ui.label.teamPlace')}</span><h1>{t('rooms.welcome.title')}</h1><p>{t('rooms.welcome.line1')}<br/>{t('rooms.welcome.line2')}<br/>{t('rooms.welcome.line3')}</p><Button variant="primary" onClick={()=>{setCreating(draftOf());setError('')}}><Plus size={16}/> {t('rooms.create')}</Button><Button variant="bare" className="room-mobile-list-button" onClick={()=>setShowList(true)}>{t('rooms.viewSaved')}</Button></div>}
  {(error||rooms.error||agents.error)&&!creating&&<div className="room-global-error"><Notice message={error||rooms.error||agents.error}/></div>}
  {creating&&<DialogBackdrop onDismiss={()=>{if(!saving)setCreating(null)}}><DialogPanel size="wide" aria-label={t('rooms.newTitle')}><div className="room-dialog-header"><div><span className="room-eyebrow">{t('ui.label.startConversation')}</span><h2>{t('rooms.who')}</h2></div><Button size="icon" variant="ghost" aria-label={t('rooms.close')} onClick={()=>setCreating(null)} disabled={saving}><X size={18}/></Button></div><div className="room-dialog-body"><RoomSettings value={creating} onChange={setCreating} agents={agents.value} providers={providers.value} disabled={saving}/><Notice message={error}/></div><div className="room-dialog-footer"><span>{t('rooms.createHint')}</span><Button variant="primary" onClick={()=>void createRoom()} disabled={saving||!creating.name.trim()||!creating.agentIds.length}>{saving?<LoaderCircle size={15} className="animate-spin"/>:<Plus size={15}/>} {t('rooms.create')}</Button></div></DialogPanel></DialogBackdrop>}
 </div>
}

function RoomSettings({value,onChange,agents,providers,disabled=false}:{value:RoomDraft;onChange:(value:RoomDraft)=>void;agents:Agent[];providers:Provider[];disabled?:boolean}){
 useLocale()
 const toggle=(id:string)=>onChange({...value,agentIds:value.agentIds.includes(id)?value.agentIds.filter(item=>item!==id):[...value.agentIds,id]})
 return <div className="room-settings-form">
  <FormField label={t('rooms.name')}><TextInput value={value.name} onChange={e=>onChange({...value,name:e.target.value})} placeholder={t('rooms.namePlaceholder')} disabled={disabled}/></FormField>
  <FormField label={t('rooms.topic')}><TextArea rows={2} value={value.topic} onChange={e=>onChange({...value,topic:e.target.value})} placeholder={t('rooms.topicPlaceholder')} disabled={disabled}/></FormField>
  <FormField label={t('rooms.ownerName')}><TextInput value={value.ownerName} onChange={e=>onChange({...value,ownerName:e.target.value})} placeholder={t('rooms.me')} disabled={disabled}/></FormField>
  <div className="room-roster-heading"><strong>{t('rooms.roster')}</strong><span>{t('rooms.rosterCount',{count:value.agentIds.length,max:MAX_ROOM_PARTICIPANTS})}</span></div>
  <div className="room-roster-options">{agents.map((agent,index)=>{const checked=value.agentIds.includes(agent.id);const unavailable=disabled||(!checked&&value.agentIds.length>=MAX_ROOM_PARTICIPANTS);return <label key={agent.id} className={'room-roster-option '+(checked?'checked':'')}><input type="checkbox" checked={checked} disabled={unavailable} onChange={()=>toggle(agent.id)}/><Avatar agent={agent} size="small" index={index}/><span><strong>{agent.name}</strong><small>{agent.model||t('rooms.defaultModel')}</small></span>{checked&&<Check size={14}/>}</label>})}{!agents.length&&<p className="room-note">{t('rooms.noAgents')}</p>}</div>
  <FormField label={t('rooms.mode')}><SelectInput value={value.routingMode} onChange={e=>onChange({...value,routingMode:e.target.value as RoutingMode})} disabled={disabled}>{Object.entries(modes).map(([key,item])=><option key={key} value={key}>{t(item.label)}</option>)}</SelectInput><small className="room-note">{t(modes[value.routingMode].description)}</small></FormField>
  <div className="room-settings-numbers"><FormField label={t('rooms.rounds')} hint={t('rooms.roundsHint')}><TextInput type="number" min={1} max={100000} value={value.rounds} onChange={e=>onChange({...value,rounds:Math.min(100000,Math.max(1,Number(e.target.value)||1))})} disabled={disabled||value.routingMode==='round_robin'}/></FormField><FormField label={t('rooms.maxTurns')} hint={t('rooms.maxTurnsHint')}><TextInput type="number" min={0} max={100000} value={value.maxTurns} onChange={e=>onChange({...value,maxTurns:Math.min(100000,Math.max(0,Number(e.target.value)||0))})} disabled={disabled}/></FormField></div>
  <FormField label={t('rooms.tokenBudget')} hint={t('rooms.tokenBudgetHint')}><TextInput type="number" min={0} max={10000000} value={value.tokenBudget} onChange={e=>onChange({...value,tokenBudget:Math.min(10000000,Math.max(0,Number(e.target.value)||0))})} disabled={disabled}/></FormField>
  <FormField label="대화 문맥 목표 (토큰)" hint="동적 대화·기억 선택의 목표값입니다. 페르소나·공통 지시는 압축하지 않으며 이 값보다 커질 수 있습니다."><TextInput type="number" min={2000} max={1000000} value={value.contextBudgetTokens} onChange={e=>onChange({...value,contextBudgetTokens:Math.min(1000000,Math.max(2000,Number(e.target.value)||2000))})} disabled={disabled}/></FormField>
  <FormField label={t('rooms.reasoning')} hint={t('rooms.reasoningHint')}><ReasoningSelect value={value.reasoningEffort} onChange={reasoningEffort=>onChange({...value,reasoningEffort})} defaultLabel={t('rooms.reasoningDefault')} disabled={disabled}/></FormField>
  <label className="room-setting-check"><input type="checkbox" checked={value.autoLearning} onChange={e=>onChange({...value,autoLearning:e.target.checked})} disabled={disabled}/><span><strong>{t('rooms.autoLearn')}</strong><small>{t('rooms.autoLearnDesc')}</small></span></label>
  <RoomModelPicker value={value.models} onChange={models=>onChange({...value,models})} providers={providers} agents={agents} disabled={disabled}/>
  <div className="room-model-plan" aria-label={t('rooms.routeTitle')}>
   <strong>{t('rooms.routeTitle')}</strong>
   <small>{t('rooms.routePreview')}</small>
   {roomModelRoutes(value,agents).map(row=><div key={row.id} className="room-model-plan-row">
    <span>{row.name}</span><b>{row.providerId&&row.model?`${providers.find(p=>p.id===row.providerId)?.name||row.providerId} / ${row.model}`:t('rooms.routeMissing')}</b>
   </div>)}
  </div>
  <label className="room-setting-check"><input type="checkbox" checked={value.autoModels} onChange={e=>onChange({...value,autoModels:e.target.checked})} disabled={disabled}/><span><strong>{t('rooms.autoModels')}</strong><small>{t('rooms.autoModelsDesc')}</small></span></label>
  <label className="room-setting-check"><input type="checkbox" checked={value.toolsEnabled} onChange={e=>onChange({...value,toolsEnabled:e.target.checked})} disabled={disabled}/><span><strong>{t('rooms.tools')}</strong><small>{t('rooms.toolsDesc')}</small></span></label>
 </div>
}


function RoomModelPicker({value,onChange,providers,agents,disabled=false}:{value:RoomModel[];onChange:(models:RoomModel[])=>void;providers:Provider[];agents:Agent[];disabled?:boolean}){
 useLocale()
 const available=providers.filter(provider=>provider.enabled!==false)
 const [providerChoice,setProviderChoice]=useState('')
 // Provider data loads asynchronously. An empty state from the first render
 // must resolve to the first enabled provider instead of freezing the picker.
 const providerId=available.some(provider=>provider.id===providerChoice)
  ?providerChoice:available[0]?.id||''
 const [catalog,setCatalog]=useState<Record<string,ModelOption[]>>({})
 const [loading,setLoading]=useState(false),[error,setError]=useState('')
 useEffect(()=>{
  if(!providerId||catalog[providerId])return
  let alive=true
  setLoading(true);setError('')
  get<ModelOption[]>('/api/providers/'+encodeURIComponent(providerId)+'/models')
   .then(models=>{if(alive)setCatalog(previous=>({...previous,[providerId]:models}))})
   .catch(reason=>{if(alive)setError((reason as Error).message)})
   .finally(()=>{if(alive)setLoading(false)})
  return()=>{alive=false}
 },[providerId,catalog])
 const has=(provider:string,model:string)=>value.some(item=>item.providerId===provider&&item.model===model)
 const effortOf=(model:string)=>value.find(item=>item.providerId===providerId&&item.model===model)?.reasoningEffort||''
 const setEffort=(model:string,effort:string)=>onChange(value.map(item=>item.providerId===providerId&&item.model===model?{...item,reasoningEffort:effort||undefined}:item))
 const toggle=(model:string)=>onChange(has(providerId,model)?value.filter(item=>!(item.providerId===providerId&&item.model===model)):[...value,{providerId,model}])
 // Not every compatible endpoint implements /models. Keep the provider's
 // configured model and real participant model IDs selectable on failure;
 // do not invent catalog entries or silently change the selected provider.
 const configured=[available.find(p=>p.id===providerId)?.model,
  ...agents.filter(agent=>agent.providerId===providerId).map(agent=>agent.model)]
  .filter((id):id is string=>Boolean(id?.trim()))
 const models=[...(catalog[providerId]||[]).filter(item=>item.id)]
 for(const id of configured){
  if(!models.some(item=>item.id===id))models.push({id,name:id})
 }
 const picked=value.filter(item=>item.providerId===providerId).length
 return <div className="room-model-picker">
  <div className="room-roster-heading"><strong>{t('rooms.models')}</strong><span>{t('rooms.modelsSelected',{count:value.length})}</span></div>
  <small className="room-note">{t('rooms.modelsDesc')}</small>
  <div className="room-model-picker-controls">
   <SelectInput aria-label={t('rooms.modelProvider')} value={providerId} onChange={e=>setProviderChoice(e.target.value)} disabled={disabled||!available.length}>{available.map(provider=><option key={provider.id} value={provider.id}>{provider.name}</option>)}</SelectInput>
   <Button size="sm" variant="ghost" onClick={()=>onChange([...value,...models.filter(item=>!has(providerId,item.id)).map(item=>({providerId,model:item.id}))])} disabled={disabled||!models.length}>{t('rooms.modelsAll')}</Button>
   <Button size="sm" variant="ghost" onClick={()=>onChange(value.filter(item=>item.providerId!==providerId))} disabled={disabled||!picked}>{t('rooms.modelsNone')}</Button>
  </div>
  {loading&&<p className="room-note">{t('rooms.modelsLoading')}</p>}
  {error&&<p className="room-message-error">{translateError(error)}</p>}
  {!loading&&<div className="room-model-list">{models.map(item=>{const checked=has(providerId,item.id);return <div key={item.id} className={'room-model-option '+(checked?'checked':'')}><label className="room-model-check"><input type="checkbox" checked={checked} onChange={()=>toggle(item.id)} disabled={disabled}/><span>{item.name||item.id}</span></label>{checked&&<ReasoningSelect className="room-model-effort" value={effortOf(item.id)} onChange={effort=>setEffort(item.id,effort)} model={item} requireCatalog={available.find(provider=>provider.id===providerId)?.kind==='codex'} defaultLabel={t('rooms.models.effortDefault')} disabled={disabled}/>}{checked&&<Check size={13}/>}</div>})}{!models.length&&<p className="room-note">{t('rooms.modelsEmpty')}</p>}</div>}
  <small className="room-note">{t('rooms.modelsHint')}</small>
 </div>
}

type RoomAttachment={id:string;name:string;size:number}
type RoomSendAttempt={input:string;targets:string[];attachmentIds:string[];id:string;reasoningLanguage:'ko'|'default';reasoningEffort?:string;thinking:boolean}
type RoomComposerDraft={text:string;targets:string[];attachments:RoomAttachment[];attempt?:RoomSendAttempt}
const maxRoomAttachmentBytes=8*1024*1024,maxRoomAttachments=8,maxRoomTotalBytes=24*1024*1024
const formatRoomAttachmentSize=(bytes:number)=>bytes>=1048576
 ?(bytes/1048576).toFixed(1)+' MiB':bytes>=1024?(bytes/1024).toFixed(1)+' KiB':bytes+' B'
async function encodeRoomFile(file:File):Promise<string>{
 const data=new Uint8Array(await file.arrayBuffer())
 let binary=''
 for(let offset=0;offset<data.length;offset+=32768)
  binary+=String.fromCharCode(...data.subarray(offset,offset+32768))
 return btoa(binary)
}
function supportedRoomFile(file:File):boolean{
 return /^(?:image\/(?:png|jpeg|gif|webp)|text\/|application\/(?:json|xml))/.test(file.type)
  ||/\.(?:txt|md|csv|ts|tsx|js|mjs|json|py|rs|yaml|yml|xml|html|css|log|sql)$/i.test(file.name)
}
const roomAttachmentOnlyPrompt='첨부한 파일을 확인해 주세요.'
const roomMessageText=(text:string,files:RoomAttachment[])=>text.trim()||(files.length?roomAttachmentOnlyPrompt:'')
const roomComposerPrefix='dominion.room.composer.v1:'
function readRoomComposer(roomId:string):RoomComposerDraft{
 const empty:RoomComposerDraft={text:'',targets:[],attachments:[]}
 try{
  const raw=sessionStorage.getItem(roomComposerPrefix+roomId)
  if(!raw)return empty
  const saved=JSON.parse(raw)
  if(!saved||typeof saved.text!=='string'||saved.text.length>250000
   ||!Array.isArray(saved.targets))return empty
  const targets:string[]=[...new Set<string>(saved.targets
   .filter((id:unknown):id is string=>typeof id==='string'&&id.length>0)
   .slice(0,32))]
  const attachments:RoomAttachment[]=Array.isArray(saved.attachments)?
   saved.attachments.filter((item:unknown):item is RoomAttachment=>{
    if(!item||typeof item!=='object')return false
    const row=item as Partial<RoomAttachment>
    return typeof row.id==='string'&&row.id.length>0&&row.id.length<=160
     &&typeof row.name==='string'&&row.name.length>0&&row.name.length<=240
     &&typeof row.size==='number'&&Number.isSafeInteger(row.size)
     &&row.size>=0&&row.size<=maxRoomAttachmentBytes
   }).slice(0,maxRoomAttachments):[]
  if(attachments.reduce((sum,file)=>sum+file.size,0)>maxRoomTotalBytes)return empty
  const pending=saved.attempt
  const validPending=pending&&typeof pending.id==='string'
   &&pending.id.length>0&&pending.id.length<=160
   &&pending.input===roomMessageText(saved.text,attachments)
   &&Array.isArray(pending.attachmentIds)
   &&JSON.stringify(pending.attachmentIds)===JSON.stringify(attachments.map(file=>file.id))
   &&Array.isArray(pending.targets)
   &&JSON.stringify(pending.targets)===JSON.stringify(targets)
   &&(pending.reasoningLanguage==='ko'||pending.reasoningLanguage==='default')
   &&typeof pending.thinking==='boolean'
   &&(pending.reasoningEffort===undefined||typeof pending.reasoningEffort==='string')
  const attempt:RoomSendAttempt|undefined=validPending?{
   id:pending.id,input:pending.input,targets,attachmentIds:attachments.map(file=>file.id),
   reasoningLanguage:pending.reasoningLanguage,
   reasoningEffort:pending.reasoningEffort,thinking:pending.thinking
  }:undefined
  return {text:saved.text,targets,attachments,attempt}
 }catch{return empty}
}
function storeRoomComposer(roomId:string,draft:RoomComposerDraft){
 try{
  if(!draft.text&&!draft.targets.length&&!draft.attachments.length)sessionStorage.removeItem(roomComposerPrefix+roomId)
  else sessionStorage.setItem(roomComposerPrefix+roomId,JSON.stringify(draft))
 }catch{/* Private mode or browser quota must not prevent composing. */}
}

function RoomConversation({room,agents,providers,openList,showList,openSession,onSaved,onRemoved,switchMode,visibleView,openSettings}:{openSettings:()=>void;switchMode:(mode:'chat'|'rooms')=>void;visibleView:boolean;room:RoomConfig;agents:Agent[];providers:Provider[];openList:()=>void;showList:boolean;openSession:(id:string)=>void;onSaved:()=>Promise<void>;onRemoved:()=>Promise<void>}){
 useLocale()
 const [composer,setComposer]=useState<RoomComposerDraft>(()=>readRoomComposer(room.id))
 const [messages,setMessages]=useState<ChatMessage[]>([]),[feed,setFeed]=useState<FeedState>(initialFeed)
 const messageUsageTotal=useMemo(()=>messages.reduce((sum,message)=>sum+(Number(message.metadata?.usage?.totalTokens)||0),0),[messages])
 const usageTotal=feed.usage?.totalTokens||messageUsageTotal
 const [loading,setLoading]=useState(true),[loadError,setLoadError]=useState(''),[error,setError]=useState('')
 const [showSettings,setShowSettings]=useState(false),[showReasoning,setShowReasoning]=useState(true),
 [reasoningKo,setReasoningKo]=useState(()=>composer.attempt?composer.attempt.reasoningLanguage==='ko':koreanReasoningEnabled()),
 [thinkingEnabled,setThinkingEnabled]=useState(()=>composer.attempt?.thinking??true),[optionsBusy,setOptionsBusy]=useState(false)
 const [reasoningEffort,setReasoningEffort]=useState(()=>composer.attempt?.reasoningEffort||''),[permissionOpen,setPermissionOpen]=useState(false),[recipientsOpen,setRecipientsOpen]=useState(false)
 const recipientsPanel=useAnchoredPanel(recipientsOpen,()=>setRecipientsOpen(false),280,'top')
 useEffect(()=>{if(!visibleView){setRecipientsOpen(false);setPermissionOpen(false);setShowSettings(false)}else if(!composer.attempt)setReasoningKo(koreanReasoningEnabled())},[visibleView])
 // A Room owns its unsent text and explicit recipients. Reopening another
 // Room must never transplant this draft or silently broadcast it to all.
 const input=composer.text,targets=composer.targets
 const setInput=(next:SetStateAction<string>)=>setComposer(prior=>{
  const text=typeof next==='function'?next(prior.text):next
  return {...prior,text,attempt:text===prior.text?prior.attempt:undefined}
 })
 const setTargets=(next:SetStateAction<string[]>)=>setComposer(prior=>{
  const targets=typeof next==='function'?next(prior.targets):next
  return {...prior,targets,attempt:JSON.stringify(targets)===JSON.stringify(prior.targets)?prior.attempt:undefined}
 })
 useEffect(()=>storeRoomComposer(room.id,composer),[room.id,composer])
 const [sending,setSending]=useState(false),[stopping,setStopping]=useState(false)
 const [uploading,setUploading]=useState(false)
 const uploadBusy=useRef(false)
 const roomFileInput=useRef<HTMLInputElement>(null)
 const attachRoomFiles=async(files:FileList|null)=>{
  if(!files||uploadBusy.current||sending)return
  uploadBusy.current=true;setUploading(true);setError('')
  // Each successful upload enters the room-scoped durable composer at once.
  // A failed later file must not discard earlier uploaded references.
  let pending=[...composer.attachments]
  try{
   for(const file of Array.from(files)){
    if(!supportedRoomFile(file))throw Error('이미지 또는 텍스트 파일만 첨부할 수 있습니다.')
    if(file.size>maxRoomAttachmentBytes)throw Error('파일 한 개는 최대 8MiB입니다.')
    if(pending.length>=maxRoomAttachments||
     pending.reduce((sum,item)=>sum+item.size,0)+file.size>maxRoomTotalBytes)
     throw Error('첨부는 최대 8개, 합계 24MiB까지 가능합니다.')
    const saved=await post<{id:string;name:string;size:number}>('/api/attachments',{
     roomId:room.id,name:file.name,mimeType:file.type||'application/octet-stream',
     data:await encodeRoomFile(file)
    })
    if(!mounted.current)return
    pending=[...pending,{id:saved.id,name:saved.name,size:saved.size}]
    setComposer(previous=>({...previous,attachments:pending,attempt:undefined}))
    sendAttempt.current=null
   }
  }catch(error){if(mounted.current)setError((error as Error).message)}
  finally{uploadBusy.current=false;if(mounted.current)setUploading(false)}
 }
 const removeRoomAttachment=(id:string)=>{
  setComposer(previous=>({...previous,
   attachments:previous.attachments.filter(file=>file.id!==id),
   attempt:undefined
  }))
  sendAttempt.current=null
 }
 const [inspector,setInspector]=useState(false),[draft,setDraft]=useState<RoomDraft>(()=>draftOf(room)),[dirty,setDirty]=useState(false),[saving,setSaving]=useState(false),[saved,setSaved]=useState(false)
 const [knowledgeOpen,setKnowledgeOpen]=useState(false),[knowledge,setKnowledge]=useState<any[]>([]),[knowledgeBusy,setKnowledgeBusy]=useState(false),[knowledgeNotice,setKnowledgeNotice]=useState('')
 const [approvalBusy,setApprovalBusy]=useState('')
 const [promptOpen,setPromptOpen]=useState(false),[promptAgent,setPromptAgent]=useState(''),[prompt,setPrompt]=useState<{prompt:string;chars:number;tokens:number;common:string;commonChars:number;commonTokens:number;persona:string;personaChars:number;personaTokens:number}|null>(null),[promptBusy,setPromptBusy]=useState(false),[promptError,setPromptError]=useState('')
 const [personaDraft,setPersonaDraft]=useState(''),[promptSaving,setPromptSaving]=useState(false),[promptNotice,setPromptNotice]=useState('')
 const promptSequence=useRef(0)
 const promptDirty=Boolean(prompt&&personaDraft!==prompt.persona)
 const [learningNotice,setLearningNotice]=useState('')
 const [showActivity,setShowActivity]=useState(false),[live,setLive]=useState<LiveDraft|null>(null),[moreBelow,setMoreBelow]=useState(false)
 const inputRef=useRef<HTMLTextAreaElement>(null),approvalRef=useRef<HTMLDivElement>(null),requestInFlight=useRef(false),reloadRequested=useRef(false),dataVersion=useRef(0),mounted=useRef(true),sendAttempt=useRef<RoomSendAttempt|null>(composer.attempt??null)
 const activeTurnId=useRef<string|null>(null),retiredTurnIds=useRef(new Set<string>()),terminalTurnIds=useRef(new Set<string>())
 const viewActive=useRef(visibleView);viewActive.current=visibleView
 const current=feed.room||room
 const roster=useMemo(()=>current.agentIds.map(id=>agents.find(agent=>agent.id===id)).filter((a):a is Agent=>Boolean(a)),[current.agentIds,agents])
 // Preserve removed recipient IDs as a visible blocked draft. Dropping the
 // last explicit ID would otherwise silently change a directed request into
 // an auto-routed/broadcast Room turn.
 const staleTargets=targets.filter(id=>!current.agentIds.includes(id))
 const runningJobs=feed.jobs.filter(job=>active(job.status))
 const running=runningJobs.length>0||current.status==='running'||feed.status==='running'
 const activeAgentId=live?.agentId||feed.activeAgentId||current.activeAgentId||runningJobs.find(job=>job.roomState?.activeAgentId)?.roomState?.activeAgentId
 const speaker=agents.find(agent=>agent.id===activeAgentId)
 const latestJob=feed.jobs[0]
 const pendingApprovals=feed.approvals.filter(approval=>approval.status==='pending')
 const mode=current.routingMode||'discussion'
 const sessionId=feed.sessionId||current.sessionId
 const activeRoutes=roomModelRoutes(current,agents)
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
 const transcriptRevision=JSON.stringify([messages.at(-1)?.id,messages.length,live?.turnId,live?.text.length,live?.reasoning.length,live?.tool,live?.done]);
 const {scrollRef,onScroll,jumpDown}=useTranscriptScroll(room.id,transcriptRevision,setMoreBelow);
 const updateDraft=(value:RoomDraft)=>{setDraft(value);setDirty(true);setSaved(false)}
 const save=async()=>{if(saving)return;setSaving(true);setError('');try{const savedRoom=await post<RoomConfig>('/api/rooms',draft);dataVersion.current+=1;setFeed(previous=>({...previous,room:savedRoom}));setDirty(false);setSaved(true);await load();await onSaved()}catch(e){setError((e as Error).message)}finally{setSaving(false)}}
 const changeRoomOption=async(field:'toolsEnabled'|'autoLearning',value:boolean)=>{
  if(optionsBusy)return
  setOptionsBusy(true);setError('')
  try{
   const updated=await post<RoomConfig>('/api/rooms',{id:room.id,[field]:value})
   dataVersion.current+=1;setFeed(previous=>({...previous,room:updated}));setDraft(previous=>({...previous,[field]:value}))
   await onSaved()
  }catch(e){setError((e as Error).message)}finally{setOptionsBusy(false)}
 }
 const remove=async()=>{if(!confirm(t('rooms.deleteConfirm',{name:current.name})))return;setError('');try{await del('/api/rooms/'+room.id);storeRoomComposer(room.id,{text:'',targets:[],attachments:[]});await onRemoved()}catch(e){setError((e as Error).message)}}
 const reset=async()=>{if(!confirm(t('rooms.resetConfirm',{name:current.name})))return;setError('');setStopping(true);try{const result=await post<{sessionId:string}>('/api/rooms/'+room.id+'/reset',{});dataVersion.current+=1;if(activeTurnId.current)terminalTurnIds.current.add(activeTurnId.current);setMessages([]);setLive(null);setFeed(previous=>({...previous,sessionId:result.sessionId,jobs:[],executions:[],operations:[],approvals:[],status:'idle',activeAgentId:undefined}));setInspector(false);await load();await onSaved()}catch(e){setError((e as Error).message)}finally{setStopping(false)}}
 const previewPrompt=(id:string)=>get<NonNullable<typeof prompt>>(`/api/rooms/${room.id}/system-prompt?agentId=${encodeURIComponent(id)}`)
 const closePrompt=()=>{
  if(promptSaving)return
  if(promptDirty&&!window.confirm(t('rooms.promptDiscard')))return
  promptSequence.current+=1;setPromptOpen(false)
 }
 const openPrompt=async(agentId?:string)=>{
  if(promptSaving)return
  if(promptOpen&&promptDirty&&!window.confirm(t('rooms.promptDiscard')))return
  const id=agentId||promptAgent||current.agentIds[0]||'',sequence=++promptSequence.current
  setPromptAgent(id);setPromptOpen(true);setPromptBusy(true);setPrompt(null);setPromptError('');setPromptNotice('')
  try{
   const preview=await previewPrompt(id)
   if(!mounted.current||sequence!==promptSequence.current)return
   setPrompt(preview);setPersonaDraft(preview.persona)
  }catch(e){if(mounted.current&&sequence===promptSequence.current)setPromptError((e as Error).message)}
  finally{if(mounted.current&&sequence===promptSequence.current)setPromptBusy(false)}
 }
 const savePrompt=async()=>{
  if(promptSaving||promptBusy||!prompt||!promptDirty)return
  const id=promptAgent;let persisted=false
  setPromptSaving(true);setPromptError('');setPromptNotice('')
  try{
   if(personaDraft!==prompt.persona){
    const latest=(await get<Agent[]>('/api/agents')).find(agent=>agent.id===id)
    if(!latest)throw Error(t('rooms.noAgents'))
    const saved=await post<Agent>('/api/agents',{...latest,systemPrompt:personaDraft})
    persisted=true;setPrompt(previous=>previous?{...previous,persona:saved.systemPrompt}:previous)
   }
   refresh()
   const preview=await previewPrompt(id)
   setPrompt(preview);setPersonaDraft(preview.persona);setPromptNotice(t('rooms.promptSaved'))
  }catch(e){setPromptError((persisted?t('rooms.promptPartialSave')+' ':'')+(e as Error).message)}
  finally{setPromptSaving(false)}
 }
 const loadKnowledge=async()=>{setKnowledgeBusy(true);try{setKnowledge(await get<any[]>('/api/rooms/'+room.id+'/knowledge'))}catch(e){setError((e as Error).message)}finally{setKnowledgeBusy(false)}}
 const openKnowledge=async()=>{setKnowledgeOpen(true);setKnowledgeNotice('');await loadKnowledge()}
 const learnKnowledge=async()=>{if(knowledgeBusy)return;setKnowledgeBusy(true);setKnowledgeNotice('');try{await post('/api/rooms/'+room.id+'/knowledge/learn',{});setKnowledgeNotice(t('rooms.knowledgeQueued'));setTimeout(()=>void loadKnowledge(),2500)}catch(e){setError((e as Error).message)}finally{setKnowledgeBusy(false)}}
 const send=async(event?:FormEvent)=>{
  event?.preventDefault()
  const contents=roomMessageText(input,composer.attachments),recipients=[...targets]
  const attachmentIds=composer.attachments.map(file=>file.id)
  if(!contents||uploading||sending||staleTargets.length>0)return
  setSending(true);setError('');jumpDown()
  if(!sendAttempt.current||sendAttempt.current.input!==contents
    ||JSON.stringify(sendAttempt.current.targets)!==JSON.stringify(recipients)
    ||JSON.stringify(sendAttempt.current.attachmentIds)!==JSON.stringify(attachmentIds)
    ||sendAttempt.current.reasoningEffort!==(reasoningEffort||undefined)
    ||sendAttempt.current.thinking!==(reasoningEffort==='none'?false:thinkingEnabled)
    ||sendAttempt.current.reasoningLanguage!==(reasoningKo?'ko':'default'))
   sendAttempt.current={input:contents,targets:recipients,attachmentIds,
    id:crypto.randomUUID(),reasoningLanguage:reasoningKo?'ko':'default',
    reasoningEffort:reasoningEffort||undefined,thinking:reasoningEffort==='none'?false:thinkingEnabled}
  // Persist the exact in-flight idempotency key BEFORE dispatch. If the
  // server commits but the tab reloads before receiving 200, retrying the
  // restored draft uses the same owner operation ID, not a second job.
  storeRoomComposer(room.id,{...composer,attempt:sendAttempt.current})
  try{const result=await post<{room:RoomConfig;message:ChatMessage;job:Job;sessionId:string}>('/api/rooms/'+room.id+'/messages',{input:contents,...(recipients.length?{agentIds:recipients}:{}),clientMessageId:sendAttempt.current.id,attachmentIds:sendAttempt.current.attachmentIds,reasoningLanguage:sendAttempt.current.reasoningLanguage,reasoningEffort:sendAttempt.current.reasoningEffort,thinking:sendAttempt.current.thinking});dataVersion.current+=1;setMessages(previous=>previous.some(message=>message.id===result.message.id)?previous:[...previous,result.message]);setFeed(previous=>({...previous,room:result.room,sessionId:result.sessionId,jobs:[result.job,...previous.jobs.filter(job=>job.id!==result.job.id)]}));setComposer({text:'',targets:[],attachments:[]});if(inputRef.current)inputRef.current.style.height='';setRecipientsOpen(false);sendAttempt.current=null;await load();await onSaved();inputRef.current?.focus({preventScroll:true})}
  catch(e){setError((e as Error).message)}finally{setSending(false)}
 }
 const approve=async(id:string,allow:boolean)=>{if(approvalBusy)return;setApprovalBusy(id);setError('');try{await resolveApproval(id,allow);dataVersion.current+=1;setFeed(previous=>({...previous,approvals:previous.approvals.filter(approval=>approval.id!==id)}));refresh()}catch(e){setError((e as Error).message)}finally{await load();setApprovalBusy('')}}
 const stop=async()=>{if(stopping)return;setStopping(true);setError('');try{await post('/api/rooms/'+room.id+'/stop',{});dataVersion.current+=1;if(activeTurnId.current)terminalTurnIds.current.add(activeTurnId.current);setLive(null);await load();await onSaved()}catch(e){setError((e as Error).message)}finally{setStopping(false)}}
 const mention=(agent:Agent)=>setTargets(previous=>previous.includes(agent.id)?previous.filter(id=>id!==agent.id):[...previous,agent.id])
 const status=running?'running':feed.status||current.status||'idle'
 return <div className={'room-conversation '+(inspector?'has-inspector':'')}>
  <section className="room-main" aria-label={t('rooms.headerAria',{name:current.name})}>
   <header className="room-header"><Button variant="bare" className="room-list-toggle" aria-label={showList?t('rooms.listCollapse'):t('rooms.listExpand')} onClick={openList}><Menu size={18}/></Button><ConversationSwitch mode="rooms" onChange={switchMode}/><div className="room-header-icon" title={current.name}><Hash size={22}/></div><div className="room-header-copy"><h1>{current.name}</h1><p>{current.topic||t('rooms.topicFallback')}</p></div><div className="room-presence"><div className="room-avatar-stack"><Avatar owner size="small"/>{roster.map((agent,index)=><Avatar key={agent.id} agent={agent} size="small" index={index}/>)}</div><span><strong>{current.ownerName||t('rooms.me')}</strong>{t('rooms.withAgents',{count:roster.length})}</span><span className="room-presence-dot">·</span><span>{t(modes[mode].label)}</span><span className={'room-status '+(running?'is-running':'')}>{running&&<i/>}{labels[status]?t(labels[status]):status}</span></div><span className="room-header-usage" aria-label={t('usage.sessionTotal',{count:usageTotal.toLocaleString()})}>{t('usage.sessionTotal',{count:usageTotal.toLocaleString()})}</span><Button size="sm" variant="ghost" className="room-roster-toggle" title={t('rooms.sharedKnowledge')} aria-label={t('rooms.sharedKnowledge')} onClick={()=>void openKnowledge()} aria-haspopup="dialog"><BookOpenText size={16}/><span>{t('rooms.sharedKnowledge')}</span><b>{knowledge.length||0}</b></Button><Button size="sm" variant="ghost" className="room-roster-toggle" title={t('rooms.systemPromptHint')} aria-label={t('rooms.systemPrompt')} onClick={()=>void openPrompt()}><FileText size={16}/><span>{t('rooms.systemPrompt')}</span></Button><Button size="sm" variant="ghost" className="room-roster-toggle" title={t('rooms.resetHint')} aria-label={t('rooms.reset')} onClick={()=>void reset()} disabled={stopping}><RotateCcw size={16}/><span>{t('rooms.reset')}</span></Button><Button size="icon" variant="ghost" className="conversation-settings" title={t('nav.settings')} aria-label={t('nav.settings')} onClick={openSettings}><Settings size={18}/></Button></header>


   <div className="room-route-bar" aria-label={t('rooms.routeTitle')}>
    <strong>{t('rooms.routeTitle')}</strong>
    <div className="room-route-bar-items">
     {activeRoutes.slice(0,8).map(row=><span key={row.id} title={row.name+' · '+row.providerId+' / '+row.model}>
      <b>{row.name}</b> · {row.model||t('rooms.routeMissing')}
     </span>)}
     {activeRoutes.length>8&&<small>+{activeRoutes.length-8}</small>}
    </div>
    <Button variant="ghost" size="sm" onClick={()=>setInspector(true)} aria-label={t('rooms.routeEdit')}><Settings2 size={13}/>{t('rooms.routeEdit')}</Button>
   </div>
   <div className="room-scroll" ref={scrollRef} onScroll={onScroll} role="log" aria-label={t('rooms.logAria')} aria-busy={loading}>
    {loading?<div className="room-feed-loading"><LoaderCircle size={20} className="animate-spin"/> {t('rooms.loading')}</div>:!visible.length?<div className="room-first-message"><div className="room-empty-avatars"><Avatar owner/>{roster.slice(0,3).map((agent,index)=><Avatar key={agent.id} agent={agent} index={index}/>)}</div><h2>{t('rooms.firstTitle')}</h2><p>{t('rooms.firstDesc')}<br/>{t('rooms.firstDesc2')}</p><div><span><AtSign size={14}/> {t('rooms.mentionOne')}</span><span><BookOpenText size={14}/> {t('rooms.roomContinues')}</span></div></div>:<><div className="room-transcript-start"><span>{t('rooms.records')}</span></div>{visible.map(message=>{const owner=message.role==='user';const shared=message.metadata?.kind==='learning_report'&&(message.metadata?.learningScope==='room'||message.metadata?.source==='knowledge');const agent=shared?undefined:agents.find(item=>item.id===message.agent_id);return <TranscriptMessage key={message.id} message={message} showReasoning={showReasoning} agent={agent} name={shared?t('rooms.sharedKnowledge'):owner?(current.ownerName||t('rooms.me')):(agent?.name||message.agent_id||t('rooms.agentFallback'))} owner={owner} index={Math.max(0,current.agentIds.indexOf(message.agent_id||''))}/>})}</>}
    {live&&<div className="room-message room-message-agent room-message-live"><Avatar agent={agents.find(agent=>agent.id===live.agentId)}/><div className="room-message-main"><div className="room-message-byline"><strong>{agents.find(agent=>agent.id===live.agentId)?.name||live.agentId}</strong><span className="room-live-label">{!live.done&&<i/>}{live.error?t('rooms.live.stopped'):live.done?t('rooms.live.saving'):t('rooms.live.responding')}</span></div>{showReasoning&&live.reasoning&&<details className="room-reasoning"><summary>{t('rooms.live.showReasoning')}</summary><pre>{live.reasoning}</pre></details>}{live.text?<MessageBody text={live.text} streaming/>:!live.error&&<span className="room-typing"><i/><i/><i/></span>}{live.tool&&<div className="room-tool-live"><TerminalSquare size={13}/>{t('rooms.live.toolRunning',{tool:live.tool})}</div>}{live.error&&<p className="room-message-error">{translateError(live.error)}</p>}</div></div>}
    {running&&!live&&<div className="room-message room-message-agent room-message-live"><Avatar agent={speaker}/><div className="room-message-main"><div className="room-message-byline"><strong>{speaker?.name||t('rooms.waitingSpeaker')}</strong><span className="room-live-label">{speaker?t('rooms.preparing'):t('rooms.queuedWait')}</span></div>{speaker&&<span className="room-typing"><i/><i/><i/></span>}</div></div>}
    {loadError&&<Notice message={loadError}/>}
   </div>
   {moreBelow&&<Button variant="bare" className="room-new-messages" onClick={jumpDown}><ArrowDown size={14}/> {t('rooms.newMessages')}</Button>}
   <div className="conversation-composer-wrap">

    {pendingApprovals.length>0&&<div className="room-approvals-stack" ref={approvalRef} tabIndex={-1}>{pendingApprovals.map(approval=><div key={approval.id} className="room-approval" data-room-approval><div><ShieldCheck size={15}/><strong>{approval.tool}</strong><span>{t('rooms.approvalRequest')}</span></div><details><summary>{t('rooms.approvalShow')}</summary><pre>{JSON.stringify(approval.args,null,2)}</pre></details><div className="room-approval-actions"><Button size="sm" variant="ghost" disabled={Boolean(approvalBusy)} onClick={()=>void approve(approval.id,false)}>{t('rooms.deny')}</Button><Button size="sm" variant="primary" disabled={Boolean(approvalBusy)} onClick={()=>void approve(approval.id,true)}>{approvalBusy===approval.id?t('rooms.processing'):t('rooms.approveOnce')}</Button></div></div>)}</div>}
    {error&&<Notice message={error}/>}
    {staleTargets.length>0&&<div role="alert" className="room-global-error">
     <span>이전에 지정한 참가자 {staleTargets.join(', ')}는 현재 방에 없습니다. 전송 대상을 다시 선택해 주세요.</span>
     <Button variant="ghost" size="sm" onClick={()=>setTargets(previous=>previous.filter(id=>current.agentIds.includes(id)))}>사용할 수 없는 대상 제외</Button>
    </div>}
    <form className="conversation-composer" onSubmit={send}>
     {composer.attachments.length>0&&<div className="chat-file-drafts" aria-label="그룹 첨부 대기 파일">{composer.attachments.map(file=><span key={file.id} className="chat-file-chip"><Paperclip size={13}/><span title={file.name}>{file.name}</span><small>{formatRoomAttachmentSize(file.size)}</small><button type="button" aria-label={file.name+' 첨부 취소'} disabled={sending||uploading} onClick={()=>removeRoomAttachment(file.id)}><X size={13}/></button></span>)}</div>}
     {uploading&&<div className="room-attachment-uploading" role="status"><LoaderCircle size={13} className="animate-spin"/>첨부 파일 업로드 중…</div>}
     <input ref={roomFileInput} type="file" className="sr-only" multiple accept="image/png,image/jpeg,image/webp,image/gif,.txt,.md,.csv,.ts,.tsx,.js,.mjs,.json,.py,.rs,.yaml,.yml,.xml,.html,.css,.log,.sql" aria-label="그룹 첨부 파일 선택" disabled={sending||uploading} onChange={event=>{void attachRoomFiles(event.currentTarget.files);event.currentTarget.value=''}}/>
     <TextArea ref={inputRef} className="conversation-composer-input" aria-label={t('rooms.messageAria')} placeholder={mode==='mentioned'?t('rooms.mentionPlaceholder'):t('chat.inputPlaceholder')} value={input} onChange={e=>{setInput(e.target.value);e.currentTarget.style.height='auto';e.currentTarget.style.height=Math.min(e.currentTarget.scrollHeight,180)+'px'}} onKeyDown={e=>{if(e.key==='Enter'&&!e.shiftKey&&!e.nativeEvent.isComposing){e.preventDefault();void send()}}} rows={1} disabled={sending}/>
     <div className="chat-composer-controls conversation-composer-controls">
      <div className="chat-composer-options conversation-composer-options">
       <Button type="button" variant="ghost" disabled={sending||uploading} className="chat-file-add" title="그룹에 이미지 또는 텍스트 파일 첨부" onClick={()=>roomFileInput.current?.click()}><Paperclip size={15}/><span className="hidden sm:inline">첨부</span></Button>
       <ExecutionPermissions active={visibleView} open={permissionOpen} onOpenChange={open=>{setPermissionOpen(open);if(open){setRecipientsOpen(false);setShowSettings(false)}}} onError={setError}/>
       <ReasoningSelect value={reasoningEffort} onChange={effort=>{setReasoningEffort(effort);setThinkingEnabled(effort!=='none')}} defaultLabel={current.reasoningEffort?`${t('rooms.reasoningDefault')} (${current.reasoningEffort})`:t('chat.reasoningAuto')} className="chat-reasoning !h-7 !w-auto !border-0 !bg-transparent !py-1 !pl-1.5 !pr-6 !text-[11px] !font-semibold !text-[#5C544A]"/>
       <div className="relative">
        <Button variant="ghost" ref={recipientsPanel.trigger} onClick={()=>{setRecipientsOpen(value=>!value);setPermissionOpen(false);setShowSettings(false)}} className="conversation-recipients chat-agent !h-7 !min-h-0 !border-0 !px-2 !py-1 !text-[11px]" aria-label={t('rooms.mentionOptions')} aria-expanded={recipientsOpen} aria-haspopup="dialog"><UsersRound size={13}/><span className="truncate">{targets.length?t('rooms.responders',{count:targets.length}):t('rooms.participants')}</span><ChevronDown size={11}/></Button>
        {recipientsOpen&&recipientsPanel.position&&createPortal(<div ref={recipientsPanel.panel} style={recipientsPanel.position} role="dialog" aria-label={t('rooms.mentionOptions')} className="z-[80] overflow-y-auto rounded-xl border border-[#E7E0D5] bg-[#FFFDF9] p-2 shadow-xl">
         <Button variant="ghost" className="mb-1 w-full !justify-start !text-xs" aria-pressed={!targets.length} onClick={()=>setTargets([])}>{t('rooms.autoResponders')}{!targets.length&&<Check size={13}/>}</Button>
         {roster.map((agent,index)=><Button variant="ghost" key={agent.id} className="flex w-full !justify-start gap-2 !text-xs" aria-pressed={targets.includes(agent.id)} onClick={()=>mention(agent)}><Avatar agent={agent} size="small" index={index}/><span className="min-w-0 flex-1 truncate text-left">{agent.name}</span>{targets.includes(agent.id)&&<Check size={13}/>}</Button>)}
        </div>,document.body)}
       </div>
       <ConversationSettings open={showSettings} onOpenChange={open=>{setShowSettings(open);if(open){setPermissionOpen(false);setRecipientsOpen(false)}}} showReasoning={showReasoning} setShowReasoning={setShowReasoning} reasoningKo={reasoningKo} setReasoningKo={setReasoningKo} thinkingEnabled={thinkingEnabled} setThinkingEnabled={setThinkingEnabled} reasoningEffort={reasoningEffort} setReasoningEffort={setReasoningEffort} toolsEnabled={current.toolsEnabled===true} setToolsEnabled={value=>void changeRoomOption('toolsEnabled',value)} autoLearn={current.autoLearning===true} setAutoLearn={value=>void changeRoomOption('autoLearning',value)} optionsBusy={optionsBusy} learning={knowledgeBusy} learnDisabled={!sessionId||running} onLearn={learnKnowledge} notice={knowledgeNotice}/>
       <Button variant="ghost" className="group-settings-trigger !h-7 !min-h-0 !gap-1.5 !rounded-md !border-0 !px-2 !py-0 !text-[11px] !font-medium text-[#7C7469]" title={t('rooms.groupSettings')} aria-label={t('rooms.groupSettings')} aria-expanded={inspector} onClick={()=>{setInspector(value=>!value);setRecipientsOpen(false);setPermissionOpen(false);setShowSettings(false)}}><Network size={14}/><span className="hidden sm:inline">{t('rooms.groupSettings')}</span></Button>
      </div>
      <div className="ml-auto flex shrink-0 items-center gap-2">
       <span className="hidden text-[10px] text-[#9C9388] sm:inline">{t('chat.enterHint')}</span>
       {running&&<Button size="sm" variant="ghost" disabled={stopping} onClick={()=>void stop()}><Square size={12} fill="currentColor"/>{stopping?t('rooms.stopping'):t('rooms.stop')}</Button>}
       <Button type="submit" className="conversation-composer-send" aria-label={t('rooms.sendAria')} disabled={sending||uploading||(!input.trim()&&!composer.attachments.length)||!current.agentIds.length||staleTargets.length>0}>{sending?<LoaderCircle size={18} className="animate-spin"/>:<ArrowUp size={19}/>}</Button>
      </div>
     </div>
    </form>
   </div>
   {showActivity&&<RoomActivity jobs={feed.jobs} executions={feed.executions} learningJobs={feed.learningJobs} recordedOperations={feed.operations} sessionId={sessionId} openSession={openSession} onClose={()=>setShowActivity(false)}/>}
  </section>
  {inspector&&<><Button variant="bare" className="room-inspector-scrim" aria-label={t('rooms.inspectorClose')} onClick={()=>setInspector(false)}/><aside className="room-inspector" aria-label={t('rooms.groupSettings')}><div className="room-inspector-title"><div><span className="room-eyebrow">{t('ui.label.inThisRoom')}</span><h2>{t('rooms.groupSettings')}</h2></div><Button size="icon" variant="ghost" aria-label={t('rooms.settingsClose')} onClick={()=>setInspector(false)}><X size={17}/></Button></div><div className="room-owner-card"><Avatar owner/><div><strong>{current.ownerName||t('rooms.me')}</strong><small>{t('rooms.ownerTag')}</small></div><span>{t('rooms.me')}</span></div><div className="room-inspector-scroll"><RoomSettings value={draft} onChange={updateDraft} agents={agents} providers={providers} disabled={saving}/>{saved&&<p className="room-saved"><Check size={13}/> {t('rooms.settingsSaved')}</p>}<Button variant="ghost" className="mt-3 w-full !justify-start !text-xs" onClick={()=>{setShowActivity(true);setInspector(false)}} aria-expanded={showActivity}><Activity size={14}/>{t('rooms.activity')}{feed.jobs.length>0&&' '+feed.jobs.length}</Button>{(learningNotice||learningActive>0||memoryCount>0)&&<p className="room-note">{learningNotice}{learningActive>0&&` · ${t('rooms.learning')} ${learningActive}`}{memoryCount>0&&` · ${t('rooms.memory',{count:memoryCount})}`}</p>}<Button variant="bare" className="room-reset" onClick={()=>void reset()} disabled={stopping}><RotateCcw size={13}/> {t('rooms.reset')}</Button><Button variant="bare" className="room-delete" onClick={()=>void remove()}><Trash2 size={13}/> {t('rooms.deleteRoom')}</Button></div><div className="room-inspector-footer"><Button variant="primary" disabled={!dirty||saving||!draft.name.trim()||!draft.agentIds.length} onClick={()=>void save()}>{saving?<LoaderCircle size={14} className="animate-spin"/>:<Check size={14}/>} {t('rooms.saveChanges')}</Button></div></aside></>}
  {promptOpen&&<DialogBackdrop onDismiss={closePrompt}><DialogPanel size="wide" aria-label={t('rooms.systemPrompt')}><div className="room-dialog-header"><div><span className="room-eyebrow">{t('rooms.systemPrompt')}</span><h2>{t('rooms.systemPromptHint')}</h2></div><Button size="icon" variant="ghost" aria-label={t('rooms.close')} disabled={promptSaving} onClick={closePrompt}><X size={18}/></Button></div><div className="room-dialog-body">{roster.length>1&&<FormField label={t('rooms.systemPromptAgent')}><SelectInput disabled={promptSaving||promptBusy} value={promptAgent} onChange={e=>void openPrompt(e.target.value)}>{roster.map(agent=><option key={agent.id} value={agent.id}>{agent.name}</option>)}</SelectInput></FormField>}{prompt&&!promptBusy&&<p className="room-note">{t('rooms.systemPromptStats',{chars:prompt.chars.toLocaleString(),tokens:prompt.tokens.toLocaleString(),personaChars:prompt.personaChars.toLocaleString(),personaTokens:prompt.personaTokens.toLocaleString()})}</p>}{promptBusy?<p className="room-note"><LoaderCircle size={14} className="animate-spin"/> {t('rooms.systemPromptLoading')}</p>:prompt&&<><details className="room-prompt-part" open><summary>{t('rooms.systemPromptCommon')} · {prompt.commonChars.toLocaleString()}자 / {prompt.commonTokens.toLocaleString()}토큰</summary><pre className="room-system-prompt">{prompt.common}</pre></details><details className="room-prompt-part"><summary>{t('rooms.systemPromptPersona')} · {prompt.personaChars.toLocaleString()}자 / {prompt.personaTokens.toLocaleString()}토큰</summary><TextArea aria-label={t('rooms.systemPromptPersona')} className="room-system-prompt" rows={10} disabled={promptSaving} value={personaDraft} onChange={e=>setPersonaDraft(e.target.value)}/></details><details className="room-prompt-part"><summary>{t('rooms.systemPromptCombined')} · {prompt.chars.toLocaleString()}자 / {prompt.tokens.toLocaleString()}토큰</summary><pre className="room-system-prompt">{prompt.prompt}</pre></details></>}<Notice message={promptError}/>{promptNotice&&<p role="status" className="room-saved"><Check size={13}/>{promptNotice}</p>}</div><div className="room-dialog-footer"><span>{t('rooms.systemPromptHint')}</span><Button variant="primary" disabled={promptSaving||promptBusy||!promptDirty} onClick={()=>void savePrompt()}>{promptSaving?<LoaderCircle size={14} className="animate-spin"/>:<Check size={14}/>} {t('rooms.saveChanges')}</Button><Button disabled={promptSaving} onClick={closePrompt}>{t('rooms.close')}</Button></div></DialogPanel></DialogBackdrop>}
{knowledgeOpen&&<DialogBackdrop onDismiss={()=>setKnowledgeOpen(false)}><DialogPanel size="wide" aria-label={t('rooms.sharedKnowledge')}><div className="room-dialog-header"><div><span className="room-eyebrow">{t('ui.label.sharedKnowledge')}</span><h2>{t('rooms.knowledgeOf',{name:current.name})}</h2></div><Button size="icon" variant="ghost" aria-label={t('rooms.close')} onClick={()=>setKnowledgeOpen(false)}><X size={18}/></Button></div><div className="room-dialog-body"><p className="room-note">{t('rooms.knowledgeDesc')}</p><div className="room-knowledge-list">{knowledgeBusy&&!knowledge.length&&<p className="room-note"><LoaderCircle size={14} className="animate-spin"/> {t('rooms.knowledgeLoading')}</p>}{!knowledgeBusy&&!knowledge.length&&<p className="room-note">{t('rooms.knowledgeEmpty')}</p>}{knowledge.map(item=>{const sources=(item.sources||[]).map((source:any)=>[source.agentName||source.agentId||'?',source.model||t('rooms.modelUnrecorded')].filter(Boolean).join(' · ')).filter(Boolean).join(', ');return <div className={'room-knowledge-item '+(item.status==='superseded'?'is-superseded':'')} key={item.id}><div className="room-knowledge-head"><strong>{item.type}</strong><span>{item.verification||'unverified'}</span><span>{item.status==='active'?t('rooms.knowledgeActive'):t('rooms.knowledgeSuperseded')}</span></div><p>{item.text}</p><small>{t('rooms.source')}: {sources||t('rooms.noRecord')}</small></div>})}</div>{knowledgeNotice&&<p className="room-saved"><Check size={13}/>{knowledgeNotice}</p>}</div><div className="room-dialog-footer"><span>{t('rooms.selectedKnowledge')}</span><div className="flex gap-2"><Button variant="secondary" disabled={knowledgeBusy} onClick={()=>void learnKnowledge()}>{knowledgeBusy?<LoaderCircle size={14} className="animate-spin"/>:<RefreshCw size={14}/>} {t('rooms.learnNow')}</Button><Button onClick={()=>setKnowledgeOpen(false)}>{t('rooms.close')}</Button></div></div></DialogPanel></DialogBackdrop>}
 </div>
}

function TranscriptMessage({message,agent,name,owner,index,showReasoning=true}:{showReasoning?:boolean;message:ChatMessage;agent?:Agent;name:string;owner:boolean;index:number}){
 useLocale()
 const [copied,setCopied]=useState(false)
 return <article className={'room-message '+(owner?'room-message-owner':'room-message-agent')+(message.metadata?.kind==='learning_report'?' room-message-review':'')}><Avatar agent={agent} owner={owner} index={index}/><div className="room-message-main"><div className="room-message-byline"><strong>{name}</strong>{message.metadata?.kind==='learning_report'?<span className="room-agent-tag">{t(message.metadata?.learningScope==='room'||message.metadata?.source==='knowledge'?'rooms.sharedLearning':'rooms.selfReview')}</span>:owner?<span className="room-author-tag">{t('rooms.authorMe')}</span>:<span className="room-agent-tag">{t('rooms.authorAgent')}</span>}<time>{stamp(message.created_at)}</time>{!owner&&message.metadata?.usage?.model&&<span className="room-message-model" title={t('rooms.routeActual')}>{message.metadata.usage.model}</span>}<TokenUsage usage={message.metadata?.usage} className="room-message-usage"/><Button variant="bare" className="room-copy" aria-label={t('rooms.copyAria')} title={t('rooms.copyAria')} onClick={()=>{void navigator.clipboard.writeText(message.content).then(()=>{setCopied(true);setTimeout(()=>setCopied(false),1600)})}}>{copied?<Check size={12}/>:<Copy size={12}/>}</Button></div>{showReasoning&&message.reasoning&&<details className="room-reasoning"><summary>{t('rooms.live.showReasoning')}</summary><pre>{message.reasoning}</pre></details>}<MessageBody text={message.content} owner={owner}/>
   {owner&&Array.isArray(message.metadata?.attachments)&&message.metadata.attachments.length>0&&
    <div className="chat-saved-attachments">
     {(message.metadata.attachments as {id:string;name:string}[]).map(file=><a key={file.id} href={'/api/attachments/'+encodeURIComponent(file.id)+'/content'} target="_blank" rel="noopener noreferrer"><Paperclip size={13}/><span>{file.name}</span></a>)}
    </div>}
   {message.tool_calls?.length?<span className="room-message-tools"><TerminalSquare size={12}/>{t('rooms.toolCallsSummary',{count:message.tool_calls.length})}</span>:null}</div></article>
}
function MessageBody({text,owner=false,streaming=false}:{text:string;owner?:boolean;streaming?:boolean}){
 return <div className="room-message-text">{owner||streaming?text:<MarkdownMessage text={text}/>}</div>
}

function RoomActivity({jobs,executions,learningJobs,recordedOperations,sessionId,openSession,onClose}:{jobs:Job[];executions:Execution[];learningJobs:Job[];recordedOperations:Operation[];sessionId?:string;openSession:(id:string)=>void;onClose:()=>void}){
 useLocale()
 const [selected,setSelected]=useState(''),[operations,setOperations]=useState<Operation[]>([]),[error,setError]=useState(''),[loading,setLoading]=useState(false)
 useEffect(()=>{let valid=true;setOperations([]);setError('');if(!selected)return;setLoading(true);get<Operation[]>('/api/executions/'+selected+'/operations').then(rows=>{if(valid)setOperations(rows)}).catch(e=>{if(valid)setError(e.message)}).finally(()=>{if(valid)setLoading(false)});return()=>{valid=false}},[selected])
 return <div className="room-activity"><div className="room-activity-title"><span><Activity size={15}/> {t('rooms.activityTitle')}</span><div>{sessionId&&<Button size="sm" variant="ghost" onClick={()=>openSession(sessionId)}>{t('rooms.originalSession')} <ArrowRight size={12}/></Button>}<Button size="icon" variant="ghost" aria-label={t('rooms.activityClose')} onClick={onClose}><X size={15}/></Button></div></div><div className="room-activity-scroll">{!jobs.length&&!executions.length&&<p className="room-note">{t('rooms.noActivity')}</p>}{jobs.slice(0,8).map(job=><div key={job.id} className="room-job-row"><span className={'room-job-status '+(active(job.status)?'is-running':'')}>{labels[job.status]?t(labels[job.status]):job.status}</span><span>{t('rooms.turns',{current:job.stepIndex||0,total:job.stepTotal||0})}</span><time>{stamp(job.createdAt)}</time>{job.error&&<p>{translateError(job.error)}</p>}</div>)}{executions.length>0&&<div className="room-execution-buttons">{executions.slice(0,12).map(execution=><Button variant="bare" key={execution.id} className={selected===execution.id?'is-selected':''} onClick={()=>setSelected(selected===execution.id?'':execution.id)}><TerminalSquare size={12}/>{t('rooms.executionButton',{status:labels[execution.status]?t(labels[execution.status]):execution.status,tools:execution.toolCalls||0,tokens:execution.tokens||0})}</Button>)}</div>}{loading&&<span className="room-note">{t('rooms.loadingEvidence')}</span>}{selected&&!loading&&!operations.length&&!error&&<p className="room-note">{t('rooms.noToolCalls')}</p>}{(selected?operations:recordedOperations.slice(0,10)).map(operation=><details className="room-operation" key={operation.id}><summary><TerminalSquare size={12}/>{operation.tool||operation.toolName||operation.name||t('rooms.toolFallback')}<span>{operation.status?t(labels[operation.status]||'rooms.recordFallback'):t('rooms.recordFallback')}</span></summary><pre>{JSON.stringify(operation,null,2)}</pre></details>)}{learningJobs.length>0&&<div className="room-learning-log"><BookOpenText size={13}/><span>{t('rooms.learningJobs',{total:learningJobs.length,done:learningJobs.filter(job=>job.status==='completed').length})}</span></div>}<Notice message={error}/></div></div>
}
