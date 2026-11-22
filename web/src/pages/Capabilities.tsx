import {useMemo,useState} from 'react'
import {ArrowRight,ExternalLink,Layers3,MessageCircle,RefreshCw,Search,TerminalSquare} from 'lucide-react'
import {Button,TextInput} from '../components/Primitives'
import {DialogBackdrop,DialogPanel} from '../components/Dialog'
import {Headline,Notice,PAGE_FRAME,Panel,StateTag,useRemote} from '../components/UI'
import {t,useLocale} from '../i18n'

/** Built-in tool descriptions come from the engine in English, written for
 *  the model. The owner reads a short Korean label instead. */
const koToolCopy:Record<string,{name:string;desc:string}>={
 'account_status':{name:'연결한 계정 보기',desc:'DOMINION에 연결해 둔 GitHub·Google 계정을 확인합니다.'},
 'github_file_read':{name:'GitHub 파일 읽기',desc:'저장소의 파일 내용을 읽습니다.'},
 'github_file_commit':{name:'GitHub 파일 저장',desc:'선택한 브랜치의 파일을 안전하게 고쳐 저장합니다.'},
 'github_api':{name:'GitHub 작업 실행',desc:'GitHub에서 필요한 작업을 대신 실행합니다. 쓰기는 확인 후 진행합니다.'},
 'github_work_read':{name:'GitHub 저장소 살펴보기',desc:'저장소, 브랜치, 이슈, PR, 실행 기록을 봅니다.'},
 'github_work_write':{name:'GitHub 브랜치·PR·이슈 만들기',desc:'브랜치, PR, 이슈를 만들고 결과를 확인합니다.'},
 'google_workspace_read':{name:'Google 메일·드라이브 읽기',desc:'Gmail, Drive, Docs, Calendar에서 필요한 내용을 찾습니다.'},
 'google_workspace_write':{name:'Google 메일·드라이브 쓰기',desc:'메일 발송, 문서 작성, 일정 추가를 확인 후 진행합니다.'},
}
const capName=(item:Capability,en:boolean)=>en?item.name:(koToolCopy[item.id]?.name||item.name)
const capDesc=(item:Capability,en:boolean)=>en?item.description:(koToolCopy[item.id]?.desc||item.description)

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
 const locale=useLocale()
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
     <span className="flex w-full items-center justify-between gap-2"><strong className="break-all text-sm">{capName(item,locale==='en')}</strong><StateTag online={item.available}>{item.namespace==='builtin'?t('cap.builtin'):!item.available?t('cap.unsupported'):item.mode==='outbound'?t('cap.outbound'):t('cap.supported')}</StateTag></span>
     <span className="text-sm font-normal text-muted">{capDesc(item,locale==='en')}</span>
    </Button>)}</div>
    {!visible.length&&<Panel className="mt-4 p-5 text-sm text-muted">{data.loading||catalog.loading?t('cap.loading'):t('cap.empty')}</Panel>}
   </div>
  </div>
  {detail&&<DialogBackdrop onDismiss={()=>setDetail(undefined)}><DialogPanel aria-label={detail.name} className="p-5">
   <h2>{capName(detail,locale==='en')}</h2><p className="mt-2">{capDesc(detail,locale==='en')}</p>
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
