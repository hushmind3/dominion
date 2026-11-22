import {t} from '../i18n'
import {post} from './transport'
import type {Attachment} from './types'

function fileToBase64(file: File, signal?: AbortSignal): Promise<string> {
 return new Promise((resolve, reject) => {
  if (signal?.aborted) {reject(signal.reason); return}
  const reader = new FileReader()
  const abort = () => reader.abort()
  signal?.addEventListener('abort', abort, {once: true})
  reader.onloadend = () => signal?.removeEventListener('abort', abort)
  reader.onabort = () => reject(signal?.reason ?? new DOMException('Aborted', 'AbortError'))
  reader.onerror = () => reject(new Error(t('chat.attachReadFailed')))
  reader.onload = () => {
   const result = typeof reader.result === 'string' ? reader.result : ''
   const comma = result.indexOf(',')
   if (comma < 0) reject(new Error(t('chat.attachReadFailed')))
   else resolve(result.slice(comma + 1))
  }
  reader.readAsDataURL(file)
 })
}

/** Store one file for this session or room. The server validates the real
 *  bytes and owns the provider URL; callers only receive the verified record. */
export async function uploadAttachment(target: {sessionId:string}|{roomId:string}, file: File, signal?: AbortSignal): Promise<Attachment> {
 const data = await fileToBase64(file, signal)
 return post<Attachment>('/api/attachments', {
  ...target, name: file.name, mimeType: file.type || 'application/octet-stream', data,
 }, signal)
}
