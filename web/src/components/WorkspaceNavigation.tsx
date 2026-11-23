import {useMemo,useState,type RefObject} from 'react'
import {ArrowRight,LoaderCircle,Menu,MessageSquare,PanelLeftClose,PanelLeftOpen,Plus,Search,Settings,UsersRound,X} from 'lucide-react'
import type {ChatSession} from '../api'
import {useRemote} from '../hooks/useRemote'
import {Button,TextInput,cn} from './Primitives'
import {OwnerProfileEditor} from './OwnerProfile'
import {t,useLocale} from '../i18n'
import type {PageId} from '../workspacePages'

type Room={id:string;name:string;agentIds:string[];status?:string}
type Props={
 page:PageId;mode:'chat'|'rooms';session?:string;selectedRoom?:string;open:boolean;mobile:boolean;online:boolean
 panel:RefObject<HTMLElement|null>;onToggle:()=>void;onNavigate:(page:PageId,session?:string)=>void
 onRoom:(id:string)=>void;onNew:()=>void;onNewGroup:()=>void;onClose:()=>void;resize:Record<string,unknown>
}

/** Sub4 conversation UX: history belongs to conversations; settings and
 * service/MCP/persona management open in their own workspace. */
export function WorkspaceNavigation({mode,page,session,selectedRoom,open,mobile,online,panel,onToggle,onNavigate,onRoom,onNew,onNewGroup,onClose,resize}:Props){
 const en=useLocale()==='en'
 const tr=(ko:string,english:string)=>en?english:ko
 const sessions=useRemote<ChatSession[]>('/api/sessions',[])
 const rooms=useRemote<Room[]>('/api/rooms',[])
 const [query,setQuery]=useState('')
 const navigate=(target:PageId,id?:string)=>{onNavigate(target,id);if(mobile)onClose()}
 const create=()=>{if(mode==='rooms')onNewGroup();else onNew();if(mobile)onClose()}
 const needle=query.trim().toLocaleLowerCase()
 const roomMatches=rooms.value.filter(room=>room.name.toLocaleLowerCase().includes(needle))
 const history=useMemo(()=>{
  const now=new Date();now.setHours(0,0,0,0)
  const week=now.getTime()-7*86400000
  const bins:[string,ChatSession[]][]=[
   [tr('오늘','Today'),[]],[tr('지난 7일','Last 7 days'),[]],[tr('이전 대화','Earlier'),[]]
  ]
  for(const item of sessions.value){
   if(item.roomId||!(item.title||'').toLocaleLowerCase().includes(needle))continue
   const updated=new Date(item.updatedAt||item.createdAt||'').getTime()
   bins[updated>=now.getTime()?0:updated>=week?1:2][1].push(item)
  }
  return bins.filter(([,items])=>items.length)
 },[sessions.value,needle,en])
 const itemClass='flex min-h-10 w-full min-w-0 items-center gap-2.5 rounded-lg px-3 py-2 text-left text-[12px] text-[#706456] transition-colors hover:bg-[#EAE3D8] aria-[current=page]:bg-[#E9DFD0] aria-[current=page]:font-semibold aria-[current=page]:text-[#44382E]'
 const selectMode=(next:'chat'|'rooms')=>navigate(next)
 return <>
  {mobile&&open&&<Button variant="bare" className="fixed inset-0 z-80 !rounded-none !bg-ink/35" aria-label={tr('메뉴 닫기','Close menu')} onClick={onClose}/>}
  <aside ref={panel} id="workspace-navigation" role={mobile&&open?'dialog':undefined} aria-modal={mobile&&open?true:undefined}
   tabIndex={-1} aria-label={tr('대화 목록','Conversation history')}
   className={cn('workspace-navigation relative flex min-h-0 shrink-0 flex-col border-r border-divider bg-[#F4F1EA]',
    mobile?(open?'fixed inset-y-0 left-0 z-81 w-[min(19rem,86vw)] shadow-2xl':'hidden'):'w-[var(--workspace-navigation-width)]',
    !open&&'items-center')} data-open={open} data-mobile={mobile}>
   <div className={cn('flex min-h-[76px] shrink-0 items-center gap-2.5',open?'px-5':'justify-center px-2')}>
    {open&&<><span className="grid size-8 shrink-0 place-items-center rounded-[10px] border border-[#C9644230] bg-[#F5E5DA] text-lg font-extrabold text-brand">D</span>
     <strong className="min-w-0 flex-1 truncate text-[21px] font-semibold tracking-[-.04em] text-[#44382E]">dominion</strong></>}
    <Button variant="ghost" size="icon" className="!shrink-0" title={open?tr('메뉴 접기','Collapse'):tr('메뉴 펼치기','Expand')}
     aria-label={open?tr('메뉴 접기','Collapse navigation'):tr('메뉴 펼치기','Expand navigation')}
     aria-expanded={open} aria-controls="workspace-navigation" onClick={onToggle}>
     {open?<PanelLeftClose size={18}/>:<PanelLeftOpen size={19}/>}
    </Button>
   </div>
   <Button variant="primary" title={mode==='chat'?tr('새 대화','New chat'):tr('새 그룹 만들기','New group')}
    aria-label={mode==='chat'?tr('새 대화','New chat'):tr('새 그룹 만들기','New group')} onClick={create}
    className={cn('!min-h-11 !shrink-0 !justify-start !gap-2.5 !rounded-xl !text-sm',open?'!mx-4 !mb-4 !px-3':'!mx-1 !mb-4 !size-11 !justify-center !p-0')}>
    <Plus size={19}/>{open&&<span>{mode==='chat'?tr('새 대화','New chat'):tr('새 그룹 만들기','New group')}</span>}
   </Button>
   <nav className={cn('flex shrink-0 gap-1',open?'mx-4 mb-4 rounded-xl bg-[#EBE5DB] p-1':'flex-col px-1')} aria-label={tr('대화 전환','Conversation switch')}>
    {([{id:'chat',label:tr('대화','Chat'),icon:MessageSquare},{id:'rooms',label:tr('그룹','Groups'),icon:UsersRound}] as const).map(({id,label,icon:Icon})=>
     <Button key={id} variant="bare" onClick={()=>selectMode(id)} aria-current={mode===id?'page':undefined}
      title={label} aria-label={label} className={cn('!min-h-9 !rounded-lg !text-[12px]',open?'!flex-1 !gap-2':'!size-11 !justify-center !p-0',
       mode===id?'!bg-surface !font-semibold !text-brand-hover !shadow-sm':'!text-[#82776A] hover:!bg-[#E1D7C9]')}>
      <Icon size={17}/>{open&&<span>{label}</span>}
     </Button>)}
   </nav>
   {open&&<label className="mx-4 mb-3 flex min-w-0 shrink-0 items-center gap-2 rounded-lg border border-[#E1DACD] bg-[#FBF9F4] px-2.5 text-muted focus-within:ring-2 focus-within:ring-brand/30">
    <Search size={15} className="shrink-0"/>
    <TextInput value={query} onChange={event=>setQuery(event.target.value)} placeholder={mode==='chat'?tr('대화 검색','Search chats'):tr('그룹 검색','Search groups')}
     aria-label={tr('대화·그룹 검색','Search chats and groups')} className="!min-h-9 !min-w-0 !flex-1 !border-0 !bg-transparent !px-0 !text-xs !shadow-none focus:!ring-0"/>
    {query&&<Button variant="bare" size="icon" onClick={()=>setQuery('')} aria-label={tr('검색 지우기','Clear search')}><X size={14}/></Button>}
   </label>}
   <nav className="min-h-0 flex-1 overflow-y-auto overscroll-contain px-2 pb-5" aria-label={mode==='chat'?tr('최근 대화','Recent chats'):tr('그룹 목록','Groups')}>
    {open&&(mode==='chat'?history.length>0?history.map(([label,items])=><section key={label} className="mb-5">
     <h2 className="mb-2 px-3 text-[11px] font-medium text-[#9B8E7E]">{label}</h2>
     {items.map(item=><Button key={item.id} variant="bare" className={itemClass} aria-current={page==='chat'&&session===item.id?'page':undefined}
      title={item.title} onClick={()=>navigate('chat',item.id)}><MessageSquare size={15} className="shrink-0 opacity-70"/><span className="min-w-0 flex-1 truncate">{item.title||tr('새 대화','New chat')}</span><ArrowRight size={13} className="shrink-0 opacity-50"/></Button>)}
    </section>):null:
     <section><h2 className="mb-2 px-3 text-[11px] font-medium text-[#9B8E7E]">{tr('그룹 목록','Groups')}</h2>
      {roomMatches.map(room=><Button key={room.id} variant="bare" className={itemClass} aria-current={page==='rooms'&&room.id===selectedRoom?'page':undefined}
       onClick={()=>{onRoom(room.id);if(mobile)onClose()}}><UsersRound size={15} className="shrink-0 opacity-70"/><span className="min-w-0 flex-1 truncate">{room.name}</span>{room.status==='running'&&<i className="size-1.5 rounded-full bg-success" aria-label={tr('답변 중','Responding')}/>}</Button>)}
     </section>)}
    {open&&((mode==='chat'?sessions.loading:rooms.loading))&&<div role="status" className="flex items-center gap-2 px-3 py-4 text-xs text-muted"><LoaderCircle size={16} className="animate-spin"/>{tr('불러오는 중','Loading')}</div>}
    {open&&((mode==='chat'?sessions.error:rooms.error))&&<Button variant="ghost" onClick={()=>{void sessions.reload();void rooms.reload()}}>{tr('목록 다시 불러오기','Retry loading')}</Button>}
    {open&&!(mode==='chat'?sessions.loading:rooms.loading)&&!(mode==='chat'?history.length:roomMatches.length)&&<p className="px-3 py-5 text-center text-xs text-muted">{tr(query?'검색 결과가 없습니다.':'아직 내역이 없습니다.',query?'No matches':'Nothing here yet')}</p>}
   </nav>
   <footer className={cn('mt-auto shrink-0 border-t border-divider',open?'px-4 py-3':'flex flex-col items-center gap-2 px-1 py-3')}>
    <Button variant="bare" className={cn('!min-h-11 !gap-2.5 !rounded-lg !text-[13px] !text-[#706456] hover:!bg-[#EAE3D8]',open?'!w-full !justify-start !px-3':'!size-11 !justify-center !p-0')}
     onClick={()=>navigate('settings')} title={t('nav.settings')} aria-label={t('nav.settings')}>
     <Settings size={18}/>{open&&<span>{t('nav.settings')}</span>}
    </Button>
    {open&&<div className="mt-2 flex items-center justify-between gap-2 border-t border-divider pt-2"><OwnerProfileEditor compact/><span role="status" className="flex items-center gap-1.5 text-[10px] text-muted"><i className={'size-1.5 rounded-full '+(online?'bg-success':'bg-warning')}/>{online?tr('연결됨','Connected'):tr('연결 끊김','Offline')}</span></div>}
   </footer>
   {open&&!mobile&&<div {...resize} className="absolute inset-y-0 -right-0.5 z-20 w-1.5 cursor-col-resize hover:bg-brand/20" aria-label={tr('메뉴 너비 조절','Resize navigation')}/>}
  </aside>
 </>
}
