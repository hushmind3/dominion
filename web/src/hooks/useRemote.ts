import {useCallback, useEffect, useRef, useState, type SetStateAction} from 'react'
import {get} from '../api'
import {matchesRefreshTarget, type RefreshScope} from '../refreshEvents'

export function useRemote<T>(endpoint: string | null, initial: T, refreshOnEvent = true) {
 const initialValue = useRef(initial)
 const [state, setState] = useState({endpoint, value: initial, loading: !!endpoint, error: ''})
 const active = useRef<AbortController | null>(null)
 const refreshPending = useRef(false)
 const reload = useCallback(async () => {
  if (!endpoint) return
  active.current?.abort()
  const controller = new AbortController()
  active.current = controller
  try {
   const value = await get<T>(endpoint, controller.signal)
   if (!controller.signal.aborted) setState({endpoint, value, loading: false, error: ''})
  } catch (e) {
   if (!controller.signal.aborted) setState(previous => ({
    endpoint, value: previous.endpoint === endpoint ? previous.value : initialValue.current,
    loading: false, error: (e as Error).message,
   }))
  } finally {
   if (active.current === controller) {
    active.current = null
    if (refreshPending.current) {
     refreshPending.current = false
     void reload()
    }
   }
  }
 }, [endpoint])
 useEffect(() => {
  void reload()
  const onRefresh = () => {
   if (active.current) refreshPending.current = true
   else void reload()
  }
  const onScopedRefresh = (event:Event) => {
   const targets=(event as CustomEvent<RefreshScope>).detail?.targets||[]
   if (matchesRefreshTarget(endpoint||undefined,targets)) onRefresh()
  }
  if (endpoint && refreshOnEvent) {
   window.addEventListener('dominion-refresh', onRefresh)
   window.addEventListener('dominion-data-change',onScopedRefresh)
  }
  return () => {
   refreshPending.current = false
   active.current?.abort()
   active.current = null
   window.removeEventListener('dominion-refresh', onRefresh)
   window.removeEventListener('dominion-data-change',onScopedRefresh)
  }
 }, [endpoint, reload, refreshOnEvent])
 const setValue = useCallback((next: SetStateAction<T>) => {
  setState(previous => {
   const state = previous.endpoint === endpoint ? previous : {
    endpoint, value: initialValue.current, error: '', loading: !!endpoint,
   }
   return {...state, value: typeof next === 'function' ? (next as (old: T) => T)(state.value) : next}
  })
 }, [endpoint])
 const visible = state.endpoint === endpoint ? state : {
  value: initialValue.current, error: '', loading: !!endpoint,
 }
 return {value: visible.value, loading: visible.loading, error: visible.error, setValue, reload}
}
