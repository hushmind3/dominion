import {useCallback, useMemo, useRef, useSyncExternalStore} from 'react'
import {remoteResource, type RemoteSnapshot} from '../services/remoteStore'

/** Components share reads while retaining explicit refresh and mutation APIs. */
export function useRemote<T>(endpoint: string | null, initial: T, refreshOnEvent = true) {
 const initialValue = useRef(initial)
 const empty = useRef<RemoteSnapshot<T>>({value: initialValue.current, loading: false, refreshing: false, error: ''})
 const resource = useMemo(() => endpoint ? remoteResource(endpoint, initialValue.current) : null, [endpoint])
 const subscribe = useCallback((listener: () => void) => resource ? resource.subscribe(listener, refreshOnEvent) : () => {}, [resource, refreshOnEvent])
 const getSnapshot = resource?.getSnapshot ?? (() => empty.current)
 const snapshot = useSyncExternalStore(subscribe, getSnapshot, getSnapshot)
 const reload = useCallback(async () => {await resource?.reload()}, [resource])
 const setValue = useCallback((next: T | ((previous: T) => T)) => {resource?.setValue(next)}, [resource])
 return {...snapshot, reload, setValue}
}
