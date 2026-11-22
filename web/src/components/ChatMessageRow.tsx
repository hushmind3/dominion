import {useState} from 'react'
import {BrainCircuit,ChevronRight,Copy,Sparkles} from 'lucide-react'
import type {ChatMessage} from '../api'
import {t} from '../i18n'
import {Button} from './Primitives'
import {ChatRichText} from './ChatRichText'
import {ChatAttachment} from './ChatAttachment'
import {TokenUsage} from './TokenUsage'

/** Pure message rendering: preserves evidence, attachments, reasoning and usage.
 * Conversation transport and persistence stay in ChatPage. */
export function ChatMessageRow({message,agentName,showReasoning}:{message:ChatMessage;agentName?:string;showReasoning?:boolean}){
 const [open,setOpen]=useState(true)
 const human=message.role==='user'
 const files=(human&&Array.isArray(message.metadata?.attachments)?message.metadata.attachments:[])
  .filter((item):item is {id:string;name:string;isImage?:boolean}=>!!item&&typeof item==='object'
   &&typeof item.id==='string'&&typeof item.name==='string')
  .slice(0,8)
 const usedMemories=(!human&&Array.isArray(message.metadata?.memoryRefs)?message.metadata.memoryRefs:[])
  .filter((item):item is {id:string;text:string}=>!!item&&typeof item==='object'
   &&typeof item.id==='string'&&typeof item.text==='string')
  .slice(0,4)
 return <div className={'chat-message flex min-w-0 items-start gap-3 '+(human?'chat-message--user justify-end':'chat-message--assistant')} data-message data-role={human?'user':'assistant'}>
  {!human&&<div className="grid size-9 shrink-0 place-items-center rounded-xl bg-brand-soft text-brand-hover"><Sparkles size={17}/></div>}
  <div className="chat-message-inner relative min-w-0">
   {!human&&<div className="chat-message-label mb-2 flex items-center gap-2 font-bold text-ink">{agentName||'DOMINION'}<TokenUsage usage={message.metadata?.usage} className="ml-auto rounded border border-divider bg-canvas px-1.5 py-0.5 text-[9px] font-medium text-muted"/></div>}
   {!human&&showReasoning&&message.reasoning&&<details className="mb-3 max-h-56 overflow-y-auto rounded-xl border border-divider bg-canvas p-3 text-[11px] leading-5 text-muted" open={open}><summary className="cursor-pointer select-none" onClick={e=>{e.preventDefault();setOpen(v=>!v)}}><BrainCircuit size={14} className="inline"/> {t('chat.reasoningProcess',{count:message.reasoning.length.toLocaleString()} )} <ChevronRight size={13} className="inline"/></summary><div className="mt-2 whitespace-pre-wrap">{message.reasoning}</div></details>}
   <div className={'chat-message-text min-w-0 whitespace-pre-wrap break-words '+(human?'chat-message-text--user':'chat-message-text--assistant')}>{human?message.content:(message.content?<ChatRichText content={message.content}/>:message.tool_calls?.length?t('chat.toolRequest'):'')}</div>
   {files.length>0&&<div className="mt-2 flex flex-wrap gap-1.5">{files.map(file=><ChatAttachment key={file.id} file={file}/>)}</div>}
   {usedMemories.length>0&&<details className="chat-message-memories mt-2 rounded-lg border border-divider bg-canvas px-3 py-2 text-[11px] leading-5 text-muted">
    <summary className="cursor-pointer select-none font-medium">{t('chat.usedMemories',{count:usedMemories.length})}</summary>
    <div className="mt-2 grid gap-2">{usedMemories.map(item=><div key={item.id} className="break-words border-t border-divider pt-2 whitespace-pre-wrap">{item.text}</div>)}</div>
   </details>}
   {!human&&message.content&&<Button className="mt-1 inline-flex items-center gap-1.5 rounded-lg px-2 py-1 text-[11px] text-muted hover:bg-canvas" onClick={()=>navigator.clipboard.writeText(message.content)} title={t('chat.copy')}><Copy size={14}/></Button>}
  </div>
 </div>
}
