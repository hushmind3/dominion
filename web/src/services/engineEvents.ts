import {classifyRefreshEvent, emptyRefreshScope, mergeRefreshScopes, type BusChange} from '../refreshEvents'

/** One event bridge owns connection recovery, burst coalescing and cleanup. */
export function connectEngineEvents(onStatus: () => void, onApprovals: () => void) {
  const stream = new EventSource('/api/events')
  const timers = new Map<string, ReturnType<typeof setTimeout>>()
  let pending = emptyRefreshScope()
  let connectedBefore = false
  let disconnected = false
  const schedule = (key: string, delay: number, action: () => void) => {
    if (timers.has(key)) return
    timers.set(key, setTimeout(() => {timers.delete(key); action()}, delay))
  }
  stream.onopen = () => {
    if (connectedBefore || disconnected) {
      // Reconnects may have missed durable changes. Refetch once, then resume
      // the normal scoped events rather than leaving old conversations visible.
      window.dispatchEvent(new Event('dominion-refresh'))
      onStatus()
      onApprovals()
    }
    connectedBefore = true
    disconnected = false
  }
  stream.onerror = () => {
    if (!disconnected) {disconnected = true; onStatus()}
  }
  stream.onmessage = event => {
    let item: BusChange
    try {item = JSON.parse(event.data)} catch {return}
    if (!item || typeof item !== 'object') return
    if (item.type === 'room_event') {
      window.dispatchEvent(new CustomEvent('dominion-room-event', {detail: item}))
      return
    }
    const next = classifyRefreshEvent(item)
    if (next.targets.length || next.rooms.length || next.sessions.length) {
      pending = mergeRefreshScopes(pending, next)
      schedule('refresh', 100, () => {
        const batch = pending
        pending = emptyRefreshScope()
        window.dispatchEvent(new CustomEvent('dominion-data-change', {detail: batch}))
      })
    }
    if (item.type === 'approval' || (item.type === 'record' && item.kind === 'approval')) {
      schedule('approvals', 250, onApprovals)
    }
    if (item.type === 'job_event' || item.type === 'goal_event' || item.type === 'execution_event' || (item.type === 'record' && item.kind === 'job')) {
      schedule('status', 250, onStatus)
    }
  }
  return () => {
    stream.close()
    for (const timer of timers.values()) clearTimeout(timer)
    timers.clear()
  }
}
