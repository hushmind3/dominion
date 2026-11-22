import {useEffect,useState} from 'react'
import {ArrowRight,Plus,X} from 'lucide-react'
import type {Provider} from '../../api'
import {DialogBackdrop,DialogPanel} from '../../components/Dialog'
import {Button} from '../../components/Primitives'
import {SearchField} from '../../components/ui/WorkspaceUI'
import {useLocale} from '../../i18n'
import {providerPresets} from './catalog'
import {ConnectionDialog} from './ConnectionTools'
import {useModelHub} from './useModelHub'

/** Reuses the connection editor without leaving the conversation. */
export function InlineConnectionDialog({provider,onClose,onConnected}:{provider?:Provider;onClose:()=>void;onConnected:(provider:Provider)=>void}){
 const en=useLocale()==='en';const tr=(ko:string,english:string)=>en?english:ko
 const hub=useModelHub({inline:true})
 const [editing,setEditing]=useState(false),[query,setQuery]=useState('')
 useEffect(()=>{if(provider){hub.open(provider);setEditing(true)}},[provider?.id])
 useEffect(()=>{
  if(!editing||hub.modal)return
  const saved=hub.providers.value.find(provider=>provider.id===hub.savedId)
  if(saved)onConnected(saved)
  else onClose()
 },[editing,hub.modal,hub.savedId])
 const choices=providerPresets.filter(preset=>preset.name.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase())).sort((a,b)=>a.name.localeCompare(b.name))
 if(editing)return <ConnectionDialog hub={hub}/>
 return <DialogBackdrop onDismiss={onClose}><DialogPanel className="flex flex-col !overflow-hidden" aria-label={tr('AI 연결 추가','Add an AI connection')}>
  <header className="flex items-center justify-between gap-4 border-b border-divider p-5"><div><h2 className="text-lg font-semibold">{tr('어떤 AI를 연결할까요?','Which AI would you like to connect?')}</h2><p className="mt-2 text-sm text-muted">{tr('대화를 나가지 않고 여기서 연결하세요.','Connect here and continue your conversation.')}</p></div><Button variant="ghost" size="icon" aria-label={tr('닫기','Close')} onClick={onClose}><X size={18}/></Button></header>
  <div className="grid min-h-0 gap-4 overflow-y-auto p-5"><SearchField value={query} onChange={setQuery} label={tr('연결할 AI 검색','Find an AI service')}/>
   <div className="grid max-h-80 grid-cols-1 gap-2 overflow-y-auto sm:grid-cols-2">{choices.map(preset=><Button key={preset.name} variant="secondary" className="!justify-between !text-left !font-medium" onClick={()=>{hub.openPreset(preset);setEditing(true)}}>{preset.name}<ArrowRight size={15}/></Button>)}</div>
   <Button variant="ghost" className="!justify-start" onClick={()=>{hub.open();setEditing(true)}}><Plus size={16}/>{tr('직접 서버 주소로 연결','Connect with a server address')}</Button>
  </div>
 </DialogPanel></DialogBackdrop>
}
