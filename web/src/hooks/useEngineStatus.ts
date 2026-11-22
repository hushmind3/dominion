import { useEffect, useState } from 'react'
import { get } from '../api'
import {
  classifyRefreshEvent,
  emptyRefreshScope,
  mergeRefreshScopes,
} from '../refreshEvents'

export function useEngineStatus() {
  const [online, setOnline] = useState(false)
  const [running, setRunning] = useState(0)
  const [pending, setPending] = useState(0)
  const [engineBuild, setEngineBuild] = useState<{
    branch: string
    commit: string
  } | null>(null)
  useEffect(() => {
    let mounted = true
    const check = async () => {
      try {
        const status = await get<{
          jobs: number
          branch: string
          commit: string
        }>('/api/status')
        if (mounted) {
          setEngineBuild({ branch: status.branch, commit: status.commit })
          setOnline(true)
          setRunning(Number(status.jobs) || 0)
        }
      } catch {
        if (mounted) {
          setEngineBuild(null)
          setOnline(false)
          setRunning(0)
        }
      }
    }
    const approvals = async () => {
      try {
        const data = await get<unknown[]>('/api/approvals')
        if (mounted) setPending(Array.isArray(data) ? data.length : 0)
      } catch {
        if (mounted) setPending(0)
      }
    }
    void check()
    void approvals()
    const poll = setInterval(() => {
      void check()
    }, 12000)
    const stream = new EventSource('/api/events')
    let refreshTimer: ReturnType<typeof setTimeout> | undefined
    let statusTimer: ReturnType<typeof setTimeout> | undefined
    let approvalsTimer: ReturnType<typeof setTimeout> | undefined
    let pendingRefresh = emptyRefreshScope()
    const scheduleStatus = () => {
      if (statusTimer) return
      statusTimer = setTimeout(() => {
        statusTimer = undefined
        void check()
      }, 250)
    }
    const scheduleApprovals = () => {
      if (approvalsTimer) return
      approvalsTimer = setTimeout(() => {
        approvalsTimer = undefined
        void approvals()
      }, 250)
    }
    stream.onmessage = (event) => {
      try {
        const item = JSON.parse(event.data)
        if (item.type === 'room_event') {
          window.dispatchEvent(
            new CustomEvent('dominion-room-event', { detail: item }),
          )
          return
        }
        // Keep SSE records scoped to their consumers. A busy tool journal should
        // not re-fetch agents, providers or account connections on every receipt.
        const next = classifyRefreshEvent(item)
        if (next.targets.length || next.rooms.length || next.sessions.length) {
          pendingRefresh = mergeRefreshScopes(pendingRefresh, next)
          refreshTimer ??= setTimeout(() => {
            refreshTimer = undefined
            const batch = pendingRefresh
            pendingRefresh = emptyRefreshScope()
            window.dispatchEvent(
              new CustomEvent('dominion-data-change', { detail: batch }),
            )
          }, 100)
        }
        if (item.type === 'approval') scheduleApprovals()
        if (
          item.type === 'job_event' ||
          item.type === 'goal_event' ||
          item.type === 'execution_event' ||
          (item.type === 'record' && item.kind === 'job')
        )
          scheduleStatus()
      } catch {}
    }
    return () => {
      mounted = false
      clearInterval(poll)
      clearTimeout(refreshTimer)
      clearTimeout(statusTimer)
      clearTimeout(approvalsTimer)
      stream.close()
    }
  }, [])
  return { online, running, pending, engineBuild }
}
