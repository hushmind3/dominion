import {useEffect,useRef,type ReactNode} from 'react'
import {BrainCircuit,Check,Copy,MessageSquareMore,Plus,Sparkles,TerminalSquare} from 'lucide-react'
import {Button,IconButton,Text,cx} from '../design/Primitives'

export function ChatFrame({history,main}:{history:ReactNode;main:ReactNode}){
 return <div className="grid min-h-[calc(100dvh-68px)] grid-cols-1 bg-white lg:h-[calc(100dvh-68px)] lg:min-h-0 lg:grid-cols-[236px_minmax(0,1fr)]">{history}{main}</div>
}
export function ChatHistory({children}:{children:ReactNode}){
 return <aside className="hidden min-h-0 flex-col border-r border-line bg-canvas/65 lg:flex">{children}</aside>
}
export function ChatHistoryList({children}:{children:ReactNode}){
 return <div className="min-h-0 flex-1 overflow-y-auto p-2">{children}</div>
}
export function SessionEntry({title,selected,onClick}:{title:string;selected:boolean;onClick:()=>void}){
 return <Button variant="ghost" size="md" onClick={onClick} className={cx('!mb-1 !w-full !justify-start !gap-3 !text-left !font-medium',selected&&'!bg-brand-soft !text-brand')}><MessageSquareMore size={16}/><span className="min-w-0 flex-1 truncate">{title}</span></Button>
}
export function ChatMain({toolbar,transcript,composer}:{toolbar:ReactNode;transcript:ReactNode;composer:ReactNode}){
 return <div className="flex min-h-[calc(100dvh-68px)] min-w-0 flex-col lg:h-[calc(100dvh-68px)] lg:min-h-0"><div className="flex min-h-[65px] items-center justify-between gap-3 border-b border-line bg-white px-4 py-3 sm:px-6">{toolbar}</div>{transcript}<div className="shrink-0 border-t border-line bg-white p-3 sm:px-7 sm:py-4">{composer}</div></div>
}
export function Transcript({children,ready}:{children:ReactNode;ready:boolean}){
 const end=useRef<HTMLDivElement>(null)
 useEffect(()=>{if(ready)end.current?.scrollIntoView({behavior:'smooth',block:'end'})},[children,ready])
 return <div className="min-h-[320px] flex-1 overflow-y-auto bg-white px-4 py-7 sm:px-8"><div className="mx-auto flex w-full max-w-3xl flex-col gap-6">{children}<div ref={end}/></div></div>
}
export function ChatWelcome({onPrompt}:{onPrompt:(t:string)=>void}){
 return <div className="flex flex-col items-center gap-5 py-8 text-center sm:py-14">
  <div className="grid size-[74px] place-items-center rounded-[22px] bg-brand-soft text-brand"><Sparkles size={37}/></div>
  <div><p className="text-[11px] font-extrabold tracking-[.14em] text-brand">YOUR INTELLIGENCE WORKSPACE</p><h1 className="mt-3 text-[clamp(25px,3vw,35px)] font-extrabold tracking-[-.04em] text-ink">무엇을 도와드릴까요?</h1><p className="mt-3 text-[13px] leading-relaxed text-muted">모델과 도구를 선택해 질문하고, 연구하고, 실행하세요.</p></div>
  <div className="grid w-full max-w-md gap-2">{['내 작업공간의 파일 분석해 줘','새 프로젝트 구조를 설계해 줘','/goal 오늘 할 일을 정리해 실행해 줘'].map(p=><Button key={p} onClick={()=>onPrompt(p)} className="!w-full !justify-start !py-3 !text-left">{p}</Button>)}</div>
 </div>
}
export function ChatBubble({role,author,content,reasoning,onCopy}:{role:string;author?:string;content:string;reasoning?:string;onCopy?:()=>void}){
 const human=role==='user'
 return <article className={cx('flex min-w-0 items-start gap-3',human&&'flex-row-reverse')}>
  <div className={cx('grid size-9 shrink-0 place-items-center rounded-xl text-xs font-extrabold',human?'bg-slate-100 text-muted':'bg-brand text-white')}>{human?'U':<Sparkles size={18}/>}</div>
  <div className={cx('flex min-w-0 max-w-[min(100%,730px)] flex-col gap-2',human?'items-end':'items-start')}>
   <span className="px-1 text-[11px] font-bold text-muted">{author|| (human?'You':'DOMINION')}</span>
   {reasoning&&<details className="w-full rounded-xl border border-line bg-canvas p-3"><summary className="flex cursor-pointer items-center gap-2 text-[11px] font-semibold text-muted"><BrainCircuit size={14}/> 추론 내용</summary><pre className="mt-3 whitespace-pre-wrap font-sans text-xs leading-relaxed text-muted">{reasoning}</pre></details>}
   {content&&<div className={cx('whitespace-pre-wrap break-words rounded-[16px] px-4 py-3 text-[13px] leading-[1.85]',human?'bg-brand-soft text-ink':'border border-line bg-white text-ink')}>{content}</div>}
   {onCopy&&Boolean(content)&&<IconButton icon={<Copy size={14}/>} label="내용 복사" size="sm" onClick={onCopy}/>}
  </div>
 </article>
}
export function ChatThinking({text='',reasoning=''}:{text?:string;reasoning?:string}){
 return <ChatBubble role="assistant" content={text||'응답 생성 중…'} reasoning={reasoning} author="DOMINION · LIVE"/>
}
export function ToolState({text}:{text:string}){return text?<p className="flex items-center gap-2 text-[11px] text-muted"><TerminalSquare size={14}/>{text}</p>:null}
