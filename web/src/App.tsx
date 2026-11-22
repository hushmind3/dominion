import { DialogBackdrop } from './components/Dialog'
import { ContextMenu } from './components/ContextMenu'
import { OwnerProfileProvider } from './components/OwnerProfile'
import {
  lazy,
  Suspense,
  useCallback,
  useEffect,
  useLayoutEffect,
  useState,
} from 'react'
import {
  PageLoadBoundary,
  checkForUpdatedUi,
} from './components/PageLoadBoundary'
import { settingsPages, isSettingsPage, type PageId } from './workspacePages'
import { t, useLocale } from './i18n'
import { useEngineStatus } from './hooks/useEngineStatus'
import { WorkspaceSidebar, SettingsHeader } from './components/WorkspaceShell'
const ChatPage = lazy(() =>
  import('./pages/Chat').then((module) => ({ default: module.ChatPage })),
)
const RoomsPage = lazy(() =>
  import('./pages/Rooms').then((module) => ({ default: module.RoomsPage })),
)
function locationState() {
  const raw = window.location.hash.replace(/^#\/?/, '')
  const [name, query] = raw.split('?')
  const valid = name === 'chat' || name === 'rooms' || isSettingsPage(name)
  const params = new URLSearchParams(query)
  return {
    page: (valid ? name : 'chat') as PageId,
    session: params.get('session') || undefined,
    agent: params.get('agent') || undefined,
    provider: params.get('provider') || undefined,
    model: params.get('model') || undefined,
  }
}
export default function App() {
  const [{ page, session, agent, provider, model }, setRoute] =
    useState(locationState)
  const [chatRoute, setChatRoute] = useState(() => {
    const route = locationState()
    return route.page === 'chat'
      ? route
      : {
          page: 'chat' as PageId,
          session: undefined,
          agent: undefined,
          provider: undefined,
          model: undefined,
        }
  })
  const [mode, setMode] = useState<'chat' | 'rooms'>(() =>
    locationState().page === 'rooms' ? 'rooms' : 'chat',
  )
  const [visited, setVisited] = useState(() => ({
    chat: locationState().page !== 'rooms',
    rooms: locationState().page === 'rooms',
  }))
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
  const navigate = useCallback((target: PageId, sessionId?: string) => {
    const next =
      '#/' +
      target +
      (sessionId ? '?session=' + encodeURIComponent(sessionId) : '')
    if (window.location.hash !== next) window.history.pushState(null, '', next)
    setRoute(locationState())
    // Hash navigation stays immediate. Reload only when the server's built
    // entry changed; never block menu clicks on a network request.
    void checkForUpdatedUi()
  }, [])
  useEffect(() => {
    const sync = () => setRoute(locationState())
    window.addEventListener('hashchange', sync)
    window.addEventListener('popstate', sync)
    return () => {
      window.removeEventListener('hashchange', sync)
      window.removeEventListener('popstate', sync)
    }
  }, [])
  useLayoutEffect(() => {
    if (page === 'chat') {
      setChatRoute({ page, session, agent, provider, model })
      setMode('chat')
      setVisited((previous) =>
        previous.chat ? previous : { ...previous, chat: true },
      )
    }
    if (page === 'rooms') {
      setMode('rooms')
      setVisited((previous) =>
        previous.rooms ? previous : { ...previous, rooms: true },
      )
    }
  }, [page, session, agent, provider, model])
  const returnToChat = useCallback(() => {
    const params = new URLSearchParams()
    for (const key of ['session', 'agent', 'provider', 'model'] as const)
      if (chatRoute[key]) params.set(key, chatRoute[key]!)
    const next = '#/chat' + (params.size ? '?' + params.toString() : '')
    if (window.location.hash !== next) window.history.pushState(null, '', next)
    setRoute(locationState())
    void checkForUpdatedUi()
  }, [chatRoute])
  const switchMode = useCallback(
    (next: 'chat' | 'rooms') => {
      if (next === 'chat') returnToChat()
      else navigate('rooms')
    },
    [returnToChat, navigate],
  )
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
                    onSession={(id) => {
                      setChatRoute((previous) => ({ ...previous, session: id }))
                      if (locationState().page === 'chat') navigate('chat', id)
                    }}
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
