import {t} from '../i18n'
import {apiFetch, readJson, responseError} from './transport'
import type {ChatRequest, StreamEvent} from './types'

export async function streamChat(data: ChatRequest, onEvent: (e: StreamEvent) => void, signal: AbortSignal) {
 const res = await apiFetch('/api/chat', {method: 'POST', body: JSON.stringify(data), signal})
 if (!res.ok) throw responseError(res.status, await readJson(res))
 if (!res.body || !res.headers.get('Content-Type')?.startsWith('text/event-stream')) {
  throw Error('No streaming response')
 }
 const reader = res.body.pipeThrough(new TextDecoderStream()).getReader()
 let buffer = ''
 try {
  while (true) {
   const {done, value} = await reader.read()
   if (done) throw Error(t('chat.streamInterrupted'))
   buffer += value
   let boundary: RegExpExecArray | null
   while ((boundary = /\r?\n\r?\n/.exec(buffer))) {
    const frame = buffer.slice(0, boundary.index)
    buffer = buffer.slice(boundary.index + boundary[0].length)
    const payload = frame.split(/\r?\n/).filter(line => line.startsWith('data:'))
     .map(line => line.slice(5).replace(/^ /, '')).join('\n')
    if (!payload) continue
    const event: StreamEvent = JSON.parse(payload)
    onEvent(event)
    if (event.type === 'done') return
   }
  }
 } finally {
  await reader.cancel().catch(() => {})
 }
}
