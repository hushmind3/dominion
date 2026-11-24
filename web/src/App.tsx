import {lazy, useCallback, useEffect, useState, type CSSProperties} from 'react'
import {Menu,MessageSquare,UsersRound,SlidersHorizontal} from 'lucide-react'
import {ContextMenu} from './components/ContextMenu'
import {OwnerProfileProvider} from './components/OwnerProfile'
import {AsyncPage} from './components/AsyncPage'
import {WorkspacePage} from './components/WorkspacePage'
import {WorkspaceNavigation} from './components/WorkspaceNavigation'
import {Button} from './components/Primitives'
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
    <Button variant="ghost" size="sm" onClick={()=>{window.location.hash='#/settings?view=reply'}} aria-label={tr('답변 설정','Reply settings')}><SlidersHorizontal size={17}/><span className="hidden sm:inline">{tr('답변 설정','Reply settings')}</span></Button>
   </header>}
   {visited.chat&&<div hidden={page!=='chat'} className="min-h-0 flex-1 page-chat"><AsyncPage label={t('shell.loadingChat')}><ChatPage newConversationRevision={newConversationRevision} onOpenNavigation={openNavigation} settingsTarget={mode==='chat'?conversationSettingsTarget:null} active={page==='chat'}
    sessionId={chatRoute.session} initialAgent={chatRoute.agent} initialProvider={chatRoute.provider} initialModel={chatRoute.model}
    onSession={selectChatSession} openModels={()=>navigate('models')} openSettings={()=>navigate('settings')} openExtensions={()=>navigate('extensions')} switchMode={switchMode}/></AsyncPage></div>}
   {visited.rooms&&<div hidden={page!=='rooms'} className="min-h-0 flex-1 page-rooms"><AsyncPage label={t('shell.loadingChat')}><RoomsPage newRoomRevision={newRoomRevision} onOpenNavigation={openNavigation} selectedRoom={roomSelection} onSelectRoom={setRoomSelection} settingsTarget={mode==='rooms'?conversationSettingsTarget:null} active={page==='rooms'}
    openSession={id=>navigate('chat',id)}/></AsyncPage></div>}
   {workspacePage&&<WorkspacePage conversationSettingsRef={setConversationSettingsTarget} page={page} mode={mode} onNavigate={navigate} onOpenNavigation={navigation.mobile?openNavigation:undefined}/>}
  </main>
 </div></OwnerProfileProvider>
}
