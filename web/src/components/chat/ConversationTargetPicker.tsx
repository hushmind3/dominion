import {lazy,Suspense,useEffect,useRef,useState} from 'react'
import {createPortal} from 'react-dom'
import {Check, ChevronDown, LoaderCircle, Plus, Sparkles, UsersRound, X} from 'lucide-react'
import type {Agent, Provider} from '../../api'
import {useAnchoredPanel} from '../../hooks/useAnchoredPanel'
import {useProviderModels} from '../../hooks/useProviderModels'
import {providerConfigured} from '../../features/conversations/resolveRoute'
import {t, useLocale} from '../../i18n'
import {Button, SelectInput} from '../Primitives'
import {SearchField, ViewTabs} from '../ui/WorkspaceUI'

const InlineConnectionDialog=lazy(()=>import('../../features/models/InlineConnectionDialog').then(module=>({default:module.InlineConnectionDialog})))

type Props={openRevision?:number;active:boolean;providers:Provider[];agents:Agent[];providerId:string;model:string;agentId:string;
 catalogue:ReturnType<typeof useProviderModels>;onModel:(providerId:string,model:string)=>void;onAgent:(id:string)=>void}

/** Browsing a provider never changes the conversation route until a model is chosen. */
export function ConversationTargetPicker({active,providers,agents,providerId,model,agentId,catalogue,onModel,onAgent,openRevision=0}:Props) {
 const en=useLocale()==='en'
 const tr=(ko:string,english:string)=>en?english:ko
 const [open,setOpen]=useState(false)
 const [connecting,setConnecting]=useState<{provider?:Provider}|null>(null)
 const lastOpen=useRef(openRevision)
 useEffect(()=>{if(lastOpen.current!==openRevision){lastOpen.current=openRevision;setOpen(true)}},[openRevision])
 const [view,setView]=useState<'models'|'personas'>('models')
 const [browse,setBrowse]=useState(providerId)
 const [query,setQuery]=useState('')
 const panel=useAnchoredPanel(active&&open,()=>setOpen(false),400,'top')
 const provider=providers.find(item=>item.id===browse)
 const models=useProviderModels(open&&view==='models'&&provider&&providerConfigured(provider)?browse:'',active&&open)
 const selectedAgent=agents.find(item=>item.id===agentId)
 const selectedModel=catalogue.value.find(item=>item.id===model)
 const focusTarget=useRef<HTMLDivElement>(null)
 useEffect(()=>{if(!active)setOpen(false)},[active])
 useEffect(()=>{if(open){setQuery('');setBrowse(providerId||providers.find(providerConfigured)?.id||'');setView(agentId||(!providers.length&&agents.length)?'personas':'models')}},[open])
 useEffect(()=>{if(open&&panel.position)focusTarget.current?.querySelector<HTMLElement>('input,button')?.focus({preventScroll:true})},[open,Boolean(panel.position)])
 const choose=()=>setOpen(value=>!value)
 const connect=(provider?:Provider)=>{setOpen(false);setConnecting({provider})}
 const closeConnection=()=>{setConnecting(null);panel.trigger.current?.focus({preventScroll:true})}
 const needle=query.trim().toLocaleLowerCase()
 const visibleModels=models.value.filter(item=>(item.name+' '+item.id).toLocaleLowerCase().includes(needle))
 const visibleAgents=agents.filter(item=>(item.name+' '+(item.description||'')).toLocaleLowerCase().includes(needle))
 return <>
  <Button ref={panel.trigger} variant="ghost" className="!min-h-9 !max-w-[min(20rem,55vw)] !justify-start !gap-1.5 !px-2 !py-1.5 !text-sm !font-medium" aria-label={tr('대화 상대 선택','Choose who to talk to')} aria-expanded={open} aria-haspopup="dialog" onClick={choose}>
   {selectedAgent?<UsersRound size={16} className="shrink-0"/>:<Sparkles size={16} className="shrink-0 text-brand"/>}<span className="min-w-0 truncate">{selectedAgent?[selectedAgent.name,selectedModel?.name||model].filter(Boolean).join(' · '):selectedModel?.name||model||tr('대화할 AI 선택','Choose an AI')}</span><ChevronDown size={13} className="shrink-0"/>
  </Button>
  {active&&open&&panel.position&&createPortal(<div ref={panel.panel} style={panel.position} role="dialog" aria-label={tr('대화 상대 선택','Choose who to talk to')} className="z-85 overflow-y-auto overscroll-contain rounded-2xl border border-divider bg-surface p-4 shadow-xl">
   <header className="mb-3 flex items-start justify-between gap-3"><div><h2 className="text-base font-semibold">{tr('누구와 대화할까요?','Who would you like to talk to?')}</h2><p className="mt-1 text-xs leading-6 text-muted">{tr('AI나 페르소나를 선택하면 이 대화에 적용됩니다.','Choose an AI or persona for this conversation.')}</p></div><Button size="icon" variant="ghost" onClick={()=>setOpen(false)} aria-label={t('rooms.close')}><X size={17}/></Button></header>
   {agents.length>0&&<ViewTabs value={view} onChange={value=>{setView(value);setQuery('')}} options={[{id:'models',label:tr('연결한 AI','Connected AIs')},{id:'personas',label:tr('페르소나','Personas'),count:agents.length}]} label={tr('대화 상대 종류','Conversation target type')}/>}
   <div ref={focusTarget} className="grid gap-3">
    {view==='models'&&providers.length>0&&<SelectInput aria-label={tr('연결 서비스 선택','Connection to browse')} value={browse} onChange={event=>{setBrowse(event.target.value);setQuery('')}}><option value="" disabled>{tr('연결을 선택하세요','Choose a connection')}</option>{providers.map(item=><option value={item.id} key={item.id}>{item.name}</option>)}</SelectInput>}
    {view==='models'&&!providers.length&&<div className="grid gap-4 py-3"><p className="text-sm leading-7 text-muted">{tr('아직 연결한 AI가 없습니다. 여기서 연결하고 대화를 이어가세요.','No AIs connected yet. Connect here and continue chatting.')}</p><Button variant="primary" onClick={()=>connect()}><Plus size={16}/>{tr('AI 연결 추가','Add an AI connection')}</Button></div>}
    {(providers.length>0||agents.length>0)&&<SearchField value={query} onChange={setQuery} label={tr('AI·페르소나 검색','Search AIs and personas')}/>}
    <div className="max-h-64 overflow-y-auto overscroll-contain">
     {view==='models'&&models.loading&&<p role="status" className="flex items-center gap-2 p-4 text-sm text-muted"><LoaderCircle size={16} className="animate-spin"/>{t('welcome.loading')}</p>}
     {view==='models'&&models.error&&<div role="alert" className="p-3 text-sm text-danger"><p>{models.error}</p><Button variant="ghost" className="mt-2" onClick={()=>void models.reload()}>{t('history.retry')}</Button></div>}
     {view==='models'&&provider&&!providerConfigured(provider)&&<div className="grid gap-3 p-3"><p className="text-sm text-muted">{tr('이 AI의 연결 정보를 입력하세요.','Enter the connection details for this AI.')}</p><Button onClick={()=>connect(provider)}>{tr('연결 정보 입력','Enter connection details')}</Button></div>}
     {view==='models'&&visibleModels.map(item=><Button key={item.id} variant="ghost" className="w-full !justify-between !text-left !font-medium" aria-pressed={!agentId&&providerId===browse&&model===item.id} onClick={()=>{onModel(browse,item.id);setOpen(false)}}><span className="min-w-0 break-words">{item.name||item.id}</span>{!agentId&&providerId===browse&&model===item.id&&<Check size={16} className="shrink-0 text-brand"/>}</Button>)}
     {view==='personas'&&visibleAgents.map(agent=><Button key={agent.id} variant="ghost" className="w-full !justify-start !gap-3 !text-left !font-medium" aria-pressed={agent.id===agentId} onClick={()=>{onAgent(agent.id);setOpen(false)}}><span className="grid size-8 shrink-0 place-items-center overflow-hidden rounded-lg bg-brand-soft text-brand-hover">{agent.avatarUrl?<img src={agent.avatarUrl} alt="" className="size-full object-cover"/>:agent.name.slice(0,1)}</span><span className="min-w-0 flex-1"><strong className="block text-sm font-semibold">{agent.name}</strong>{agent.description&&<small className="mt-1 block line-clamp-2 text-xs font-normal leading-5 text-muted">{agent.description}</small>}</span>{agent.id===agentId&&<Check size={16} className="shrink-0 text-brand"/>}</Button>)}
     {((view==='personas'&&!visibleAgents.length)||(view==='models'&&!models.loading&&!models.error&&provider&&providerConfigured(provider)&&!visibleModels.length))&&<p className="p-4 text-sm text-muted">{tr('선택할 항목이 없습니다.','No matching items.')}</p>}
    </div>
   </div>
   {(providers.length>0||agents.length>0)&&<footer className="mt-4 border-t border-divider pt-3"><Button variant="ghost" onClick={()=>connect()}><Plus size={15}/>{tr('연결 추가','Add connection')}</Button></footer>}
  </div>,document.body)}
  {active&&connecting&&<Suspense fallback={<p role="status" className="sr-only">{tr('연결 창 불러오는 중','Loading connection editor')}</p>}><InlineConnectionDialog provider={connecting.provider} onClose={closeConnection} onConnected={provider=>{onModel(provider.id,provider.model||'');closeConnection()}}/></Suspense>}
 </>
}
