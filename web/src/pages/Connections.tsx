import {useMemo,useState} from 'react'
import {
 Activity,Blocks,BookOpenText,Check,ExternalLink,Filter,Globe2,
 Link2,LoaderCircle,MessageCircle,PackagePlus,Plus,RadioTower,
 Search,Send,Server,Settings2,ShieldCheck,Sparkles,Trash2,UsersRound,X,
} from 'lucide-react'
import {DialogBackdrop,DialogPanel} from '../components/Dialog'
import {StatusPill} from '../components/StatusPill'
import {Button,TextInput,SelectInput} from '../components/Primitives'
import {del,post} from '../api'
import {Action,FormField,Notice,Panel,useRemote} from '../components/UI'
import {t,useLocale,type LocaleKey} from '../i18n'
import {ExtensionsPage,type ExtensionTab} from './Extensions'
import {CapabilitiesPage} from './Capabilities'
import {ConnectedAccounts} from '../components/ConnectedAccounts'
import {
 type ChannelEntry,type ChannelBinding,type ChannelDraft,channelsByKind,
 visibleChannelCatalog,channelDraftMissing,channelStatusTone,
} from '../utils/connectionState'

type Section='channels'|'quick-connect'|'capabilities'|ExtensionTab
type Editor={entry:ChannelEntry;record:ChannelDraft}
const groups=[
 {id:'all',name:'conn.group.all'},
 {id:'messaging',name:'conn.group.messaging'},
 {id:'workplace',name:'conn.group.workplace'},
 {id:'community',name:'conn.group.community'},
 {id:'decentralized',name:'conn.group.decentralized'},
] as const

const channelIcons:Record<string,typeof MessageCircle>={
 discord:MessageCircle,telegram:Send,slack:Blocks,whatsapp:MessageCircle,
 signal:ShieldCheck,teams:UsersRound,matrix:Blocks,webhook:Link2,
}

