import {useEffect,useRef,useState} from 'react'
import {OwnerProfileEditor} from './OwnerProfile'
import {ChevronDown,MessageSquare,Search,Settings,UsersRound} from 'lucide-react'
import {Button} from './Primitives'
import {t,useLocale} from '../i18n'
import {navigationItems,pageLabels,type PageId} from '../navigation'

/** Primary surfaces stay visible as tabs; every other page lives in ⌘K. */
const primary: PageId[] = ['home', 'chat', 'rooms', 'models', 'agents']

export function WorkspaceTopBar({page,online,running,pending,onNavigate,onSearch}:{
 page: PageId; online: boolean; running: number; pending: number
 onNavigate: (id: PageId) => void; onSearch: () => void
}) {
 const locale = useLocale()
 const [moreOpen, setMoreOpen] = useState(false)
 const moreRef = useRef<HTMLDivElement>(null)
 const badge = running + pending
 const secondary: PageId[] = ['models','agents','connections','extensions','capabilities','memory','activity']
 useEffect(() => {
  if (!moreOpen) return
  const close = (event: MouseEvent) => {
   if (!moreRef.current?.contains(event.target as Node)) setMoreOpen(false)
  }
  const escape = (event: KeyboardEvent) => { if (event.key === 'Escape') setMoreOpen(false) }
  document.addEventListener('mousedown', close)
  document.addEventListener('keydown', escape)
  return () => { document.removeEventListener('mousedown', close); document.removeEventListener('keydown', escape) }
 }, [moreOpen])
 const menuItem = (id: PageId) => {
  const entry = navigationItems.find(item => item.id === id)
  if (!entry) return null
  const Icon = entry.icon
  const active = page === id
  return <button key={id} type="button" aria-current={active ? 'page' : undefined}
   onClick={() => { setMoreOpen(false); onNavigate(id) }}
   className={'flex w-full items-center gap-2.5 rounded-lg px-2.5 py-2 text-left text-[12.5px] transition-colors '+
    (active ? 'bg-canvas font-semibold text-ink' : 'text-ink hover:bg-canvas')}>
   <Icon size={16} className="text-muted"/>{t(entry.label)}
  </button>
 }
 return <header className="relative flex h-14 shrink-0 items-center gap-2 border-b border-divider bg-surface px-2 sm:px-3">
  <button type="button" onClick={() => onNavigate('home')} aria-label="DOMINION"
   className="flex shrink-0 items-center gap-2 rounded-lg px-1.5 py-1 transition-colors hover:bg-canvas">
   <span className="grid size-7 place-items-center rounded-lg bg-ink text-[11px] font-black text-white">D</span>
   <strong className="hidden text-[12px] font-semibold tracking-[.12em] text-ink lg:block">DOMINION</strong>
  </button>
  <nav aria-label={t('shell.mainNav')} className="flex min-w-0 flex-1 items-center gap-0.5 overflow-x-auto [scrollbar-width:none]">
   {primary.map(id => {
    const item = navigationItems.find(entry => entry.id === id)
    if (!item) return null
    const Icon = item.icon
    const active = page === id
    return <button key={id} type="button" aria-current={active ? 'page' : undefined}
     title={t(item.label)} onClick={() => onNavigate(id)}
     className={'inline-flex h-10 shrink-0 items-center gap-1.5 rounded-lg px-3 text-[13px] transition-colors '+
      (active ? 'bg-canvas font-semibold text-ink' : 'text-muted hover:bg-canvas hover:text-ink')}>
     <Icon size={17}/><span className="hidden sm:inline">{t(item.label)}</span>
    </button>
   })}
  </nav>
  <div className="ml-auto flex shrink-0 items-center gap-1">
   {badge > 0 && <span className="hidden items-center gap-1.5 rounded-full border border-divider px-2 py-1 text-[10px] text-muted sm:inline-flex">
    <i className="size-1.5 animate-pulse rounded-full bg-emerald-500"/>{badge}
   </span>}
   <Button variant="ghost" size="icon" aria-label={locale === 'en' ? 'Search' : '검색'} title="⌘K" onClick={onSearch}>
    <Search size={18}/>
   </Button>
   <div className="relative" ref={moreRef}>
    <Button variant="ghost" aria-label={t('shell.moreAria')} aria-expanded={moreOpen} onClick={() => setMoreOpen(open => !open)}
     className={moreOpen ? '!bg-canvas !text-ink' : ''}>
     <span className="hidden sm:inline">{t('shell.more')}</span><ChevronDown size={15} className={'transition-transform '+(moreOpen ? 'rotate-180' : '')}/>
    </Button>
    {moreOpen && <div role="menu" aria-label={t('shell.more')}
     className="absolute right-0 top-full z-50 mt-2 w-60 rounded-xl border border-divider bg-surface p-1.5 shadow-[0_12px_40px_rgb(17_19_24_/_0.12)]">
     {menuItem('models')}
     {menuItem('agents')}
     <div className="my-1.5 border-t border-divider"/>
     {menuItem('connections')}
     {menuItem('extensions')}
     {menuItem('capabilities')}
     <div className="my-1.5 border-t border-divider"/>
     {menuItem('activity')}
     {menuItem('memory')}
     <div className="my-1.5 border-t border-divider"/>
     {menuItem('settings')}
    </div>}
   </div>
   <span className={'hidden size-2 rounded-full sm:block '+(online ? 'bg-emerald-500' : 'bg-rose-500')}
    title={online ? t('shell.connected') : t('shell.disconnected')}/>
   <OwnerProfileEditor compact/>
  </div>
 </header>
}

