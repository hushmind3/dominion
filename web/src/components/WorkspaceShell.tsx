import {OwnerProfileEditor} from './OwnerProfile'
import {Activity,BookOpenText,Bot,Cpu,House,MessageSquare,RadioTower,Settings2,UsersRound,Plus,X,Settings,Menu} from 'lucide-react'
import {Button} from './Primitives'
import {t,type LocaleKey,useLocale} from '../i18n'

export type PageId='home'|'chat'|'models'|'agents'|'rooms'|'memory'|'capabilities'|'extensions'|'connections'|'activity'|'settings'
type NavItem={id:PageId;label:LocaleKey;icon:typeof House}
export const navigation:{title:LocaleKey;items:NavItem[];collapsible?:boolean}[]=[
 {title:'nav.group.workspace',items:[{id:'home',label:'nav.home',icon:House},{id:'chat',label:'nav.chat',icon:MessageSquare},{id:'rooms',label:'nav.rooms',icon:UsersRound},{id:'models',label:'nav.models',icon:Cpu},{id:'agents',label:'nav.agents',icon:Bot}]},
 {title:'nav.group.extensions',items:[{id:'connections',label:'nav.connections',icon:RadioTower},{id:'memory',label:'nav.memory',icon:BookOpenText}]},
 {title:'nav.group.operations',items:[{id:'activity',label:'nav.activity',icon:Activity},{id:'settings',label:'nav.settings',icon:Settings2}]}
]
const items=navigation.flatMap(g=>g.items)
type SidebarProps={settingsOnly?:boolean;page:PageId;open:boolean;collapsed:boolean;online:boolean;running:number;pending:number;
 onNavigate:(id:PageId)=>void;onNewChat:()=>void;onClose:()=>void;navOpen?:boolean;onToggleNav?:()=>void}