const statusKeys:Record<string,LocaleKey>={
 connected:'conn.status.connected',outbound_ready:'conn.status.outbound_ready',
 sync_disabled:'conn.status.sync_disabled',webhook_ready:'conn.status.webhook_ready',
 starting:'conn.status.starting',reconnecting:'conn.status.reconnecting',
 error:'conn.status.error',disabled:'conn.status.disabled',
 not_started:'conn.status.not_started',stopped:'conn.status.stopped',
}
function statusText(status:string){
 return statusKeys[status]?t(statusKeys[status]):status
}
function ChannelHealth({channel}:{channel:ChannelBinding}){
 const tone=channelStatusTone(channel)
 const colors={
  online:'border-emerald-200 bg-emerald-50 text-emerald-700',
  pending:'border-amber-200 bg-amber-50 text-amber-800',
  error:'border-rose-200 bg-rose-50 text-rose-700',
  disabled:'border-divider bg-[#F3EEE5] text-[#70675C]',
 }
 return <span role="status" className={'inline-flex shrink-0 items-center gap-1.5 rounded-full border px-2.5 py-1 text-[11px] font-medium '+colors[tone]}>
  <span className={'size-1.5 rounded-full '+(tone==='online'?'bg-emerald-500':tone==='error'?'bg-rose-500':tone==='pending'?'bg-amber-500':'bg-stone-400')}/>
  {statusText(channel.status)}
 </span>
}
type CardProps={
 entry:ChannelEntry;bindings:ChannelBinding[];rooms:any[];
 probe:Record<string,string>;probing:string|null;deleting:string|null;
 onEdit:(entry:ChannelEntry,channel?:ChannelBinding)=>void;
 onCreate:(entry:ChannelEntry)=>void;
 onDiagnose:(id:string)=>void;onRemove:(channel:ChannelBinding)=>void;
}
function ChannelCard({entry,bindings,rooms,probe,probing,deleting,onEdit,onCreate,onDiagnose,onRemove}:CardProps){
 const Icon=channelIcons[entry.id.toLowerCase()]||MessageCircle
 return <Panel className="flex min-w-0 flex-col gap-3 rounded-2xl border border-divider bg-white p-4 sm:p-5">
  <div className="flex min-w-0 items-start gap-3">
   <div className="grid size-11 shrink-0 place-items-center rounded-xl bg-[#F7E8E1] text-brand"><Icon size={22}/></div>
   <div className="min-w-0 flex-1">
    <h3 className="m-0 break-words text-[15px] font-bold text-ink">{entry.name}</h3>
    <p className="mt-1 break-words text-xs leading-5 text-muted">{entry.description}</p>
   </div>
   <StatusPill status={entry.available?'available':'planned'}>
    {entry.available?t('conn.available'):t('conn.unavailable')}
   </StatusPill>
  </div>
  <div className="flex min-h-6 flex-wrap gap-1.5">
   {entry.features.map(feature=><span key={feature} className="rounded-md bg-[#FAF7F1] px-2 py-1 text-[10px] text-[#776D63]">{feature}</span>)}
   <span className="rounded-md bg-[#FAF7F1] px-2 py-1 text-[10px] text-[#776D63]">{entry.mode==='outbound'?t('conn.outboundOnly'):entry.mode}</span>
  </div>
  {bindings.length>0&&<div className="grid gap-2 border-t border-[#EFE9DF] pt-3">
   {bindings.map(channel=><div key={channel.id} className="min-w-0 rounded-xl border border-divider bg-[#FAF7F1] p-3">
    <div className="flex flex-wrap items-center justify-between gap-2">
     <strong className="min-w-0 break-words text-[12px] font-semibold text-[#3A342D]">{channel.name}</strong>
     <ChannelHealth channel={channel}/>
    </div>
    <div className="mt-1 grid gap-1 text-[11px] leading-5 text-muted">
     {channel.roomId&&<span className="break-words">{t('conn.room')} · {rooms.find(room=>room.id===channel.roomId)?.name||t('conn.roomDeleted')}</span>}
     {entry.mode==='outbound'&&(channel.chatId||channel.channelId)&&<span className="break-all">{t('conn.target')} · {channel.chatId||channel.channelId}</span>}
     {!channel.configured&&channel.missing?.length? <span className="break-words text-amber-800">{channel.missing.join(', ')}</span>:null}
     {channel.runtimeError&&<span className="break-words text-rose-700" role="alert">{channel.runtimeError}</span>}
     {entry.mode==='outbound'&&channel.delivery&&<span>{t('conn.delivery',{pending:channel.delivery.pending??0,sending:channel.delivery.sending??0,sent:channel.delivery.sent??0,review:channel.delivery.needsReview??0})}</span>}
     {entry.mode==='outbound'&&channel.delivery?.lastError&&<span className="break-words text-rose-700">{t('conn.lastError')} · {channel.delivery.lastError}</span>}
     {probe[channel.id]&&<span role="status" className="break-words font-medium text-brand-hover">{probe[channel.id]}</span>}
    </div>
    <div className="mt-2 flex flex-wrap justify-end gap-1 border-t border-divider pt-2">
     <Button size="sm" variant="ghost" onClick={()=>onEdit(entry,channel)} aria-label={t('conn.dlg.settings')+' '+channel.name}>
      <Settings2 size={14}/>{t('conn.dlg.settings')}
     </Button>
     <Button size="sm" variant="ghost" disabled={probing===channel.id} onClick={()=>onDiagnose(channel.id)} aria-label={t('conn.dlg.diagnose')+' '+channel.name}>
      {probing===channel.id?<LoaderCircle size={14} className="animate-spin"/>:<Activity size={14}/>}
      {t('conn.dlg.diagnose')}
     </Button>
     <Button size="sm" variant="ghost" disabled={deleting===channel.id} className="text-rose-700" onClick={()=>onRemove(channel)} aria-label={t('conn.dlg.delete')+' '+channel.name}>
      {deleting===channel.id?<LoaderCircle size={14} className="animate-spin"/>:<Trash2 size={14}/>}
      {t('conn.dlg.delete')}
     </Button>
    </div>
   </div>)}
  </div>}
  <div className="mt-auto flex flex-wrap items-center justify-between gap-2 border-t border-[#EFE9DF] pt-3">
   {entry.docs?<a href={entry.docs} target="_blank" rel="noopener noreferrer" className="inline-flex items-center gap-1 text-xs font-semibold text-brand-hover">
    {t('conn.docs')}<ExternalLink size={13}/>
   </a>:<span/>}
   {entry.available?<Button size="sm" variant="secondary" onClick={()=>onCreate(entry)}>
    <Plus size={15}/>{bindings.length?t('conn.addAccount'):t('conn.setup')}
   </Button>:<span className="rounded-lg bg-amber-50 px-2.5 py-1.5 text-[11px] text-amber-800">{t('conn.noCode')}</span>}
  </div>
 </Panel>
}

