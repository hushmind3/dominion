import {useMemo,useState} from 'react'
import {Activity,Blocks,CheckCircle2,ExternalLink,Filter,MessageCircle,Plus,Search,Send,Settings2,ShieldCheck,Trash2} from 'lucide-react'
import {Button,TextInput} from './Primitives'
import {Panel} from './UI'
import {t,useLocale,type LocaleKey} from '../i18n'
import {channelHealth,matchingChannels,sortChannelBindings,type Channel,type ChannelEntry,type ChannelHealth} from '../utils/channelView'

const categories=[
 {id:'all',label:'conn.group.all'},
 {id:'messaging',label:'conn.group.messaging'},
 {id:'workplace',label:'conn.group.workplace'},
 {id:'community',label:'conn.group.community'},
 {id:'decentralized',label:'conn.group.decentralized'},
] as const
const icons:Record<string,typeof MessageCircle>={
 discord:MessageCircle,telegram:Send,slack:Blocks,whatsapp:MessageCircle,
 signal:ShieldCheck,matrix:Blocks,line:MessageCircle,whatsapp_business:MessageCircle,
 mattermost:Blocks,zulip:MessageCircle
}
const healthLabels:Record<ChannelHealth,LocaleKey>={
 setup:'conn.health.setup',issue:'conn.health.issue',connected:'conn.status.connected',
 outbound:'conn.status.outbound_ready',paused:'conn.status.sync_disabled',
 disabled:'conn.status.disabled',starting:'conn.status.starting',stopped:'conn.status.not_started'
}
function Health({channel}:{channel:Channel}){
 const status=channelHealth(channel)
 const tone=status==='connected'||status==='outbound'?'bg-emerald-50 text-emerald-800 ring-emerald-200':
  status==='issue'||status==='setup'?'bg-amber-50 text-amber-900 ring-amber-200':
  status==='starting'?'bg-blue-50 text-blue-800 ring-blue-200':
  'bg-[#F3EEE5] text-[#635B50] ring-[#E6DED2]'
 return <span className={'inline-flex max-w-full items-center gap-1.5 rounded-full px-2.5 py-1 text-[11px] font-semibold ring-1 ring-inset '+tone}>
  <span aria-hidden="true" className={'size-1.5 shrink-0 rounded-full '+(status==='connected'||status==='outbound'?'bg-emerald-500':status==='issue'||status==='setup'?'bg-amber-500':'bg-slate-400')}/>
  {t(healthLabels[status])}
 </span>
}
type Props={
 entries:ChannelEntry[];bindings:Channel[];rooms:{id:string;name:string}[];
 loading:boolean;error:string;probes:Record<string,string>;busyAction:string|null;
 onEdit:(entry:ChannelEntry,existing?:Channel)=>void;
 onDiagnose:(channel:Channel)=>void;onDelete:(channel:Channel)=>void;
}
export function ChannelDirectory({entries,bindings,rooms,loading,error,probes,busyAction,onEdit,onDiagnose,onDelete}:Props){
 useLocale()
 const [query,setQuery]=useState('')
 const [category,setCategory]=useState('all')
 const visibleEntries=useMemo(()=>matchingChannels(entries,query,category),[entries,query,category])
 const entryMap=useMemo(()=>new Map(entries.map(entry=>[entry.id,entry])),[entries])
 const roomNames=useMemo(()=>new Map(rooms.map(room=>[room.id,room.name])),[rooms])
 const needle=query.trim().toLocaleLowerCase()
 const visibleBindings=useMemo(()=>sortChannelBindings(bindings.filter(binding=>{
  const entry=entryMap.get(binding.kind)
  if(category!=='all'&&entry?.category!==category)return false
  return [binding.name,binding.kind,entry?.name||'',binding.roomId?roomNames.get(binding.roomId)||'':'']
   .join(' ').toLocaleLowerCase().includes(needle)
 })),[bindings,entryMap,roomNames,needle,category])
 const usable=entries.filter(entry=>entry.available).length
 const operating=bindings.filter(binding=>['connected','outbound'].includes(channelHealth(binding))).length
 const needsAttention=bindings.filter(binding=>['setup','issue'].includes(channelHealth(binding))).length
 return <div className="space-y-5">
  <div className="grid grid-cols-3 gap-2 rounded-2xl border border-[#E7E0D5] bg-[#FFFDF9] p-3 sm:gap-4 sm:p-4" aria-label={t('conn.overview')}>
   {[
    {label:t('conn.code'),value:usable},
    {label:t('conn.operating'),value:operating},
    {label:t('conn.attention'),value:needsAttention}
   ].map(stat=><div key={stat.label} className="min-w-0 border-r border-[#EFE9DF] px-1 last:border-0 sm:px-3">
    <p className="text-[10px] font-semibold text-[#8A8176] sm:text-xs">{stat.label}</p>
    <strong className="mt-1 block text-xl font-bold tabular-nums text-[#29231D] sm:text-2xl">{stat.value}</strong>
   </div>)}
  </div>
  <div className="flex flex-col gap-3 rounded-xl border border-[#E7E0D5] bg-white p-3 sm:flex-row sm:items-center" >
   <label className="flex min-w-0 flex-1 items-center gap-2 text-[#8A8176]">
    <Search size={17} className="shrink-0"/><TextInput className="!border-0 !bg-transparent !p-1 !shadow-none !ring-0"
     value={query} onChange={event=>setQuery(event.target.value)} aria-label={t('conn.search')}
     placeholder={t('conn.search')}/>
   </label>
   <div className="flex flex-wrap items-center gap-1.5 border-t border-[#EFE9DF] pt-3 sm:border-l sm:border-t-0 sm:pl-3 sm:pt-0" aria-label={t('conn.filterCategory')}>
    <Filter size={15} className="shrink-0 text-[#8A8176]" aria-hidden="true"/>
    {categories.filter(item=>item.id==='all'||entries.some(entry=>entry.category===item.id)).map(item=>
     <Button key={item.id} size="sm" variant={category===item.id?'primary':'ghost'} aria-pressed={category===item.id}
      onClick={()=>setCategory(item.id)}>{t(item.label)}</Button>)}
   </div>
  </div>
  {error&&<p role="alert" className="rounded-xl border border-rose-200 bg-rose-50 p-3 text-sm text-rose-700">{error}</p>}
  {loading&&!entries.length&&<Panel className="p-5 text-sm text-[#7C7469]" role="status">{t('conn.loading')}</Panel>}
  {bindings.length>0&&<section aria-label={t('conn.activeBindings')} className="overflow-hidden rounded-2xl border border-[#E7E0D5] bg-white">
   <header className="flex items-center justify-between gap-2 bg-[#FAF7F2] px-4 py-3">
    <div><h2 className="m-0 text-sm font-bold text-[#30291F]">{t('conn.activeBindings')}</h2>
     <p className="mt-1 text-[11px] text-[#7C7469]">{t('conn.activeBindingsDesc')}</p></div>
    <span className="shrink-0 rounded-full bg-white px-3 py-1 text-xs font-semibold tabular-nums text-[#635B50]">{visibleBindings.length}</span>
   </header>
   {visibleBindings.map(binding=>{
    const entry=entryMap.get(binding.kind)
    const Icon=icons[binding.kind]||MessageCircle
    const room=binding.roomId?roomNames.get(binding.roomId)||t('conn.roomDeleted'):null
    const reviewing=binding.delivery?.needsReview||0
    return <article key={binding.id} className="border-t border-[#EFE9DF] px-4 py-3">
     <div className="flex flex-wrap items-start gap-3">
      <span className="grid size-10 shrink-0 place-items-center rounded-xl bg-[#F7E8E1] text-[#C96442]"><Icon size={20}/></span>
      <div className="min-w-0 flex-1">
       <div className="flex flex-wrap items-center gap-2"><h3 className="m-0 break-words text-sm font-semibold text-[#2E2922]">{binding.name}</h3><Health channel={binding}/></div>
       <p className="mt-0.5 text-xs text-[#7C7469]">{entry?.name||binding.kind}{room?' · '+t('conn.room')+': '+room:''}{!room&&entry?.requiresRoom?' · '+t('conn.pickRoom'):''}</p>
       {(binding.channelId||binding.chatId)&&<p className="mt-1 break-all text-[11px] text-[#7C7469]">{t('conn.target')}: {binding.channelId||binding.chatId}</p>}
       {binding.missing?.length? <p className="mt-2 text-xs font-medium text-amber-800">{t('conn.missingFields',{fields:binding.missing.join(', ')})}</p>:null}
       {binding.runtimeError&&<p className="mt-2 break-words text-xs text-rose-700">{binding.runtimeError}</p>}
       {!!reviewing&&<p className="mt-1 text-xs font-medium text-amber-800">{t('conn.pendingReview',{count:reviewing})}</p>}
       {binding.delivery?.lastError&&<p className="mt-1 break-words text-xs text-rose-700">{t('conn.lastError')}: {binding.delivery.lastError}</p>}
       {probes[binding.id]&&<p className="mt-2 text-xs text-[#4B5E6A]" role="status">{probes[binding.id]}</p>}
      </div>
      <div className="flex w-full flex-wrap justify-end gap-1.5 sm:w-auto" role="group" aria-label={binding.name}>
       <Button size="sm" variant="secondary" onClick={()=>entry&&onEdit(entry,binding)} disabled={!entry||busyAction!==null} aria-label={binding.name+' '+t('conn.dlg.settings')}><Settings2 size={14}/>{t('conn.dlg.settings')}</Button>
       <Button size="sm" variant="secondary" onClick={()=>onDiagnose(binding)} disabled={busyAction!==null} aria-label={binding.name+' '+t('conn.dlg.diagnose')}><Activity size={14}/>{busyAction==='probe:'+binding.id?t('conn.dlg.diagnosing'):t('conn.dlg.diagnose')}</Button>
       <Button size="sm" variant="ghost" onClick={()=>onDelete(binding)} disabled={busyAction!==null} aria-label={binding.name+' '+t('conn.dlg.delete')}><Trash2 size={14}/>{t('conn.dlg.delete')}</Button>
      </div>
     </div>
    </article>
   })}
   {!visibleBindings.length&&<p className="border-t border-[#EFE9DF] p-4 text-xs text-[#7C7469]">{t('conn.noResults')}</p>}
  </section>}
  <section aria-label={t('conn.catalogTitle')}>
   <div className="mb-3 flex flex-wrap items-end justify-between gap-2">
    <div><h2 className="text-base font-bold text-[#2E2922]">{t('conn.catalogTitle')}</h2>
     <p className="mt-1 text-xs text-[#7C7469]">{t('conn.catalogDesc')}</p></div>
    <span className="text-xs tabular-nums text-[#8A8176]">{visibleEntries.length} / {entries.length}</span>
   </div>
   <div className="grid grid-cols-1 gap-3 md:grid-cols-2 xl:grid-cols-3">
    {visibleEntries.map(entry=>{
     const Icon=icons[entry.id]||MessageCircle
     const count=bindings.filter(binding=>binding.kind===entry.id).length
     return <Panel key={entry.id} className="flex min-w-0 flex-col gap-3 p-4 transition-colors hover:border-[#D5C7B8]">
      <div className="flex items-start gap-3">
       <span className="grid size-10 shrink-0 place-items-center rounded-xl bg-[#F7E8E1] text-[#C96442]"><Icon size={21}/></span>
       <div className="min-w-0 flex-1">
        <h3 className="m-0 break-words text-sm font-bold text-[#2E2922]">{entry.name}</h3>
        <p className="mt-0.5 text-[11px] text-[#7C7469]">{entry.mode==='outbound'?t('conn.outboundOnly'):t('conn.bidirectional')}{count?' · '+t('conn.boundCount',{count}):''}</p>
       </div>
       {entry.available&&<CheckCircle2 size={16} className="shrink-0 text-emerald-600" aria-label={t('conn.available')}/>}
      </div>
      <p className="min-h-10 text-xs leading-5 text-[#635B50]">{entry.description}</p>
      <div className="mt-auto flex flex-wrap items-center justify-between gap-2 border-t border-[#EFE9DF] pt-3">
       {entry.docs?<a href={entry.docs} target="_blank" rel="noopener noreferrer"
        className="inline-flex items-center gap-1 text-xs font-semibold text-[#8B5D44] hover:underline">{t('conn.docs')}<ExternalLink size={13}/></a>:<span/>}
       <Button size="sm" variant="primary" disabled={!entry.available} onClick={()=>onEdit(entry)}><Plus size={14}/>{count?t('conn.addAccount'):t('conn.setup')}</Button>
      </div>
     </Panel>
    })}
   </div>
   {!visibleEntries.length&&!loading&&<Panel className="p-7 text-center text-sm text-[#7C7469]">{t('conn.noResults')}</Panel>}
  </section>
  <p className="rounded-xl border border-[#E7E0D5] bg-[#FAF7F2] px-4 py-3 text-xs leading-5 text-[#7C7469]">{t('conn.note')}</p>
 </div>
}
