import { useCallback, useEffect, useState } from 'react'
import { checkForUpdatedUi } from '../components/PageLoadBoundary'
import { isSettingsPage, type PageId } from '../workspacePages'

type ConversationMode = 'chat' | 'rooms'
type ChatRoute = {
  session?: string
  agent?: string
  provider?: string
  model?: string
}
type WorkspaceRoute =
  | { page: 'chat'; chat: ChatRoute }
  | { page: Exclude<PageId, 'chat'> }
type WorkspaceState = {
  page: PageId
  mode: ConversationMode
  visited: Record<ConversationMode, boolean>
  chatRoute: ChatRoute
}

function readWorkspaceRoute(): WorkspaceRoute {
  const [name, query] = window.location.hash.replace(/^#\/?/, '').split('?')
  if (name === 'agents') return {page:'prompts'}
  if (name === 'rooms' || isSettingsPage(name)) return { page: name }

  const params = new URLSearchParams(query)
  return {
    page: 'chat',
    chat: {
      session: params.get('session') || undefined,
      agent: params.get('agent') || undefined,
      provider: params.get('provider') || undefined,
      model: params.get('model') || undefined,
    },
  }
}

function applyRoute(previous: WorkspaceState, route: WorkspaceRoute): WorkspaceState {
  const { page } = route
  const mode = page === 'chat' || page === 'rooms' ? page : previous.mode
  return {
    page,
    mode,
    visited: { ...previous.visited, [mode]: true },
    chatRoute: route.page === 'chat' ? route.chat : previous.chatRoute,
  }
}

function initialWorkspace(): WorkspaceState {
  return applyRoute({
    page: 'chat',
    mode: 'chat',
    visited: { chat: false, rooms: false },
    chatRoute: {},
  }, readWorkspaceRoute())
}

export function useWorkspaceNavigation() {
  const [workspace, setWorkspace] = useState(initialWorkspace)
  const { chatRoute } = workspace

  const syncRoute = useCallback(() => {
    if (/^#\/agents(?:\?|$)/.test(window.location.hash)) {
      const params = new URLSearchParams(window.location.hash.split('?')[1])
      params.set('view', 'personas')
      window.history.replaceState(null, '', '#/prompts?' + params)
    }
    const route = readWorkspaceRoute()
    setWorkspace((previous) => applyRoute(previous, route))
  }, [])

  useEffect(() => {
    syncRoute()
    window.addEventListener('hashchange', syncRoute)
    window.addEventListener('popstate', syncRoute)
    return () => {
      window.removeEventListener('hashchange', syncRoute)
      window.removeEventListener('popstate', syncRoute)
    }
  }, [syncRoute])

  const navigateHash = useCallback((hash: string) => {
    if (window.location.hash !== hash) {
      window.history.pushState(null, '', hash)
      window.dispatchEvent(new HashChangeEvent('hashchange'))
    } else {
      syncRoute()
    }
    // Navigation is immediate while the updated UI check runs in the background.
    void checkForUpdatedUi()
  }, [syncRoute])

  const navigate = useCallback((page: PageId, sessionId?: string) => {
    if(page==='agents'){navigateHash('#/prompts?view=personas');return}
    const query = sessionId ? `?session=${encodeURIComponent(sessionId)}` : ''
    navigateHash(`#/${page}${query}`)
  }, [navigateHash])

  const switchMode = useCallback((mode: ConversationMode) => {
    if (mode === 'rooms') {
      navigate('rooms')
      return
    }

    const params = new URLSearchParams()
    for (const key of ['session', 'agent', 'provider', 'model'] as const) {
      const value = chatRoute[key]
      if (value) params.set(key, value)
    }
    navigateHash(params.size ? `#/chat?${params}` : '#/chat')
  }, [chatRoute, navigate, navigateHash])

  const selectChatSession = useCallback((session: string | undefined) => {
    if (readWorkspaceRoute().page === 'chat') {
      navigate('chat', session)
      return
    }

    // A session can finish opening while its chat is behind settings or rooms.
    setWorkspace((previous) => ({
      ...previous,
      chatRoute: { ...previous.chatRoute, session },
    }))
  }, [navigate])

  return { ...workspace, navigate, switchMode, selectChatSession }
}
