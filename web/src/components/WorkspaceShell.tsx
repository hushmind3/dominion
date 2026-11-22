import {OwnerProfileEditor} from './OwnerProfile'
import {MessageSquare,Search,Settings,UsersRound} from 'lucide-react'
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
 const badge = running + pending
 return <header className="flex h-14 shrink-0 items-center gap-2 border-b border-divider bg-surface px-2 sm:px-3">
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
     className={'inline-flex h-9 shrink-0 items-center gap-1.5 rounded-lg px-2.5 text-[12px] transition-colors '+
      (active ? 'bg-canvas font-semibold text-ink' : 'text-muted hover:bg-canvas hover:text-ink')}>
     <Icon size={16}/><span className="hidden sm:inline">{t(item.label)}</span>
    </button>
   })}
  </nav>
  <div className="ml-auto flex shrink-0 items-center gap-1">
   {badge > 0 && <span className="hidden items-center gap-1.5 rounded-full border border-divider px-2 py-1 text-[10px] text-muted sm:inline-flex">
    <i className="size-1.5 animate-pulse rounded-full bg-emerald-500"/>{badge}
   </span>}
   <Button variant="ghost" size="icon" aria-label={locale === 'en' ? 'Search' : '검색'} title="⌘K" onClick={onSearch}>
    <Search size={17}/>
   </Button>
   <span className={'hidden size-1.5 rounded-full sm:block '+(online ? 'bg-emerald-500' : 'bg-rose-500')}/>
   <Button variant="ghost" size="icon" aria-label={t('nav.settings')} title={t('nav.settings')} onClick={() => onNavigate('settings')}>
    <Settings size={17}/>
   </Button>
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
