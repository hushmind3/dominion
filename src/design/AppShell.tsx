import {useCallback,useEffect,useMemo,useState,type ReactNode} from 'react'
import {Activity,Blocks,BookOpenText,BrainCircuit,ChevronsUpDown,Command,Compass,House,Layers3,Menu,MessageSquareMore,PanelLeftClose,PanelLeftOpen,Plus,RadioTower,Search,Settings2,ShieldCheck,UsersRound,Workflow,X,type LucideIcon} from 'lucide-react'
import {Badge,Button,Dialog,IconButton,SearchBox,Text,cx} from './Primitives'

export type PageId='home'|'chat'|'models'|'agents'|'rooms'|'workflows'|'memory'|'capabilities'|'extensions'|'connections'|'automations'|'activity'|'settings'
const navigation:{title:string;items:{id:PageId;label:string;icon:LucideIcon}[]}[]=[
 {title:'WORKSPACE',items:[{id:'home',label:'개요',icon:House},{id:'chat',label:'대화',icon:MessageSquareMore},{id:'models',label:'모델 라이브러리',icon:Compass},{id:'agents',label:'에이전트',icon:BrainCircuit},{id:'rooms',label:'그룹 · 회의',icon:UsersRound},{id:'workflows',label:'워크플로',icon:Workflow},{id:'memory',label:'기억',icon:BookOpenText}]},
 {title:'EXTENSIONS',items:[{id:'capabilities',label:'기능 디렉터리',icon:Layers3},{id:'extensions',label:'MCP · 스킬',icon:Blocks},{id:'connections',label:'연결 · 채널',icon:RadioTower},{id:'automations',label:'자동화',icon:Workflow}]},
 {title:'SYSTEM',items:[{id:'activity',label:'실행 기록',icon:Activity},{id:'settings',label:'설정',icon:Settings2}]}
]
const items=navigation.flatMap(x=>x.items)
const byId=new Map(items.map(x=>[x.id,x]))
function NavEntry({id,label,Icon,active,collapsed,onClick,count}:{id:PageId;label:string;Icon:LucideIcon;active:boolean;collapsed:boolean;onClick:()=>void;count?:number}){
 return <button type="button" title={label} aria-label={label} aria-current={active?'page':undefined} onClick={onClick}
 className={cx('relative flex h-11 w-full items-center gap-3 rounded-[10px] px-3 text-[13px] font-semibold transition-colors',collapsed?'justify-center':'justify-start',active?'bg-white/13 text-white shadow-sm ring-1 ring-white/12':'text-slate-400 hover:bg-rail-hover hover:text-white')}>
  {active&&<span className="absolute inset-y-3 left-0 w-[3px] rounded-full bg-brand"/>}
  <Icon size={19} strokeWidth={1.8} className="shrink-0"/>
  {!collapsed&&<span className="min-w-0 flex-1 truncate text-left">{label}</span>}
  {!collapsed&&count!==undefined&&count>0&&<span className="rounded-md bg-white/15 px-1.5 py-0.5 text-[10px]">{count}</span>}
  {collapsed&&count!==undefined&&count>0&&<span className="absolute right-1 top-1 size-2 rounded-full bg-brand"/>}
 </button>
}
function SidebarContent({page,collapsed,onNavigate,running,pending}:{page:PageId;collapsed:boolean;onNavigate:(id:PageId)=>void;running:number;pending:number}){
 return <>
  <div className={cx('flex h-[78px] shrink-0 items-center gap-3 border-b border-white/10',collapsed?'justify-center px-2':'px-5')}>
   <button title="DOMINION 홈" onClick={()=>onNavigate('home')} className="grid size-10 shrink-0 place-items-center rounded-[13px] bg-brand text-white shadow-md shadow-brand/20"><Layers3 size={21}/></button>
   {!collapsed&&<span className="min-w-0"><strong className="block text-[15px] font-extrabold tracking-[.14em] text-white">DOMINION</strong><small className="mt-0.5 block text-[9px] font-bold tracking-[.14em] text-slate-400">INTELLIGENCE OS</small></span>}
  </div>
  <div className={cx('my-4 flex shrink-0 items-center rounded-xl border border-white/10 bg-white/5 text-slate-200',collapsed?'mx-3 justify-center p-2':'mx-3 gap-2.5 px-3 py-3')}>
   <div className="grid size-8 shrink-0 place-items-center rounded-[9px] bg-white/15 text-[12px] font-extrabold">D</div>
   {!collapsed&&<><span className="min-w-0 flex-1"><strong className="block truncate text-[11px]">My Workspace</strong><small className="block truncate text-[10px] text-slate-400">Personal · Local-first</small></span><ChevronsUpDown size={15} className="text-slate-500"/></>}
  </div>
  <nav aria-label="주 메뉴" className="min-h-0 flex-1 overflow-y-auto px-3 pb-4">
   {navigation.map(group=><section key={group.title} className="mb-5">
    {!collapsed?<p className="mb-2 px-3 text-[10px] font-extrabold tracking-[.1em] text-slate-500">{group.title}</p>:<div className="mx-3 my-3 h-px bg-white/10"/>}
    <div className="flex flex-col gap-1">{group.items.map(item=><NavEntry key={item.id} id={item.id} label={item.label} Icon={item.icon} active={page===item.id} collapsed={collapsed} onClick={()=>onNavigate(item.id)} count={item.id==='activity'?running:item.id==='settings'?pending:undefined}/>)}</div>
   </section>)}
  </nav>
 </>
}
export function AppShell({page,onNavigate,onNewChat,online,running=0,pending=0,children}:{page:PageId;onNavigate:(id:PageId)=>void;onNewChat:()=>void;online:boolean;running?:number;pending?:number;children:ReactNode}){
 const [collapsed,setCollapsed]=useState(()=>{try{return localStorage.getItem('dominion.nav.collapsed')==='true'}catch{return false}})
 const [mobile,setMobile]=useState(false)
 const [palette,setPalette]=useState(false)
 const [query,setQuery]=useState('')
 const selected=byId.get(page)
 const toggle=useCallback(()=>{
  if(window.matchMedia('(max-width: 900px)').matches){setMobile(x=>!x);return}
  setCollapsed(x=>{const next=!x;try{localStorage.setItem('dominion.nav.collapsed',String(next))}catch{}return next})
 },[])
 const navigate=(id:PageId)=>{onNavigate(id);setMobile(false);setPalette(false)}
 useEffect(()=>{
  const listener=(event:KeyboardEvent)=>{
   if((event.metaKey||event.ctrlKey)&&event.key.toLowerCase()==='k'){event.preventDefault();setPalette(x=>!x);setQuery('')}
   if((event.metaKey||event.ctrlKey)&&event.key.toLowerCase()==='b'){event.preventDefault();toggle()}
   if(event.key==='Escape')setMobile(false)
  }
  window.addEventListener('keydown',listener)
  return()=>window.removeEventListener('keydown',listener)
 },[toggle])
 const matching=useMemo(()=>items.filter(x=>x.label.toLowerCase().includes(query.trim().toLowerCase())),[query])
 return <div className="min-h-dvh bg-canvas">
  <aside aria-label="데스크톱 메뉴" className={cx('fixed inset-y-0 left-0 z-40 hidden flex-col overflow-hidden bg-rail shadow-lg transition-[width] duration-200 lg:flex',collapsed?'w-[74px]':'w-[250px]')}>
   <SidebarContent page={page} collapsed={collapsed} onNavigate={navigate} running={running} pending={pending}/>
   <div className={cx('shrink-0 border-t border-white/10 p-4',collapsed?'flex justify-center':'')}>
    {collapsed?<span title={online?'서버 연결됨':'서버 오프라인'} className={cx('size-2.5 rounded-full',online?'bg-emerald-400':'bg-amber-400')}/>:<><div className="flex items-center gap-2.5"><span className={cx('size-2.5 shrink-0 rounded-full',online?'bg-emerald-400':'bg-amber-400')}/><span className="min-w-0"><strong className="block text-[11px] font-semibold text-slate-100">{online?'Gateway online':'Gateway offline'}</strong><small className="text-[10px] text-slate-400">{online?'Local runtime available':'서버 연결 상태 확인'}</small></span></div><p className="mt-4 flex items-center gap-1.5 text-[9px] tracking-wider text-slate-500"><ShieldCheck size={13}/> LOCAL FIRST · v0.1</p></>}
   </div>
  </aside>
  {mobile&&<><button type="button" aria-label="메뉴 닫기" onClick={()=>setMobile(false)} className="fixed inset-0 z-40 bg-black/45 lg:hidden"/><aside aria-label="모바일 메뉴" className="fixed inset-y-0 left-0 z-50 flex w-[270px] flex-col bg-rail shadow-dialog lg:hidden"><SidebarContent page={page} collapsed={false} onNavigate={navigate} running={running} pending={pending}/><div className="border-t border-white/10 p-3"><Button variant="ghost" onClick={()=>setMobile(false)} className="!text-white">메뉴 닫기 <X size={15}/></Button></div></aside></>}
  <div className={cx('min-h-dvh min-w-0 transition-[padding] duration-200',collapsed?'lg:pl-[74px]':'lg:pl-[250px]')}>
   <header className="sticky top-0 z-30 flex h-[68px] items-center justify-between gap-4 border-b border-line bg-white/95 px-4 backdrop-blur-md sm:px-6 lg:px-8">
    <div className="flex min-w-0 items-center gap-3">
     <IconButton icon={collapsed?<PanelLeftOpen size={20}/>:<Menu size={20}/>} label={mobile?'메뉴 닫기':'사이드바 접기 또는 펼치기'} onClick={toggle} title="메뉴 (⌘B)"/>
     <div className="h-5 w-px bg-line"/>
     <div className="flex min-w-0 items-center gap-2"><Text size="sm" tone="subtle">Workspace</Text><Text size="sm" tone="subtle">/</Text>{selected&&<selected.icon size={15} className="shrink-0 text-brand"/>}<strong className="truncate text-[13px] font-bold text-ink">{selected?.label||'Workspace'}</strong></div>
    </div>
    <div className="flex shrink-0 items-center gap-2 sm:gap-3">
     <Button variant="secondary" size="sm" onClick={()=>{setQuery('');setPalette(true)}} title="검색 (⌘K)" className="!px-2.5 sm:!px-3"><Search size={16}/><span className="hidden sm:inline">빠른 이동</span><kbd className="hidden rounded-md bg-canvas px-1.5 py-0.5 text-[10px] text-subtle md:inline">⌘K</kbd></Button>
     <span title={online?'API 서버 연결됨':'API 서버 연결 안 됨'} className="flex items-center gap-1.5 px-1 text-[10px] font-bold text-subtle"><i className={cx('size-2 rounded-full',online?'bg-emerald-500':'bg-amber-500')}/><span className="hidden md:inline">{online?'ONLINE':'OFFLINE'}</span></span>
     <Button variant="primary" size="sm" onClick={()=>{onNewChat();setMobile(false)}}><Plus size={16}/><span className="hidden sm:inline">새 대화</span></Button>
    </div>
   </header>
   <main id="main-content" className="min-w-0">{children}</main>
  </div>
  <Dialog open={palette} onClose={()=>setPalette(false)} title="빠른 이동" description="페이지를 검색하거나 바로 이동하세요.">
   <div className="flex flex-col gap-3"><SearchBox value={query} onChange={setQuery} placeholder="페이지 이름 검색"/><div className="max-h-80 overflow-y-auto rounded-xl border border-line">{matching.map(item=><Button key={item.id} size="md" variant="ghost" onClick={()=>navigate(item.id)} className="!w-full !justify-start !rounded-none !border-b !border-line !text-left last:!border-0"><item.icon size={17}/><span className="flex-1">{item.label}</span><ArrowIcon/></Button>)}{!matching.length&&<p className="p-5 text-center text-xs text-subtle">검색 결과가 없습니다</p>}</div><p className="text-right text-[11px] text-subtle"><Command size={12} className="mr-1 inline"/> K · Esc</p></div>
  </Dialog>
 </div>
}
function ArrowIcon(){return <span className="text-subtle">↵</span>}
