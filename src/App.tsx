import {useCallback,useEffect,useMemo,useState} from 'react'
import {Activity,Blocks,BookOpenText,BrainCircuit,ChevronDown,Command,Compass,House,Layers3,Menu,MessageSquareMore,PanelLeftClose,PanelLeftOpen,Plus,RadioTower,Search,Settings2,ShieldCheck,UsersRound,Workflow,X} from 'lucide-react'
import {get} from './api'
import {HomePage} from './pages/Home'
import {CapabilitiesPage} from './pages/Capabilities'
import {MemoryPage} from './pages/Memory'
import {WorkflowsPage} from './pages/Workflows'
import {ChatPage} from './pages/Chat'
import {ModelHub} from './pages/Models'
import {AgentsPage,RoomsPage} from './pages/Agents'
import {ExtensionsPage} from './pages/Extensions'
import {ConnectionsPage} from './pages/Connections'
import {OperationsPage} from './pages/Operations'

type Page='capabilities'|'home'|'chat'|'models'|'agents'|'rooms'|'workflows'|'memory'|'extensions'|'connections'|'automations'|'activity'|'settings'
type Item={id:Page;label:string;icon:typeof House;hint?:string}
const groups:{title:string;items:Item[]}[]=[
 {title:'WORKSPACE',items:[{id:'home',label:'개요',icon:House},{id:'chat',label:'대화',icon:MessageSquareMore},{id:'models',label:'모델 라이브러리',icon:Compass},{id:'agents',label:'에이전트',icon:BrainCircuit},{id:'rooms',label:'그룹 · 회의',icon:UsersRound},{id:'workflows',label:'워크플로',icon:Workflow},{id:'memory',label:'기억',icon:BookOpenText}]},
 {title:'EXTENSIONS',items:[{id:'capabilities',label:'기능 디렉터리',icon:Layers3},{id:'extensions',label:'MCP · 스킬',icon:Blocks},{id:'connections',label:'연결 · 채널',icon:RadioTower},{id:'automations',label:'자동화',icon:Workflow}]},
 {title:'SYSTEM',items:[{id:'activity',label:'실행 기록',icon:Activity},{id:'settings',label:'설정',icon:Settings2}]}
]
const labels:Record<Page,string>=Object.fromEntries(groups.flatMap(g=>g.items.map(x=>[x.id,x.label]))) as Record<Page,string>

