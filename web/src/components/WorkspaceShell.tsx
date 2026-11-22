import {useEffect, useState} from 'react'
import {ArrowLeft, ChevronDown, Menu, MessageSquare, Search, UsersRound, X} from 'lucide-react'
import {OwnerProfileEditor} from './OwnerProfile'
import {Button, TextInput} from './Primitives'
import {t, useLocale} from '../i18n'
import {settingsGroups, settingsPages, isSettingsPage, type SettingsPage} from '../workspacePages'

const primaryPages: SettingsPage[] = ['home', 'models', 'memory', 'settings']
type SidebarProps = {
  page: SettingsPage
  navOpen: boolean
  online: boolean
  connecting: boolean
  running: number
  pending: number
  onNavigate: (page: SettingsPage) => void
  onToggleNav: () => void
}

export function WorkspaceSidebar({page, navOpen, online, connecting, running, pending, onNavigate, onToggleNav}: SidebarProps) {
  useLocale()
  const [query, setQuery] = useState('')
  const [expanded, setExpanded] = useState(!primaryPages.includes(page))
  useEffect(() => {if (!primaryPages.includes(page)) setExpanded(true)}, [page])
  const needle = navOpen ? query.trim().toLocaleLowerCase() : ''
  const visible = (id: SettingsPage) => !needle || [t(settingsPages[id].label), t(settingsPages[id].description)].join(' ').toLocaleLowerCase().includes(needle)
  const secondaryPages = Object.keys(settingsPages).filter(isSettingsPage).filter(id => !primaryPages.includes(id))
  const renderItem = (id: SettingsPage) => {
    const entry = settingsPages[id]
    const Icon = entry.icon
    const count = id === 'activity' ? running : id === 'settings' ? pending : 0
    return <Button variant="bare" key={id} aria-label={t(entry.label)} aria-current={page === id ? 'page' : undefined} title={t(entry.label)}
      onClick={() => onNavigate(id)} className="workspace-nav-item">
      <Icon size={18} strokeWidth={1.65}/><span>{t(entry.label)}</span>{count > 0 && <small>{count}</small>}
    </Button>
  }
  const matches = [...primaryPages, ...secondaryPages].filter(visible)
  return <aside aria-label={t('shell.mainNav')} className="workspace-sidebar" data-collapsed={!navOpen}>
    <div className="workspace-sidebar-brand"><span className="workspace-logo">D</span><strong>dominion</strong>
      <Button variant="ghost" size="icon" aria-label={navOpen ? t('shell.hideMenu') : t('shell.showMenu')} aria-expanded={navOpen} onClick={onToggleNav}><Menu size={17}/></Button>
    </div>
    {navOpen && <label className="workspace-nav-search"><Search size={15}/><TextInput value={query} onChange={event => setQuery(event.target.value)} aria-label={t('workspace.search')} placeholder={t('workspace.search')}/></label>}
    <nav className="workspace-nav">
      {navOpen && <h2>{t('workspace.personal')}</h2>}
      {primaryPages.filter(visible).map(renderItem)}
      {navOpen ? <>
        <Button variant="bare" className="workspace-nav-more" aria-expanded={expanded || !!needle} onClick={() => setExpanded(value => !value)}>
          {t('workspace.more')}<ChevronDown size={14} className={expanded || needle ? 'rotate-180' : ''}/>
        </Button>
        {(expanded || !!needle) && secondaryPages.filter(visible).map(renderItem)}
        {!matches.length && <p className="p-3 text-xs leading-5 text-muted">{t('history.noResults')}</p>}
      </> : secondaryPages.map(renderItem)}
    </nav>
    <div className="workspace-sidebar-footer">
      <span className="workspace-connection" data-online={online} role="status" title={connecting ? t('shell.checkingConnection') : online ? t('shell.connected') : t('shell.disconnected')}><i/>{navOpen && (connecting ? t('shell.checkingConnection') : online ? t('shell.connected') : t('shell.disconnected'))}</span>
      <OwnerProfileEditor compact={!navOpen}/>
    </div>
  </aside>
}

export function SettingsHeader({page, onNavigate, online, connecting, onClose}: {
  page: SettingsPage
  onNavigate: (page: SettingsPage) => void
  online: boolean
  connecting: boolean
  onClose: () => void
}) {
  useLocale()
  const selected = settingsPages[page]
  return <header className="workspace-settings-header">
    <div className="workspace-settings-heading"><span className="workspace-settings-eyebrow">{t('nav.settings')}</span><h1>{t(selected.label)}</h1><p>{t(selected.description)}</p></div>
    <Button variant="secondary" onClick={onClose} className="workspace-back"><ArrowLeft size={16}/><span>{t('workspace.back')}</span></Button>
    <select className="workspace-mobile-nav" aria-label={t('workspace.search')} value={page} onChange={event => {if (isSettingsPage(event.target.value)) onNavigate(event.target.value)}}>
      {Array.from(settingsGroups, ([group, entries]) => <optgroup key={group} label={t(group)}>{entries.map(entry => <option key={entry.id} value={entry.id}>{t(entry.label)}</option>)}</optgroup>)}
    </select>
    {!online && !connecting && <span className="workspace-offline" role="status">{t('shell.disconnected')}</span>}
    <Button variant="ghost" size="icon" className="workspace-close" onClick={onClose} aria-label={t('rooms.close')}><X size={18}/></Button>
  </header>
}

/** One mode switch can be placed in each conversation navigation. */
export function ConversationSwitch({mode, onChange}: {mode: 'chat' | 'rooms'; onChange: (mode: 'chat' | 'rooms') => void}) {
  useLocale()
  return <nav className="conversation-switch" aria-label={t('shell.conversationSwitch')}>
    <Button variant="ghost" aria-current={mode === 'chat' ? 'page' : undefined} onClick={() => onChange('chat')}><MessageSquare size={16}/><span>{t('nav.chat')}</span></Button>
    <Button variant="ghost" aria-current={mode === 'rooms' ? 'page' : undefined} onClick={() => onChange('rooms')}><UsersRound size={16}/><span>{t('rooms.listTitle')}</span></Button>
  </nav>
}
