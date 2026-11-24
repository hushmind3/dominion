import {translateError} from '../i18n/errors'
import {plainError} from '../i18n/plain'
import {t} from '../i18n'

let authToken = ''
export function setAuthToken(value: string) {
 const next = value.trim()
 if (next === authToken) return
 authToken = next
 // Credentials changed: clear privileged cached reads before refetching.
 if (typeof window !== 'undefined') window.dispatchEvent(new Event('dominion-auth-changed'))
}

export async function apiFetch(url: string, init: RequestInit = {}) {
 const headers = new Headers(init.headers)
 if (typeof init.body === 'string' && !headers.has('Content-Type')) {
  headers.set('Content-Type', 'application/json')
 }
 if (authToken) headers.set('X-Dominion-Token', authToken)
 try {
  return await fetch(url, {...init, headers, cache: 'no-store'})
 } catch (error) {
  if (init.signal?.aborted || (error instanceof DOMException && error.name === 'AbortError')) throw error
  throw new ApiError(0, t('request.unreachable'), 'network_error', true)
 }
}

export class ApiError extends Error {
 readonly status: number
 readonly code?: string
 readonly retryable: boolean
 constructor(status: number, message: string, code?: string, retryable = false) {
  super(message)
  this.name = 'ApiError'
  this.status = status
  this.code = code
  this.retryable = retryable
 }
}

export async function readJson(res: Response) {
 if (res.status === 204 || res.status === 205) return undefined
 const content = await res.text()
 try { return JSON.parse(content) }
 catch {
  throw new ApiError(res.status, !res.ok && content && !content.trimStart().startsWith('<')
   ? plainError(translateError(content)) : t('request.invalidResponse'), 'invalid_response', true)
 }
}

export async function request<T = any>(url: string, init?: RequestInit): Promise<T> {
 const res = await apiFetch(url, init)
 const data = await readJson(res)
 if (!res.ok) throw responseError(res.status, data)
 return data as T
}

export const get = <T = any>(url: string, signal?: AbortSignal) => request<T>(url, {signal})
export function responseError(status: number, data: unknown): ApiError {
 const body = data && typeof data === 'object' ? data as Record<string, unknown> : {}
 const fallback = status === 401 ? 'request.signIn' : status === 429 ? 'request.rateLimited' : 'request.failed'
 return new ApiError(status,
  typeof body.error === 'string' ? plainError(translateError(body.error)) : t(fallback),
  typeof body.code === 'string' ? body.code : undefined,
  typeof body.retryable === 'boolean' ? body.retryable : [408, 429, 502, 503, 504].includes(status),
 )
}

export const post = <T = any>(url: string, data: unknown, signal?: AbortSignal) =>
 request<T>(url, {method: 'POST', body: JSON.stringify(data), signal})
export const del = (url: string, signal?: AbortSignal) => request(url, {method: 'DELETE', signal})
