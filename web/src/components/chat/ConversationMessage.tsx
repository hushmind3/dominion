import {memo,useEffect,useRef,useState,type ReactNode} from 'react'
import {BrainCircuit,Check,Copy,Hash,MoreHorizontal,Sparkles,X} from 'lucide-react'
import {createPortal} from 'react-dom'
import type {ChatMessage} from '../../api'
import {t,useLocale} from '../../i18n'
import {Button} from '../Primitives'
import {MarkdownMessage} from '../MarkdownMessage'
import {AttachmentLink} from '../AttachmentLink'
import {useAnchoredPanel} from '../../hooks/useAnchoredPanel'
import {ExecutionTokenDetails} from './ExecutionTokenDetails'

/** The same saved-message renderer and on-demand execution evidence for private and group conversations. */
export const ConversationMessage=memo(function ConversationMessage({message,agentName,showReasoning,active=true,room=false,avatar,byline,extra}:{message:ChatMessage;agentName?:string;showReasoning?:boolean;active?:boolean;room?:boolean;avatar?:ReactNode;byline?:ReactNode;extra?:ReactNode}){
 useLocale()
 const [detail,setDetail]=useState<'reasoning'|'usage'|null>(null)
 const [menuOpen,setMenuOpen]=useState(false)
 const [copied,setCopied]=useState(false)
 const menuRef=useRef<HTMLDivElement>(null)
 const tokenPanel=useAnchoredPanel(active&&detail==='usage',()=>setDetail(null),420,'top')
 useEffect(()=>{if(!active){setDetail(null);setMenuOpen(false)}},[active])
 const human=message.role==='user'
 const usage=message.metadata?.usage
 const executionId=typeof message.metadata?.executionId==='string'?message.metadata.executionId:undefined
 const canShowReasoning=Boolean(showReasoning&&message.reasoning)
 const canShowUsage=Boolean(usage?.totalTokens||executionId)
 const attached=(Array.isArray(message.metadata?.attachments)?message.metadata.attachments:[]) as {id:string;name:string;size?:number;isImage?:boolean}[]
 const sources=Array.isArray(message.metadata?.sources)
  ?message.metadata.sources.filter(source=>typeof source.title==='string'
   &&typeof source.url==='string'&&/^https?:\/\//i.test(source.url)).slice(0,8):[]
 useEffect(()=>{
  if(!menuOpen)return
  const outside=(event:PointerEvent)=>{
   if(menuRef.current&&!menuRef.current.contains(event.target as Node))setMenuOpen(false)
  }
  const escape=(event:globalThis.KeyboardEvent)=>{
   if(event.key==='Escape')setMenuOpen(false)
  }
  document.addEventListener('pointerdown',outside)
  document.addEventListener('keydown',escape)
  return()=>{
   document.removeEventListener('pointerdown',outside)
   document.removeEventListener('keydown',escape)
  }
 },[menuOpen])
 const toggleDetail=(next:'reasoning'|'usage')=>{
  setDetail(current=>current===next?null:next)
  setMenuOpen(false)
 }
 const copyAnswer=async()=>{
  try{await navigator.clipboard.writeText(message.content||'');setCopied(true)}
  catch{setCopied(false)}
 }
 return <article className={'chat-turn '+(human?'chat-turn-owner':'chat-turn-assistant')+(room?' room-message '+(human?'room-message-owner':'room-message-agent')+(message.metadata?.kind==='learning_report'?' room-message-review':''):'')} data-message data-role={message.role}>
  {!human&&(avatar||<div className="grid size-9 shrink-0 place-items-center rounded-xl bg-[#F3EEE5] text-[#72685E]"><Sparkles size={17}/></div>)}
  <div className={'chat-turn-body'+(room?' room-message-main':'')}>
   {byline||(!human&&<div className="chat-turn-byline">{agentName||'DOMINION'}</div>)}
   {(message.content||!human)&&<div className={'chat-turn-text'+(room?' room-message-text':'')}>{human?message.content:<MarkdownMessage text={message.content||(!human&&message.tool_calls?.length?t('chat.toolRequest'):'')}/>}</div>}
   {human&&attached.length>0&&<div className="chat-saved-attachments">{attached.map(file=><AttachmentLink key={file.id} file={file}/>)}</div>}
   {!human&&sources.length>0&&<details className="chat-web-sources" aria-label="웹 검색 결과">
    <summary>웹 검색 결과 {sources.length}개</summary>
    <div>{sources.map((source,i)=><a key={source.url} href={source.url} target="_blank" rel="noopener noreferrer">
     <span className="chat-web-source-number">{i+1}</span>
     <span className="chat-web-source-title">{source.title}</span>
     {source.published&&<small>{source.published}</small>}
    </a>)}</div>
   </details>}
   {extra}
   {!human&&detail==='reasoning'&&canShowReasoning&&<section className="chat-turn-detail" aria-label="추론 정보">
    <div className="chat-turn-detail-heading"><BrainCircuit size={14}/> 추론 정보 <Button variant="bare" type="button" onClick={()=>setDetail(null)} aria-label="추론 정보 접기"><X size={14}/></Button></div>
    <div className="max-h-56 overflow-auto whitespace-pre-wrap">{message.reasoning}</div>
   </section>}

   {!human&&Boolean(message.content)&&<div className="chat-turn-actions" role="group" aria-label="답변 작업">
    <Button type="button" variant="bare" className="chat-turn-action" onClick={()=>void copyAnswer()}
     aria-label={copied?'답변 복사됨':'답변 복사'} title={copied?'복사됨':t('chat.copy')}>
     {copied?<Check size={15}/>:<Copy size={15}/>}<span>{copied?'복사됨':'복사'}</span>
    </Button>
    {(canShowReasoning||canShowUsage)&&<div className="chat-turn-action-menu" ref={menuRef}>
     <Button ref={tokenPanel.trigger} type="button" variant="bare" className="chat-turn-action" aria-label="답변 옵션"
      aria-haspopup="menu" aria-expanded={menuOpen} title="추론·토큰 상세" onClick={()=>setMenuOpen(value=>!value)}>
      <MoreHorizontal size={17}/>
     </Button>
     {menuOpen&&<div className="chat-turn-action-popover" role="menu" aria-label="답변 상세 옵션">
      {canShowReasoning&&<button type="button" role="menuitem" onClick={()=>toggleDetail('reasoning')}><BrainCircuit size={14}/>{detail==='reasoning'?'추론 정보 접기':'추론 정보 보기'}</button>}
      {canShowUsage&&<button type="button" role="menuitem" onClick={()=>toggleDetail('usage')}><Hash size={14}/>{detail==='usage'?'토큰 상세 접기':'토큰 사용량 보기'}</button>}
     </div>}
    </div>}
   </div>}
  </div>
  {active&&!human&&detail==='usage'&&canShowUsage&&tokenPanel.position&&createPortal(
   <div ref={tokenPanel.panel} style={tokenPanel.position} role="dialog" tabIndex={-1}
    aria-label="실행·토큰 상세" className="chat-token-detail-popover">
    <div className="chat-token-detail-title"><strong>실행·토큰 상세</strong>
     <button type="button" onClick={()=>setDetail(null)} aria-label="실행·토큰 상세 닫기"><X size={15}/></button>
    </div>
    <ExecutionTokenDetails executionId={executionId} usage={usage} createdAt={message.created_at}/>
   </div>,document.body)}
 </article>
})