export function WorkspaceSidebar({page,open,collapsed,online,running,pending,onNavigate,onNewChat,onClose,settingsOnly=false,navOpen=true,onToggleNav}:SidebarProps){
 useLocale()
 const all=navigation.flatMap(group=>group.items)
 const compact=settingsOnly&&!navOpen
 const navigateTo=(id:PageId)=>{
  onNavigate(id)
  if(settingsOnly&&navOpen&&window.matchMedia('(max-width: 767px)').matches)onToggleNav?.()
 }
 const renderItem=(entry:NavItem)=>{
  const Icon=entry.icon
  const count=entry.id==='activity'?running:entry.id==='settings'?pending:0
  return <Button variant="bare" key={entry.id} type="button" title={t(entry.label)}
   aria-label={t(entry.label)} aria-current={page===entry.id?'page':undefined}
   onClick={()=>navigateTo(entry.id)} className="workspace-settings-nav-item">
   <Icon size={18} strokeWidth={1.7}/><span>{t(entry.label)}</span>
   {count>0&&<small>{count}</small>}
  </Button>
 }
 if(settingsOnly)return <aside id="workspace-settings-sidebar" data-collapsed={compact}
  aria-label={t('shell.mainNav')} className="workspace-settings-sidebar">
  <div className="workspace-settings-brand">
   <span className="workspace-settings-logo">D</span>
   <strong className="workspace-settings-wordmark">DOMINION</strong>
   <Button variant="ghost" size="icon" type="button" aria-controls="workspace-settings-sidebar"
    aria-label={navOpen?t('shell.hideMenu'):t('shell.showMenu')}
    aria-expanded={navOpen} onClick={onToggleNav}><Menu size={19}/></Button>
  </div>
  <nav className="workspace-settings-nav" aria-label={t('shell.mainNav')}>
   {navigation.map(group=><section key={group.title} className="workspace-settings-group">
    {!compact&&<h2 className="workspace-settings-group-title">{t(group.title)}</h2>}
    <div className="workspace-settings-group-items">
     {group.items.filter(item=>item.id!=='chat'&&item.id!=='rooms').map(renderItem)}
    </div>
   </section>)}
  </nav>
  <div className="workspace-settings-footer">
   <span className="workspace-settings-connection" role="status"
    title={online?t('shell.engineConnected'):t('shell.engineDisconnected')}>
    <i className={online?'bg-emerald-500':'bg-rose-400'}/>
    {!compact&&<span>{online?t('shell.engineConnected'):t('shell.engineDisconnected')}</span>}
   </span>
   <OwnerProfileEditor compact={compact}/>
  </div>
 </aside>
 return <>
  {open&&<Button variant="bare" type="button" className="fixed inset-0 z-40 bg-slate-900/40 min-[520px]:hidden"
   aria-label={t('shell.menuClose')} onClick={onClose}/>}
  <aside aria-label={t('shell.mainNav')} className={'fixed inset-y-0 left-0 z-50 flex w-[216px] flex-col border-r border-[#E0D8CC] bg-[#EFEAE1] transition-transform duration-200 '+(collapsed?'min-[520px]:-translate-x-full ':'')+(open?'max-[519px]:translate-x-0':'max-[519px]:-translate-x-full')}>
   <div className="flex h-16 shrink-0 items-center gap-3 px-5"><span className="grid size-8 place-items-center rounded-xl bg-[#C96442] font-bold text-white">D</span><strong>DOMINION</strong></div>
   <Button variant="primary" type="button" onClick={onNewChat}><Plus size={17}/>{t('shell.newChat')}</Button>
   <nav className="min-h-0 flex-1 overflow-y-auto p-2">{all.map(renderItem)}</nav>
   <OwnerProfileEditor/>
  </aside>
 </>
}
export function SettingsHeader({page,online,build,onClose,onToggleNav}:{page:PageId;online:boolean;build?:{branch:string;commit:string}|null;onClose:()=>void;onToggleNav?:()=>void}){
 useLocale()
 const selected=items.find(x=>x.id===page)
 return <header className="workspace-responsive-header flex h-13 shrink-0 items-center gap-3 border-b border-[#E7E0D5] bg-[#FFFDF9] px-4">
  <Button variant="ghost" size="icon" type="button" className="workspace-responsive-mobile-toggle"
   aria-label={t('shell.showMenu')} aria-controls="workspace-settings-sidebar"
   onClick={onToggleNav}><Menu size={19}/></Button>
  <Settings size={17} className="text-[#8A8176]"/><h1 className="min-w-0 flex-1 truncate text-[14px] font-semibold text-[#24211D]">{selected?t(selected.label):t('nav.settings')}</h1>
  <span className={'hidden items-center gap-1.5 text-[11px] sm:inline-flex '+(online?'text-emerald-700':'text-rose-600')}><i className={'size-1.5 rounded-full '+(online?'bg-emerald-500':'bg-rose-500')}/>{online?t('shell.connected'):t('shell.disconnected')}</span>
  {build?.branch&&build.branch!=='unknown'&&<span className="hidden max-w-[185px] truncate rounded-md border border-[#E7E0D5] bg-[#F7F3EC] px-2 py-1 text-[10px] font-mono text-[#8A8176] lg:block" title={build.branch+' · '+build.commit}>{build.branch.replace(/^dominion-/,'')} · {build.commit.slice(0,7)}</span>}
  <Button variant="ghost" size="icon" aria-label={t('rooms.close')} onClick={onClose}><X size={18}/></Button>
 </header>
}
/** Persistent toolbar navigation; independent of either session list. */
export function ConversationSwitch({mode,onChange}:{mode:'chat'|'rooms';onChange:(mode:'chat'|'rooms')=>void}){
 useLocale()
 return <nav className="conversation-switch" aria-label={t('shell.conversationSwitch')}><Button variant="ghost" aria-current={mode==='chat'?'page':undefined} onClick={()=>onChange('chat')}><MessageSquare size={16}/><span>{t('nav.chat')}</span></Button><Button variant="ghost" aria-current={mode==='rooms'?'page':undefined} onClick={()=>onChange('rooms')}><UsersRound size={16}/><span>{t('rooms.listTitle')}</span></Button></nav>
}
