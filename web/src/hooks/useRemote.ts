import {useCallback, useEffect, useRef, useState, type SetStateAction} from 'react'
import {get} from '../api'
import {matchesRefreshTarget, type RefreshScope} from '../refreshEvents'

type RemoteState<T> = {endpoint: string | null; value: T; loading: boolean; refreshing: boolean; error: string}

/** Request lifetime follows the endpoint; visibility only controls subscription.
 * Returning to a kept-alive page refreshes it once without refetching on hide. */
export function useRemote<T>(endpoint: string | null, initial: T, refreshOnEvent = true) {
  const initialValue = useRef(initial)
  const [state, setState] = useState<RemoteState<T>>({endpoint, value: initial, loading: !!endpoint, refreshing: !!endpoint, error: ''})
  const active = useRef<AbortController | null>(null)
  const refreshPending = useRef(false)
  const subscribed = useRef(refreshOnEvent)
  const reload = useCallback(async () => {
    if (!endpoint) return
    active.current?.abort()
    const controller = new AbortController()
    active.current = controller
    setState(previous => previous.endpoint === endpoint ? {...previous, refreshing: true} : {
      endpoint, value: initialValue.current, loading: true, refreshing: true, error: '',
    })
    try {
      const value = await get<T>(endpoint, controller.signal)
      if (!controller.signal.aborted) setState({endpoint, value, loading: false, refreshing: false, error: ''})
    } catch (error) {
      if (!controller.signal.aborted) setState(previous => ({
        endpoint, value: previous.endpoint === endpoint ? previous.value : initialValue.current,
        loading: false, refreshing: false, error: error instanceof Error ? error.message : String(error),
      }))
    } finally {
      if (active.current === controller) {
        active.current = null
        if (refreshPending.current) {refreshPending.current = false; void reload()}
      }
    }
  }, [endpoint])

  useEffect(() => {
    void reload()
    return () => {
      refreshPending.current = false
      active.current?.abort()
      active.current = null
    }
  }, [reload])

  useEffect(() => {
    const resumed = !subscribed.current && refreshOnEvent
    subscribed.current = refreshOnEvent
    if (!endpoint || !refreshOnEvent) return
    if (resumed) void reload()
    const onRefresh = () => {
      if (active.current) refreshPending.current = true
      else void reload()
    }
    const onScopedRefresh = (event: Event) => {
      if (matchesRefreshTarget(endpoint, (event as CustomEvent<RefreshScope>).detail?.targets || [])) onRefresh()
    }
    window.addEventListener('dominion-refresh', onRefresh)
    window.addEventListener('dominion-data-change', onScopedRefresh)
    return () => {
      window.removeEventListener('dominion-refresh', onRefresh)
      window.removeEventListener('dominion-data-change', onScopedRefresh)
    }
  }, [endpoint, refreshOnEvent, reload])

  const setValue = useCallback((next: SetStateAction<T>) => {
    setState(previous => {
      const current = previous.endpoint === endpoint ? previous : {
        endpoint, value: initialValue.current, error: '', loading: !!endpoint, refreshing: !!endpoint,
      }
      return {...current, value: typeof next === 'function' ? (next as (old: T) => T)(current.value) : next}
    })
  }, [endpoint])
  const visible = state.endpoint === endpoint ? state : {
    value: initialValue.current, error: '', loading: !!endpoint, refreshing: !!endpoint,
  }
  return {value: visible.value, loading: visible.loading, refreshing: visible.refreshing, error: visible.error, setValue, reload}
}
