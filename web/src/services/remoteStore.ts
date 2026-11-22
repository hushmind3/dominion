import {get} from '../api'
import {matchesRefreshTarget, type RefreshScope} from '../refreshEvents'

export type RemoteSnapshot<T> = {value: T; loading: boolean; refreshing: boolean; error: string}
type Listener = () => void

/** One read and one confirmed value per endpoint. Mutations are never retried. */
export class RemoteResource<T> {
 private snapshot: RemoteSnapshot<T>
 private listeners = new Set<Listener>()
 private activeReaders = 0
 private controller: AbortController | null = null
 private pending = false
 private inFlight: Promise<void> | null = null
 private version = 0
 private loadedAt = 0
 touched = Date.now()
 constructor(readonly endpoint: string, initial: T) {
  this.snapshot = {value: initial, loading: true, refreshing: true, error: ''}
 }
 getSnapshot = () => this.snapshot
 private update(patch: Partial<RemoteSnapshot<T>>) {
  this.snapshot = {...this.snapshot, ...patch}
  for (const listener of this.listeners) listener()
 }
 subscribe = (listener: Listener, active: boolean) => {
  const resumed = active && this.activeReaders === 0 && this.loadedAt > 0
  this.listeners.add(listener)
  if (active) this.activeReaders++
  this.touched = Date.now()
  if (!this.inFlight && (!this.loadedAt || resumed || Date.now() - this.loadedAt > 15_000)) void this.reload()
  return () => {
   this.listeners.delete(listener)
   if (active) this.activeReaders--
   // StrictMode and another subscriber can reattach before cancellation.
   setTimeout(() => {if (!this.listeners.size) {this.pending = false; this.controller?.abort()}}, 0)
  }
 }
 invalidate() {if (this.activeReaders > 0) void this.reload()}
 reload = (): Promise<void> => {
  if (this.inFlight) {this.pending = true; return this.inFlight}
  this.inFlight = this.read().finally(() => {this.inFlight = null})
  return this.inFlight
 }
 private async read() {
  do {
   this.pending = false
   const controller = new AbortController()
   this.controller = controller
   const version = this.version
   this.update({refreshing: true})
   try {
    const value = await get<T>(this.endpoint, controller.signal)
    if (controller.signal.aborted) break
    if (version !== this.version) {this.pending = true; continue}
    this.loadedAt = Date.now()
    this.update({value, loading: false, refreshing: false, error: ''})
   } catch (error) {
    if (controller.signal.aborted) break
    this.loadedAt = Date.now()
    this.update({loading: false, refreshing: false, error: error instanceof Error ? error.message : String(error)})
   } finally {if (this.controller === controller) this.controller = null}
  } while (this.pending && this.listeners.size > 0)
 }
 setValue = (next: T | ((previous: T) => T)) => {
  this.version++
  this.update({value: typeof next === 'function' ? (next as (previous: T) => T)(this.snapshot.value) : next})
 }
 get observed() {return this.listeners.size > 0}
}

const resources = new Map<string, RemoteResource<unknown>>()
export function remoteResource<T>(endpoint: string, initial: T): RemoteResource<T> {
 let resource = resources.get(endpoint)
 if (!resource) {
  // Keep inactive transcript caches bounded; the engine remains authoritative.
  if (resources.size >= 100) {
   const oldest = [...resources.values()].filter(item => !item.observed).sort((a, b) => a.touched - b.touched)[0]
   if (oldest) resources.delete(oldest.endpoint)
  }
  resource = new RemoteResource<unknown>(endpoint, initial)
  resources.set(endpoint, resource)
 }
 return resource as RemoteResource<T>
}

if (typeof window !== 'undefined') {
 window.addEventListener('dominion-refresh', () => {for (const resource of resources.values()) resource.invalidate()})
 window.addEventListener('dominion-data-change', event => {
  const targets = (event as CustomEvent<RefreshScope>).detail?.targets || []
  for (const resource of resources.values()) if (matchesRefreshTarget(resource.endpoint, targets)) resource.invalidate()
 })
 window.addEventListener('online', () => {for (const resource of resources.values()) resource.invalidate()})
}
