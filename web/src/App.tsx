import {lazy, useCallback, useEffect, useState} from 'react'
import {ContextMenu} from './components/ContextMenu'
import {OwnerProfileProvider} from './components/OwnerProfile'
import {AsyncPage} from './components/AsyncPage'
import {SettingsWorkspace} from './components/SettingsWorkspace'
import {checkForUpdatedUi} from './components/PageLoadBoundary'
import {isSettingsPage} from './workspacePages'
import {t, useLocale} from './i18n'
import {useEngineStatus} from './hooks/useEngineStatus'
import {useWorkspaceNavigation} from './hooks/useWorkspaceNavigation'

const ChatPage = lazy(() => import('./pages/Chat').then(module => ({default: module.ChatPage})))
const RoomsPage = lazy(() => import('./pages/Rooms').then(module => ({default: module.RoomsPage})))

export default function App() {
  const {page, mode, visited, chatRoute, navigate, switchMode, selectChatSession} = useWorkspaceNavigation()
  useLocale()
  const [conversationSettingsTarget,setConversationSettingsTarget]=useState<HTMLDivElement|null>(null)
  const settingsOpen = isSettingsPage(page)
  const {online, connecting, running, pending} = useEngineStatus()
  const closeSettings = useCallback(() => switchMode(mode), [switchMode, mode])

  useEffect(() => {
    const check = () => {if (document.visibilityState === 'visible') void checkForUpdatedUi()}
    window.addEventListener('focus', check)
    document.addEventListener('visibilitychange', check)
    return () => {
      window.removeEventListener('focus', check)
      document.removeEventListener('visibilitychange', check)
    }
  }, [])
  useEffect(() => {
    const shortcuts = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key === ',') {
        event.preventDefault()
        navigate('settings')
      }
    }
    window.addEventListener('keydown', shortcuts)
    return () => window.removeEventListener('keydown', shortcuts)
  }, [navigate])

  return <OwnerProfileProvider>
    <div className="h-dvh overflow-hidden bg-surface font-sans text-ink">
      <ContextMenu/>
      <main inert={settingsOpen} className="h-dvh min-w-0">
        {visited.chat && <div hidden={mode !== 'chat'} className="h-full page-chat">
          <AsyncPage label={t('shell.loadingChat')}><ChatPage settingsTarget={mode==='chat'?conversationSettingsTarget:null} active={mode === 'chat' && !settingsOpen}
            sessionId={chatRoute.session} initialAgent={chatRoute.agent} initialProvider={chatRoute.provider} initialModel={chatRoute.model}
            onSession={selectChatSession} openModels={() => navigate('models')} openSettings={() => navigate('settings')} switchMode={switchMode}/></AsyncPage>
        </div>}
        {visited.rooms && <div hidden={mode !== 'rooms'} className="h-full page-rooms">
          <AsyncPage label={t('shell.loadingChat')}><RoomsPage settingsTarget={mode==='rooms'?conversationSettingsTarget:null} active={mode === 'rooms' && !settingsOpen}
            openSession={id => navigate('chat', id)} openSettings={() => navigate('settings')} switchMode={switchMode}/></AsyncPage>
        </div>}
      </main>
      {isSettingsPage(page) && <SettingsWorkspace conversationSettingsRef={setConversationSettingsTarget} page={page} online={online} connecting={connecting} running={running} pending={pending} onNavigate={navigate} onClose={closeSettings}/>}
    </div>
  </OwnerProfileProvider>
}
