import {useEffect,useRef,useState,type RefObject} from 'react'
import {BookOpenText,LoaderCircle,Menu,MessageSquare,PanelLeftClose,Plus,RadioTower,Search,Settings,UsersRound,Workflow,X} from 'lucide-react'
import type {ChatSession,Room} from '../api'
import {useRemote} from '../hooks/useRemote'
import {Button,TextInput,cn} from './Primitives'
import {OwnerProfileEditor} from './OwnerProfile'
import {t,useLocale} from '../i18n'
import type {PageId} from '../workspacePages'

type Props={
 page:PageId;session?:string;selectedRoom?:string;open:boolean;mobile:boolean;online:boolean
 panel:RefObject<HTMLElement|null>;onToggle:()=>void;onNavigate:(page:PageId,session?:string)=>void
 onRoom:(id:string)=>void;onNew:()=>void;onNewGroup:()=>void;onClose:()=>void;resize:Record<string,unknown>
}
const rowClass='flex min-h-9 w-full min-w-0 items-center gap-2.5 rounded-lg px-2.5 py-2 text-left text-sm text-muted hover:bg-canvas hover:text-ink aria-[current=page]:bg-selected aria-[current=page]:font-semibold aria-[current=page]:text-brand-hover [&>svg]:shrink-0'
const newest=(a:{createdAt?:string;updatedAt?:string},b:{createdAt?:string;updatedAt?:string})=>(b.updatedAt||b.createdAt||'').localeCompare(a.updatedAt||a.createdAt||'')

