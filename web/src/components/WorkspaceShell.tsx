import {OwnerProfileEditor} from './OwnerProfile'
import {useEffect,useRef} from 'react'
import {MessageSquare,UsersRound,Plus,X,Settings,Menu,Search} from 'lucide-react'
import {Button} from './Primitives'
import {t,useLocale} from '../i18n'
import {navigation,navigationItems,type PageId} from '../navigation'

const items=navigationItems

export function WorkspaceSidebar({page,open,collapsed,online,running,pending,onNavigate,onNewChat,onClose,settingsOnly=false,navOpen=true,onToggleNav}:{settingsOnly?:boolean;page:PageId;open:boolean;collapsed:boolean;online:boolean;running:number;pending:number;onNavigate:(id:PageId)=>void;onNewChat:()=>void;onClose:()=>void;navOpen?:boolean;onToggleNav?:()=>void}){
 useLocale()
 const navigationRef=useRef<HTMLElement>(null)
 useEffect(()=>{
  if(!settingsOnly)return
  navigationRef.current?.querySelector<HTMLElement>('[aria-current="page"]')
   ?.scrollIntoView({block:'nearest',inline:'nearest',behavior:'instant'})
 },[page,settingsOnly])
 const compact=!navOpen
 return <>
  {!settingsOnly&&open&&<Button variant="bare" type="button" className="fixed inset-0 z-40 bg-slate-950/50 md:hidden" aria-label={t('shell.menuClose')} onClick={onClose}/>}
  <aside aria-label={t('shell.mainNav')} className={[
   'z-30 flex min-h-0 min-w-0 flex-col border-workspace-line bg-workspace-sidebar text-workspace-sidebar-text',
   'col-start-1 row-start-2 h-16 border-t md:col-start-1 md:row-start-1 md:h-full md:border-t-0 md:border-r',
   'transition-[width] duration-150',
   compact?'md:w-16':'md:w-56',
   !settingsOnly&&!open?'max-md:hidden':'',
   collapsed?'md:opacity-80':''
  ].join(' ')}>
   <div className={'hidden h-16 shrink-0 items-center gap-2 border-b border-workspace-line md:flex '+(compact?'justify-center px-1':'px-4')}>
    <div className="grid size-8 shrink-0 place-items-center rounded-lg bg-workspace-accent text-sm font-black text-workspace-sidebar">D</div>
    {!compact&&<strong className="min-w-0 flex-1 truncate text-xs tracking-[.14em] text-white">DOMINION</strong>}
    {settingsOnly&&onToggleNav&&<Button variant="ghost" size="icon" aria-label={navOpen?t('shell.hideMenu'):t('shell.showMenu')} aria-expanded={navOpen} onClick={onToggleNav}
     className="!size-8 !shrink-0 !text-workspace-sidebar-text hover:!bg-workspace-selected hover:!text-white"><Menu size={17}/></Button>}
   </div>
   {!settingsOnly&&<div className="hidden p-3 md:block"><Button variant="bare" type="button" onClick={onNewChat} className="flex h-10 w-full items-center justify-center gap-2 rounded-lg bg-workspace-accent text-xs font-semibold text-workspace-sidebar"><Plus size={16}/>{!compact&&t('shell.newChat')}</Button></div>}
   <nav ref={navigationRef} aria-label={t('shell.mainNav')} className="flex min-h-0 min-w-0 flex-1 items-center gap-1 overflow-x-auto overflow-y-hidden px-1.5 pb-0.5 pt-0.5 [scrollbar-width:none] md:mt-3 md:block md:overflow-x-hidden md:overflow-y-auto md:px-2 md:py-0 md:pb-5">
    {navigation.map(group=><section key={group.title} className="contents md:mt-4 md:block">
     <h2 className={'mb-2 hidden px-3 text-[10px] font-semibold text-workspace-muted md:block '+(compact?'md:sr-only':'')}>{t(group.title)}</h2>
     <div className="contents md:block md:space-y-1">{group.items.map(item=>{
      const Icon=item.icon
      const count=item.id==='activity'?running:item.id==='settings'?pending:0
      return <Button variant="bare" key={item.id} type="button" aria-current={page===item.id?'page':undefined} title={t(item.label)} onClick={()=>onNavigate(item.id)}
       className={[
        'group flex min-h-12 w-17 shrink-0 flex-col items-center justify-center gap-1 rounded-lg px-1 text-center text-[10px] leading-tight',
        'transition-colors md:min-h-10 md:w-full md:flex-row md:justify-start md:gap-3 md:px-3 md:text-left md:text-xs',
        compact?'md:justify-center md:px-0':'',
        page===item.id?'bg-workspace-selected font-semibold text-white ring-1 ring-workspace-line':'text-workspace-sidebar-text hover:bg-workspace-hover hover:text-white'
       ].join(' ')}>
       <Icon size={17} strokeWidth={1.8} className="shrink-0"/>
       <span className={'max-w-full truncate '+(compact?'md:sr-only':'md:min-w-0 md:flex-1')}>{t(item.label)}</span>
       {count>0&&<span className={'rounded bg-workspace-accent px-1 text-[10px] font-semibold text-workspace-sidebar '+(compact?'md:sr-only':'hidden md:inline')}>{count}</span>}
      </Button>
     })}</div>
    </section>)}
   </nav>
   <div className="hidden md:block"><OwnerProfileEditor/></div>
   <div className={'hidden h-13 shrink-0 items-center gap-2 border-t border-workspace-line text-xs text-workspace-sidebar-text md:flex '+(compact?'justify-center':'px-4')}>
    <span className={'size-2 shrink-0 rounded-full '+(online?'bg-emerald-400':'bg-rose-400')}/>
    {!compact&&<span>{online?t('shell.engineConnected'):t('shell.engineDisconnected')}</span>}
   </div>
  </aside>
 </>
}
export function SettingsHeader({page,online,build,onClose,onSearch}:{page:PageId;online:boolean;build?:{branch:string;commit:string}|null;onClose:()=>void;onSearch?:()=>void}){
 const locale=useLocale()
 const selected=items.find(x=>x.id===page)
 const SelectedIcon=selected?.icon||Settings
 return <header className="flex min-h-16 shrink-0 items-center gap-3 border-b border-divider bg-surface px-4 sm:px-6">
  <SelectedIcon size={18} className="text-muted"/><div className="min-w-0 flex-1"><p className="text-[10px] font-semibold uppercase tracking-wide text-muted">DOMINION / {locale==='en'?'Workspace':'워크스페이스'}</p><h1 className="mt-0.5 truncate text-sm font-semibold text-ink">{selected?t(selected.label):t('nav.settings')}</h1></div>
  <span className={'hidden items-center gap-1.5 text-[11px] sm:inline-flex '+(online?'text-emerald-700':'text-rose-600')}><i className={'size-1.5 rounded-full '+(online?'bg-emerald-500':'bg-rose-500')}/>{online?t('shell.connected'):t('shell.disconnected')}</span>
  {build?.branch&&build.branch!=='unknown'&&<span className="hidden max-w-48 truncate rounded-md border border-divider bg-canvas px-2 py-1 text-[10px] font-mono text-muted lg:block" title={build.branch+' · '+build.commit}>{build.branch.replace(/^dominion-/,'')} · {build.commit.slice(0,7)}</span>}
  {onSearch&&<Button variant="ghost" size="icon" aria-label={locale==='en'?'Search workspace menus':'도미니언 메뉴 검색'} title="Ctrl / ⌘ K" onClick={onSearch}><Search size={18}/></Button>}
  <Button variant="ghost" size="icon" aria-label={t('rooms.close')} onClick={onClose}><X size={18}/></Button>
 </header>
}
/** Persistent toolbar navigation; independent of either session list. */
export function ConversationSwitch({mode,onChange}:{mode:'chat'|'rooms';onChange:(mode:'chat'|'rooms')=>void}){
 useLocale()
 return <nav className="conversation-switch flex min-w-0 shrink items-center gap-1 whitespace-nowrap" aria-label={t('shell.conversationSwitch')}>
  <Button variant="ghost" aria-current={mode==='chat'?'page':undefined} onClick={()=>onChange('chat')}
   className={mode==='chat'?'!bg-canvas !font-semibold !text-ink':'!text-muted hover:!bg-canvas hover:!text-ink'}>
   <MessageSquare size={16}/><span>{t('nav.chat')}</span>
  </Button>
  <Button variant="ghost" aria-current={mode==='rooms'?'page':undefined} onClick={()=>onChange('rooms')}
   className={mode==='rooms'?'!bg-canvas !font-semibold !text-ink':'!text-muted hover:!bg-canvas hover:!text-ink'}>
   <UsersRound size={16}/><span>{t('rooms.listTitle')}</span>
  </Button>
 </nav>
}
