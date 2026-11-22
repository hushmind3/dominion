import {useEffect,useMemo,useRef,useState} from 'react'
import {ArrowRight,CornerDownLeft,MessageSquarePlus,Search,X} from 'lucide-react'
import {DialogBackdrop,DialogPanel} from './Dialog'
import {Button,TextInput} from './Primitives'
import {navigation,type PageId} from './WorkspaceShell'
import {t,useLocale} from '../i18n'

type Destination={id:PageId|'new-chat';name:string;section:string;icon:typeof Search;keywords?:string}
const aliases:Partial<Record<PageId,string>>={
 home:'대시보드 시작 상황',chat:'채팅 대화 질문',rooms:'그룹 대화 회의 토론',
 models:'모델 LLM API 키',agents:'에이전트 비서 역할',
 connections:'연결 채널 MCP 스킬 마켓 플러그인 계정',
 memory:'기억 지식 메모 회상',activity:'실행 기록 작업 실패 승인 로그',
 settings:'설정 권한 보안',
}
export function QuickSwitcher({page,onClose,onNavigate,onNewChat,running,pending}:{
 page:PageId;onClose:()=>void;onNavigate:(page:PageId)=>void;
 onNewChat:()=>void;running:number;pending:number
}){
 const en=useLocale()==='en'
 const [query,setQuery]=useState('')
 const [active,setActive]=useState(0)
 const input=useRef<HTMLInputElement>(null)
 const items=useMemo<Destination[]>(()=>[
  {id:'new-chat',name:en?'Start a new chat':'새 대화 시작',section:en?'Quick action':'바로 실행',icon:MessageSquarePlus},
  ...navigation.flatMap(group=>group.items.map(item=>({
   id:item.id,name:t(item.label),section:t(group.title),icon:item.icon,keywords:aliases[item.id]
  })))
 ],[en])
 const choices=useMemo(()=>{
  const term=query.trim().toLocaleLowerCase()
  return term?items.filter(item=>[item.name,item.section,item.id,item.keywords].join(' ').toLocaleLowerCase().includes(term)):items
 },[query,items])
 useEffect(()=>{input.current?.focus({preventScroll:true})},[])
 const select=(item:Destination)=>{
  if(item.id==='new-chat')onNewChat()
  else onNavigate(item.id)
  onClose()
 }
 const choice=choices[Math.min(active,choices.length-1)]
 return <DialogBackdrop onDismiss={onClose}>
  <DialogPanel size="wide" aria-label={en?'Go to a workspace page':'작업공간 빠른 이동'} className="max-w-[600px] overflow-hidden p-0">
   <div className="flex items-center gap-3 border-b border-[#EFE9DF] px-4 py-3">
    <Search size={19} className="shrink-0 text-[#A94E30]"/>
    <TextInput ref={input} type="search" role="combobox" aria-label={en?'Find feature':'기능 검색'}
     aria-activedescendant={choice?'dominion-quick-'+Math.min(active,choices.length-1):undefined} aria-expanded={choices.length>0}
     aria-controls="dominion-quick-options" aria-autocomplete="list"
     value={query} onChange={event=>{setQuery(event.target.value);setActive(0)}}
     onKeyDown={event=>{
      if(event.key==='ArrowDown'){event.preventDefault();setActive(index=>Math.min(choices.length-1,index+1))}
      if(event.key==='ArrowUp'){event.preventDefault();setActive(index=>Math.max(0,index-1))}
      if(event.key==='Enter'&&choice){event.preventDefault();select(choice)}
     }}
     placeholder={en?'Search pages and actions':'메뉴·기능 검색'} className="min-h-10 border-0 bg-transparent px-0 shadow-none focus:ring-0"/>
    <Button variant="ghost" size="icon" aria-label={en?'Close':'닫기'} onClick={onClose}><X size={17}/></Button>
   </div>
   <div className="flex flex-wrap gap-2 border-b border-[#EFE9DF] px-4 py-2 text-[11px] text-[#7C7469]">
    <span>{en?'In progress':'진행 중'} <strong className="tabular-nums">{running}</strong></span>
    <span>·</span>
    <span>{en?'Pending approvals':'승인 대기'} <strong className="tabular-nums">{pending}</strong></span>
    <span className="ml-auto text-[10px]">{en?'↑ ↓ select · Enter open':'↑ ↓ 선택 · Enter 열기'}</span>
   </div>
   <div id="dominion-quick-options" role="listbox" aria-label={en?'Results':'검색 결과'} className="max-h-[58dvh] overflow-y-auto p-2">
    {choices.map((item,index)=>{
     const Icon=item.icon
     return <Button key={item.id} id={'dominion-quick-'+index} variant="bare" role="option"
      aria-selected={index===active} onMouseEnter={()=>setActive(index)}
      onClick={()=>select(item)}
      className={'flex min-h-12 w-full min-w-0 items-center gap-3 rounded-xl px-3 py-2 text-left focus-visible:outline-2 focus-visible:outline-[#C96442] '+
       (index===active?'bg-[#F7E8E1] text-[#24211D]':'hover:bg-[#FAF7F1] text-[#3A342D]')}>
      <Icon size={17} className="shrink-0 text-[#A94E30]"/>
      <span className="min-w-0 flex-1"><span className="block truncate text-[12px] font-semibold">{item.name}</span><small className="mt-0.5 block truncate text-[10px] text-[#8A8176]">{item.section}</small></span>
      {page===item.id?<span className="text-[10px] text-[#8A8176]">{en?'Current':'현재'}</span>:<ArrowRight size={14} className="text-[#B1A594]"/>}
     </Button>
    })}
    {!choices.length&&<p role="status" className="px-4 py-8 text-center text-[12px] text-[#8A8176]">{en?'No matching feature. Try another term.':'검색 결과가 없습니다. 다른 기능 이름으로 찾아보세요.'}</p>}
   </div>
   <div className="flex justify-end border-t border-[#EFE9DF] px-4 py-2 text-[10px] text-[#8A8176]"><CornerDownLeft size={13}/><span className="ml-1">{en?'Enter to open':'선택한 화면 열기'}</span></div>
  </DialogPanel>
 </DialogBackdrop>
}
