import {useEffect,useMemo,useState} from 'react'
import {Activity,AlertCircle,ArrowLeft,ArrowUpRight,Check,ChevronRight,ExternalLink,MessageCircle,Plus,Search,Send,Settings2,ShieldCheck,Trash2,Users} from 'lucide-react'
import {Button,TextInput} from './Primitives'
import {ChannelDeliveryReview} from './ChannelDeliveryReview'
import {t,useLocale,type LocaleKey} from '../i18n'
import {channelHealth,matchingChannels,sortChannelBindings,type Channel,type ChannelEntry,type ChannelHealth,type ChannelProbe} from '../utils/channelView'

const iconByChannel:Record<string,typeof MessageCircle>={
 discord:MessageCircle,telegram:Send,slack:MessageCircle,
 whatsapp:MessageCircle,line:MessageCircle,signal:ShieldCheck,matrix:Users,
 mattermost:Users,zulip:MessageCircle
}
const statusKeys:Record<ChannelHealth,LocaleKey>={
 setup:'conn.health.setup',issue:'conn.health.issue',connected:'conn.status.connected',
 outbound:'conn.status.outbound_ready',paused:'conn.status.sync_disabled',
 disabled:'conn.status.disabled',starting:'conn.status.starting',stopped:'conn.status.not_started'
}
const categories:[
 {id:'all';label:LocaleKey},
 ...{id:string;label:LocaleKey}[]
]=[
 {id:'all',label:'conn.group.all'},{id:'messaging',label:'conn.group.messaging'},
 {id:'workplace',label:'conn.group.workplace'},{id:'community',label:'conn.group.community'},
 {id:'decentralized',label:'conn.group.decentralized'}
]
type ConnectionFilter='all'|'attention'|'ready'
type Props={
 entries:ChannelEntry[];bindings:Channel[];rooms:{id:string;name:string}[];focusId?:string|null;
 loading:boolean;error:string;probes:Record<string,ChannelProbe>;busyAction:string|null;
 onEdit:(entry:ChannelEntry,existing?:Channel)=>void;
 onDiagnose:(channel:Channel)=>void;onDelete:(channel:Channel)=>void;
 onDeliveryReconciled:()=>Promise<void>
}
function needsAttention(channel:Channel){
 return ['issue','setup'].includes(channelHealth(channel))
}
function isWorking(channel:Channel){
 return ['connected','outbound'].includes(channelHealth(channel))
}
function Status({channel}:{channel:Channel}){
 const state=channelHealth(channel)
 const tone=state==='connected'||state==='outbound'
  ?'bg-emerald-50 text-emerald-800 ring-emerald-200'
  :state==='issue'||state==='setup'
   ?'bg-amber-50 text-amber-800 ring-amber-200'
   :state==='starting'
    ?'bg-blue-50 text-blue-800 ring-blue-200'
    :'bg-[#F4F0EA] text-[#6C6358] ring-[#E7E0D5]'
 return <span className={'inline-flex max-w-full items-center gap-1.5 rounded-full px-2.5 py-1 text-[11px] font-semibold ring-1 ring-inset '+tone}>
  <span aria-hidden className={'size-1.5 shrink-0 rounded-full '+(state==='connected'||state==='outbound'?'bg-emerald-500':state==='issue'||state==='setup'?'bg-amber-500':'bg-slate-400')}/>
  {t(statusKeys[state])}
 </span>
}
function DataPair({label,value}:{label:string;value:string|number}){
 return <div className="flex min-w-0 items-start justify-between gap-4 py-2 text-[12px]">
  <span className="shrink-0 text-[#887F73]">{label}</span>
  <span className="min-w-0 break-all text-right font-semibold text-[#3C342C]">{value}</span>
 </div>
}
export function ChannelDirectory({entries,bindings,rooms,focusId,loading,error,probes,busyAction,onEdit,onDiagnose,onDelete,onDeliveryReconciled}:Props){
 useLocale()
 const [adding,setAdding]=useState(false)
 const [selectedId,setSelectedId]=useState<string|null>(null)
 useEffect(()=>{if(focusId)setSelectedId(focusId)},[focusId])
 const [filter,setFilter]=useState<ConnectionFilter>('all')
 const [query,setQuery]=useState('')
 const [category,setCategory]=useState('all')
 const allBindings=useMemo(()=>sortChannelBindings(bindings),[bindings])
 const visibleBindings=useMemo(()=>allBindings.filter(connection=>{
  if(filter==='attention'&&!needsAttention(connection))return false
  if(filter==='ready'&&!isWorking(connection))return false
  const term=query.trim().toLocaleLowerCase()
  if(!term)return true
  const entry=entries.find(item=>item.id===connection.kind)
  const room=rooms.find(item=>item.id===connection.roomId)
  return [connection.name,connection.kind,entry?.name,room?.name,connection.channelId,connection.chatId]
   .some(value=>String(value||'').toLocaleLowerCase().includes(term))
 }),[allBindings,entries,rooms,filter,query])
 const selected=visibleBindings.find(connection=>connection.id===selectedId)||visibleBindings[0]
 const selectedEntry=entries.find(entry=>entry.id===selected?.kind)
 const room=rooms.find(item=>item.id===selected?.roomId)
 const reviewCount=bindings.filter(needsAttention).length
 const readyCount=bindings.filter(isWorking).length
 const catalog=useMemo(()=>matchingChannels(entries,query,category),[entries,query,category])
 const add=()=>{
  setQuery('')
  setCategory('all')
  setAdding(true)
 }
 const choose=(entry:ChannelEntry)=>{
  setAdding(false)
  setQuery('')
  onEdit(entry)
 }
 return <div className="space-y-4">
  <div className="flex flex-wrap items-center justify-between gap-3 pb-1">
   <div className="min-w-0">
    {adding?<div className="flex items-center gap-2">
     <Button size="icon" variant="ghost" onClick={()=>{setAdding(false);setQuery('')}} aria-label={t('conn.backToConnections')}><ArrowLeft size={18}/></Button>
     <div><h2 className="text-[18px] font-bold text-[#29241F]">{t('conn.catalogTitle')}</h2>
      <p className="text-[12px] text-[#83786D]">{t('conn.catalogDesc')}</p></div>
    </div>:<div>
     <h2 className="text-[18px] font-bold text-[#29241F]">{t('conn.activeBindings')}</h2>
     <p className="mt-1 text-[12px] text-[#83786D]">
      {t('conn.overviewInline',{total:bindings.length,ready:readyCount,issues:reviewCount})}
     </p>
    </div>}
   </div>
   {adding
    ?<Button size="sm" variant="secondary" onClick={()=>{setAdding(false);setQuery('')}}><ArrowLeft size={15}/>{t('conn.backToConnections')}</Button>
    :<Button variant="primary" onClick={add}><Plus size={17}/>{t('conn.addChannel')}</Button>}
  </div>
  {error&&<div role="alert" className="rounded-xl border border-rose-200 bg-rose-50 px-4 py-3 text-sm text-rose-700">{error}</div>}
  {adding?<section aria-label={t('conn.catalogTitle')} className="space-y-3">
   <div className="flex flex-wrap items-center gap-3 rounded-xl border border-[#E7E0D5] bg-white px-3 py-2">
    <label className="flex min-w-[220px] flex-1 items-center gap-2 text-[#8D8376]">
     <Search size={17} className="shrink-0"/>
     <TextInput autoFocus aria-label={t('conn.search')} value={query} onChange={e=>setQuery(e.target.value)}
      className="!border-0 !bg-transparent !p-1 focus:!ring-0" placeholder={t('conn.search')}/>
    </label>
   </div>
   <div className="flex flex-wrap gap-1.5" aria-label={t('conn.filterCategory')}>
    {categories.filter(group=>group.id==='all'||entries.some(entry=>entry.category===group.id))
     .map(group=><Button key={group.id} size="sm" variant={category===group.id?'primary':'ghost'}
      aria-pressed={category===group.id} onClick={()=>setCategory(group.id)}>{t(group.label)}</Button>)}
   </div>
   <div className="grid gap-2 sm:grid-cols-2">
    {catalog.map(entry=>{
     const Icon=iconByChannel[entry.id]||MessageCircle
     const count=bindings.filter(item=>item.kind===entry.id).length
     return <article key={entry.id} className="flex min-w-0 flex-col gap-3 rounded-xl border border-[#E7E0D5] bg-white p-4 hover:border-[#C9B7A6]">
      <div className="flex min-w-0 items-start gap-3">
       <span className="grid size-10 shrink-0 place-items-center rounded-xl bg-[#F6EAE3] text-[#C96442]"><Icon size={19}/></span>
       <div className="min-w-0 flex-1">
        <h3 className="font-bold text-[#302A23]">{entry.name}</h3>
        <p className="mt-0.5 text-[11px] text-[#83786D]">{entry.mode==='outbound'?t('conn.outboundOnly'):t('conn.bidirectional')}
         {count>0?' · '+t('conn.boundCount',{count}):''}</p>
       </div>
       {entry.available&&<Check size={16} className="shrink-0 text-emerald-600" aria-label={t('conn.available')}/>}
      </div>
      <p className="text-[12px] leading-5 text-[#655D53]">{entry.description}</p>
      <div className="mt-auto flex items-center justify-between gap-3 border-t border-[#F0EBE4] pt-3">
       {entry.docs?<a href={entry.docs} target="_blank" rel="noopener noreferrer"
        className="inline-flex items-center gap-1 text-xs text-[#8F674C] hover:underline">
        {t('conn.docs')} <ExternalLink size={13}/></a>:<span/>}
       <Button size="sm" variant="primary" disabled={!entry.available} onClick={()=>choose(entry)}>
        {t('conn.setup')} <ChevronRight size={14}/></Button>
      </div>
     </article>
    })}
   </div>
   {!catalog.length&&<div className="rounded-xl border border-dashed border-[#DFD5C8] bg-white p-8 text-center text-sm text-[#796F62]">{loading?t('conn.loading'):t('conn.noResults')}</div>}
  </section>:bindings.length===0
  ?<section className="flex min-h-[340px] flex-col items-center justify-center gap-4 rounded-2xl border border-dashed border-[#DCCFC1] bg-[#FFFDF9] p-8 text-center">
    <span className="grid size-16 place-items-center rounded-2xl bg-[#F7E8E1] text-[#C96442]"><MessageCircle size={29}/></span>
    <div><h3 className="text-lg font-bold text-[#322A23]">{t('conn.emptyTitle')}</h3>
     <p className="mt-2 max-w-sm text-[13px] leading-6 text-[#82776B]">{t('conn.emptyDescription')}</p></div>
    <Button variant="primary" onClick={add}><Plus size={16}/>{t('conn.addChannel')}</Button>
   </section>
  :<div className="grid min-w-0 gap-4 lg:grid-cols-[minmax(265px,350px)_minmax(0,1fr)]">
   <section aria-label={t('conn.activeBindings')} className="min-w-0 overflow-hidden rounded-2xl border border-[#E8E0D5] bg-white">
    <div className="border-b border-[#EEE7DD] p-3">
     <label className="flex items-center gap-2 rounded-xl bg-[#F8F5F0] px-3 text-[#8B8175]">
      <Search size={15} className="shrink-0"/>
      <TextInput aria-label={t('conn.searchConnected')} value={query} onChange={e=>setQuery(e.target.value)}
       placeholder={t('conn.searchConnected')} className="!border-0 !bg-transparent !px-0 !py-2.5 focus:!ring-0"/>
     </label>
     <div className="mt-3 flex flex-wrap gap-1.5">
      {([
       {id:'all',name:t('conn.viewAll'),count:bindings.length},
       {id:'attention',name:t('conn.viewAttention'),count:reviewCount},
       {id:'ready',name:t('conn.viewReady'),count:readyCount}
      ] as const).map(item=><Button key={item.id} size="sm" aria-pressed={filter===item.id}
       variant={filter===item.id?'primary':'ghost'} className="!px-2.5"
       onClick={()=>setFilter(item.id)}>{item.name} <span className="text-[10px] opacity-75">{item.count}</span></Button>)}
     </div>
    </div>
    <div className="max-h-[58vh] overflow-y-auto">
     {visibleBindings.map(binding=>{
      const Icon=iconByChannel[binding.kind]||MessageCircle
      const chosen=selected?.id===binding.id
      const roomName=rooms.find(item=>item.id===binding.roomId)?.name
      return <button type="button" key={binding.id} aria-pressed={chosen} onClick={()=>setSelectedId(binding.id)}
       className={'flex w-full items-start gap-3 border-b border-[#F1EBE3] px-3 py-3.5 text-left transition-colors last:border-b-0 focus-visible:outline focus-visible:outline-2 focus-visible:outline-[#C96442] '+(chosen?'bg-[#FCF0E9]':'hover:bg-[#FAF8F5]')}>
       <span className={'mt-0.5 grid size-9 shrink-0 place-items-center rounded-xl '+(chosen?'bg-[#F3D9CB] text-[#A85136]':'bg-[#F7F2EB] text-[#857462]')}><Icon size={18}/></span>
       <span className="min-w-0 flex-1">
        <span className="block truncate text-[13px] font-semibold text-[#332C24]">{binding.name}</span>
        <span className="mt-0.5 block truncate text-[11px] text-[#897F73]">{roomName||binding.kind}</span>
        <span className="mt-1.5 inline-flex items-center gap-1 text-[11px] font-medium text-[#665D51]">
         <span aria-hidden className={'size-1.5 rounded-full '+(isWorking(binding)?'bg-emerald-500':needsAttention(binding)?'bg-amber-500':'bg-slate-400')}/>
         {t(statusKeys[channelHealth(binding)])}
        </span>
       </span>
       <ChevronRight size={15} className="mt-2 shrink-0 text-[#A79A8C]"/>
      </button>
     })}
     {!visibleBindings.length&&<p className="p-5 text-center text-xs leading-6 text-[#857C71]">{t('conn.noMatchingConnections')}</p>}
    </div>
   </section>
   <section aria-label={t('conn.connectionDetails')} className="min-w-0 rounded-2xl border border-[#E8E0D5] bg-[#FFFDFB]">
    {!selected?<p className="p-8 text-center text-sm text-[#8A8072]">{t('conn.selectConnection')}</p>
    :<div className="flex min-h-full flex-col">
     <header className="flex flex-wrap items-start justify-between gap-3 border-b border-[#EFE8DE] p-5">
      <div className="flex min-w-0 items-start gap-3">
       {(()=>{
        const Icon=iconByChannel[selected.kind]||MessageCircle
        return <span className="grid size-11 shrink-0 place-items-center rounded-xl bg-[#F3E4DA] text-[#C96442]"><Icon size={22}/></span>
       })()}
       <div className="min-w-0">
        <h3 className="break-words text-[17px] font-bold text-[#2F2922]">{selected.name}</h3>
        <p className="mt-1 text-xs text-[#80766A]">{selectedEntry?.name||selected.kind}</p>
        <div className="mt-2"><Status channel={selected}/></div>
       </div>
      </div>
      <Button size="sm" variant="primary" onClick={()=>selectedEntry&&onEdit(selectedEntry,selected)}
       disabled={!selectedEntry||busyAction!==null}><Settings2 size={15}/>{t('conn.dlg.settings')}</Button>
     </header>
     <div className="space-y-5 p-5">
      {(needsAttention(selected)||selected.runtimeError||selected.delivery?.lastError)&&
       <div className="rounded-xl border border-amber-200 bg-amber-50 px-3 py-3 text-[12px] leading-5 text-amber-900" role="status">
        <div className="mb-1 flex items-center gap-2 font-bold"><AlertCircle size={16}/>{t('conn.diagnosisTitle')}</div>
        {selected.runtimeError||selected.delivery?.lastError||
         (selected.missing?.length?t('conn.missingFields',{fields:selected.missing.join(', ')}):t('conn.setupNeededHint'))}
       </div>}
      <div>
       <h4 className="mb-2 text-[12px] font-bold text-[#4A4035]">{t('conn.connectionDetails')}</h4>
       <div className="divide-y divide-[#F1EAE0] rounded-xl border border-[#EEE7DC] bg-white px-4">
        <DataPair label={t('conn.platforms')} value={selectedEntry?.name||selected.kind}/>
        <DataPair label={t('conn.room')} value={selected.roomId?(room?.name||t('conn.roomDeleted')):t('conn.singleAgent')}/>
        {(selected.channelId||selected.chatId)&&<DataPair label={t('conn.target')} value={selected.channelId||selected.chatId||''}/>}
        <DataPair label={t('conn.enable')} value={selected.enabled?t('conn.enabled'):t('conn.status.disabled')}/>
       </div>
      </div>
      {selectedEntry?.mode==='outbound'&&<div>
       <h4 className="mb-2 text-[12px] font-bold text-[#4A4035]">{t('conn.deliveryTitle')}</h4>
       <div className="grid grid-cols-2 gap-2 sm:grid-cols-4">
        {([
         {label:t('conn.deliveryPending'),value:(selected.delivery?.pending||0)+(selected.delivery?.sending||0)},
         {label:t('conn.deliverySent'),value:selected.delivery?.sent||0},
         {label:t('conn.deliveryReview'),value:selected.delivery?.needsReview||0}
        ]).map(item=><div key={item.label} className="rounded-lg border border-[#EEE7DD] bg-white px-3 py-3">
         <div className="text-[11px] text-[#897F72]">{item.label}</div><strong className="mt-1 block text-lg tabular-nums text-[#302920]">{item.value}</strong>
        </div>)}
       </div>
       <p className="mt-2 text-[11px] leading-5 text-[#8A8074]">{t('conn.outboundNoteCompact')}</p>
      </div>}
      {(selected.delivery?.needsReview||0)>0&&<ChannelDeliveryReview
       channelId={selected.id} onChanged={onDeliveryReconciled}/>}
      {probes[selected.id]&&<div role="status" aria-live="polite" className={'rounded-xl border p-3 text-xs leading-5 '+(probes[selected.id].status==='verified'?'border-emerald-200 bg-emerald-50 text-emerald-800':probes[selected.id].status==='unverified'?'border-amber-200 bg-amber-50 text-amber-900':'border-[#D8E5E6] bg-[#F3FAFA] text-[#42656B]')}>
       <strong className="block text-[12px]">{probes[selected.id].status==='verified'?t('conn.probeVerified'):probes[selected.id].status==='checking'?t('conn.dlg.diagnosing'):t('conn.probeUnverified')}</strong>
       <span className="mt-1 block break-words">{probes[selected.id].message}</span>
       {probes[selected.id].checkedAt&&<span className="mt-1 block opacity-70">{t('conn.probeCheckedAt',{time:new Date(probes[selected.id].checkedAt||'').toLocaleString()})}</span>}
       <span className="mt-1 block opacity-80">{t('conn.probeIsNotDelivery')}</span>
      </div>}
      <div className="flex flex-wrap items-center gap-2 border-t border-[#EFE8DD] pt-4">
       <Button size="sm" variant={probes[selected.id]?.status==='unverified'?'primary':'secondary'} disabled={busyAction!==null} onClick={()=>onDiagnose(selected)}>
        <Activity size={15} className={busyAction==='probe:'+selected.id?'animate-spin':''}/>
        {busyAction==='probe:'+selected.id?t('conn.dlg.diagnosing'):t('conn.dlg.diagnose')}
       </Button>
       {selectedEntry?.docs&&<a href={selectedEntry.docs} target="_blank" rel="noopener noreferrer" className="inline-flex min-h-9 items-center gap-1 rounded-lg px-3 text-xs font-semibold text-[#80614F] hover:bg-[#F7F0E8]">
        {t('conn.docs')} <ArrowUpRight size={14}/></a>}
       <Button size="sm" variant="ghost" className="sm:ml-auto text-rose-700" disabled={busyAction!==null}
        onClick={()=>onDelete(selected)}><Trash2 size={14}/>{t('conn.dlg.delete')}</Button>
      </div>
     </div>
    </div>}
   </section>
  </div>}
 </div>
}
