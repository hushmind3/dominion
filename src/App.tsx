import {SidebarNavItem} from './components/SidebarNavigation'
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
 return <div className="min-h-screen bg-slate-50 text-slate-800">
  <aside className={'fixed inset-y-0 left-0 z-[22] flex w-[249px] flex-col border-r border-slate-200 bg-white shadow-sm max-[850px]:transition-transform '+(sidebar?'max-[850px]:translate-x-0':'max-[850px]:-translate-x-full')}>
   <div className="flex items-center gap-3 px-5 pb-5 pt-7"><div className="grid size-10 shrink-0 place-items-center rounded-xl bg-gradient-to-br from-blue-700 to-blue-400 text-white shadow-md"><span>D</span></div><div className="flex min-w-0 flex-1 flex-col gap-0.5"><strong className="text-sm font-extrabold tracking-wide text-slate-900">DOMINION</strong><small className="text-[9px] font-semibold tracking-widest text-slate-500">INTELLIGENCE OS</small></div><Button variant="ghost" size="icon" className="ml-auto !hidden max-[850px]:!inline-flex" onClick={()=>setSidebar(false)}><X size={18}/></Button></div>
   <div className="mx-3 mb-5 flex items-center gap-3 rounded-xl border border-slate-100 bg-slate-50 p-3"><div className="grid size-9 place-items-center rounded-xl bg-blue-100 text-blue-600"><Layers3 size={17}/></div><div className="flex min-w-0 flex-1 flex-col gap-1"><strong className="text-xs font-bold text-slate-800">My Workspace</strong><small className="text-[10px] text-slate-500">Personal · Local-first</small></div><ChevronsUpDown size={15}/></div>
   <nav className="min-h-0 flex-1 overflow-y-auto px-2">{groups.map(g=><div className="mb-5" key={g.title}><div className="mx-4 mb-2 text-[10px] font-extrabold uppercase tracking-widest text-slate-400">{g.title}</div>{g.items.map(item=><SidebarNavItem key={item.id} icon={item.icon} label={item.label} active={page===item.id} count={item.id==='activity'?running:item.id==='settings'?pending:0} onClick={()=>navigate(item.id)}/>)}</div>)}</nav>
   <div className="mt-auto border-t border-slate-100 p-4"><div className="flex items-center gap-3 rounded-xl border border-slate-100 bg-slate-50 p-3"><div className="grid size-9 place-items-center rounded-xl bg-emerald-50 text-emerald-600 [&_span]:size-3 [&_span]:rounded-full [&_span]:bg-emerald-500"><span/></div><div className="flex min-w-0 flex-1 flex-col gap-1"><strong className="text-xs font-bold text-slate-800">{online?'Gateway online':'Gateway offline'}</strong><small className="text-[10px] text-slate-500">{online?'Local engine connected':'Check Node.js server'}</small></div><span className={'size-2 shrink-0 rounded-full bg-slate-300 '+(online?'!bg-emerald-500':'')}/></div><div className="mt-3 flex items-center gap-2 text-[10px] text-slate-400"><ShieldCheck size={15}/> Built to work for you <span>v0.1</span></div></div>
  </aside>
  {sidebar&&<div className="fixed inset-0 z-20 bg-slate-950/40 max-[850px]:block" onClick={()=>setSidebar(false)}/>}
  <main className="ml-[249px] min-h-screen min-w-0 max-[850px]:ml-0">
   <header className="sticky top-0 z-10 flex h-[69px] items-center justify-between gap-4 border-b border-slate-200 bg-white/90 px-7 backdrop-blur-md"><div className="flex min-w-0 items-center gap-3 text-sm"><Button variant="ghost" size="icon" className="!hidden max-[850px]:!inline-flex" onClick={()=>setSidebar(true)}><Menu size={21}/></Button><span className="text-slate-400">Workspace</span><span className="text-slate-300">/</span><strong>{labels[page]}</strong></div><div className="flex items-center gap-3"><Button className="flex min-w-[180px] items-center gap-2 rounded-xl border border-slate-200 bg-slate-50 px-3 py-2 text-xs text-slate-500 hover:bg-slate-100 max-sm:hidden" onClick={()=>{setPalette(true);setSearch('')}}><Search size={16}/> 빠른 이동 <kbd>⌘ K</kbd></Button><span className="h-5 w-px bg-slate-200 max-sm:hidden"/><div className={'inline-flex items-center gap-2 text-xs font-semibold text-slate-500 [&_span]:size-2 [&_span]:rounded-full [&_span]:bg-emerald-500 '+(online?'text-emerald-700':'text-rose-700')}><span/>{online?'시스템 연결됨':'오프라인'}</div><Button className="grid size-9 place-items-center rounded-xl bg-blue-50 text-sm font-extrabold text-blue-700" title="Personal workspace">D</Button></div></header>
   <div className={'min-w-0 page-'+page}>
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
  {palette&&<div className="fixed inset-0 z-50 flex items-start justify-center bg-slate-950/50 px-4 pt-[15vh] backdrop-blur-sm" onMouseDown={()=>setPalette(false)}><div className="w-full max-w-xl overflow-hidden rounded-2xl border border-slate-200 bg-white shadow-2xl" onMouseDown={e=>e.stopPropagation()}><div className="flex items-center gap-3 border-b border-slate-200 px-5 py-4 [&_input]:min-w-0 [&_input]:flex-1 [&_input]:border-0 [&_input]:bg-transparent [&_input]:outline-none"><Search size={19}/><TextInput autoFocus placeholder="무엇을 열까요?" value={search} onChange={e=>setSearch(e.target.value)}/><kbd>ESC</kbd></div><div className="max-h-[50vh] overflow-y-auto p-2 [&_button]:flex [&_button]:w-full [&_button]:items-center [&_button]:gap-3 [&_button]:rounded-xl [&_button]:px-3 [&_button]:py-3 [&_button]:text-left [&_button]:hover:bg-slate-50">{filtered.map(item=>{const Icon=item.icon;return <Button key={item.id} onClick={()=>navigate(item.id)}><Icon size={17}/><span>{item.label}</span><span>↵</span></Button>})}{!filtered.length&&<p>검색 결과가 없습니다.</p>}</div><div className="border-t border-slate-100 px-5 py-3 text-xs text-slate-400">DOMINION · 모든 기능을 한곳에서</div></div></div>}
 </div>
}
