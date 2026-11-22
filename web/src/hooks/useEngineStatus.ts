import {useEffect, useState} from 'react'
import {get} from '../api'
import {connectEngineEvents} from '../services/engineEvents'

type EngineStatus = {online: boolean; connecting: boolean; running: number; pending: number; engineBuild: {branch: string; commit: string} | null}
const initialStatus: EngineStatus = {online: false, connecting: true, running: 0, pending: 0, engineBuild: null}

export function useEngineStatus() {
  const [status, setStatus] = useState(initialStatus)
  useEffect(() => {
    let mounted = true
    let statusRequest: AbortController | undefined
    let approvalRequest: AbortController | undefined
    const check = async () => {
      if (statusRequest) return
      const controller = new AbortController()
      statusRequest = controller
      try {
        const value = await get<{jobs: number; branch: string; commit: string}>('/api/status', controller.signal)
        if (mounted) setStatus(previous => ({...previous, online: true, connecting: false, running: Number(value.jobs) || 0, engineBuild: {branch: value.branch, commit: value.commit}}))
      } catch {
        if (mounted) setStatus(previous => ({...previous, online: false, connecting: false, running: 0, engineBuild: null}))
      } finally {statusRequest = undefined}
    }
    const approvals = async () => {
      if (approvalRequest) return
      const controller = new AbortController()
      approvalRequest = controller
      try {
        const value = await get<unknown[]>('/api/approvals', controller.signal)
        if (mounted) setStatus(previous => ({...previous, pending: Array.isArray(value) ? value.length : 0}))
      } catch {
        if (mounted) setStatus(previous => ({...previous, pending: 0}))
      } finally {approvalRequest = undefined}
    }
    void check()
    void approvals()
    const poll = setInterval(() => {void check()}, 12000)
    const disconnect = connectEngineEvents(() => {void check()}, () => {void approvals()})
    return () => {
      mounted = false
      clearInterval(poll)
      disconnect()
      statusRequest?.abort()
      approvalRequest?.abort()
    }
  }, [])
  return status
}
