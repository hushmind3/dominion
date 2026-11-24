import {useEffect, useRef, useState, type RefObject} from 'react'
import {Blocks, BookOpenText, Bot, Cpu, FileText, House, LoaderCircle, Menu, MessageSquare, PanelLeftClose, Plus, RadioTower, Search, Settings, UsersRound, Workflow, X} from 'lucide-react'
import type {ChatSession} from '../api'
import {useRemote} from '../hooks/useRemote'
import {Button, TextInput, cn} from './Primitives'
import {OwnerProfileEditor} from './OwnerProfile'
import {t, useLocale} from '../i18n'
import type {PageId} from '../workspacePages'

type Room = {id:string;name:string;agentIds:string[];status?:string}
type Props = {
 page:PageId; session?:string; selectedRoom?:string; open:boolean; mobile:boolean; online:boolean
 panel:RefObject<HTMLElement|null>; onToggle:()=>void; onNavigate:(page:PageId,session?:string)=>void
 onRoom:(id:string)=>void; onNew:()=>void; onNewGroup:()=>void; onClose:()=>void; resize:Record<string,unknown>
}

/** One navigation and one conversation index across the entire sub3 client. */
export function WorkspaceNavigation({page,session,selectedRoom,open,mobile,online,panel,onToggle,onNavigate,onRoom,onNew,onNewGroup,onClose,resize}:Props) {
 const en=useLocale()==='en'
 const tr=(ko:string,english:string)=>en?english:ko
 const sessions=useRemote<ChatSession[]>('/api/sessions',[])
 const rooms=useRemote<Room[]>('/api/rooms',[])
 const [query,setQuery]=useState('')
 const searchRef=useRef<HTMLInputElement>(null)
 const [focusSearch,setFocusSearch]=useState(false)
 useEffect(()=>{
  if(open&&focusSearch){searchRef.current?.focus();setFocusSearch(false)}
 },[open,focusSearch])
 useEffect(()=>{
  const shortcut=(event:KeyboardEvent)=>{
   if((event.metaKey||event.ctrlKey)&&event.key.toLowerCase()==='k'){
    event.preventDefault()
    if(!open){setFocusSearch(true);onToggle()}else searchRef.current?.focus()
   }else if(event.key==='Escape'&&mobile&&open){onClose()}
  }
  window.addEventListener('keydown',shortcut)
  return()=>window.removeEventListener('keydown',shortcut)
 },[open,mobile,onToggle,onClose])
 const navigate=(target:PageId,id?:string)=>{onNavigate(target,id);if(mobile)onClose()}
 const main=[{id:'chat' as const,label:tr('대화','Chat'),icon:MessageSquare},{id:'rooms' as const,label:tr('그룹','Groups'),icon:UsersRound},{id:'tasks' as const,label:t('nav.tasks'),icon:Workflow}]
 const tools=[{id:'connections' as const,label:tr('서비스 연결','Connected services'),icon:RadioTower},{id:'memory' as const,label:tr('기억','Memory'),icon:BookOpenText}]
 const renderItem=({id,label,icon:Icon}:typeof main[number]|typeof tools[number])=><Button variant="bare" key={id} className={cn('flex min-h-11 w-full min-w-0 flex-1 items-center gap-3 rounded-xl px-3 py-2 text-left text-sm text-ink/75 hover:bg-surface/75 aria-[current=page]:bg-selected aria-[current=page]:font-semibold aria-[current=page]:text-brand-hover [&>svg]:shrink-0 [&>span]:min-w-0 [&>span]:flex-1',!open&&'justify-center !px-2')} aria-current={page===id?'page':undefined} title={!open?label:undefined} aria-label={label} onClick={()=>navigate(id)}><Icon size={19}/>{open&&<span>{label}</span>}</Button>
 const needle=query.trim().toLocaleLowerCase()
 const destinations=[...main,...tools,{id:'settings' as const,label:tr('설정','Settings'),icon:Settings}]
 const deepLinks=[
  {label:tr('AI 모델 연결','AI connections'),terms:'ai model llm api 모델 연결',icon:Cpu,hash:'#/settings?view=ai'},
  {label:tr('AI 도우미·페르소나','Assistants & personas'),terms:'agent bot 페르소나 에이전트 도우미',icon:Bot,hash:'#/memory?view=personas'},
  {label:tr('MCP·플러그인','MCP & plugins'),terms:'mcp plugin extension 확장 플러그인 도구',icon:Blocks,hash:'#/connections?view=mcp'},
  {label:tr('공통·시스템 지침','Shared instructions'),terms:'prompt 프롬프트 지침 시스템',icon:FileText,hash:'#/memory?view=shared'},
  {label:tr('내 공간·사용 현황','Overview & usage'),terms:'home dashboard 대시보드 현황',icon:House,hash:'#/settings?view=overview'},
 ]
 const pageResults=needle?destinations.filter(item=>`${item.label} ${item.id}`.toLocaleLowerCase().includes(needle)):[]
 const deepResults=needle?deepLinks.filter(item=>`${item.label} ${item.terms}`.toLocaleLowerCase().includes(needle)):[]
 const privateChats=sessions.value.filter(item=>!item.roomId&&(item.title||'').toLocaleLowerCase().includes(needle)).slice(0,50)
 const groups=rooms.value.filter(item=>item.name.toLocaleLowerCase().includes(needle))
 return <>
  {mobile&&open&&<Button variant="bare" className="fixed inset-0 z-80 border-0 bg-ink/30" aria-label={tr('메뉴 닫기','Close navigation')} onClick={onClose}/>}
  <aside ref={panel} id="workspace-navigation" role={mobile&&open?'dialog':undefined} aria-modal={mobile&&open?true:undefined} tabIndex={-1} aria-label={tr('작업공간 메뉴','Workspace navigation')} className={cn('workspace-navigation relative flex min-h-0 flex-col border-r border-divider bg-sidebar p-3',mobile?(open?'fixed inset-y-0 left-0 z-81 w-[min(20rem,86vw)] shadow-xl':'hidden'):'w-[var(--workspace-navigation-width)] shrink-0',!open&&'!px-2')} data-open={open} data-mobile={mobile}>
   <div className="mb-4 flex min-h-12 items-center justify-between gap-3 px-2 [&>a]:text-2xl [&>a]:font-semibold [&>a]:tracking-tight">{open&&<a href="#/chat" onClick={event=>{event.preventDefault();navigate('chat')}}>dominion</a>}<Button size="icon" variant="ghost" onClick={onToggle} aria-label={open?tr('메뉴 접기','Collapse navigation'):tr('메뉴 펼치기','Expand navigation')} aria-expanded={open} aria-controls="workspace-navigation">{open?<PanelLeftClose size={18}/>:<Menu size={20}/>}</Button></div>
   <Button variant="bare" className="mb-4 flex min-h-11 items-center justify-center gap-2 rounded-xl bg-brand px-2 text-sm font-semibold text-white hover:bg-brand-hover" title={tr('새 대화','New conversation')} onClick={()=>{onNew();if(mobile)onClose()}}><Plus size={20}/>{open&&<span>{tr('새 대화','New conversation')}</span>}</Button>
   <nav className="border-b border-divider pb-3" aria-label={tr('대화와 작업','Conversations and work')}>{main.map(item=><div className="flex min-w-0 items-center gap-0.5" key={item.id}>{renderItem(item)}{item.id==='rooms'&&open&&<Button variant="ghost" size="icon" className="!h-8 !w-8 !p-0" title={tr('새 그룹 만들기','Create a group')} aria-label={tr('새 그룹 만들기','Create a group')} onClick={()=>{onNewGroup();if(mobile)onClose()}}><Plus size={16}/></Button>}</div>)}</nav>
   <div className="mt-3 min-h-0 flex-1 overflow-y-auto overscroll-contain">
    <nav className="border-b border-divider pb-3" aria-label={tr('자주 쓰는 기능','Frequently used features')}>{open&&<h2 className="px-3 pb-2 text-xs font-medium text-muted">{tr('자주 쓰는 기능','Quick access')}</h2>}{tools.map(renderItem)}</nav>
    {open&&<>
     <div className="my-3 flex items-center gap-2 rounded-lg border border-divider bg-surface/50 px-2 text-muted focus-within:ring-2 focus-within:ring-brand/25 [&>svg]:shrink-0 [&_input]:!border-0 [&_input]:!bg-transparent [&_input]:!px-0 [&_input]:!text-xs [&_input]:!shadow-none [&_input]:!ring-0"><Search size={15}/><TextInput ref={searchRef} value={query} onChange={event=>setQuery(event.target.value)} placeholder={tr('메뉴·대화·그룹 검색  ⌘/Ctrl K','Find pages, chats, groups  ⌘/Ctrl K')} aria-label={tr('메뉴·대화·그룹 검색','Search pages, chats and groups')}/>{query&&<Button size="icon" variant="ghost" aria-label={tr('검색 지우기','Clear search')} onClick={()=>setQuery('')}><X size={14}/></Button>}</div>
     {needle&&(pageResults.length>0||deepResults.length>0)&&<section className="mb-4" aria-label={tr('화면 바로가기','Page shortcuts')}><h2 className="mb-2 px-3 text-xs font-medium text-muted">{tr('메뉴 검색 결과','Matching pages')}</h2><div className="grid gap-1">{pageResults.map(({id,label,icon:Icon})=><Button key={id} variant="bare" className="flex min-h-10 w-full items-center justify-start gap-3 rounded-lg px-3 py-2 text-left text-sm text-ink hover:bg-surface/75" onClick={()=>{setQuery('');navigate(id)}}><Icon size={16}/><span className="truncate">{label}</span></Button>)}{deepResults.map(({hash,label,icon:Icon})=><Button key={hash} variant="bare" className="flex min-h-10 w-full items-center justify-start gap-3 rounded-lg px-3 py-2 text-left text-sm text-ink hover:bg-surface/75" onClick={()=>{setQuery('');window.location.hash=hash;if(mobile)onClose()}}><Icon size={16}/><span className="truncate">{label}</span></Button>)}</div></section>}
     <div className="space-y-5 [&_h2]:mb-2 [&_h2]:px-3 [&_h2]:text-xs [&_h2]:font-medium [&_h2]:text-muted">
      {groups.length>0&&<section><h2>{tr('그룹 대화','Groups')}</h2>{groups.map(room=><Button key={room.id} variant="bare" className="flex min-h-10 w-full min-w-0 items-center gap-2 rounded-lg px-3 py-2 text-left text-xs text-muted hover:bg-surface/60 aria-[current=page]:bg-selected aria-[current=page]:font-semibold aria-[current=page]:text-brand-hover [&>svg]:shrink-0 [&>span]:min-w-0 [&>span]:flex-1 [&>span]:truncate [&>i]:size-1.5 [&>i]:rounded-full [&>i]:bg-success" aria-current={page==='rooms'&&room.id===selectedRoom?'page':undefined} onClick={()=>{onRoom(room.id);if(mobile)onClose()}}><UsersRound size={15}/><span>{room.name}</span>{room.status==='running'&&<i aria-label={tr('답변 중','Responding')}/>}</Button>)}</section>}
      {privateChats.length>0&&<section><h2>{tr('최근 대화','Recent conversations')}</h2>{privateChats.map(item=><Button key={item.id} variant="bare" className="flex min-h-10 w-full min-w-0 items-center gap-2 rounded-lg px-3 py-2 text-left text-xs text-muted hover:bg-surface/60 aria-[current=page]:bg-selected aria-[current=page]:font-semibold aria-[current=page]:text-brand-hover [&>svg]:shrink-0 [&>span]:min-w-0 [&>span]:flex-1 [&>span]:truncate [&>i]:size-1.5 [&>i]:rounded-full [&>i]:bg-success" title={item.title} aria-current={page==='chat'&&session===item.id?'page':undefined} onClick={()=>navigate('chat',item.id)}><MessageSquare size={15}/><span>{item.title||tr('새 대화','New conversation')}</span></Button>)}</section>}
      {(sessions.loading||rooms.loading)&&!privateChats.length&&!groups.length&&<div className="flex items-center justify-center gap-2 px-3 py-4 text-center text-xs leading-6 text-muted" role="status"><LoaderCircle size={15} className="animate-spin"/>{tr('대화 불러오는 중','Loading conversations')}</div>}
      {!sessions.loading&&!rooms.loading&&!privateChats.length&&!groups.length&&!pageResults.length&&!deepResults.length&&<p className="flex items-center justify-center gap-2 px-3 py-4 text-center text-xs leading-6 text-muted">{tr(query?'검색 결과가 없습니다.':'대화를 시작하면 여기에 저장됩니다.',query?'No matching conversations.':'Your conversations will appear here.')}</p>}
      {(sessions.error||rooms.error)&&<Button variant="ghost" className="w-full !text-xs" onClick={()=>{void sessions.reload();void rooms.reload()}}>{tr('대화 목록 다시 불러오기','Reload conversations')}</Button>}
     </div>
    </>}
   </div>
   <div className="flex items-center gap-1.5 border-t border-divider pt-3"><Button variant="bare" className={cn('flex min-h-11 w-full min-w-0 flex-1 items-center gap-3 rounded-xl px-3 py-2 text-left text-sm text-ink/75 hover:bg-surface/75 aria-[current=page]:bg-selected aria-[current=page]:font-semibold aria-[current=page]:text-brand-hover [&>svg]:shrink-0 [&>span]:min-w-0 [&>span]:flex-1',!open&&'justify-center !px-2')} aria-current={page==='settings'?'page':undefined} onClick={()=>navigate('settings')} aria-label={t('nav.settings')}><Settings size={19}/>{open&&<span>{t('nav.settings')}</span>}<i className="size-1.5 shrink-0 rounded-full bg-divider data-[online=true]:bg-success" data-online={online} aria-label={online?tr('연결됨','Connected'):tr('연결 끊김','Disconnected')}/></Button>{open&&<OwnerProfileEditor compact/>}</div>
   {open&&!mobile&&<div {...resize} className="absolute inset-y-0 -right-0.5 z-20 w-1.5 cursor-col-resize hover:bg-brand/15" aria-label={tr('메뉴 너비 조절','Resize navigation')}/>}
  </aside>
 </>
}
