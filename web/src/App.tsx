import { DialogBackdrop } from './components/Dialog'
import { ContextMenu } from './components/ContextMenu'
import { OwnerProfileProvider } from './components/OwnerProfile'
import {
  lazy,
  Suspense,
  useCallback,
  useEffect,
  useState,
} from 'react'
import {
  PageLoadBoundary,
  checkForUpdatedUi,
} from './components/PageLoadBoundary'
import { settingsPages, isSettingsPage } from './workspacePages'
import { t, useLocale } from './i18n'
import { useEngineStatus } from './hooks/useEngineStatus'
import { useWorkspaceNavigation } from './hooks/useWorkspaceNavigation'
import { WorkspaceSidebar, SettingsHeader } from './components/WorkspaceShell'
const ChatPage = lazy(() =>
  import('./pages/Chat').then((module) => ({ default: module.ChatPage })),
)
const RoomsPage = lazy(() =>
  import('./pages/Rooms').then((module) => ({ default: module.RoomsPage })),
)
export default function App() {
  const { page, mode, visited, chatRoute, navigate, switchMode, selectChatSession } =
    useWorkspaceNavigation()
  useLocale()
  const [settingsNav, setSettingsNav] = useState(() => {
    try {
      return localStorage.getItem('dominion.settings.nav') !== '0'
    } catch {
      return true
    }
  })
  const toggleSettingsNav = useCallback(
    () =>
      setSettingsNav((open) => {
        const next = !open
        try {
          localStorage.setItem('dominion.settings.nav', next ? '1' : '0')
        } catch {}
        return next
      }),
    [],
  )
  const settingsOpen = isSettingsPage(page)
  const { online, running, pending, engineBuild } = useEngineStatus()
  const closeSettings = useCallback(() => switchMode(mode), [switchMode, mode])
  useEffect(() => {
    const check = () => {
      if (document.visibilityState === 'visible') void checkForUpdatedUi()
    }
    window.addEventListener('focus', check)
    document.addEventListener('visibilitychange', check)
    return () => {
      window.removeEventListener('focus', check)
      document.removeEventListener('visibilitychange', check)
    }
  }, [])
  useEffect(() => {
    const shortcuts = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === ',') {
        e.preventDefault()
        navigate('settings')
      }
    }
    window.addEventListener('keydown', shortcuts)
    return () => window.removeEventListener('keydown', shortcuts)
  }, [navigate])
  return (
    <OwnerProfileProvider>
      <div className="h-dvh overflow-hidden bg-surface font-sans text-ink">
        <ContextMenu />
        <main inert={settingsOpen} className="h-dvh min-w-0">
          {visited.chat && (
            <div hidden={mode !== 'chat'} className="h-full page-chat">
              <PageLoadBoundary>
                <Suspense
                  fallback={
                    <div role="status" className="p-8 text-sm text-muted">
                      {t('shell.loadingChat')}
                    </div>
                  }
                >
                  <ChatPage
                    active={mode === 'chat' && !settingsOpen}
                    sessionId={chatRoute.session}
                    initialAgent={chatRoute.agent}
                    initialProvider={chatRoute.provider}
                    initialModel={chatRoute.model}
                    onSession={selectChatSession}
                    openModels={() => navigate('models')}
                    openSettings={() => navigate('settings')}
                    switchMode={switchMode}
                  />
                </Suspense>
              </PageLoadBoundary>
            </div>
          )}
          {visited.rooms && (
            <div hidden={mode !== 'rooms'} className="h-full page-rooms">
              <PageLoadBoundary>
                <Suspense
                  fallback={
                    <div role="status" className="p-8 text-sm text-muted">
                      {t('shell.loadingChat')}
                    </div>
                  }
                >
                  <RoomsPage
                    active={mode === 'rooms' && !settingsOpen}
                    openSession={(id) => navigate('chat', id)}
                    openSettings={() => navigate('settings')}
                    switchMode={switchMode}
                  />
                </Suspense>
              </PageLoadBoundary>
            </div>
          )}
        </main>
        {settingsOpen && (
          <DialogBackdrop onDismiss={closeSettings}>
            <div
              role="dialog"
              aria-modal="true"
              aria-label={t('nav.settings')}
              className="workspace-settings relative flex h-[calc(100dvh-40px)] w-full max-w-[1440px] overflow-hidden rounded-2xl border border-divider bg-canvas shadow-2xl"
            >
              <WorkspaceSidebar
                page={page}
                online={online}
                running={running}
                pending={pending}
                onNavigate={navigate}
                navOpen={settingsNav}
                onToggleNav={toggleSettingsNav}
              />
              <div className="flex h-full min-w-0 flex-1 flex-col">
                <SettingsHeader
                  onNavigate={navigate}
                  page={page}
                  online={online}
                  build={engineBuild}
                  onClose={closeSettings}
                />
                <div className="workspace-settings-content min-h-0 flex-1 overflow-y-auto">
                  <PageLoadBoundary key={page}>
                    <Suspense
                      fallback={
                        <div role="status" className="p-8 text-sm text-muted">
                          {t('shell.loadingPage')}
                        </div>
                      }
                    >
                      {settingsPages[page].render(navigate)}
                    </Suspense>
                  </PageLoadBoundary>
                </div>
              </div>
            </div>
          </DialogBackdrop>
        )}
      </div>
    </OwnerProfileProvider>
  )
}
