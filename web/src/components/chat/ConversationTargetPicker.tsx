import {useEffect,useRef,useState} from 'react'
import {createPortal} from 'react-dom'
import {Check,ChevronDown,LoaderCircle,Plus,Search,Sparkles,UsersRound,X} from 'lucide-react'
import type {Agent,Provider} from '../../api'
import {useAnchoredPanel} from '../../hooks/useAnchoredPanel'
import {useProviderModels} from '../../hooks/useProviderModels'
import {useLocale} from '../../i18n'
import {Button,SelectInput} from '../Primitives'

type Props={
 active:boolean;disabled?:boolean;providers:Provider[];agents:Agent[];
 providerId:string;model:string;agentId:string;mode:'chat'|'agent';
 onModel:(providerId:string,model:string)=>void;
 onPersona:(agentId:string)=>void;onManage:()=>void
}

/** Sub3's grouped consumer picker adapted to Sub2's existing Chat routing.
 * Browsing a different connection does not change the active conversation;
 * only clicking a model or persona commits a selection. */
export function ConversationTargetPicker({
 active,disabled=false,providers,agents,providerId,model,agentId,mode,
 onModel,onPersona,onManage,
}:Props){
 const en=useLocale()==='en'
 const tr=(ko:string,english:string)=>en?english:ko
 const [open,setOpen]=useState(false)
 const [view,setView]=useState<'models'|'personas'>('models')
 const [browse,setBrowse]=useState('')
 const [query,setQuery]=useState('')
 const inputRef=useRef<HTMLInputElement>(null)
 const panel=useAnchoredPanel(active&&open,()=>setOpen(false),480,'top')
 const catalogue=useProviderModels(active&&open&&view==='models'?browse:'')
 const selectedAgent=mode==='agent'?agents.find(agent=>agent.id===agentId):undefined
 const selectedModel=mode==='chat'?model:(selectedAgent?.model||model)
 const selectedProvider=providers.find(provider=>provider.id===browse)
 const needle=query.trim().toLocaleLowerCase()
 const visibleModels=catalogue.value.filter(item=>
  (item.name+' '+item.id).toLocaleLowerCase().includes(needle))
 const visibleAgents=agents.filter(agent=>
  (agent.name+' '+(agent.description||'')).toLocaleLowerCase().includes(needle))

 useEffect(()=>{if(!active||disabled)setOpen(false)},[active,disabled])
 useEffect(()=>{
  if(!open)return
  setView(mode==='agent'&&agents.length?'personas':'models')
  setBrowse(providerId||providers[0]?.id||'')
  setQuery('')
 },[open])
 useEffect(()=>{
  if(open&&panel.position)inputRef.current?.focus({preventScroll:true})
 },[open,Boolean(panel.position),view])
 const chooseModel=(id:string)=>{onModel(browse,id);setOpen(false)}
 const choosePersona=(id:string)=>{onPersona(id);setOpen(false)}
 const manage=()=>{setOpen(false);onManage()}
 return <>
  <Button ref={panel.trigger} type="button" variant="ghost" disabled={disabled||!active}
   aria-label={tr('대화 상대 선택','Choose an AI or persona')}
   aria-haspopup="dialog" aria-expanded={open}
   onClick={()=>setOpen(value=>!value)}
   className="!min-h-8 !min-w-0 !max-w-[min(19rem,52vw)] !justify-start !gap-1.5 !px-2 !py-1 !text-xs !font-semibold">
   {selectedAgent?<UsersRound size={16} className="shrink-0"/>:
    <Sparkles size={16} className="shrink-0 text-[#C96442]"/>}
   <span className="min-w-0 truncate">{selectedAgent?.name||selectedModel||tr('AI 선택','Choose AI')}</span>
   <ChevronDown size={13} className="shrink-0"/>
  </Button>
  {active&&open&&panel.position&&createPortal(
   <div ref={panel.panel} style={panel.position}
    role="dialog" aria-label={tr('대화 상대 선택','Select conversation target')}
    className="z-[130] flex min-h-0 flex-col overflow-hidden rounded-2xl border border-[#E7E0D5] bg-[#FFFDF9] shadow-[0_20px_60px_rgba(36,33,29,.18)]">
    <header className="flex shrink-0 items-start justify-between gap-3 px-5 pb-3 pt-5">
     <div className="min-w-0">
      <h2 className="text-lg font-bold tracking-tight text-[#24211D]">
       {tr('누구와 대화할까요?','Who would you like to talk to?')}
      </h2>
      <p className="mt-1 text-xs leading-5 text-[#8A8176]">
       {tr('AI나 페르소나를 선택하면 이 대화에 적용됩니다.','Choose an AI or persona for this conversation.')}
      </p>
     </div>
     <Button type="button" variant="ghost" size="icon" onClick={()=>setOpen(false)}
      aria-label={tr('닫기','Close')}><X size={19}/></Button>
    </header>
    <div className="mx-5 flex shrink-0 gap-1 border-b border-[#E7E0D5]" role="tablist"
     aria-label={tr('대화 상대 종류','Conversation target type')}>
     <button type="button" role="tab" aria-selected={view==='models'} onClick={()=>{setView('models');setQuery('')}}
      className={'min-h-11 border-b-[3px] px-3 text-sm font-semibold transition-colors '+
       (view==='models'?'border-[#C96442] text-[#A94E30]':'border-transparent text-[#7C7469] hover:text-[#3A342D]')}>
      {tr('연결한 AI','Connected AIs')}
     </button>
     <button type="button" role="tab" aria-selected={view==='personas'} onClick={()=>{setView('personas');setQuery('')}}
      className={'min-h-11 border-b-[3px] px-3 text-sm font-semibold transition-colors '+
       (view==='personas'?'border-[#C96442] text-[#A94E30]':'border-transparent text-[#7C7469] hover:text-[#3A342D]')}>
      {tr('페르소나','Personas')} <span className="ml-1 rounded-lg bg-[#F3EEE5] px-2 py-0.5 text-xs">{agents.length}</span>
     </button>
    </div>
    <div className="flex min-h-0 flex-1 flex-col px-5 pt-4">
     {view==='models'&&providers.length>0&&
      <SelectInput value={browse} aria-label={tr('연결된 AI 업체','Connected AI provider')}
       onChange={event=>{setBrowse(event.target.value);setQuery('')}}
       className="mb-3 !min-h-11 !rounded-xl !bg-[#FFFDF9] !text-sm">
       {providers.map(provider=><option key={provider.id} value={provider.id}>{provider.name}</option>)}
      </SelectInput>}
     <label className="mb-2 flex shrink-0 items-center gap-2 rounded-xl border border-[#E1D9CC] bg-[#FFFDF9] px-3 py-2 focus-within:border-[#C96442] focus-within:ring-2 focus-within:ring-[#C96442]/15">
      <Search size={17} className="shrink-0 text-[#8A8176]"/>
      <input ref={inputRef} type="search" value={query} onChange={event=>setQuery(event.target.value)}
       aria-label={tr('AI·페르소나 검색','Search AIs and personas')}
       placeholder={tr('AI·페르소나 검색','Search AIs and personas')}
       className="min-w-0 flex-1 bg-transparent text-sm text-[#3A342D] outline-none placeholder:text-[#A79C8E]"/>
     </label>
     <div role="tabpanel" className="min-h-0 flex-1 overflow-y-auto overscroll-contain pb-3">
      {view==='models'&&catalogue.loading&&
       <p role="status" className="flex items-center gap-2 px-3 py-5 text-sm text-[#7C7469]">
        <LoaderCircle size={15} className="animate-spin"/>{tr('모델 불러오는 중','Loading models')}
       </p>}
      {view==='models'&&catalogue.error&&
       <div role="alert" className="px-3 py-4 text-sm text-rose-700">
        <p>{catalogue.error}</p>
        <Button type="button" variant="ghost" size="sm" onClick={()=>void catalogue.reload()}>
         {tr('다시 시도','Retry')}
        </Button>
       </div>}
      {view==='models'&&!providers.length&&
       <p className="px-3 py-5 text-sm text-[#7C7469]">
        {tr('연결된 AI가 없습니다. 아래에서 연결을 추가하세요.','No AI connections yet. Add one below.')}
       </p>}
      {view==='models'&&!catalogue.loading&&!catalogue.error&&visibleModels.map(item=>
       <button type="button" key={item.id} aria-pressed={mode==='chat'&&providerId===browse&&model===item.id}
        onClick={()=>chooseModel(item.id)}
        className="flex min-h-11 w-full items-center justify-between gap-3 rounded-xl px-3 py-2 text-left text-sm text-[#5C544A] hover:bg-[#F7F2EA] focus-visible:bg-[#F7F2EA]">
        <span className="min-w-0 break-words">{item.name||item.id}</span>
        {mode==='chat'&&providerId===browse&&model===item.id&&<Check size={17} className="shrink-0 text-[#C96442]"/>}
       </button>)}
      {view==='models'&&!!browse&&!catalogue.loading&&!catalogue.error&&!visibleModels.length&&
       <p className="px-3 py-5 text-sm text-[#7C7469]">
        {tr('선택할 모델이 없습니다.','No matching models.')}
       </p>}
      {view==='personas'&&visibleAgents.map(agent=>
       <button type="button" key={agent.id} aria-pressed={mode==='agent'&&agent.id===agentId}
        onClick={()=>choosePersona(agent.id)}
        className="flex min-h-12 w-full items-center gap-3 rounded-xl px-3 py-2 text-left hover:bg-[#F7F2EA]">
        <span className="grid size-9 shrink-0 place-items-center overflow-hidden rounded-xl bg-[#F7E8E1] text-sm font-bold text-[#A94E30]">
         {agent.avatarUrl?<img src={agent.avatarUrl} alt="" className="size-full object-cover"/>:agent.name.slice(0,1)}
        </span>
        <span className="min-w-0 flex-1">
         <strong className="block truncate text-sm font-semibold text-[#3A342D]">{agent.name}</strong>
         {agent.description&&<small className="mt-0.5 block line-clamp-2 text-xs leading-5 text-[#8A8176]">{agent.description}</small>}
        </span>
        {mode==='agent'&&agent.id===agentId&&<Check size={17} className="shrink-0 text-[#C96442]"/>}
       </button>)}
      {view==='personas'&&!visibleAgents.length&&
       <p className="px-3 py-5 text-sm text-[#7C7469]">
        {tr('선택할 페르소나가 없습니다.','No matching personas.')}
       </p>}
     </div>
    </div>
    <footer className="shrink-0 border-t border-[#E7E0D5] px-5 py-3">
     <Button type="button" variant="ghost" onClick={manage}
      className="!justify-start !gap-2 !px-2 !text-sm">
      <Plus size={18}/>{tr('연결 추가 · AI 관리','Add connection · Manage AIs')}
     </Button>
    </footer>
   </div>,document.body)}
 </>
}
