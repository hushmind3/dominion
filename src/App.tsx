import {Button,TextInput} from './components/Primitives'
import {useCallback,useEffect,useMemo,useState} from 'react'
import {Activity,Blocks,BookOpenText,BrainCircuit,ChevronsUpDown,Compass,House,Layers3,Menu,MessageSquareMore,RadioTower,Search,Settings2,ShieldCheck,UsersRound,Workflow,X} from 'lucide-react'
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
const groups:{title:string;items:{id:Page;label:string;icon:any}[]}[]=[
 {title:'WORKSPACE',items:[{id:'home',label:'홈',icon:House},{id:'chat',label:'대화',icon:MessageSquareMore},{id:'models',label:'모델 허브',icon:Compass},{id:'agents',label:'에이전트',icon:BrainCircuit},{id:'rooms',label:'그룹 · 회의실',icon:UsersRound},{id:'workflows',label:'워크플로',icon:Workflow},{id:'memory',label:'기억 라이브러리',icon:BookOpenText}]},
 {title:'CAPABILITIES',items:[{id:'capabilities',label:'확장 허브',icon:Layers3},{id:'extensions',label:'MCP · 스킬',icon:Blocks},{id:'connections',label:'연결 · 채널',icon:RadioTower},{id:'automations',label:'자동화',icon:Workflow}]},
 {title:'SYSTEM',items:[{id:'activity',label:'실행 기록',icon:Activity},{id:'settings',label:'설정',icon:Settings2}]}
]
const labels=Object.fromEntries(groups.flatMap(g=>g.items.map(x=>[x.id,x.label])))
export default function App(){
 const [page,setPage]=useState<Page>('home'),[session,setSession]=useState<string|undefined>(),[sidebar,setSidebar]=useState(false),[palette,setPalette]=useState(false),[search,setSearch]=useState(''),[online,setOnline]=useState(false),[running,setRunning]=useState(0),[pending,setPending]=useState(0);
 const navigate=useCallback((p:Page,s?:string)=>{setPage(p);if(s!==undefined)setSession(s);setSidebar(false);setPalette(false)},[]);
 useEffect(()=>{
  let alive=true;
  const check=()=>get<any>('/api/status').then(x=>{if(alive){setOnline(true);setRunning(x.jobs)}}).catch(()=>{if(alive)setOnline(false)});
  void check();const poll=setInterval(check,12000);
  const evt=new EventSource('/api/events');
  evt.onmessage=e=>{try{const x=JSON.parse(e.data);if(['record','message','job_event','audit','approval','channel_status','goal_event'].includes(x.type)){window.dispatchEvent(new Event('dominion-refresh'));if(x.type==='approval')get<any[]>('/api/approvals').then(a=>setPending(a.length)).catch(()=>{});}}catch{}};
  const shortcuts=(e:KeyboardEvent)=>{if((e.metaKey||e.ctrlKey)&&e.key.toLowerCase()==='k'){e.preventDefault();setPalette(x=>!x)}if(e.key==='Escape')setPalette(false);};
  window.addEventListener('keydown',shortcuts);
  return()=>{alive=false;clearInterval(poll);evt.close();window.removeEventListener('keydown',shortcuts)};
 },[]);
 const filtered=useMemo(()=>groups.flatMap(g=>g.items).filter(x=>x.label.toLowerCase().includes(search.toLowerCase())),[search]);
 return <div className="app-shell">
  <aside className={'sidebar '+(sidebar?'mobile-open':'')}>
   <div className="brand"><div className="brand-mark"><span>D</span></div><div className="brand-type"><strong>DOMINION</strong><small>INTELLIGENCE OS</small></div><Button className="mobile-close" onClick={()=>setSidebar(false)}><X size={18}/></Button></div>
   <div className="workspace-switch"><div className="workspace-icon"><Layers3 size={17}/></div><div><strong>My Workspace</strong><small>Personal · Local-first</small></div><ChevronsUpDown size={15}/></div>
   <nav className="main-nav">{groups.map(g=><div className="nav-group" key={g.title}><div className="nav-label">{g.title}</div>{g.items.map(item=>{const Icon=item.icon;return <Button key={item.id} className={'nav-item '+(page===item.id?'active':'')} onClick={()=>navigate(item.id)}><Icon size={19} strokeWidth={1.9}/><span>{item.label}</span>{item.id==='activity'&&running>0&&<span className="count-dot">{running}</span>}{item.id==='settings'&&pending>0&&<span className="count-dot">{pending}</span>}{page===item.id&&<span className="nav-selection"/>}</Button>})}</div>)}</nav>
   <div className="sidebar-bottom"><div className="local-node"><div className="pulse-icon"><span/></div><div><strong>{online?'Gateway online':'Gateway offline'}</strong><small>{online?'Local engine connected':'Check Node.js server'}</small></div><span className={'status-light '+(online?'on':'')}/></div><div className="sidebar-footer"><ShieldCheck size={15}/> Built to work for you <span>v0.1</span></div></div>
  </aside>
  {sidebar&&<div className="mobile-shade" onClick={()=>setSidebar(false)}/>}
  <main className="main-area">
   <header className="topbar"><div className="topbar-left"><Button className="icon-button mobile-menu" onClick={()=>setSidebar(true)}><Menu size={21}/></Button><span className="topbar-location">Workspace</span><span className="topbar-divider">/</span><strong>{labels[page]}</strong></div><div className="topbar-right"><Button className="command-trigger" onClick={()=>{setPalette(true);setSearch('')}}><Search size={16}/> 빠른 이동 <kbd>⌘ K</kbd></Button><span className="topbar-sep"/><div className={'topbar-status '+(online?'healthy':'unhealthy')}><span/>{online?'시스템 연결됨':'오프라인'}</div><Button className="avatar" title="Personal workspace">D</Button></div></header>
   <div className={'page-content page-'+page}>
    {page==='capabilities'&&<CapabilitiesPage navigate={navigate}/>}
    {page==='home'&&<HomePage navigate={navigate}/>}
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
  {palette&&<div className="command-backdrop" onMouseDown={()=>setPalette(false)}><div className="command-modal" onMouseDown={e=>e.stopPropagation()}><div className="command-search"><Search size={19}/><TextInput autoFocus placeholder="무엇을 열까요?" value={search} onChange={e=>setSearch(e.target.value)}/><kbd>ESC</kbd></div><div className="command-results">{filtered.map(item=>{const Icon=item.icon;return <Button key={item.id} onClick={()=>navigate(item.id)}><Icon size={17}/><span>{item.label}</span><span>↵</span></Button>})}{!filtered.length&&<p>검색 결과가 없습니다.</p>}</div><div className="command-hint">DOMINION · 모든 기능을 한곳에서</div></div></div>}
 </div>
}
