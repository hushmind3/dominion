import {lazy, useCallback, useEffect, useState, type CSSProperties} from 'react'
import {Menu,MessageSquare,UsersRound,SlidersHorizontal,X,Pencil,Plus} from 'lucide-react'
import {ContextMenu} from './components/ContextMenu'
import {OwnerProfileProvider} from './components/OwnerProfile'
import {AsyncPage} from './components/AsyncPage'
import {WorkspacePage} from './components/WorkspacePage'
import {WorkspaceNavigation} from './components/WorkspaceNavigation'
import {Button,TextInput} from './components/Primitives'
import {DialogBackdrop,DialogPanel} from './components/Dialog'
import {post} from './api'
import {checkForUpdatedUi} from './components/PageLoadBoundary'
import {isSettingsPage} from './workspacePages'
import {t, useLocale} from './i18n'
import {useEngineStatus} from './hooks/useEngineStatus'
import {useWorkspaceNavigation} from './hooks/useWorkspaceNavigation'
import {useConversationSidebar} from './hooks/useConversationSidebar'
import {useRemote} from './hooks/useRemote'
import type {ChatSession,Room} from './api'

const ChatPage=lazy(()=>import('./pages/Chat').then(module=>({default:module.ChatPage})))
const RoomsPage=lazy(()=>import('./pages/Rooms').then(module=>({default:module.RoomsPage})))

