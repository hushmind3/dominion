import {useEffect,useMemo,useState} from 'react'
import {Activity,Blocks,BookOpenText,Bot,ChevronDown,ChevronRight,Command,CommandIcon,Cpu,House,Layers3,Menu,MessageSquare,RadioTower,Search,Settings2,UsersRound,Workflow,X,Plus} from 'lucide-react'
import {Button,TextInput} from './Primitives'

export type PageId='home'|'chat'|'models'|'agents'|'rooms'|'workflows'|'memory'|'capabilities'|'extensions'|'connections'|'automations'|'activity'|'settings'
type NavItem={id:PageId;label:string;icon:typeof House}
export const navigation:{title:string;items:NavItem[];collapsible?:boolean}[]=[
 {title:'작업 공간',items:[{id:'home',label:'대시보드',icon:House},{id:'chat',label:'채팅',icon:MessageSquare},{id:'models',label:'모델 관리',icon:Cpu},{id:'agents',label:'에이전트',icon:Bot}]},
 {title:'확장',items:[{id:'capabilities',label:'연결 · 채널',icon:RadioTower},{id:'extensions',label:'MCP · 스킬',icon:Blocks},{id:'memory',label:'메모리',icon:BookOpenText}]},
 {title:'운영',items:[{id:'automations',label:'자동화',icon:Workflow},{id:'activity',label:'실행 기록',icon:Activity},{id:'settings',label:'설정',icon:Settings2}]},
 {title:'고급',collapsible:true,items:[{id:'rooms',label:'멀티 에이전트',icon:UsersRound},{id:'workflows',label:'워크플로',icon:Workflow}]}
]
const items=navigation.flatMap(g=>g.items)
export function WorkspaceSidebar({page,open,online,running,pending,onNavigate,onClose}:{page:PageId;open:boolean;online:boolean;running:number;pending:number;onNavigate:(id:PageId)=>void;onClose:()=>void}){
 const advancedItems=navigation.find(g=>g.collapsible)?.items||[]
 const [advancedOpen,setAdvancedOpen]=useState(()=>{try{return localStorage.getItem('dominion.nav.advanced')==='1'||advancedItems.some(x=>x.id===page)}catch{return advancedItems.some(x=>x.id===page)}})
 useEffect(()=>{if(advancedItems.some(x=>x.id===page))setAdvancedOpen(true)},[page])
 useEffect(()=>{try{localStorage.setItem('dominion.nav.advanced',advancedOpen?'1':'0')}catch{}},[advancedOpen])
 return <>
  {open&&<button type="button" className="fixed inset-0 z-40 bg-slate-900/40 min-[860px]:hidden" aria-label="메뉴 닫기" onClick={onClose}/>}
  <aside aria-label="주 탐색" className={'fixed inset-y-0 left-0 z-50 flex w-[216px] flex-col border-r border-[#26303e] bg-[#141b25] text-[#d7deeb] transition-transform duration-200 max-[859px]:-translate-x-full '+(open?'max-[859px]:translate-x-0':'')}>
   <div className="flex h-14 shrink-0 items-center gap-2.5 border-b border-white/[.07] px-4">
    <div className="grid size-7 place-items-center rounded-[8px] bg-[#5c7cfa] text-[16px] font-black text-white">D</div>
    <div className="min-w-0 flex-1 text-[13px] font-extrabold tracking-[.09em] text-white">DOMINION</div>
    <button type="button" className="min-[860px]:hidden" aria-label="닫기" onClick={onClose}><X size={16}/></button>
   </div>
   <div className="px-3 pt-4"><button type="button" onClick={()=>onNavigate('chat')} className="flex h-9 w-full items-center justify-center gap-2 rounded-lg bg-[#5475eb] text-xs font-semibold text-white hover:bg-[#4564d2]"><Plus size={15}/> 새 대화</button></div>
   <nav className="mt-3 min-h-0 flex-1 overflow-y-auto px-2 pb-3">
    {navigation.map(group=>{const expanded=!group.collapsible||advancedOpen;return <section key={group.title} className="mt-4">
     {group.collapsible?<button type="button" onClick={()=>setAdvancedOpen(x=>!x)} aria-expanded={advancedOpen} className="mb-1.5 flex w-full items-center justify-between px-3 text-[10px] font-semibold text-[#748197] hover:text-[#aeb8c7]"><span>{group.title}</span><ChevronDown size={13} className={'transition-transform '+(advancedOpen?'':' -rotate-90')}/></button>:<h2 className="mb-1.5 px-3 text-[10px] font-semibold text-[#748197]">{group.title}</h2>}
     {expanded&&<div className="space-y-0.5">{group.items.map(item=>{const Icon=item.icon;const count=item.id==='activity'?running:item.id==='settings'?pending:0;return <button key={item.id} type="button" aria-current={page===item.id?'page':undefined} onClick={()=>onNavigate(item.id)}
      className={'group flex h-8 w-full items-center gap-2.5 rounded-md px-3 text-left text-[12px] transition-colors '+(page===item.id?'bg-[#303f5a] font-semibold text-white':'text-[#aeb8c7] hover:bg-white/[.07] hover:text-white')}>
      <Icon size={15} strokeWidth={1.8}/><span className="min-w-0 flex-1">{item.label}</span>{count>0&&<span className="text-[10px] text-sky-300">{count}</span>}
     </button>})}</div>}
    </section>})}
   </nav>
   <div className="flex h-12 shrink-0 items-center gap-2.5 border-t border-white/[.07] px-4">
    <span className={'size-1.5 rounded-full '+(online?'bg-emerald-400':'bg-rose-400')}/>
    <span className="text-[11px] text-[#aeb8c7]">{online?'게이트웨이 연결됨':'게이트웨이 연결 안 됨'}</span>
   </div>
  </aside>
 </>
}
export function WorkspaceHeader({page,online,onMenu,onSearch}:{page:PageId;online:boolean;onMenu:()=>void;onSearch:()=>void}){
 const selected=items.find(x=>x.id===page)
 return <header className="sticky top-0 z-30 flex h-14 items-center justify-between gap-3 border-b border-[#e2e6ec] bg-white/95 px-5 backdrop-blur-sm">
  <div className="flex min-w-0 items-center gap-2">
   <button type="button" className="mr-2 rounded-md p-1.5 text-slate-600 min-[860px]:hidden" aria-label="메뉴 열기" onClick={onMenu}><Menu size={18}/></button>
   <span className="hidden text-[11px] text-slate-400 sm:block">워크스페이스</span><ChevronRight size={13} className="hidden text-slate-300 sm:block"/>
   <h1 className="truncate text-[13px] font-bold text-[#273242]">{selected?.label||'대시보드'}</h1>
  </div>
  <div className="flex items-center gap-3">
   <button type="button" onClick={onSearch} className="flex h-8 items-center gap-2 rounded-md border border-slate-200 bg-[#f7f8fa] px-3 text-[11px] text-slate-500 hover:border-slate-300"><Search size={14}/><span className="hidden sm:inline">빠른 이동</span><kbd className="hidden text-[10px] md:inline">Ctrl K</kbd></button>
   <span className={'hidden items-center gap-1.5 text-[11px] sm:inline-flex '+(online?'text-emerald-700':'text-rose-600')}><i className={'size-1.5 rounded-full '+(online?'bg-emerald-500':'bg-rose-500')}/>{online?'연결됨':'연결 끊김'}</span>
  </div>
 </header>
}
export function CommandPalette({onNavigate,onClose}:{onNavigate:(id:PageId)=>void;onClose:()=>void}){
 const [query,setQuery]=useState('')
 const filtered=useMemo(()=>items.filter(item=>item.label.toLowerCase().includes(query.trim().toLowerCase())),[query])
 useEffect(()=>{const h=(e:KeyboardEvent)=>{if(e.key==='Escape')onClose()};window.addEventListener('keydown',h);return()=>window.removeEventListener('keydown',h)},[onClose])
 return <div className="fixed inset-0 z-[90] flex justify-center bg-black/40 px-3 pt-[12vh]" onMouseDown={onClose} role="presentation">
  <div className="h-fit w-full max-w-[480px] overflow-hidden rounded-xl border border-slate-200 bg-white shadow-2xl" role="dialog" aria-modal="true" aria-label="빠른 이동" onMouseDown={e=>e.stopPropagation()}>
   <div className="flex items-center gap-2 border-b border-slate-100 px-4 py-3"><Search size={16} className="text-slate-400"/><TextInput autoFocus aria-label="메뉴 검색" value={query} onChange={e=>setQuery(e.target.value)} onKeyDown={e=>{if(e.key==='Enter'&&filtered[0])onNavigate(filtered[0].id)}} placeholder="메뉴 검색" className="!border-0 !shadow-none !ring-0"/></div>
   <div className="max-h-[55vh] overflow-auto p-2">{filtered.map(item=>{const Icon=item.icon;return <button type="button" key={item.id} onClick={()=>onNavigate(item.id)} className="flex h-10 w-full items-center gap-3 rounded-md px-3 text-left text-xs hover:bg-slate-100"><Icon size={16}/><span className="flex-1">{item.label}</span><ChevronRight size={13} className="text-slate-300"/></button>})}{!filtered.length&&<p className="p-5 text-center text-xs text-slate-500">검색 결과 없음</p>}</div>
   <div className="border-t border-slate-100 px-4 py-2 text-[10px] text-slate-400">Enter 이동 · Esc 닫기</div>
  </div>
 </div>
}