type EditorProps={
 editor:Editor;saving:boolean;error:string;rooms:any[];agents:any[];providers:any[];
 onClose:()=>void;onSave:()=>void;onUpdate:(record:ChannelDraft)=>void;english:boolean;
}
function ChannelEditor({editor,saving,error,rooms,agents,providers,onClose,onSave,onUpdate,english}:EditorProps){
 const {entry,record}=editor
 const set=(key:string,value:unknown)=>onUpdate({...record,[key]:value})
 const missing=channelDraftMissing(entry,record)
 const invalidRoom=Boolean(entry.maxRoomParticipants&&record.roomId&&rooms.some(room=>
  room.id===record.roomId&&(room.agentIds?.length||0)>entry.maxRoomParticipants!))
 const valid=!missing.length&&!invalidRoom
 return <DialogBackdrop onDismiss={()=>{if(!saving)onClose()}}>
  <DialogPanel size="wide">
   <div className="flex items-start justify-between gap-3 border-b border-[#EFE9DF] p-5">
    <div className="min-w-0">
     <small className="text-[10px] font-semibold tracking-widest text-[#8A8176]">{entry.name.toUpperCase()} · CHANNEL</small>
     <h2 className="mt-1 text-lg font-bold text-ink">{record.id?t('conn.edit'):t('conn.new')}</h2>
     <p className="mt-1 text-xs leading-5 text-muted">{entry.description}</p>
    </div>
    <Button size="icon" variant="ghost" disabled={saving} onClick={onClose} aria-label={t('conn.cancel')}><X size={18}/></Button>
   </div>
   <div className="grid max-h-[65vh] gap-5 overflow-y-auto p-5">
    <div className="grid gap-3">
     <h3 className="text-[13px] font-bold text-[#3A342D]">{english?'Connection details':'연결 정보'}</h3>
     <FormField label={t('conn.name')}><TextInput autoFocus value={String(record.name||'')} onChange={e=>set('name',e.target.value)}/></FormField>
     {entry.fields.map(field=><FormField key={field.key}
       label={field.label+(field.required?' *':'')}
       hint={field.type==='secret'&&record.id&&record.credentialsPresent?.[field.key]?t('conn.dlg.secretHint'):undefined}>
      <TextInput type={field.type==='secret'?'password':field.type==='url'?'url':'text'}
       autoComplete={field.type==='secret'?'new-password':undefined}
       value={String(record[field.key]||'')}
       placeholder={field.placeholder||''}
       onChange={e=>set(field.key,e.target.value)}/>
     </FormField>)}
    </div>
    <div className="grid gap-3 border-t border-[#EFE9DF] pt-4">
     <h3 className="text-[13px] font-bold text-[#3A342D]">{english?'Destination and routing':'전송 대상과 라우팅'}</h3>
     <FormField label={entry.requiresRoom?t('conn.roomOrAgent'):t('conn.replyMode')}>
      <SelectInput value={record.roomId||''} onChange={e=>set('roomId',e.target.value)}>
       <option value="">{entry.requiresRoom?t('conn.pickRoom'):t('conn.singleAgent')}</option>
       {rooms.map(room=><option value={room.id} key={room.id}
        disabled={!!entry.maxRoomParticipants&&(room.agentIds?.length||0)>entry.maxRoomParticipants}>
        {t('conn.dlg.roomOption',{name:room.name,count:room.agentIds?.length||0})}
       </option>)}
      </SelectInput>
     </FormField>
     {record.roomId&&<div className="flex gap-2 rounded-xl border border-divider bg-[#FAF7F1] p-3 text-xs leading-5 text-muted">
      <UsersRound size={16} className="shrink-0"/><div><strong>{t('conn.roomNote')}</strong><p>{entry.maxRoomParticipants?t('conn.dlg.maxParticipants',{count:entry.maxRoomParticipants}):entry.mode==='outbound'?t('conn.dlg.outboundNote'):t('conn.dlg.roomPolicyNote')}</p></div>
     </div>}
     {record.roomId&&entry.mode!=='outbound'&&<FormField label={t('conn.policy')}>
      <SelectInput value={record.roomPolicy||'all_or_named'} onChange={e=>set('roomPolicy',e.target.value)}>
       <option value="all_or_named">{t('conn.dlg.policyAll')}</option>
       <option value="named_only">{t('conn.dlg.policyNamed')}</option>
       <option value="round_robin">{t('conn.dlg.policyRound')}</option>
      </SelectInput>
     </FormField>}
     {entry.mode!=='outbound'&&<>
      <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
       <FormField label={t('conn.agent')}><SelectInput value={record.agentId||''} onChange={e=>set('agentId',e.target.value)}>
        <option value="">{t('conn.dlg.pickAgent')}</option>{agents.map(agent=><option value={agent.id} key={agent.id}>{agent.name}</option>)}
       </SelectInput></FormField>
       <FormField label={t('conn.provider')}><SelectInput value={record.providerId||''} onChange={e=>set('providerId',e.target.value)}>
        <option value="">{t('conn.dlg.agentDefault')}</option>{providers.map(provider=><option value={provider.id} key={provider.id}>{provider.name}</option>)}
       </SelectInput></FormField>
      </div>
      <FormField label={t('conn.model')}><TextInput value={record.model||''} placeholder={t('conn.dlg.agentDefaultModel')} onChange={e=>set('model',e.target.value)}/></FormField>
      <label className="flex items-center gap-2 text-[12px] text-[#4A443C]">
       <TextInput type="checkbox" checked={record.mentionOnly!==false} onChange={e=>set('mentionOnly',e.target.checked)}/>{t('conn.mentionOnly')}
      </label>
     </>}
    </div>
    <div className="grid gap-3 border-t border-[#EFE9DF] pt-4">
     <h3 className="text-[13px] font-bold text-[#3A342D]">{english?'Delivery options':'전송 옵션'}</h3>
     {entry.id==='discord'&&<>
      <label className="flex items-center gap-2 text-[12px] text-[#4A443C]">
       <TextInput type="checkbox" checked={Boolean(record.replyMentionBots)} onChange={e=>set('replyMentionBots',e.target.checked)}/>{t('conn.dlg.discordMention')}
      </label>
      <label className="flex items-center gap-2 text-[12px] text-[#4A443C]">
       <TextInput type="checkbox" checked={Boolean(record.toolsEnabled)} onChange={e=>set('toolsEnabled',e.target.checked)}/>{t('conn.dlg.discordTools')}
      </label>
     </>}
     {record.roomId&&<label className="flex items-center gap-2 text-[12px] text-[#4A443C]">
      <TextInput type="checkbox" checked={Boolean(record.syncRoomMessages)} onChange={e=>set('syncRoomMessages',e.target.checked)}/>{t('conn.dlg.syncRoom')}
     </label>}
     <label className="flex items-center gap-2 rounded-xl border border-divider bg-[#FAF7F1] p-3 text-[12px] font-semibold text-[#3A342D]">
      <TextInput type="checkbox" checked={Boolean(record.enabled)} onChange={e=>set('enabled',e.target.checked)}/>{t('conn.enable')}
     </label>
    </div>
    {record.webhookPath&&<div className="rounded-xl bg-[#FAF7F1] p-3 text-xs text-[#4A443C]">
     <strong className="block">{t('conn.webhook')}</strong><code className="block break-all">{record.webhookPath}</code>
     <small>{t('conn.dlg.webhookNote')}</small>
    </div>}
    {entry.instructions?.length?<details className="rounded-xl border border-divider p-4 text-xs leading-6">
     <summary className="cursor-pointer font-bold">{t('conn.instructions')}</summary>
     <ol className="mt-2 list-decimal space-y-1 pl-5">{entry.instructions.map((line,index)=><li key={index}>{line}</li>)}</ol>
     {entry.docs&&<a href={entry.docs} target="_blank" rel="noopener noreferrer" className="mt-2 inline-flex items-center gap-1 font-semibold text-brand-hover">{t('conn.openDocs')}<ExternalLink size={13}/></a>}
    </details>:null}
   </div>
   {error&&<div className="px-5"><Notice message={error}/></div>}
   <div className="flex flex-wrap items-center justify-between gap-2 border-t border-[#EFE9DF] px-5 py-4">
    <span role="status" className="text-[11px] text-[#8A8176]">
     {missing.length?(english?'Required fields missing':'필수 항목을 입력하세요'):invalidRoom?(english?'Room capacity exceeded':'회의실 참여 인원 초과'):''}
    </span>
    <div className="flex gap-2">
     <Action variant="secondary" onClick={onClose} disabled={saving}>{t('conn.cancel')}</Action>
     <Action onClick={onSave} disabled={saving||!valid}>
      {saving?<LoaderCircle size={16} className="animate-spin"/>:<Check size={16}/>}
      {t('conn.save')}
     </Action>
    </div>
   </div>
  </DialogPanel>
 </DialogBackdrop>
}

