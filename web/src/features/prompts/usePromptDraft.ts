import {useLayoutEffect, useRef, useState} from 'react'

type Draft<T> = {value: T; baseline: T}
const drafts = new Map<string, Draft<unknown>>()
const same = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b)
if (typeof window !== 'undefined') window.addEventListener('beforeunload', event => {
 if (!drafts.size) return
 event.preventDefault()
 event.returnValue = ''
})

/** Unsaved prompt text survives page/scope changes in memory, never on disk. */
export function usePromptDraft<T>(key: string, confirmed: T, ready: boolean) {
 const server = useRef(confirmed)
 server.current = confirmed
 const [,forceRevision] = useState(0)
 const currentKey = useRef(key)
 const [value, setValue] = useState(() => (drafts.get(key) as Draft<T> | undefined)?.value ?? confirmed)
 const serialized = JSON.stringify(confirmed)
 useLayoutEffect(() => {
  currentKey.current = key
  const existing = drafts.get(key) as Draft<T> | undefined
  if (ready && existing && same(existing.value, server.current)) drafts.delete(key)
  setValue((drafts.get(key) as Draft<T> | undefined)?.value ?? server.current)
 }, [key, serialized, ready])
 const change = (next: T) => {
  const field = currentKey.current
  const existing = drafts.get(field) as Draft<T> | undefined
  if (same(next, server.current)) drafts.delete(field)
  else drafts.set(field, {value: next, baseline: existing?.baseline ?? server.current})
  setValue(next)
 }
 const discard = () => {drafts.delete(key); setValue(server.current)}
 const rebase = () => {drafts.set(key, {value, baseline: server.current}); setValue(current => current); forceRevision(revision => revision + 1)}
 const accept = (saved: T) => {drafts.delete(key); setValue(saved)}
 const pending = drafts.get(key) as Draft<T> | undefined
 return {value, change, discard, accept, rebase, confirmed, dirty: ready && !same(value, confirmed),
  baseline: pending?.baseline ?? confirmed,
  conflict: Boolean(ready && pending && !same(pending.baseline, confirmed) && !same(pending.value, confirmed))}
}
