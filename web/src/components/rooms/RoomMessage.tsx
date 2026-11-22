import {useState,type ReactNode} from 'react'
import {Check, Copy, TerminalSquare} from 'lucide-react'
import type {Agent, Attachment, ChatMessage} from '../../api'
import {t, useLocale} from '../../i18n'
import {Button,cn} from '../Primitives'
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
 return <RoomMessageFrame agent={agent} name={name} owner={owner} index={index} review={review} metadata={review?<span className="rounded bg-canvas px-1.5 py-0.5 text-[10px] text-muted">{t(shared?'rooms.sharedLearning':'rooms.selfReview')}</span>:undefined}>
   {showReasoning && message.reasoning && <details className="mb-3 text-xs leading-6 text-muted [&>summary]:cursor-pointer [&>pre]:mt-2 [&>pre]:max-h-56 [&>pre]:overflow-auto [&>pre]:whitespace-pre-wrap [&>pre]:border-l-2 [&>pre]:border-divider [&>pre]:pl-3">
    <summary>{t('rooms.live.showReasoning')}</summary><pre>{message.reasoning}</pre>
   </details>}
   <RoomMessageBody text={message.content} plain={owner}/>
   {owner && attachments.length > 0 && <div className="mt-2 flex flex-wrap gap-2">
    {attachments.map(item => item.isImage
     ? <a key={item.id} href={item.url} target="_blank" rel="noreferrer" title={item.name} className="block overflow-hidden rounded-lg border border-divider">
        <img src={item.url} alt={item.name} className="h-24 w-24 object-cover"/>
       </a>
     : <a key={item.id} href={item.url} target="_blank" rel="noreferrer" className="rounded-lg border border-divider bg-surface px-2 py-1 text-[11px] text-muted">{item.name}</a>)}
   </div>}
   {Boolean(message.tool_calls?.length) && <span className="mt-2 inline-flex items-center gap-1.5 text-xs text-muted">
    <TerminalSquare size={12}/>{t('rooms.toolCallsSummary', {count: message.tool_calls!.length})}
   </span>}
   <div className="mt-2 flex min-h-8 items-center gap-2 sm:opacity-0 sm:transition-opacity sm:group-hover:opacity-100 sm:group-focus-within:opacity-100 sm:has-[[aria-expanded=true]]:opacity-100">
    <Button variant="bare" className="inline-grid size-8 place-items-center rounded-lg text-muted hover:bg-canvas hover:text-ink" aria-label={t('rooms.copyAria')} title={t('rooms.copyAria')}
     onClick={() => {void navigator.clipboard.writeText(message.content).then(() => {setCopied(true); setTimeout(() => setCopied(false), 1600)}).catch(() => setCopied(false))}}>
     {copied ? <Check size={14}/> : <Copy size={14}/>}
    </Button>
    <MessageInfo message={message} active={active}/>
   </div>
 </RoomMessageFrame>
}

export function RoomMessageBody({text, plain = false}: {text: string; plain?: boolean}) {
 return <div className={cn('room-message-text break-words text-base leading-8 text-ink [&_.chat-rich-text>:first-child]:mt-0 [&_.chat-rich-text>:last-child]:mb-0',plain&&'inline-block max-w-full whitespace-pre-wrap rounded-2xl border border-divider bg-canvas px-4 py-3')}>{plain ? text : <ChatRichText text={text}/>}</div>
}

export function RoomMessageFrame({agent,name,owner=false,index=0,review=false,metadata,children}:{agent?:Agent;name:string;owner?:boolean;index?:number;review?:boolean;metadata?:ReactNode;children:ReactNode}) {
 return <article className={cn('room-message group relative mb-6 flex min-w-0 gap-3',owner?'room-message-owner':'room-message-agent',review&&'rounded-xl bg-success-soft/15 p-3')}>
  <RoomAvatar agent={agent} owner={owner} index={index}/><div className="min-w-0 flex-1"><div className="mb-2 flex min-h-6 flex-wrap items-center gap-2"><strong className="text-sm font-semibold text-ink">{name}</strong>{metadata}</div>{children}</div>
 </article>
}
export function RoomTyping() {return <span aria-label={t('rooms.live.responding')} className="inline-flex h-7 items-center gap-1.5">{[0,1,2].map(index=><i key={index} className="size-1 rounded-full bg-muted motion-safe:animate-pulse"/>)}</span>}
