import {lazy, useCallback, useEffect, useState, type CSSProperties} from 'react'
import {Menu} from 'lucide-react'
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

const ChatPage=lazy(()=>import('./pages/Chat').then(module=>({default:module.ChatPage})))
const RoomsPage=lazy(()=>import('./pages/Rooms').then(module=>({default:module.RoomsPage})))

export default function App() {
 const {page,mode,visited,chatRoute,navigate,switchMode,selectChatSession}=useWorkspaceNavigation()
 useLocale()
 const [conversationSettingsTarget,setConversationSettingsTarget]=useState<HTMLDivElement|null>(null)
 const [newRoomRevision,setNewRoomRevision]=useState(0)
 const [newConversationRevision,setNewConversationRevision]=useState(0)
 const [roomSelection,setRoomSelection]=useState(()=>{try{return localStorage.getItem('dominion.room.selected')||''}catch{return ''}})
 const workspacePage=isSettingsPage(page)
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
  <main className="relative min-h-0 min-w-0 flex-1 overflow-hidden" inert={navigation.mobile&&navigation.open} aria-hidden={navigation.mobile&&navigation.open||undefined}>
   {visited.chat&&<div hidden={page!=='chat'} className="h-full page-chat"><AsyncPage label={t('shell.loadingChat')}><ChatPage newConversationRevision={newConversationRevision} onOpenNavigation={openNavigation} settingsTarget={mode==='chat'?conversationSettingsTarget:null} active={page==='chat'}
    sessionId={chatRoute.session} initialAgent={chatRoute.agent} initialProvider={chatRoute.provider} initialModel={chatRoute.model}
    onSession={selectChatSession} openModels={()=>navigate('models')} openSettings={()=>navigate('settings')} openExtensions={()=>navigate('extensions')} switchMode={switchMode}/></AsyncPage></div>}
   {visited.rooms&&<div hidden={page!=='rooms'} className="h-full page-rooms"><AsyncPage label={t('shell.loadingChat')}><RoomsPage newRoomRevision={newRoomRevision} onOpenNavigation={openNavigation} selectedRoom={roomSelection} onSelectRoom={setRoomSelection} settingsTarget={mode==='rooms'?conversationSettingsTarget:null} active={page==='rooms'}
    openSession={id=>navigate('chat',id)}/></AsyncPage></div>}
   {workspacePage&&<WorkspacePage conversationSettingsRef={setConversationSettingsTarget} page={page} mode={mode} onNavigate={navigate}/>}
   {navigation.mobile&&!navigation.open&&workspacePage&&<Button className="absolute bottom-5 left-4 z-30 !size-11 !p-0 shadow-lg" variant="secondary" onClick={openNavigation} aria-label={t('shell.showMenu')}><Menu size={20}/></Button>}
  </main>
 </div></OwnerProfileProvider>
}
