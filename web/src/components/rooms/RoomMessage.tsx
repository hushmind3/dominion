import {useState} from 'react'
import {Check, Copy, TerminalSquare} from 'lucide-react'
import type {Agent, Attachment, ChatMessage} from '../../api'
import {t, useLocale} from '../../i18n'
import {Button} from '../Primitives'
import {ChatRichText} from '../ChatRichText'
import {MessageInfo} from '../chat/MessageInfo'
import {RoomAvatar} from './RoomAvatar'

type Props = {
 message: ChatMessage; agent?: Agent; name: string; owner: boolean; index: number; showReasoning?: boolean; active?: boolean
}

export function RoomMessage({message, agent, name, owner, index, showReasoning = true, active = true}: Props) {
 useLocale()
 const [copied, setCopied] = useState(false)
 const attachments = Array.isArray(message.metadata?.attachments) ? message.metadata.attachments as Attachment[] : []
 const review = message.metadata?.kind === 'learning_report'
 const shared = message.metadata?.learningScope === 'room' || message.metadata?.source === 'knowledge'
 return <article className={'room-message ' + (owner ? 'room-message-owner' : 'room-message-agent') + (review ? ' room-message-review' : '')}>
  <RoomAvatar agent={agent} owner={owner} index={index}/>
  <div className="room-message-main">
   <div className="room-message-byline">
    <strong>{name}</strong>
    {review && <span className="room-agent-tag">{t(shared ? 'rooms.sharedLearning' : 'rooms.selfReview')}</span>}
   </div>
   {showReasoning && message.reasoning && <details className="room-reasoning">
    <summary>{t('rooms.live.showReasoning')}</summary><pre>{message.reasoning}</pre>
   </details>}
   <RoomMessageBody text={message.content} plain={owner}/>
   {owner && attachments.length > 0 && <div className="mt-2 flex flex-wrap gap-2">
    {attachments.map(item => item.isImage
     ? <a key={item.id} href={item.url} target="_blank" rel="noreferrer" title={item.name} className="block overflow-hidden rounded-lg border border-divider">
        <img src={item.url} alt={item.name} className="h-24 w-24 object-cover"/>
       </a>
     : <a key={item.id} href={item.url} target="_blank" rel="noreferrer" className="rounded-lg border border-divider bg-white px-2 py-1 text-[11px] text-[var(--color-muted)]">{item.name}</a>)}
   </div>}
   {Boolean(message.tool_calls?.length) && <span className="room-message-tools">
    <TerminalSquare size={12}/>{t('rooms.toolCallsSummary', {count: message.tool_calls!.length})}
   </span>}
   <div className="room-message-actions">
    <Button variant="bare" className="room-copy" aria-label={t('rooms.copyAria')} title={t('rooms.copyAria')}
     onClick={() => {void navigator.clipboard.writeText(message.content).then(() => {setCopied(true); setTimeout(() => setCopied(false), 1600)}).catch(() => setCopied(false))}}>
     {copied ? <Check size={14}/> : <Copy size={14}/>}
    </Button>
    <MessageInfo message={message} active={active}/>
   </div>
  </div>
 </article>
}

export function RoomMessageBody({text, plain = false}: {text: string; plain?: boolean}) {
 return <div className="room-message-text">{plain ? text : <ChatRichText text={text}/>}</div>
}
