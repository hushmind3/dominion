import {forwardRef,useLayoutEffect,useRef,type ReactNode,type TextareaHTMLAttributes} from 'react'
import {Hash,Paperclip,X} from 'lucide-react'
import {TextArea} from '../Primitives'
import {t,useLocale} from '../../i18n'

export function ConversationUsage({total,className='',children}:{total:number;className?:string;children:ReactNode}){
 useLocale()
 return <details className={'chat-usage-details '+className}>
  <summary aria-label={t('chat.cumulativeTokens',{count:total.toLocaleString()})} title={t('chat.cumulativeTokens',{count:total.toLocaleString()})}><Hash size={13}/><span>{t('chat.cumulativeTokens',{count:total.toLocaleString()})}</span></summary>
  <div role="note" className="chat-usage-popover">{children}</div>
 </details>
}

const formatBytes=(bytes:number)=>bytes>=1048576
 ?(bytes/1048576).toFixed(1)+' MiB'
 :bytes>=1024?(bytes/1024).toFixed(1)+' KiB':bytes+' B'
type DraftFile={id?:string;name:string;size:number}
export function DraftAttachments<T extends DraftFile>({files,disabled,onRemove,label='첨부 대기 파일'}:{
 files:readonly T[];disabled?:boolean;onRemove:(file:T,index:number)=>void;label?:string
}){
 if(!files.length)return null
 return <div className="chat-file-drafts" aria-label={label}>{files.map((file,i)=><span key={file.id??i} className="chat-file-chip"><Paperclip size={13}/><span title={file.name}>{file.name}</span><small>{formatBytes(file.size)}</small><button type="button" aria-label={file.name+' 첨부 취소'} disabled={disabled} onClick={()=>onRemove(file,i)}><X size={13}/></button></span>)}</div>
}

/** Resize after restored drafts and every edit, including programmatic changes.
 * Each page retains ownership of text, keyboard rules and upload lifecycle. */
export const ConversationInput=forwardRef<HTMLTextAreaElement,TextareaHTMLAttributes<HTMLTextAreaElement>>(function ConversationInput({className='',value,onChange,...props},ref){
 const inner=useRef<HTMLTextAreaElement|null>(null)
 const resize=()=>{const node=inner.current;if(node&&node.offsetWidth){node.style.height='auto';node.style.height=Math.min(node.scrollHeight,180)+'px'}}
 useLayoutEffect(resize,[value])
 useLayoutEffect(()=>{
  const node=inner.current;if(!node)return
  // Observe width (not height) to avoid an auto-size feedback loop.
  let width=node.offsetWidth
  const observer=new ResizeObserver(()=>{if(node.offsetWidth!==width){width=node.offsetWidth;resize()}})
  observer.observe(node)
  return()=>observer.disconnect()
 },[])
 return <TextArea {...props} ref={node=>{inner.current=node;if(typeof ref==='function')ref(node);else if(ref)ref.current=node}} value={value} rows={1} className={'conversation-composer-input '+className} onChange={onChange}/>
})