export function WorkspaceNavigation({page,session,selectedRoom,open,mobile,online,panel,onToggle,onNavigate,onRoom,onNew,onNewGroup,onClose,resize}:Props){
 const en=useLocale()==='en',tr=(ko:string,english:string)=>en?english:ko
 const sessions=useRemote<ChatSession[]>('/api/sessions',[])
 const rooms=useRemote<Room[]>('/api/rooms',[])
 const [query,setQuery]=useState('')
 const [chatLimit,setChatLimit]=useState(30),[roomLimit,setRoomLimit]=useState(15)
 const [historyFilter,setHistoryFilter]=useState<'all'|'chat'|'rooms'>('all')
 const searchRef=useRef<HTMLInputElement>(null)
 const focusSearch=useRef(false)
 useEffect(()=>{
  const shortcut=(event:KeyboardEvent)=>{
   if(!(event.metaKey||event.ctrlKey)||event.key.toLowerCase()!=='k'||event.isComposing)return
   event.preventDefault()
   if(open)searchRef.current?.focus()
   else{focusSearch.current=true;onToggle()}
  }
  window.addEventListener('keydown',shortcut)
  return()=>window.removeEventListener('keydown',shortcut)
 },[open,onToggle])
 useEffect(()=>{if(open&&focusSearch.current){focusSearch.current=false;searchRef.current?.focus()}},[open])
 const navigate=(target:PageId,id?:string)=>{onNavigate(target,id);if(mobile)onClose()}
 const main=[{id:'chat' as const,label:tr('대화','Chat'),icon:MessageSquare},{id:'rooms' as const,label:tr('그룹','Groups'),icon:UsersRound},{id:'tasks' as const,label:t('nav.tasks'),icon:Workflow}]
 const tools=[{id:'connections' as const,label:tr('서비스 연결','Connected services'),icon:RadioTower},{id:'memory' as const,label:tr('기억','Memory'),icon:BookOpenText}]
 const renderItem=({id,label,icon:Icon}:typeof main[number]|typeof tools[number])=><Button variant="bare" key={id} className={cn(rowClass,'min-h-10 flex-1 font-medium',!open&&'justify-center !px-2')} aria-current={page===id?'page':undefined} title={label} aria-label={label} onClick={()=>navigate(id)}><Icon size={18} strokeWidth={1.8}/>{open&&<span className="min-w-0 flex-1 truncate">{label}</span>}</Button>
 const needle=query.trim().toLocaleLowerCase()
 const privateChats=sessions.value.filter(item=>(historyFilter!=='rooms'||Boolean(needle))&&!item.roomId&&[item.title,item.model,item.id].some(value=>(value||'').toLocaleLowerCase().includes(needle))).sort(newest)
 const groups=rooms.value.filter(item=>(historyFilter!=='chat'||Boolean(needle))&&(item.name||'').toLocaleLowerCase().includes(needle)).sort(newest)
 const changeQuery=(value:string)=>{setQuery(value);setChatLimit(30);setRoomLimit(15)}
 return <>
  {mobile&&open&&<Button variant="bare" className="fixed inset-0 z-80 border-0 bg-ink/35" aria-label={tr('메뉴 닫기','Close navigation')} onClick={onClose}/>}
  <aside ref={panel} id="workspace-navigation" role={mobile&&open?'dialog':undefined} aria-modal={mobile&&open?true:undefined} tabIndex={-1} aria-label={tr('작업공간 메뉴','Workspace navigation')} className={cn('workspace-navigation relative flex min-h-0 flex-col border-r border-divider bg-sidebar p-3',mobile?(open?'fixed inset-y-0 left-0 z-81 w-[min(20rem,86vw)] shadow-xl':'hidden'):'w-[var(--workspace-navigation-width)] shrink-0',!open&&'!px-2')} data-open={open} data-mobile={mobile}>
   <div className="mb-3 flex min-h-11 shrink-0 items-center justify-between gap-2 px-1">
    {open&&<a href="#/chat" className="flex min-w-0 items-center gap-2.5" onClick={event=>{event.preventDefault();navigate('chat')}}><span className="grid size-8 shrink-0 place-items-center rounded-lg bg-ink text-sm font-bold text-surface">D</span><span className="truncate text-lg font-semibold tracking-tight text-ink">DOMINION</span></a>}
    <Button size="icon" variant="ghost" className="!size-9" onClick={onToggle} aria-label={open?tr('메뉴 접기','Collapse navigation'):tr('메뉴 펼치기','Expand navigation')} aria-expanded={open} aria-controls="workspace-navigation">{open?<PanelLeftClose size={18}/>:<Menu size={20}/>}</Button>
   </div>
   <Button variant="primary" className={cn('mb-3 min-h-11 w-full shrink-0 !rounded-lg !shadow-none',!open&&'!px-0')} aria-label={tr('새 대화','New conversation')} title={tr('새 대화','New conversation')} onClick={()=>{onNew();if(mobile)onClose()}}><Plus size={18}/>{open&&<span>{tr('새 대화','New conversation')}</span>}</Button>
   <nav className="grid shrink-0 gap-0.5" aria-label={tr('대화와 작업','Conversations and work')}>{main.map(item=><div className="flex min-w-0 items-center gap-0.5" key={item.id}>{renderItem(item)}{item.id==='rooms'&&open&&<Button variant="ghost" size="icon" className="!size-8 !p-0" title={tr('새 그룹 만들기','Create a group')} aria-label={tr('새 그룹 만들기','Create a group')} onClick={()=>{onNewGroup();if(mobile)onClose()}}><Plus size={16}/></Button>}</div>)}</nav>
   <nav className="mt-3 grid shrink-0 gap-0.5 border-t border-divider pt-3" aria-label={tr('서비스 연결과 기억','Service connections and memory')}>{open&&<div className="px-2.5 pb-1 text-[11px] font-semibold text-muted">{tr('작업 환경','Workspace')}</div>}{tools.map(renderItem)}</nav>
   {open&&<div className="mt-4 flex shrink-0 items-center gap-2 rounded-lg border border-divider bg-surface px-2.5 text-muted focus-within:ring-2 focus-within:ring-brand/25">
    <Search size={16} className="shrink-0"/><TextInput ref={searchRef} value={query} onChange={event=>changeQuery(event.target.value)} placeholder={tr('대화 검색','Search conversations')} aria-label={tr('대화·그룹 검색','Search chats and groups')} className="!min-h-9 !border-0 !bg-transparent !px-0 !py-1.5 !text-sm !shadow-none !ring-0" onKeyDown={event=>{if(event.key==='Escape')changeQuery('')}}/>
    {query?<Button size="icon" variant="ghost" className="!size-7" aria-label={tr('검색 지우기','Clear search')} onClick={()=>changeQuery('')}><X size={14}/></Button>:<kbd className="shrink-0 rounded border border-divider px-1 text-[10px] text-muted">Ctrl/⌘ K</kbd>}
   </div>}
   {open&&!needle&&<div role="group" aria-label={tr('대화 목록 보기','Conversation list view')} className="mt-2 flex shrink-0 gap-1 rounded-lg bg-canvas p-1">{([{id:'all',label:tr('전체','All')},{id:'chat',label:tr('개인','Direct')},{id:'rooms',label:tr('그룹','Groups')}] as const).map(item=><Button key={item.id} variant="bare" aria-pressed={historyFilter===item.id} className="min-h-8 flex-1 rounded-md px-1 text-xs text-muted aria-pressed:bg-surface aria-pressed:font-semibold aria-pressed:text-ink aria-pressed:shadow-sm" onClick={()=>setHistoryFilter(item.id)}>{item.label}</Button>)}</div>}
   <div className="mt-3 min-h-0 flex-1 overflow-y-auto overscroll-contain">
    {open&&<div className="space-y-5">
     {groups.length>0&&<section aria-label={tr('그룹 대화','Groups')}><h2 className="mb-1 flex items-center justify-between px-2.5 text-[11px] font-semibold text-muted">{tr('그룹 대화','Groups')}<span className="tabular-nums">{groups.length}</span></h2>{groups.slice(0,roomLimit).map(room=><Button key={room.id} variant="bare" title={room.name} className={rowClass} aria-current={page==='rooms'&&room.id===selectedRoom?'page':undefined} onClick={()=>{onRoom(room.id);if(mobile)onClose()}}><UsersRound size={15}/><span className="min-w-0 flex-1 truncate">{room.name}</span>{room.status==='running'&&<i className="size-1.5 shrink-0 rounded-full bg-success" aria-label={tr('답변 중','Responding')}/>}</Button>)}{groups.length>roomLimit&&<Button variant="ghost" size="sm" className="w-full !justify-start" onClick={()=>setRoomLimit(n=>n+15)}>{tr('그룹 더 보기','More groups')}</Button>}</section>}
     {privateChats.length>0&&<section aria-label={tr('최근 대화','Recent conversations')}><h2 className="mb-1 flex items-center justify-between px-2.5 text-[11px] font-semibold text-muted">{tr('최근 대화','Recent conversations')}<span className="tabular-nums">{privateChats.length}</span></h2>{privateChats.slice(0,chatLimit).map(item=><Button key={item.id} variant="bare" className={rowClass} title={item.title} aria-current={page==='chat'&&session===item.id?'page':undefined} onClick={()=>navigate('chat',item.id)}><MessageSquare size={15}/><span className="min-w-0 flex-1 truncate">{item.title||tr('새 대화','New conversation')}</span></Button>)}{privateChats.length>chatLimit&&<Button variant="ghost" size="sm" className="w-full !justify-start" onClick={()=>setChatLimit(n=>n+30)}>{tr('대화 더 보기','More conversations')}</Button>}</section>}
     {(sessions.loading||rooms.loading)&&!privateChats.length&&!groups.length&&<div className="flex items-center gap-2 px-2.5 py-3 text-xs text-muted" role="status"><LoaderCircle size={15} className="animate-spin"/>{tr('대화 불러오는 중','Loading conversations')}</div>}
     {!sessions.loading&&!rooms.loading&&!sessions.error&&!rooms.error&&!privateChats.length&&!groups.length&&<p className="px-2.5 py-3 text-xs leading-6 text-muted">{tr(query?'검색 결과가 없습니다.':'대화를 시작하면 여기에 저장됩니다.',query?'No matching conversations.':'Your conversations will appear here.')}</p>}
     {(sessions.error||rooms.error)&&<Button variant="ghost" className="w-full !text-xs" onClick={()=>{void sessions.reload();void rooms.reload()}}>{tr('대화 목록 다시 불러오기','Reload conversations')}</Button>}
    </div>}
   </div>
   <div className="shrink-0 border-t border-divider pt-2">
    {open&&<div className="flex items-center gap-1.5 px-2.5 pb-1 text-[11px] text-muted"><span className={cn('size-1.5 rounded-full',online?'bg-success':'bg-danger')}/>{online?tr('엔진 연결됨','Engine connected'):tr('엔진 연결 끊김','Engine disconnected')}</div>}
    <div className="flex items-center gap-1"><Button variant="bare" className={cn(rowClass,'min-h-10 flex-1 font-medium',!open&&'justify-center !px-2')} aria-current={page==='settings'?'page':undefined} onClick={()=>navigate('settings')} aria-label={t('nav.settings')} title={t('nav.settings')}><Settings size={18}/>{open&&<span className="min-w-0 flex-1 truncate">{t('nav.settings')}</span>}</Button>{open&&<OwnerProfileEditor compact/>}</div>
   </div>
   {open&&!mobile&&<div {...resize} className="absolute inset-y-0 -right-0.5 z-20 w-1.5 cursor-col-resize hover:bg-brand/15" aria-label={tr('메뉴 너비 조절','Resize navigation')}/>}
  </aside>
 </>
}
