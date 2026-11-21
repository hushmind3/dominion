import {useState,type KeyboardEvent} from 'react'
import {ArrowRight,Search} from 'lucide-react'
import {DialogBackdrop,DialogPanel} from './Dialog'
import {navigation,type PageId} from './WorkspaceShell'
import {t,useLocale} from '../i18n'

// All existing destinations, not an extra navigation level. The same
// command panel works from chat, rooms, desktop and mobile settings.
export function WorkspaceSearch({onNavigate,onDismiss}:{
 onNavigate:(page:PageId)=>void;onDismiss:()=>void
}){
 const en=useLocale()==='en'
 const [query,setQuery]=useState('')
 const [active,setActive]=useState(0)
 const all=navigation.flatMap(section=>section.items.map(item=>({
  ...item,group:t(section.title),name:t(item.label)
 })))
 const q=query.trim().toLocaleLowerCase()
 const items=q?all.filter(item=>
  (item.name+' '+item.id+' '+item.group).toLocaleLowerCase().includes(q)):all
 const select=(id:PageId)=>{onDismiss();onNavigate(id)}
 const keydown=(event:KeyboardEvent<HTMLInputElement>)=>{
  if(event.key==='ArrowDown'){
   event.preventDefault()
   setActive(i=>Math.min(i+1,Math.max(0,items.length-1)))
  }else if(event.key==='ArrowUp'){
   event.preventDefault();setActive(i=>Math.max(0,i-1))
  }else if(event.key==='Enter'&&!event.isComposing&&items[active]){
   event.preventDefault();select(items[active].id)
  }
 }
 return <DialogBackdrop onDismiss={onDismiss}>
  <DialogPanel aria-label={en?'Find a workspace page':'도미니언 메뉴 검색'}
   className="w-[min(96vw,560px)] p-0">
   <label htmlFor="dominion-menu-search"
    className="flex items-center gap-3 border-b border-[#E7E0D5] px-4 py-3">
    <Search size={19} className="shrink-0 text-[#8A8176]"/>
    <span className="sr-only">{en?'Search menus':'메뉴 검색'}</span>
    <input id="dominion-menu-search" type="search" autoComplete="off"
     placeholder={en?'Search pages and settings…':'대화, 모델, 에이전트, 연결 등 메뉴 검색…'}
     className="min-h-10 min-w-0 flex-1 bg-transparent text-[14px] outline-none"
     value={query} onChange={event=>{setQuery(event.target.value);setActive(0)}}
     onKeyDown={keydown} aria-controls="dominion-menu-results"
     aria-activedescendant={items[active]?'dominion-menu-'+items[active].id:undefined}/>
    <kbd className="hidden rounded-lg border border-[#E7E0D5] px-1.5 py-0.5 text-[10px] text-[#8A8176] sm:inline">
     Esc</kbd>
   </label>
   <div id="dominion-menu-results" role="listbox" aria-label={en?'Workspace pages':'도미니언 메뉴'}
    className="max-h-[min(65dvh,480px)] overflow-y-auto p-2">
    {items.length?items.map((item,index)=>{
     const Icon=item.icon
     return <button key={item.id} id={'dominion-menu-'+item.id} type="button"
      role="option" aria-selected={index===active} onMouseEnter={()=>setActive(index)}
      onClick={()=>select(item.id)}
      className={'flex min-h-12 w-full items-center gap-3 rounded-xl px-3 text-left '+
       (index===active?'bg-[#F7E8E1] text-[#8E442D]':'text-[#51493D] hover:bg-[#F7F3EC]')}>
      <Icon size={17} className="shrink-0"/>
      <span className="min-w-0 flex-1">
       <strong className="block text-[12px]">{item.name}</strong>
       <small className="block text-[10px] text-[#8A8176]">{item.group}</small>
      </span><ArrowRight size={14} className="shrink-0 opacity-60"/>
     </button>
    }):<p className="p-5 text-center text-[12px] text-[#8A8176]">
     {en?'No matching menus.':'일치하는 메뉴가 없습니다.'}</p>}
   </div>
   <p className="border-t border-[#E7E0D5] px-4 py-2 text-[10px] text-[#8A8176]">
    {en?'Use ↑↓ to move, Enter to open, Esc to close · Ctrl/⌘ K':
     '↑↓ 이동 · Enter 열기 · Esc 닫기 · Ctrl/⌘ K'}
   </p>
  </DialogPanel>
 </DialogBackdrop>
}
