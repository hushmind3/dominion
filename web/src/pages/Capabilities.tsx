import {useMemo,useState} from 'react'
import {ArrowRight,ExternalLink,Layers3,MessageCircle,RefreshCw,Search,TerminalSquare} from 'lucide-react'
import {Button,TextInput} from '../components/Primitives'
import {DialogBackdrop,DialogPanel} from '../components/Dialog'
import {Headline,Notice,PAGE_FRAME,Panel,StateTag,useRemote} from '../components/UI'
import {t,useLocale} from '../i18n'

type Capability={id:string;name:string;description?:string;namespace:string;available?:boolean}
type Channel=Omit<Capability,'namespace'>&{mode:string;features?:string[];docs?:string;requiresRoom?:boolean;maxRoomParticipants?:number}
type Item=Capability&Partial<Channel>
type Page='connections'|'extensions'
const families=[
 {id:'all',label:t('cap.all'),icon:Layers3},
 {id:'channel',label:t('cap.messenger'),icon:MessageCircle},
 {id:'builtin',label:t('cap.builtin'),icon:TerminalSquare},
] as const

export function CapabilitiesPage({navigate,embedded=false}:{navigate:(page:Page)=>void;embedded?:boolean}){
 useLocale()
 const data=useRemote<{items:Capability[]}>('/api/capabilities',{items:[]})
 const catalog=useRemote<Channel[]>('/api/channels/catalog',[])
 const [family,setFamily]=useState<(typeof families)[number]['id']>('all')
 const [query,setQuery]=useState(''),[showPlanned,setShowPlanned]=useState(false)
 const [detail,setDetail]=useState<Item>(),[refreshing,setRefreshing]=useState(false)
 const items=useMemo<Item[]>(()=>[
  ...data.value.items.filter(item=>item.namespace==='builtin'),
  ...catalog.value.filter(item=>item.available||showPlanned).map(item=>({...item,namespace:'channel'})),
 ],[data.value.items,catalog.value,showPlanned])
 const needle=query.trim().toLowerCase()
 const visible=items.filter(item=>(family==='all'||item.namespace===family)&&
  [item.name,item.id,item.description].join(' ').toLowerCase().includes(needle))
 const reload=async()=>{
  setRefreshing(true)
  try{await Promise.all([data.reload(),catalog.reload()])}finally{setRefreshing(false)}
 }
 return <div className={embedded?'min-w-0':PAGE_FRAME}>
  <Headline eyebrow={t('ui.eyebrow.capabilities')} title={t('cap.title')} description={t('cap.description')} actions={<Button variant="secondary" disabled={refreshing} onClick={()=>void reload()}><RefreshCw size={16}/> {t('cap.refresh')}</Button>}/>
  <Notice message={data.error||catalog.error}/>
  <div className="grid gap-6 lg:grid-cols-[200px_minmax(0,1fr)]">
   <aside className="min-w-0">
    <div className="mb-3 flex items-center gap-2"><Search size={16} className="shrink-0 text-muted"/><TextInput aria-label={t('cap.searchAria')} value={query} onChange={event=>setQuery(event.target.value)} placeholder={t('cap.searchPlaceholder')}/></div>
    <nav className="flex flex-wrap gap-1 lg:flex-col">
     {families.map(({id,label,icon:Icon})=><Button key={id} variant={id===family?'primary':'ghost'} aria-pressed={id===family} onClick={()=>setFamily(id)} className="lg:w-full lg:justify-start"><Icon size={16}/>{label} {items.filter(item=>id==='all'||item.namespace===id).length}</Button>)}
    </nav>
    {family!=='builtin'&&<label className="mt-3 flex items-center gap-2 text-[12px] text-muted"><TextInput type="checkbox" checked={showPlanned} onChange={event=>setShowPlanned(event.target.checked)}/> {t('cap.includePlanned')}</label>}
   </aside>
   <div className="min-w-0">
    <div className="grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3">{visible.map(item=><Button key={item.namespace+':'+item.id} onClick={()=>setDetail(item)} className="!flex-col !items-start !justify-start !p-4 text-left">
     <span className="flex w-full items-center justify-between gap-2"><strong className="break-all text-sm">{item.name}</strong><StateTag online={item.available}>{item.namespace==='builtin'?t('cap.builtin'):!item.available?t('cap.unsupported'):item.mode==='outbound'?t('cap.outbound'):t('cap.supported')}</StateTag></span>
     <span className="text-sm font-normal text-muted">{item.description}</span>
    </Button>)}</div>
    {!visible.length&&<Panel className="mt-4 p-5 text-sm text-muted">{data.loading||catalog.loading?t('cap.loading'):t('cap.empty')}</Panel>}
   </div>
  </div>
  {detail&&<DialogBackdrop onDismiss={()=>setDetail(undefined)}><DialogPanel aria-label={detail.name} className="p-5">
   <h2>{detail.name}</h2><p className="mt-2">{detail.description}</p>
   {detail.maxRoomParticipants&&<p className="mt-2">{t('cap.maxRoom',{count:detail.maxRoomParticipants})}</p>}
   {detail.namespace==='builtin'&&<p className="mt-2 text-sm">{t('cap.builtinNote')}</p>}
   <div className="mt-4 flex flex-wrap justify-end gap-2">
    {detail.docs&&<a className="inline-flex items-center gap-1 p-2 text-sm text-brand-hover" href={detail.docs} target="_blank" rel="noreferrer">{t('cap.docs')} <ExternalLink size={14}/></a>}
    {detail.namespace==='channel'&&detail.available&&<Button onClick={()=>navigate('connections')}>{t('cap.openConnections')} <ArrowRight size={15}/></Button>}
    <Button variant="ghost" onClick={()=>setDetail(undefined)}>{t('cap.close')}</Button>
   </div>
  </DialogPanel></DialogBackdrop>}
 </div>
}