export default function App() {
 const {page,mode,visited,chatRoute,navigate,switchMode,selectChatSession}=useWorkspaceNavigation()
 const en=useLocale()==='en'
 const tr=(ko:string,english:string)=>en?english:ko
 const [conversationSettingsTarget,setConversationSettingsTarget]=useState<HTMLDivElement|null>(null)
 const [quickTarget,setQuickTarget]=useState<HTMLDivElement|null>(null)
 const [quickScope,setQuickScope]=useState<string|null>(null)
 const [rename,setRename]=useState<{id:string;title:string}|null>(null)
 const [renaming,setRenaming]=useState(false)
 const [renameError,setRenameError]=useState('')
 const [newRoomRevision,setNewRoomRevision]=useState(0)
 const [newConversationRevision,setNewConversationRevision]=useState(0)
 const [roomSelection,setRoomSelection]=useState(()=>{try{return localStorage.getItem('dominion.room.selected')||''}catch{return ''}})
 const workspacePage=isSettingsPage(page)
 const sessions=useRemote<ChatSession[]>(page==='chat'?'/api/sessions':null,[])
 const rooms=useRemote<Room[]>(page==='rooms'?'/api/rooms':null,[])
 const activeSession=sessions.value.find(item=>item.id===chatRoute.session)
 const activeRoom=rooms.value.find(item=>item.id===roomSelection)
 const conversationTitle=page==='rooms'?(activeRoom?.name||tr('그룹 대화','Group conversation')):(activeSession?.title||tr('새 대화','New conversation'))
 const conversationSubtitle=page==='rooms'?(activeRoom?tr('AI '+activeRoom.agentIds.length+'명 참여',activeRoom.agentIds.length+' AI participants'):tr('여러 AI와 협업','Collaborate with AIs')):(activeSession?.model||tr('개인 대화','Direct conversation'))
 const scope=page==='chat'?'chat:'+String(chatRoute.session||'new'):page==='rooms'?'rooms:'+roomSelection:''
 const quickOpen=Boolean(scope)&&quickScope===scope
 const settingsTarget=quickOpen?quickTarget:conversationSettingsTarget
 useEffect(()=>{setQuickScope(null);setRename(null);setRenameError('')},[scope])
 const saveTitle=async()=>{
  if(!rename||renaming||!rename.title.trim())return
  setRenaming(true);setRenameError('')
  try{await post('/api/sessions/'+encodeURIComponent(rename.id)+'/rename',{title:rename.title.trim()});await sessions.reload();setRename(null)}
  catch(error){setRenameError((error as Error).message)}finally{setRenaming(false)}
 }
 const {online}=useEngineStatus()
 const navigation=useConversationSidebar('dominion.sub3.navigation',true)
 const openNavigation=useCallback(()=>navigation.setOpen(true),[navigation.setOpen])
 const openRoom=(id:string)=>{setRoomSelection(id);try{localStorage.setItem('dominion.room.selected',id)}catch{};navigate('rooms')}
 useEffect(()=>{
  const check=()=>{if(document.visibilityState==='visible')void checkForUpdatedUi()}
  window.addEventListener('focus',check);document.addEventListener('visibilitychange',check)
  return()=>{window.removeEventListener('focus',check);document.removeEventListener('visibilitychange',check)}
 },[])
 useEffect(()=>{
  const shortcuts=(event:KeyboardEvent)=>{if((event.metaKey||event.ctrlKey)&&event.key===','){event.preventDefault();navigate('settings')}}
  window.addEventListener('keydown',shortcuts);return()=>window.removeEventListener('keydown',shortcuts)
 },[navigate])
 return <OwnerProfileProvider><div className="sub3-workspace flex h-dvh min-h-0 overflow-hidden bg-surface text-ink" style={{'--workspace-navigation-width':navigation.open?navigation.width+'px':'64px'} as CSSProperties}>
  <ContextMenu/>
  <WorkspaceNavigation page={page} session={chatRoute.session} selectedRoom={roomSelection} open={navigation.open} mobile={navigation.mobile} online={online} panel={navigation.panel} resize={navigation.resize}
   onToggle={()=>navigation.setOpen(value=>!value)} onNavigate={(target,id)=>target==='chat'&&!id?switchMode('chat'):navigate(target,id)} onRoom={openRoom} onNew={()=>{setNewConversationRevision(value=>value+1);navigate('chat')}} onNewGroup={()=>{setNewRoomRevision(value=>value+1);navigate('rooms')}} onClose={()=>navigation.setOpen(false)}/>
  <main className="relative flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden" inert={navigation.mobile&&navigation.open} aria-hidden={navigation.mobile&&navigation.open||undefined}>
   {(page==='chat'||page==='rooms')&&<header className="conversation-topbar flex min-h-14 shrink-0 items-center gap-3 border-b border-divider bg-surface px-3 sm:px-6" aria-label={tr('현재 대화','Current conversation')}>
    {navigation.mobile&&<Button variant="ghost" size="icon" onClick={openNavigation} aria-label={t('shell.showMenu')}><Menu size={20}/></Button>}
    <span className="hidden size-9 shrink-0 place-items-center rounded-xl bg-canvas text-muted sm:grid">{page==='rooms'?<UsersRound size={19}/>:<MessageSquare size={19}/>}</span>
    <div className="min-w-0 flex-1"><div className="truncate text-sm font-semibold text-ink" title={conversationTitle}>{conversationTitle}</div><div className="truncate text-xs text-muted">{conversationSubtitle}</div></div>
    {page==='chat'&&activeSession&&<Button variant="ghost" size="icon" className="!size-9" title={tr('대화 이름 변경','Rename conversation')} aria-label={tr('대화 이름 변경','Rename conversation')} onClick={()=>{setRenameError('');setRename({id:activeSession.id,title:activeSession.title||''})}}><Pencil size={16}/></Button>}
    {page==='rooms'&&<Button variant="ghost" size="icon" title={tr('새 그룹 만들기','Create a group')} aria-label={tr('새 그룹 만들기','Create a group')} onClick={()=>setNewRoomRevision(n=>n+1)}><Plus size={18}/></Button>}
    <Button variant="ghost" size="sm" disabled={page==='rooms'&&!activeRoom} onClick={()=>setQuickScope(scope)} aria-haspopup="dialog" aria-expanded={quickOpen} aria-label={tr('답변 설정','Reply settings')}><SlidersHorizontal size={17}/><span className="hidden sm:inline">{tr('답변 설정','Reply settings')}</span></Button>
   </header>}
   {visited.chat&&<div hidden={page!=='chat'} className="min-h-0 flex-1 page-chat"><AsyncPage label={t('shell.loadingChat')}><ChatPage newConversationRevision={newConversationRevision} onOpenNavigation={openNavigation} settingsTarget={mode==='chat'?settingsTarget:null} active={page==='chat'}
    sessionId={chatRoute.session} initialAgent={chatRoute.agent} initialProvider={chatRoute.provider} initialModel={chatRoute.model}
    onSession={selectChatSession} openModels={()=>navigate('models')} openSettings={()=>navigate('settings')} openExtensions={()=>navigate('extensions')} switchMode={switchMode}/></AsyncPage></div>}
   {visited.rooms&&<div hidden={page!=='rooms'} className="min-h-0 flex-1 page-rooms"><AsyncPage label={t('shell.loadingChat')}><RoomsPage newRoomRevision={newRoomRevision} onOpenNavigation={openNavigation} selectedRoom={roomSelection} onSelectRoom={setRoomSelection} settingsTarget={mode==='rooms'?settingsTarget:null} active={page==='rooms'}
    openSession={id=>navigate('chat',id)}/></AsyncPage></div>}
   {workspacePage&&<WorkspacePage conversationSettingsRef={setConversationSettingsTarget} page={page} mode={mode} onNavigate={navigate} onOpenNavigation={navigation.mobile?openNavigation:undefined}/>}
  </main>
  {quickOpen&&<DialogBackdrop onDismiss={()=>setQuickScope(null)}><DialogPanel aria-label={tr('답변 설정','Reply settings')} className="!max-w-lg"><header className="sticky top-0 z-10 flex items-center justify-between gap-3 border-b border-divider bg-surface px-4 py-3"><div className="min-w-0"><h2 className="text-base font-semibold">{tr('답변 설정','Reply settings')}</h2><p className="truncate text-xs text-muted">{conversationTitle}</p></div><Button variant="ghost" size="icon" onClick={()=>setQuickScope(null)} aria-label={tr('닫기','Close')}><X size={19}/></Button></header><div className="p-2"><div ref={setQuickTarget} className="quick-reply-settings peer"/><p className="hidden px-4 py-6 text-sm text-muted peer-empty:block" role="status">{tr('답변 설정을 불러오는 중…','Loading reply controls…')}</p></div><footer className="sticky bottom-0 border-t border-divider bg-surface p-3"><Button variant="primary" className="w-full" onClick={()=>setQuickScope(null)}>{tr('대화로 돌아가기','Back to conversation')}</Button></footer></DialogPanel></DialogBackdrop>}
  {rename&&scope==='chat:'+rename.id&&<DialogBackdrop onDismiss={()=>{if(!renaming)setRename(null)}}><DialogPanel aria-label={tr('대화 이름 변경','Rename conversation')} className="!max-w-sm"><form className="grid gap-4 p-5" onSubmit={event=>{event.preventDefault();void saveTitle()}}><h2 className="text-lg font-semibold">{tr('대화 이름 변경','Rename conversation')}</h2><TextInput autoFocus aria-label={tr('대화 제목','Conversation title')} value={rename.title} maxLength={200} disabled={renaming} onChange={event=>setRename({...rename,title:event.target.value})}/>{renameError&&<p role="alert" className="text-sm text-danger">{renameError}</p>}<div className="flex justify-end gap-2"><Button variant="ghost" disabled={renaming} onClick={()=>setRename(null)}>{tr('취소','Cancel')}</Button><Button type="submit" variant="primary" disabled={renaming||!rename.title.trim()}>{tr('저장','Save')}</Button></div></form></DialogPanel></DialogBackdrop>}
 </div></OwnerProfileProvider>
}
