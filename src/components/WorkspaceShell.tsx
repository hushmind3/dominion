import {useEffect,useMemo,useState,type ReactNode} from 'react'
import {Activity,Blocks,BookOpenText,BrainCircuit,ChevronRight,Command,Compass,House,Layers3,Menu,MessageSquareMore,RadioTower,Search,Settings2,ShieldCheck,UsersRound,Workflow,X} from 'lucide-react'
import {Button,TextInput} from './Primitives'
import {SidebarNavItem} from './SidebarNavigation'

export type PageId='home'|'chat'|'models'|'agents'|'rooms'|'workflows'|'memory'|'capabilities'|'extensions'|'connections'|'automations'|'activity'|'settings'
type NavItem={id:PageId;label:string;icon:typeof House}
export const navigation:{title:string;items:NavItem[]}[]=[
 {title:'WORKSPACE',items:[{id:'home',label:'개요',icon:House},{id:'chat',label:'대화',icon:MessageSquareMore},{id:'models',label:'모델 허브',icon:Compass},{id:'agents',label:'에이전트',icon:BrainCircuit},{id:'rooms',label:'그룹 · 회의실',icon:UsersRound},{id:'workflows',label:'워크플로',icon:Workflow},{id:'memory',label:'기억 라이브러리',icon:BookOpenText}]},
 {title:'INTEGRATIONS',items:[{id:'capabilities',label:'확장 허브',icon:Layers3},{id:'extensions',label:'MCP · 스킬',icon:Blocks},{id:'connections',label:'연결 · 채널',icon:RadioTower},{id:'automations',label:'자동화',icon:Workflow}]},
 {title:'CONTROL',items:[{id:'activity',label:'실행 기록',icon:Activity},{id:'settings',label:'설정',icon:Settings2}]}
]
const allItems=navigation.flatMap(group=>group.items)
export function WorkspaceSidebar({page,open,online,running,pending,onNavigate,onClose}:{page:PageId;open:boolean;online:boolean;running:number;pending:number;onNavigate:(id:PageId)=>void;onClose:()=>void}){
 return <>
  {open&&<div className="fixed inset-0 z-30 bg-slate-950/60 min-[851px]:hidden" onClick={onClose} aria-hidden="true"/>}
  <aside aria-label="주 탐색" className={'fixed inset-y-0 left-0 z-40 flex w-[264px] flex-col border-r border-white/10 bg-[#111827] text-slate-100 shadow-2xl transition-transform duration-200 max-[850px]:w-[min(84vw,300px)] '+(open?'translate-x-0':'max-[850px]:-translate-x-full')}>
   <div className="flex items-center gap-3 border-b border-white/10 px-5 py-6">
    <div className="grid size-11 shrink-0 place-items-center rounded-2xl bg-gradient-to-br from-indigo-400 via-blue-500 to-cyan-500 text-xl font-black text-white shadow-lg shadow-blue-950/50">D</div>
    <div className="min-w-0 flex-1"><div className="text-base font-black tracking-[.13em]">DOMINION</div><div className="mt-0.5 text-[10px] font-semibold tracking-[.2em] text-slate-400">INTELLIGENCE OS</div></div>
    <Button variant="ghost" size="icon" aria-label="메뉴 닫기" className="!text-slate-300 min-[851px]:hidden" onClick={onClose}><X size={19}/></Button>
   </div>
   <div className="mx-4 mt-5 rounded-2xl border border-white/10 bg-white/[.055] px-4 py-3">
    <div className="flex items-center gap-2.5"><span className="size-2 rounded-full bg-cyan-400 shadow-[0_0_0_4px_rgba(34,211,238,.1)]"/><span className="text-xs font-bold text-slate-200">Personal workspace</span></div>
    <div className="mt-1 pl-4 text-[11px] text-slate-400">로컬 우선 · 직접 제어</div>
   </div>
   <nav className="mt-5 min-h-0 flex-1 space-y-5 overflow-y-auto px-3 pb-4">
    {navigation.map(group=><div key={group.title}><div className="mb-2 px-3 text-[10px] font-bold tracking-[.18em] text-slate-500">{group.title}</div>
     <div className="space-y-1">{group.items.map(item=><SidebarNavItem key={item.id} icon={item.icon} label={item.label} active={page===item.id} count={item.id==='activity'?running:item.id==='settings'?pending:0} onClick={()=>onNavigate(item.id)}/>)}</div>
    </div>)}
   </nav>
   <div className="border-t border-white/10 p-4">
    <div className="flex items-center gap-3 rounded-xl bg-white/[.06] px-3 py-3">
     <span className={'size-2.5 rounded-full '+(online?'bg-emerald-400 shadow-[0_0_10px_rgba(52,211,153,.45)]':'bg-rose-400')}/>
     <div className="min-w-0 flex-1"><div className="text-xs font-semibold">{online?'게이트웨이 연결됨':'게이트웨이 오프라인'}</div><div className="mt-1 text-[10px] text-slate-400">DOMINION · Local gateway</div></div>
     <ShieldCheck className="text-slate-500" size={17}/>
    </div>
   </div>
  </aside>
 </>
}
export function WorkspaceHeader({page,online,onMenu,onSearch}:{page:PageId;online:boolean;onMenu:()=>void;onSearch:()=>void}){
 const title=allItems.find(item=>item.id===page)?.label||'개요'
 return <header className="sticky top-0 z-20 flex h-[72px] items-center justify-between gap-3 border-b border-slate-200/90 bg-white/90 px-5 backdrop-blur-xl sm:px-8">
  <div className="flex min-w-0 items-center gap-3">
   <Button variant="ghost" size="icon" title="모바일 메뉴" aria-label="모바일 메뉴" className="min-[851px]:hidden" onClick={onMenu}><Menu size={22}/></Button>
   <div className="hidden text-xs font-semibold tracking-widest text-slate-400 sm:block">WORKSPACE</div><ChevronRight size={15} className="hidden text-slate-300 sm:block"/><h1 className="truncate text-base font-extrabold text-slate-900">{title}</h1>
  </div>
  <div className="flex shrink-0 items-center gap-3">
   <Button variant="secondary" size="sm" onClick={onSearch} aria-label="빠른 이동 열기" className="!border-slate-200 !bg-slate-50 !text-slate-500"><Search size={15}/><span className="hidden sm:inline">빠른 이동</span><kbd className="ml-2 hidden rounded-md border border-slate-200 bg-white px-1.5 py-0.5 text-[10px] lg:inline">⌘ K</kbd></Button>
   <div className="hidden h-6 w-px bg-slate-200 sm:block"/>
   <span className={'flex items-center gap-2 rounded-full px-2.5 py-1.5 text-[11px] font-bold '+(online?'bg-emerald-50 text-emerald-700':'bg-rose-50 text-rose-700')}><span className={'size-1.5 rounded-full '+(online?'bg-emerald-500':'bg-rose-500')}/>{online?'온라인':'오프라인'}</span>
  </div>
 </header>
}
export function CommandPalette({onNavigate,onClose}:{onNavigate:(id:PageId)=>void;onClose:()=>void}){
 const [query,setQuery]=useState('')
 const items=useMemo(()=>allItems.filter(x=>x.label.toLowerCase().includes(query.trim().toLowerCase())),[query])
 useEffect(()=>{const handler=(e:KeyboardEvent)=>{if(e.key==='Escape')onClose()};window.addEventListener('keydown',handler);return()=>window.removeEventListener('keydown',handler)},[onClose])
 return <div role="presentation" className="fixed inset-0 z-50 flex items-start justify-center bg-slate-950/60 px-3 pt-[12vh] backdrop-blur-sm" onMouseDown={onClose}>
  <div role="dialog" aria-modal="true" aria-label="빠른 이동" className="w-full max-w-xl overflow-hidden rounded-2xl border border-slate-200 bg-white shadow-2xl" onMouseDown={e=>e.stopPropagation()}>
   <div className="flex items-center gap-3 border-b border-slate-100 px-4 py-4"><Search size={20} className="text-slate-400"/><TextInput autoFocus aria-label="메뉴 검색" placeholder="어느 작업 공간으로 이동할까요?" value={query} onChange={e=>setQuery(e.target.value)} onKeyDown={e=>{if(e.key==='Enter'&&items[0])onNavigate(items[0].id)}} className="!border-0 !bg-transparent !shadow-none focus:!ring-0"/><kbd className="rounded-md border border-slate-200 px-2 py-1 text-[10px] text-slate-400">ESC</kbd></div>
   <div className="max-h-[50vh] overflow-y-auto p-2">{items.map(item=>{const Icon=item.icon;return <Button variant="ghost" key={item.id} onClick={()=>onNavigate(item.id)} className="!flex !w-full !justify-start !rounded-xl !border-0 !px-3 !py-3 !text-left hover:!bg-slate-100"><span className="grid size-9 shrink-0 place-items-center rounded-lg bg-slate-100 text-slate-500"><Icon size={18}/></span><span className="flex-1 font-semibold">{item.label}</span><ChevronRight size={15} className="text-slate-300"/></Button>})}
    {!items.length&&<div className="p-8 text-center text-sm text-slate-500">일치하는 메뉴가 없습니다.</div>}
   </div>
   <div className="flex items-center gap-2 border-t border-slate-100 bg-slate-50 px-4 py-3 text-[11px] text-slate-400"><Command size={13}/> 메뉴 이름 검색 · Enter로 이동 · ESC로 닫기</div>
  </div>
 </div>
}