export function ConnectionsPage(){
 const locale=useLocale()
 const english=locale==='en'
 const registry=useRemote<ChannelEntry[]>('/api/channels/catalog',[])
 const channels=useRemote<ChannelBinding[]>('/api/channels',[])
 const agents=useRemote<any[]>('/api/agents',[])
 const rooms=useRemote<any[]>('/api/rooms',[])
 const providers=useRemote<any[]>('/api/providers',[])
 const [section,setSection]=useState<Section>('channels')
 const [query,setQuery]=useState('')
 const [category,setCategory]=useState('all')
 const [onlyConfigured,setOnlyConfigured]=useState(false)
 const [editor,setEditor]=useState<Editor|null>(null)
 const [error,setError]=useState('')
 const [info,setInfo]=useState('')
 const [probe,setProbe]=useState<Record<string,string>>({})
 const [probing,setProbing]=useState<string|null>(null)
 const [deleting,setDeleting]=useState<string|null>(null)
 const [saving,setSaving]=useState(false)
 const byKind=useMemo(()=>channelsByKind(channels.value),[channels.value])
 const filtered=useMemo(()=>
  visibleChannelCatalog(registry.value,byKind,query,category,onlyConfigured),
  [registry.value,byKind,query,category,onlyConfigured])
 const connected=channels.value.filter(channel=>channel.status==='connected').length
 const outbound=channels.value.filter(channel=>channel.status==='outbound_ready').length
 const ready=registry.value.filter(entry=>entry.available).length
 const tabs=[
  {id:'channels',label:t('conn.tab.channels'),icon:RadioTower},
  {id:'quick-connect',label:t('conn.tab.quickConnect'),icon:Link2},
  {id:'market',label:'MCP 마켓',icon:Blocks},
  {id:'installed',label:t('ext.tab.installed'),icon:Server},
  {id:'skills',label:t('ext.tab.skills'),icon:BookOpenText},
  {id:'plugins',label:t('ext.tab.plugins'),icon:PackagePlus},
  {id:'capabilities',label:english?'Built-in tools':'기본 도구',icon:Sparkles},
 ] as const
 const openEditor=(entry:ChannelEntry,existing?:ChannelBinding)=>{
  if(!entry.available)return
  const record:ChannelDraft=existing?{...existing}:{
   name:entry.name,kind:entry.id,enabled:false,
   syncRoomMessages:entry.mode==='outbound',mentionOnly:true,
   roomId:'',roomPolicy:'all_or_named',agentId:'',providerId:'',model:'',
  }
  for(const field of entry.fields)if(field.type==='secret')record[field.key]=''
  setEditor({entry,record});setError('');setInfo('')
 }
 const save=async()=>{
  if(!editor||saving||channelDraftMissing(editor.entry,editor.record).length)return
  setSaving(true);setError('')
  try{
   await post('/api/channels',editor.record)
   setEditor(null)
   setInfo(t('conn.dlg.saved',{name:editor.entry.name}))
   await Promise.all([channels.reload(),registry.reload()])
  }catch(e){setError((e as Error).message)}
  finally{setSaving(false)}
 }
 const diagnose=async(id:string)=>{
  if(probing)return
  setProbing(id)
  setProbe(current=>({...current,[id]:t('conn.dlg.diagnosing')}))
  try{
   const result=await post<{verified:boolean;message?:string}>('/api/channels/'+encodeURIComponent(id)+'/diagnose',{})
   setProbe(current=>({...current,[id]:result.verified?t('conn.dlg.verified'):result.message||t('conn.dlg.checkNeeded')}))
   await channels.reload()
  }catch(e){setProbe(current=>({...current,[id]:(e as Error).message}))}
  finally{setProbing(null)}
 }
 const remove=async(channel:ChannelBinding)=>{
  if(deleting||!window.confirm(t('conn.dlg.removeConfirm',{name:channel.name})))return
  setDeleting(channel.id);setError('');setInfo('')
  try{
   await del('/api/channels/'+encodeURIComponent(channel.id))
   setProbe(current=>{const next={...current};delete next[channel.id];return next})
   await channels.reload()
  }catch(e){setError((e as Error).message)}
  finally{setDeleting(null)}
 }
 const header=<div className="mb-3 flex flex-wrap items-center justify-between gap-3">
  <div>
   <h1 className="m-0 text-[19px] font-bold text-ink">{t('conn.title')}</h1>
   <p className="mt-1 text-[12px] text-muted">{t('conn.description')}</p>
  </div>
  {section==='channels'&&<Button variant="secondary" disabled={registry.loading||channels.loading}
    onClick={()=>{void registry.reload();void channels.reload();setInfo('');setError('')}}>
   <Activity size={15}/>{t('conn.refresh')}
  </Button>}
 </div>
 return <div className="mx-auto w-full max-w-[1320px] px-4 py-3 sm:px-6">
  {header}
  <nav className="connection-tabs" aria-label={english?'Connections and extensions':'연결 및 확장 기능'}>
   {tabs.map(({id,label,icon:Icon})=><Button key={id} variant="bare"
    aria-current={section===id?'page':undefined} onClick={()=>{setSection(id);setInfo('');setError('')}}>
    <Icon size={15}/>{label}
   </Button>)}
  </nav>
  {section==='quick-connect'&&<ConnectedAccounts/>}
  {section==='capabilities'&&<CapabilitiesPage embedded navigate={page=>setSection(page==='extensions'?'market':'channels')}/>}
  {section!=='channels'&&section!=='quick-connect'&&section!=='capabilities'&&<ExtensionsPage section={section} embedded/>}
  {section==='channels'&&<>
   <div className="mb-4 grid grid-cols-2 gap-2 lg:grid-cols-4">
    {[
     {label:t('conn.platforms'),value:registry.value.length,icon:Globe2},
     {label:t('conn.code'),value:ready,icon:RadioTower},
     {label:t('conn.connected'),value:connected,icon:Check},
     {label:t('conn.outbound'),value:outbound,icon:Send},
    ].map(({label,value,icon:Icon})=><div key={label} className="flex min-w-0 items-center gap-3 rounded-xl border border-divider bg-white p-3 sm:p-4">
     <span className="grid size-9 shrink-0 place-items-center rounded-lg bg-[#F7E8E1] text-brand"><Icon size={17}/></span>
     <div className="min-w-0"><small className="block text-[11px] text-[#8A8176]">{label}</small><strong className="block text-lg font-bold text-ink">{value}</strong></div>
    </div>)}
   </div>
   {registry.error&&<Notice message={registry.error}/>}
   {channels.error&&<Notice message={channels.error}/>}
   {error&&!editor&&<Notice message={error}/>}
   {info&&<div role="status" className="my-3 flex items-center gap-2 rounded-xl border border-emerald-200 bg-emerald-50 p-3 text-xs text-emerald-700"><Check size={16}/>{info}</div>}
   <div className="mb-4 grid gap-3">
    <div className="flex min-w-0 items-center gap-2 rounded-xl border border-divider bg-white px-3 py-1">
     <Search size={17} className="shrink-0 text-[#8A8176]"/>
     <TextInput aria-label={t('conn.search')} className="!border-0 !bg-transparent !shadow-none focus:!ring-0"
      value={query} onChange={e=>setQuery(e.target.value)} placeholder={t('conn.search')}
      onKeyDown={e=>{if(e.key==='Escape')setQuery('')}}/>
     {query&&<Button size="icon" variant="ghost" aria-label={english?'Clear search':'검색 지우기'} onClick={()=>setQuery('')}><X size={15}/></Button>}
    </div>
    <div className="flex flex-wrap items-center gap-1.5">
     <Filter size={15} className="mr-1 text-[#8A8176]"/>
     {groups.map(group=><Button key={group.id} size="sm"
      variant={category===group.id?'primary':'ghost'} onClick={()=>setCategory(group.id)}>
      {t(group.name)}
     </Button>)}
     <span className="mx-1 h-5 border-l border-divider"/>
     <Button size="sm" variant={onlyConfigured?'primary':'secondary'}
      onClick={()=>setOnlyConfigured(current=>!current)} aria-pressed={onlyConfigured}>
      {english?'Configured only':'설정된 채널만'}
     </Button>
     <span aria-live="polite" className="ml-auto text-[11px] text-[#8A8176]">{filtered.length} / {registry.value.length}</span>
    </div>
   </div>
   <div className="grid grid-cols-1 gap-3 md:grid-cols-2 xl:grid-cols-3">
    {filtered.map(entry=><ChannelCard key={entry.id} entry={entry}
     bindings={byKind.get(entry.id.toLowerCase())||[]} rooms={rooms.value}
     probe={probe} probing={probing} deleting={deleting}
     onEdit={openEditor} onCreate={openEditor} onDiagnose={id=>{void diagnose(id)}}
     onRemove={channel=>{void remove(channel)}}/>)}
   </div>
   {!filtered.length&&<Panel><div className="flex min-h-40 flex-col items-center justify-center gap-3 p-6 text-center">
    <Search size={23} className="text-[#8A8176]"/>
    <strong className="text-sm text-[#3A342D]">{t('conn.noResults')}</strong>
    <Button size="sm" variant="secondary" onClick={()=>{setQuery('');setCategory('all');setOnlyConfigured(false)}}>
     {english?'Reset filters':'필터 초기화'}
    </Button>
   </div></Panel>}
   <Panel className="mt-4 flex items-start gap-3 rounded-xl border border-[#EED9CC] bg-[#F7E8E1] p-4 text-xs leading-5">
    <ShieldCheck size={20} className="shrink-0 text-brand-hover"/>
    <div><strong>{t('conn.noteTitle')}</strong><p className="mt-1">{t('conn.note')}</p></div>
   </Panel>
  </>}
  {editor&&<ChannelEditor editor={editor} saving={saving} error={error}
   rooms={rooms.value} agents={agents.value} providers={providers.value} english={english}
   onClose={()=>{if(!saving){setEditor(null);setError('')}}}
   onSave={()=>{void save()}}
   onUpdate={record=>setEditor(current=>current?{...current,record}:current)}/>}
 </div>
}
