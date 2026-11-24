import {PanelLeftClose,Plus} from 'lucide-react'
import {Button} from '../Primitives'
import {ConversationSwitch} from '../WorkspaceShell'
import {t,useLocale} from '../../i18n'

/** Identical navigation chrome for both transcript surfaces. */
export function ConversationSidebarHeader({mode,sidebarId,onClose,onNew,onSwitchMode}:{
 mode:'chat'|'rooms';sidebarId:string;onClose:()=>void;onNew:()=>void;onSwitchMode:(mode:'chat'|'rooms')=>void
}){
 useLocale()
 const closeLabel=t(mode==='chat'?'chat.listCollapse':'rooms.listCollapse')
 return <div className="conversation-sidebar-header">
  <div className="flex min-h-16 shrink-0 items-center gap-2 px-3">
   <span className="grid size-8 shrink-0 place-items-center rounded-xl border border-[#EED0BC] bg-[#F7E8E1] text-lg font-bold text-[#C96442]">D</span>
   <strong className="min-w-0 flex-1 truncate text-lg font-semibold tracking-tight text-[#24211D]">dominion</strong>
   <Button size="icon" variant="ghost" type="button" onClick={onClose} aria-label={closeLabel} aria-controls={sidebarId} aria-expanded={true} title={closeLabel}><PanelLeftClose size={19}/></Button>
  </div>
  <Button className="mx-3 mb-3 inline-flex items-center justify-center gap-2 rounded-xl border border-[#EED9CC] bg-[#C96442] px-4 py-2.5 text-xs font-bold text-white hover:bg-[#A94E30]" onClick={onNew}><Plus size={17}/>{t(mode==='chat'?'chat.newChat':'rooms.create')}</Button>
  <div className="mx-3 mb-3"><ConversationSwitch mode={mode} onChange={onSwitchMode}/></div>
 </div>
}
