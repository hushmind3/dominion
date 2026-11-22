import {useEffect,useMemo,useState} from 'react'
import {Activity,AlertCircle,ArrowLeft,ArrowRight,ArrowUpRight,Check,CheckCheck,ChevronRight,Clock3,ExternalLink,MessageCircle,Plus,Search,Send,Settings2,ShieldCheck,Trash2,Users,Wifi} from 'lucide-react'
import {Button,TextInput} from './Primitives'
import {t,useLocale,type LocaleKey} from '../i18n'
import {ChannelDeliveryReview} from './ChannelDeliveryReview'
import {channelHealth,matchingChannels,sortChannelBindings,type Channel,type ChannelEntry,type ChannelHealth,type ChannelProbe} from '../utils/channelView'

const icons:Record<string,typeof MessageCircle>={
 discord:MessageCircle,telegram:Send,slack:MessageCircle,whatsapp:MessageCircle,
 signal:ShieldCheck,matrix:Users,line:MessageCircle,zulip:MessageCircle,mattermost:Users
}
const identity:Record<string,{back:string;ink:string;dot:string}>={
 discord:{back:'bg-[#EEEDFF]',ink:'text-[#5865F2]',dot:'bg-[#5865F2]'},
 telegram:{back:'bg-[#E6F5FE]',ink:'text-[#229ED9]',dot:'bg-[#229ED9]'},
 slack:{back:'bg-[#F4EAF5]',ink:'text-[#7A316E]',dot:'bg-[#7A316E]'},
 whatsapp:{back:'bg-[#E9F8EC]',ink:'text-[#168A57]',dot:'bg-[#168A57]'},
 signal:{back:'bg-[#E9F0FF]',ink:'text-[#2762DC]',dot:'bg-[#2762DC]'},
 line:{back:'bg-[#E9F8EC]',ink:'text-[#20A344]',dot:'bg-[#20A344]'},
 matrix:{back:'bg-[#E7F2F0]',ink:'text-[#087B69]',dot:'bg-[#087B69]'},
 mattermost:{back:'bg-[#E7F0F9]',ink:'text-[#265E92]',dot:'bg-[#265E92]'},
 zulip:{back:'bg-[#F0EEFF]',ink:'text-[#6951B8]',dot:'bg-[#6951B8]'}
}
const colorFor=(kind:string)=>identity[kind]||{back:'bg-[var(--color-canvas)]',ink:'text-[var(--color-muted)]',dot:'bg-[var(--color-muted)]'}
const stateLabels:Record<ChannelHealth,LocaleKey>={
 setup:'conn.health.setup',issue:'conn.health.issue',connected:'conn.status.connected',
 outbound:'conn.status.outbound_ready',paused:'conn.status.sync_disabled',
 disabled:'conn.status.disabled',starting:'conn.status.starting',stopped:'conn.status.not_started'
}
type Lens='all'|'attention'|'ready'
type Props={
 entries:ChannelEntry[];bindings:Channel[];rooms:{id:string;name:string}[];
 focusId?:string|null;loading:boolean;error:string;probes:Record<string,ChannelProbe>;
 busyAction:string|null;onEdit:(entry:ChannelEntry,existing?:Channel)=>void;
 onDiagnose:(channel:Channel)=>void;onDelete:(channel:Channel)=>void;
 onDeliveryReconciled:()=>Promise<void>
}
const needHelp=(connection:Channel)=>['setup','issue'].includes(channelHealth(connection))
const ready=(connection:Channel)=>['connected','outbound'].includes(channelHealth(connection))
function PlatformIcon({kind,size=20}:{kind:string;size?:number}){
 const Icon=icons[kind]||MessageCircle
 const color=colorFor(kind)
 return <span aria-hidden className={'grid size-11 shrink-0 place-items-center rounded-[14px] '+color.back+' '+color.ink}><Icon size={size} strokeWidth={2}/></span>
}
function State({connection}:{connection:Channel}){
 const status=channelHealth(connection)
 const color=status==='connected'||status==='outbound'?'text-emerald-700 bg-emerald-50 ring-emerald-200':
  status==='issue'||status==='setup'?'text-amber-800 bg-amber-50 ring-amber-200':
  status==='starting'?'text-blue-700 bg-blue-50 ring-blue-200':
  'text-[#706659] bg-[var(--color-canvas)] ring-[#E8E0D7]'
 return <span className={'inline-flex items-center gap-1.5 rounded-md px-2 py-1 text-[11px] font-semibold ring-1 ring-inset '+color}>
  <span className={'size-1.5 rounded-full '+(ready(connection)?'bg-emerald-500':needHelp(connection)?'bg-amber-500':'bg-muted/60')}/>
  {t(stateLabels[status])}
 </span>
}
function Pair({label,value}:{label:string;value:string}){
 return <div className="grid min-w-0 grid-cols-[105px_minmax(0,1fr)] gap-4 py-2.5 text-[12px]">
  <dt className="text-[var(--color-muted)]">{label}</dt>
  <dd className="min-w-0 break-all text-right font-medium text-[#3F3830]">{value}</dd>
 </div>
}
export function ChannelDirectory({entries,bindings,rooms,focusId,loading,error,probes,busyAction,onEdit,onDiagnose,onDelete,onDeliveryReconciled}:Props){
 useLocale()
 const [adding,setAdding]=useState(false)
 const [selectedId,setSelectedId]=useState<string|null>(null)
 const [mobileDetail,setMobileDetail]=useState(false)
 const [lens,setLens]=useState<Lens>('all')
 const [query,setQuery]=useState('')
 const [category,setCategory]=useState('all')
 useEffect(()=>{if(focusId){setSelectedId(focusId);setAdding(false);setMobileDetail(true);setLens('all');setQuery('')}},[focusId])
 const byKind=useMemo(()=>new Map(entries.map(entry=>[entry.id,entry])),[entries])
 const roomNames=useMemo(()=>new Map(rooms.map(room=>[room.id,room.name])),[rooms])
 const ordered=useMemo(()=>sortChannelBindings(bindings),[bindings])
 const filtered=useMemo(()=>ordered.filter(connection=>{
  if(lens==='attention'&&!needHelp(connection))return false
  if(lens==='ready'&&!ready(connection))return false
  const search=query.trim().toLocaleLowerCase()
  return !search||[connection.name,connection.kind,byKind.get(connection.kind)?.name,
   roomNames.get(connection.roomId||''),connection.channelId,connection.chatId]
   .some(value=>String(value||'').toLocaleLowerCase().includes(search))
 }),[ordered,lens,query,byKind,roomNames])
 const selected=filtered.find(connection=>connection.id===selectedId)||filtered[0]
 const selectedEntry=selected?byKind.get(selected.kind):undefined
 const healthy=bindings.filter(ready).length
 const attention=bindings.filter(needHelp).length
 const catalog=useMemo(()=>matchingChannels(entries,query,category),[entries,category,query])
 const categories=[
  {id:'all',label:t('conn.group.all')},
  {id:'messaging',label:t('conn.group.messaging')},
  {id:'workplace',label:t('conn.group.workplace')},
  {id:'community',label:t('conn.group.community')},
  {id:'decentralized',label:t('conn.group.decentralized')}
 ].filter(item=>item.id==='all'||entries.some(entry=>entry.category===item.id))
 const showAdd=()=>{setAdding(true);setQuery('');setCategory('all');setMobileDetail(false)}
 const back=()=>{setAdding(false);setMobileDetail(false);setQuery('')}
 const pick=(entry:ChannelEntry)=>{if(!entry.available)return;setAdding(false);setQuery('');onEdit(entry)}
 return <div className="min-w-0 space-y-5">
  {error&&<div role="alert" className="rounded-xl border border-rose-200 bg-rose-50 px-4 py-3 text-[13px] text-rose-800">{error}</div>}
  {adding?<section aria-label={t('conn.catalogTitle')} className="space-y-5">
   <div className="flex flex-wrap items-start justify-between gap-4">
    <div><p className="mb-2 text-[11px] font-bold uppercase tracking-[.16em] text-[#B66A50]">CONNECT / 01</p>
     <h2 className="text-[25px] font-bold tracking-tight text-[#29241F]">{t('conn.choosePlatform')}</h2>
     <p className="mt-1.5 text-[13px] text-[#84776B]">{t('conn.catalogDesc')}</p></div>
    <Button variant="secondary" onClick={back}><ArrowLeft size={15}/>{t('conn.backToConnections')}</Button>
   </div>
   <div className="flex items-center gap-2 rounded-xl border border-[#E3D9CD] bg-white px-3 py-1.5 shadow-[0_2px_8px_rgba(40,30,20,.025)]">
    <Search size={17} className="shrink-0 text-[var(--color-muted)]"/>
    <TextInput autoFocus aria-label={t('conn.search')} value={query} onChange={e=>setQuery(e.target.value)}
     className="!border-0 !bg-transparent !px-1 !py-2 !shadow-none !ring-0" placeholder={t('conn.search')}/>
    <span className="shrink-0 text-[11px] tabular-nums text-[var(--color-muted)]">{catalog.length} / {entries.length}</span>
   </div>
   <div className="grid gap-5 lg:grid-cols-[190px_minmax(0,1fr)]">
    <div className="flex gap-1 overflow-x-auto pb-1 lg:flex-col" aria-label={t('conn.filterCategory')}>
     {categories.map(item=><button key={item.id} type="button" aria-pressed={category===item.id}
      className={'flex shrink-0 items-center justify-between gap-3 rounded-lg px-3 py-2.5 text-left text-[12px] font-semibold transition-colors '+(category===item.id?'bg-[#F4E4D9] text-[#9A4B32]':'text-[#776C5F] hover:bg-[var(--color-canvas)]')}
      onClick={()=>setCategory(item.id)}><span>{item.label}</span><ChevronRight size={14} className="hidden lg:block"/></button>)}
    </div>
    <div className="grid content-start gap-3 sm:grid-cols-2">
     {catalog.map(entry=>{
      const count=bindings.filter(binding=>binding.kind===entry.id).length
      return <button type="button" key={entry.id} disabled={!entry.available} onClick={()=>pick(entry)}
       className="group flex min-h-[142px] min-w-0 flex-col rounded-2xl border border-[var(--color-divider)] bg-surface p-4 text-left shadow-[0_2px_8px_rgba(40,30,20,.025)] transition-all hover:-translate-y-0.5 hover:border-[var(--color-brand)] hover:shadow-[0_8px_26px_rgba(93,63,42,.10)] focus-visible:outline focus-visible:outline-2 focus-visible:outline-[#BD7656] disabled:opacity-50">
       <div className="flex items-center gap-3"><PlatformIcon kind={entry.id}/>
        <span className="min-w-0 flex-1">
         <strong className="block truncate text-[14px] text-[#302A24]">{entry.name}</strong>
         <span className="mt-0.5 block text-[11px] text-[var(--color-muted)]">{entry.mode==='outbound'?t('conn.outboundOnly'):t('conn.bidirectional')}{count>0?' · '+t('conn.boundCount',{count}):''}</span>
        </span><ArrowUpRight size={16} className="shrink-0 text-[var(--color-muted)] transition-transform group-hover:-translate-y-0.5 group-hover:translate-x-0.5"/>
       </div>
       <p className="mt-3 line-clamp-2 text-[12px] leading-[1.55] text-[var(--color-muted)]">{entry.description}</p>
       <div className="mt-auto flex items-center gap-1 pt-3 text-[11px] font-semibold text-[var(--color-brand)]">{t('conn.configurePlatform')}<ChevronRight size={13}/></div>
      </button>
     })}
     {!catalog.length&&<div className="col-span-full rounded-2xl border border-dashed border-[var(--color-divider)] bg-white px-6 py-12 text-center text-sm text-[var(--color-muted)]">{t('conn.noResults')}</div>}
    </div>
   </div>
  </section>:<>
   <div className={'flex flex-wrap items-center justify-between gap-3 '+(mobileDetail?'hidden lg:flex':'')}>
    <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
     <h2 className="text-[21px] font-bold tracking-tight text-[var(--color-ink)]">{t('conn.activeBindings')}</h2>
     <span className="rounded-full bg-[#EDE5DB] px-2.5 py-1 text-[11px] font-semibold tabular-nums text-[var(--color-muted)]">{bindings.length}</span>
     {bindings.length>0&&<div className="flex items-center gap-3 text-[11px] text-[var(--color-muted)]">
      <span className="inline-flex items-center gap-1.5"><span className="size-1.5 rounded-full bg-emerald-500"/>{t('conn.viewReady')} {healthy}</span>
      {attention>0&&<span className="inline-flex items-center gap-1.5 text-amber-800"><span className="size-1.5 rounded-full bg-amber-500"/>{t('conn.viewAttention')} {attention}</span>}
     </div>}
    </div>
    <Button variant="primary" onClick={showAdd}><Plus size={16}/>{t('conn.addChannel')}</Button>
   </div>
   {loading&&bindings.length===0
    ?<div className="flex min-h-56 items-center justify-center rounded-2xl border border-[var(--color-divider)] bg-white text-sm text-[var(--color-muted)]" role="status"><Activity size={16} className="mr-2 animate-spin"/>{t('conn.loading')}</div>
    :bindings.length===0
     ?<section className="overflow-hidden rounded-[22px] border border-[var(--color-divider)] bg-surface">
       <div className="grid gap-0 lg:grid-cols-[1.15fr_.85fr]">
        <div className="relative overflow-hidden px-6 py-10 sm:px-10 sm:py-14">
         <div aria-hidden className="pointer-events-none absolute -right-20 -top-28 size-72 rounded-full border-[35px] border-[var(--color-brand-soft)]"/>
         <h3 className="relative max-w-lg text-[27px] font-bold leading-tight tracking-[-.035em] text-[var(--color-ink)] sm:text-[32px]">{t('conn.emptyTitle')}</h3>
         <p className="relative mt-3 max-w-md text-[13px] leading-6 text-[var(--color-muted)]">{t('conn.emptyDescription')}</p>
         <div className="relative mt-7 flex flex-wrap gap-2">{entries.filter(item=>item.available).map(entry=>
          <button type="button" key={entry.id} onClick={()=>pick(entry)}
           className="inline-flex items-center gap-2 rounded-lg border border-[var(--color-divider)] bg-white px-3 py-2 text-[12px] font-semibold text-[var(--color-ink)] hover:border-[var(--color-brand)]">
           <span className={'size-2 rounded-full '+colorFor(entry.id).dot}/>{entry.name}<Plus size={12} className="text-[var(--color-muted)]"/></button>)}</div>
        </div>
        <div className="border-t border-[var(--color-divider)] bg-[var(--color-canvas)] px-7 py-9 lg:border-l lg:border-t-0 lg:px-8 lg:py-12">
         <p className="mb-6 text-[11px] font-bold tracking-[.15em] text-[var(--color-muted)]">{t('conn.onboardingTitle')}</p>
         {[
          {n:'01',title:t('conn.onboardingPlatform'),desc:t('conn.onboardingPlatformDesc')},
          {n:'02',title:t('conn.onboardingCredentials'),desc:t('conn.onboardingCredentialsDesc')},
          {n:'03',title:t('conn.onboardingVerify'),desc:t('conn.onboardingVerifyDesc')}
         ].map((step,index)=><div key={step.n} className="flex gap-4">
          <div className="flex flex-col items-center">
           <span className="grid size-9 shrink-0 place-items-center rounded-full border border-[var(--color-divider)] bg-white text-[11px] font-bold text-[var(--color-brand)]">{step.n}</span>
           {index!==2&&<span className="my-1 h-11 w-px bg-[var(--color-divider)]"/>}
          </div>
          <div className="pb-5"><strong className="text-[13px] text-[var(--color-ink)]">{step.title}</strong>
           <p className="mt-1 text-xs leading-5 text-[var(--color-muted)]">{step.desc}</p></div>
         </div>)}
        </div>
       </div>
      </section>
     :<div className="grid min-w-0 gap-4 lg:grid-cols-[minmax(276px,340px)_minmax(0,1fr)]">
       <section aria-label={t('conn.activeBindings')} className={(mobileDetail?'hidden lg:flex':'flex')+' min-w-0 flex-col overflow-hidden rounded-2xl border border-[var(--color-divider)] bg-white'}>
        <div className="border-b border-[var(--color-divider)] p-3">
         <label className="flex items-center gap-2 rounded-lg bg-[var(--color-canvas)] px-3 text-[var(--color-muted)]">
          <Search size={16} className="shrink-0"/><TextInput value={query} onChange={e=>setQuery(e.target.value)}
           aria-label={t('conn.searchConnected')} placeholder={t('conn.searchConnected')}
           className="!border-0 !bg-transparent !px-0 !py-2.5 !shadow-none !ring-0"/>
         </label>
         <div className="mt-3 flex items-center gap-1">
          {([
           {id:'all' as Lens,label:t('conn.viewAll'),count:bindings.length},
           {id:'attention' as Lens,label:t('conn.viewAttention'),count:attention},
           {id:'ready' as Lens,label:t('conn.viewReady'),count:healthy}
          ]).map(item=><button type="button" key={item.id} aria-pressed={lens===item.id} onClick={()=>setLens(item.id)}
           className={'flex min-w-0 items-center justify-center gap-1.5 rounded-lg px-2.5 py-2 text-[11px] font-semibold transition-colors '+(lens===item.id?'bg-[var(--color-brand-soft)] text-[var(--color-brand)]':'text-[var(--color-muted)] hover:bg-[var(--color-canvas)]')}>
           <span className="truncate">{item.label}</span><span className="tabular-nums opacity-65">{item.count}</span>
          </button>)}
         </div>
        </div>
        <div className="max-h-[65vh] overflow-y-auto py-1">
         {filtered.map(connection=>{
          const chosen=selected?.id===connection.id
          const place=roomNames.get(connection.roomId||'')
          const hasIssues=needHelp(connection)
          return <button type="button" key={connection.id} aria-pressed={chosen}
           onClick={()=>{setSelectedId(connection.id);setMobileDetail(true)}}
           className={'flex w-full items-start gap-3 border-l-[3px] px-3 py-3.5 text-left transition-colors focus-visible:outline focus-visible:outline-2 focus-visible:outline-[var(--color-brand)] '+(chosen?'border-brand bg-[var(--color-brand-soft)]':'border-transparent hover:bg-[var(--color-canvas)]')}>
           <PlatformIcon kind={connection.kind} size={18}/>
           <span className="min-w-0 flex-1">
            <span className="block truncate text-[13px] font-bold text-[#362E27]">{connection.name}</span>
            <span className="mt-0.5 block truncate text-[11px] text-[var(--color-muted)]">{byKind.get(connection.kind)?.name||connection.kind}{place?' · '+place:''}</span>
            <span className={'mt-2 inline-flex items-center gap-1.5 text-[11px] font-medium '+(hasIssues?'text-amber-800':ready(connection)?'text-emerald-700':'text-[var(--color-muted)]')}>
             <span className={'size-1.5 rounded-full '+(hasIssues?'bg-amber-500':ready(connection)?'bg-emerald-500':'bg-muted/60')}/>{t(stateLabels[channelHealth(connection)])}
             {(connection.delivery?.needsReview||0)>0&&<span className="ml-1 font-semibold">{connection.delivery?.needsReview}</span>}
            </span>
           </span>
           <ChevronRight size={16} className="mt-2 shrink-0 text-[#BAAEA0]"/>
          </button>
         })}
         {!filtered.length&&<p className="px-5 py-10 text-center text-xs leading-6 text-[#897B6B]">{t('conn.noMatchingConnections')}</p>}
        </div>
       </section>
       <section aria-label={t('conn.connectionDetails')} className={(mobileDetail?'block':'hidden lg:block')+' min-w-0 overflow-hidden rounded-2xl border border-[var(--color-divider)] bg-surface shadow-[0_3px_18px_rgba(45,32,20,.025)]'}>
        {!selected?<p className="p-10 text-center text-sm text-[#887E72]">{t('conn.selectConnection')}</p>
        :<div>
         <header className="border-b border-[var(--color-divider)] bg-white px-4 py-5 sm:px-6">
          <Button variant="ghost" size="sm" className="mb-3 lg:hidden" onClick={()=>setMobileDetail(false)}><ArrowLeft size={15}/>{t('conn.backToConnections')}</Button>
          <div className="flex flex-wrap items-start justify-between gap-3">
           <div className="flex min-w-0 items-start gap-3">
            <PlatformIcon kind={selected.kind} size={23}/>
            <div className="min-w-0">
             <p className="text-[10px] font-semibold uppercase tracking-[.11em] text-[#B08A73]">{selectedEntry?.name||selected.kind}</p>
             <h3 className="mt-0.5 break-words text-xl font-bold tracking-tight text-[#302920]">{selected.name}</h3>
             <div className="mt-2"><State connection={selected}/></div>
            </div>
           </div>
           <Button variant="primary" size="sm" disabled={!selectedEntry||busyAction!==null} onClick={()=>selectedEntry&&onEdit(selectedEntry,selected)}>
            <Settings2 size={15}/>{t('conn.dlg.settings')}
           </Button>
          </div>
         </header>
         <div className="space-y-6 px-4 py-5 sm:px-6">
          {(needHelp(selected)||selected.runtimeError||selected.delivery?.lastError)&&<div role="status"
           className="flex items-start gap-3 rounded-xl border border-[#EDCDAA] bg-[var(--color-canvas)] p-3.5">
           <AlertCircle size={18} className="mt-0.5 shrink-0 text-[#B76E2D]"/>
           <div className="min-w-0">
            <h4 className="text-[12px] font-bold text-[#8A521D]">{t('conn.diagnosisTitle')}</h4>
            <p className="mt-1 break-words text-[12px] leading-5 text-[#815A35]">
             {selected.runtimeError||selected.delivery?.lastError||(selected.missing?.length?t('conn.missingFields',{fields:selected.missing.join(', ')}):t('conn.setupNeededHint'))}
            </p>
           </div>
          </div>}
          <div>
           <div className="mb-2 flex items-center justify-between gap-3">
            <h4 className="text-[12px] font-bold text-[var(--color-ink)]">{t('conn.connectionDetails')}</h4>
            <span className="text-[10px] tracking-wide text-[#A39A8E]">CONFIGURATION</span>
           </div>
           <dl className="divide-y divide-[#F1EAE1] rounded-xl border border-[#EEE7DD] bg-white px-4">
            <Pair label={t('conn.platforms')} value={selectedEntry?.name||selected.kind}/>
            <Pair label={t('conn.room')} value={selected.roomId?roomNames.get(selected.roomId)||t('conn.roomDeleted'):t('conn.singleAgent')}/>
            {(selected.channelId||selected.chatId)&&<Pair label={t('conn.target')} value={selected.channelId||selected.chatId||''}/>}
            <Pair label={t('conn.enable')} value={selected.enabled?t('conn.enabled'):t('conn.status.disabled')}/>
           </dl>
          </div>
          <div>
           <div className="mb-3 flex items-center justify-between gap-2">
            <h4 className="text-[12px] font-bold text-[var(--color-ink)]">{t('conn.deliveryTitle')}</h4>
            <span className="inline-flex items-center gap-1 text-[11px] text-[#958A7E]"><Clock3 size={13}/>{t('conn.deliveryRecorded')}</span>
           </div>
           <div className="grid grid-cols-3 gap-2">
            {[
             {label:t('conn.deliveryPending'),n:(selected.delivery?.pending||0)+(selected.delivery?.sending||0),color:'text-[#3E3730]'},
             {label:t('conn.deliverySent'),n:selected.delivery?.sent||0,color:'text-emerald-700'},
             {label:t('conn.deliveryReview'),n:selected.delivery?.needsReview||0,color:'text-amber-800'}
            ].map(entry=><div key={entry.label} className="rounded-xl border border-[#EBE3D9] bg-[var(--color-canvas)] px-3 py-3">
             <span className="block text-[10px] leading-4 text-[#8D8073]">{entry.label}</span>
             <strong className={'mt-1 block text-[23px] font-bold tabular-nums tracking-tight '+entry.color}>{entry.n}</strong>
            </div>)}
           </div>
           <p className="mt-2 text-[11px] leading-5 text-[#96897B]">{t('conn.outboundNoteCompact')}</p>
          </div>
          {(selected.delivery?.needsReview||0)>0&&<ChannelDeliveryReview channelId={selected.id} onChanged={onDeliveryReconciled}/>}
          {probes[selected.id]&&<div role="status" aria-live="polite"
           className={'rounded-xl border px-4 py-3 text-[12px] leading-5 '+(probes[selected.id].status==='verified'?'border-emerald-200 bg-emerald-50 text-emerald-900':probes[selected.id].status==='unverified'?'border-amber-200 bg-amber-50 text-amber-900':'border-[#D8E5E6] bg-[#F3FAFA] text-[#42656B]')}>
           <div className="flex items-center gap-2 font-bold">{probes[selected.id].status==='verified'?<CheckCheck size={17}/>:<Wifi size={16}/>}
            {probes[selected.id].status==='verified'?t('conn.probeVerified'):probes[selected.id].status==='checking'?t('conn.dlg.diagnosing'):t('conn.probeUnverified')}
           </div>
           <p className="mt-1 break-words">{probes[selected.id].message}</p>
           {probes[selected.id].checkedAt&&<p className="mt-1 text-[11px] opacity-70">{t('conn.probeCheckedAt',{time:new Date(probes[selected.id].checkedAt||'').toLocaleString()})}</p>}
           <p className="mt-1 text-[11px] opacity-80">{t('conn.probeIsNotDelivery')}</p>
          </div>}
         </div>
         <footer className="flex flex-wrap items-center gap-2 border-t border-[#EEE7DC] bg-white px-4 py-4 sm:px-6">
          <Button variant={probes[selected.id]?.status==='unverified'?'primary':'secondary'} size="sm" disabled={busyAction!==null} onClick={()=>onDiagnose(selected)}>
           <Activity size={15} className={busyAction==='probe:'+selected.id?'animate-spin':''}/>
           {busyAction==='probe:'+selected.id?t('conn.dlg.diagnosing'):t('conn.dlg.diagnose')}
          </Button>
          {selectedEntry?.docs&&<a href={selectedEntry.docs} target="_blank" rel="noopener noreferrer"
           className="inline-flex min-h-9 items-center gap-1 rounded-lg px-3 text-xs font-semibold text-[#896650] hover:bg-[var(--color-canvas)]">
           {t('conn.docs')} <ExternalLink size={13}/></a>}
          <Button variant="ghost" size="sm" className="sm:ml-auto !text-rose-600 hover:!bg-rose-50"
           disabled={busyAction!==null} onClick={()=>onDelete(selected)}><Trash2 size={14}/>{t('conn.dlg.delete')}</Button>
         </footer>
        </div>}
       </section>
      </div>}
  </>}
 </div>
}