export default function App(){
 const [page,setPage]=useState<Page>('home')
 const [session,setSession]=useState<string|undefined>()
 const [collapsed,setCollapsed]=useState(()=>{try{return localStorage.getItem('dominion.sidebar.collapsed')==='1'}catch{return false}})
 const [mobileOpen,setMobileOpen]=useState(false)
 const [palette,setPalette]=useState(false)
 const [search,setSearch]=useState('')
 const [online,setOnline]=useState(false)
 const [running,setRunning]=useState(0)
 const [pending,setPending]=useState(0)
 const navigate=useCallback((p:Page,s?:string)=>{
  setPage(p);if(s!==undefined)setSession(s);setMobileOpen(false);setPalette(false)
 },[])
 const toggleSidebar=useCallback(()=>{
  if(window.matchMedia('(max-width: 860px)').matches){setMobileOpen(x=>!x)}
  else setCollapsed(x=>{try{localStorage.setItem('dominion.sidebar.collapsed',x?'0':'1')}catch{}return !x})
 },[])
 useEffect(()=>{
  let live=true
  const refreshStatus=()=>get<{jobs:number}>('/api/status').then(s=>{if(live){setOnline(true);setRunning(s.jobs||0)}}).catch(()=>{if(live)setOnline(false)})
  void refreshStatus()
  const interval=setInterval(refreshStatus,12000)
  const events=new EventSource('/api/events')
  events.onmessage=e=>{try{
   const msg=JSON.parse(e.data)
   if(['record','message','job_event','audit','approval','channel_status','room_event'].includes(msg.type)){
    window.dispatchEvent(new Event('dominion-refresh'))
    if(msg.type==='approval')void get<unknown[]>('/api/approvals').then(a=>{if(live)setPending(a.length)}).catch(()=>{})
   }
  }catch{}}
  const onKey=(e:KeyboardEvent)=>{
   if((e.metaKey||e.ctrlKey)&&e.key.toLowerCase()==='k'){e.preventDefault();setPalette(v=>!v);setSearch('')}
   if((e.metaKey||e.ctrlKey)&&e.key.toLowerCase()==='b'){e.preventDefault();toggleSidebar()}
   if(e.key==='Escape'){setPalette(false);setMobileOpen(false)}
  }
  window.addEventListener('keydown',onKey)
  return()=>{live=false;clearInterval(interval);events.close();window.removeEventListener('keydown',onKey)}
 },[toggleSidebar])
 const filtered=useMemo(()=>groups.flatMap(x=>x.items).filter(x=>x.label.toLowerCase().includes(search.trim().toLowerCase())),[search])
 const activeIcon=groups.flatMap(x=>x.items).find(x=>x.id===page)?.icon||House
 const ActiveIcon=activeIcon
 return <div className={'app-shell ds-shell '+(collapsed?'ds-collapsed':'')}>
  <aside className={'sidebar ds-sidebar '+(mobileOpen?'mobile-open':'')} aria-label="주 메뉴">
   <div className="ds-brand-row">
    <button className="ds-brand-emblem" title="개요" onClick={()=>navigate('home')} aria-label="DOMINION 홈"><Layers3 size={21}/></button>
    <button className="ds-brand-wordmark" onClick={()=>navigate('home')} tabIndex={collapsed?-1:0}><strong>DOMINION</strong><span>LOCAL INTELLIGENCE</span></button>
    <button className="ds-sidebar-close" onClick={()=>setMobileOpen(false)} aria-label="메뉴 닫기"><X size={19}/></button>
   </div>
   <div className="ds-workspace-row"><span className="ds-workspace-icon"><span>D</span></span><div><strong>Personal workspace</strong><small>로컬 실행 환경</small></div><ChevronDown size={15}/></div>
   <div className="ds-nav-scroller"><nav className="ds-nav">
    {groups.map(g=><section className="ds-nav-section" key={g.title} aria-label={g.title}>
     <div className="ds-nav-label">{g.title}</div>
     {g.items.map(item=>{const Icon=item.icon;return <button type="button" key={item.id} title={item.label} aria-label={item.label} aria-current={page===item.id?'page':undefined} className={'ds-nav-link '+(page===item.id?'is-active':'')} onClick={()=>navigate(item.id)}>
      <Icon size={19} strokeWidth={1.8}/><span className="ds-nav-text">{item.label}</span>
      {item.id==='activity'&&running>0&&<span className="ds-count">{running}</span>}
      {item.id==='settings'&&pending>0&&<span className="ds-count">{pending}</span>}
     </button>})}
    </section>)}
   </nav></div>
   <div className="ds-sidebar-footer">
    <div className="ds-connection-state"><i className={online?'connected':'disconnected'}/><div><strong>{online?'서버 연결됨':'서버 연결 안 됨'}</strong><small>{online?'DOMINION Gateway':'Node 서버 상태 확인'}</small></div></div>
    <span className="ds-sidebar-version"><ShieldCheck size={13}/> LOCAL-FIRST · v0.1</span>
   </div>
  </aside>
  {mobileOpen&&<button type="button" aria-label="메뉴 닫기" className="ds-mobile-backdrop" onClick={()=>setMobileOpen(false)}/>}
  <main className="main-area ds-main" id="main-content">
   <header className="ds-topbar">
    <div className="ds-topbar-left">
     <button className="ds-toggle" type="button" onClick={toggleSidebar} aria-label={mobileOpen?'메뉴 닫기':collapsed?'사이드바 펼치기':'사이드바 접기'} aria-expanded={mobileOpen||!collapsed} title="사이드바 접기 / 펼치기 (⌘B)">
      {mobileOpen?<X size={20}/>:collapsed?<PanelLeftOpen size={20}/>:<Menu size={21}/>}
     </button>
     <span className="ds-topbar-separator"/>
     <span className="ds-location"><span>Workspace</span><span className="ds-chevron">/</span><ActiveIcon size={16}/><strong>{labels[page]}</strong></span>
    </div>
    <div className="ds-topbar-right">
     <button type="button" className="ds-command-button" onClick={()=>{setSearch('');setPalette(true)}} aria-label="페이지 검색" title="페이지 검색 (⌘K)"><Search size={16}/><span>검색 또는 빠른 이동</span><kbd>⌘ K</kbd></button>
     <span className={'ds-health '+(online?'is-online':'')} title={online?'API 서버 정상 응답':'API 서버 연결 실패'}><i/>{online?'ONLINE':'OFFLINE'}</span>
     <button type="button" className="ds-new-chat" onClick={()=>{setSession(undefined);navigate('chat')}}><Plus size={17}/><span>새 대화</span></button>
    </div>
   </header>
   <div className={'page-content ds-content page-'+page}>
    {page==='home'&&<HomePage navigate={navigate}/>}
    {page==='capabilities'&&<CapabilitiesPage navigate={navigate}/>}
    {page==='memory'&&<MemoryPage/>}
    {page==='workflows'&&<WorkflowsPage openSession={id=>navigate('chat',id)}/>}
    {page==='chat'&&<ChatPage sessionId={session} onSession={setSession} openModels={()=>navigate('models')}/>}
    {page==='models'&&<ModelHub/>}
    {page==='agents'&&<AgentsPage/>}
    {page==='rooms'&&<RoomsPage openSession={id=>navigate('chat',id)}/>}
    {page==='extensions'&&<ExtensionsPage/>}
    {page==='connections'&&<ConnectionsPage/>}
    {(page==='automations'||page==='activity'||page==='settings')&&<OperationsPage section={page}/>}
   </div>
  </main>
  {palette&&<div className="ds-palette-overlay" onMouseDown={()=>setPalette(false)} role="presentation">
   <div className="ds-palette" role="dialog" aria-modal="true" aria-label="빠른 이동" onMouseDown={e=>e.stopPropagation()}>
    <div className="ds-palette-search"><Search size={20}/><input autoFocus value={search} onChange={e=>setSearch(e.target.value)} placeholder="이동할 화면을 검색하세요"/><button onClick={()=>setPalette(false)} aria-label="검색 닫기"><X size={16}/></button></div>
    <span className="ds-palette-caption">PAGES · {filtered.length} RESULTS</span>
    <div className="ds-palette-results">{filtered.map(item=>{const Icon=item.icon;return <button key={item.id} onClick={()=>navigate(item.id)}><Icon size={17}/><span>{item.label}</span><span>↵</span></button>})}
    {!filtered.length&&<p>일치하는 화면이 없습니다.</p>}</div>
    <div className="ds-palette-footer"><Command size={13}/> K 열기 · Esc 닫기</div>
   </div>
  </div>}
 </div>
}