export function SettingsHeader({page,online,build,onClose}:{
 page: PageId; online: boolean; build?: {branch: string; commit: string} | null; onClose: () => void
}) {
 const locale = useLocale()
 const selected = navigationItems.find(item => item.id === page)
 const SelectedIcon = selected?.icon || Settings
 return <header className="flex min-h-14 shrink-0 items-center gap-3 border-b border-divider bg-surface px-4 sm:px-6">
  <SelectedIcon size={17} className="text-muted"/>
  <div className="min-w-0 flex-1">
   <p className="text-[10px] font-semibold uppercase tracking-[.14em] text-muted">DOMINION / {locale === 'en' ? 'Workspace' : '워크스페이스'}</p>
   <h1 className="mt-0.5 truncate text-[14px] font-semibold text-ink">{t(pageLabels[page])}</h1>
  </div>
  <span className={'hidden items-center gap-1.5 text-[11px] sm:inline-flex '+(online ? 'text-emerald-700' : 'text-rose-600')}>
   <i className={'size-1.5 rounded-full '+(online ? 'bg-emerald-500' : 'bg-rose-500')}/>{online ? t('shell.connected') : t('shell.disconnected')}
  </span>
  {build?.branch && build.branch !== 'unknown' && <span className="hidden max-w-48 truncate rounded-md border border-divider bg-canvas px-2 py-1 font-mono text-[10px] text-muted lg:block" title={build.branch + ' · ' + build.commit}>{build.branch.replace(/^dominion-/, '')} · {build.commit.slice(0, 7)}</span>}
  <Button variant="ghost" size="icon" aria-label={t('rooms.close')} onClick={onClose}><span className="text-[16px] leading-none">×</span></Button>
 </header>
}

/** Persistent toolbar navigation; independent of either session list. */
export function ConversationSwitch({mode,onChange}:{mode:'chat'|'rooms';onChange:(mode:'chat'|'rooms')=>void}) {
 useLocale()
 return <nav className="conversation-switch flex min-w-0 shrink items-center gap-1 whitespace-nowrap" aria-label={t('shell.conversationSwitch')}>
  <Button variant="ghost" aria-current={mode === 'chat' ? 'page' : undefined} onClick={() => onChange('chat')}
   className={mode === 'chat' ? '!bg-canvas !font-semibold !text-ink' : '!text-muted hover:!bg-canvas hover:!text-ink'}>
   <MessageSquare size={16}/><span>{t('nav.chat')}</span>
  </Button>
  <Button variant="ghost" aria-current={mode === 'rooms' ? 'page' : undefined} onClick={() => onChange('rooms')}
   className={mode === 'rooms' ? '!bg-canvas !font-semibold !text-ink' : '!text-muted hover:!bg-canvas hover:!text-ink'}>
   <UsersRound size={16}/><span>{t('rooms.listTitle')}</span>
  </Button>
 </nav>
}
