import {FeatureSearchButton} from './FeatureSearch'
import {useEffect, useState} from 'react'
import {ArrowLeft, ChevronDown, Menu, MessageSquare, Plug, Search, UsersRound} from 'lucide-react'
import {OwnerProfileEditor} from './OwnerProfile'
import {Button, TextInput} from './Primitives'
import {t, useLocale} from '../i18n'
import {settingsPages, isSettingsPage, type SettingsPage} from '../workspacePages'

const primaryPages: SettingsPage[] = ['home', 'models', 'extensions', 'memory', 'settings']
type SidebarProps = {
  page: SettingsPage
  navOpen: boolean
  online: boolean
  connecting: boolean
  running: number
  pending: number
  onNavigate: (page: SettingsPage) => void
  onToggleNav: () => void
  onClose: () => void
}

export function WorkspaceSidebar({page, navOpen, online, connecting, running, pending, onNavigate, onToggleNav, onClose}: SidebarProps) {
  useLocale()
  const [query, setQuery] = useState('')
  const [expanded, setExpanded] = useState(!primaryPages.includes(page))
  useEffect(() => {if (!primaryPages.includes(page)) setExpanded(true)}, [page])
  const needle = navOpen ? query.trim().toLocaleLowerCase() : ''
  const visible = (id: SettingsPage) => !needle || [t(settingsPages[id].label), t(settingsPages[id].description)].join(' ').toLocaleLowerCase().includes(needle)
  const secondaryPages = Object.keys(settingsPages).filter(isSettingsPage).filter(id => !primaryPages.includes(id)&&!('hidden' in settingsPages[id]&&settingsPages[id].hidden))
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
    <div className="workspace-sidebar-brand"><strong>dominion</strong>{navOpen&&<span className="workspace-feature-search"><FeatureSearchButton/></span>}
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
      <Button variant="ghost" size="icon" onClick={onClose} className="workspace-back" aria-label={t('workspace.back')} title={t('workspace.back')}><ArrowLeft size={18}/></Button>
      <span className="workspace-connection" data-online={online} role="status" title={connecting ? t('shell.checkingConnection') : online ? t('shell.connected') : t('shell.disconnected')}><i/>{navOpen && (connecting ? t('shell.checkingConnection') : online ? t('shell.connected') : t('shell.disconnected'))}</span>
      <OwnerProfileEditor compact={!navOpen}/>
    </div>
  </aside>
}

/** One mode switch can be placed in each conversation navigation. */
export function ConversationSwitch({mode, onChange}: {mode: 'chat' | 'rooms'; onChange: (mode: 'chat' | 'rooms') => void}) {
  useLocale()
  return <><nav className="conversation-switch" aria-label={t('shell.conversationSwitch')}>
    <Button variant="ghost" aria-current={mode === 'chat' ? 'page' : undefined} onClick={() => onChange('chat')}><MessageSquare size={16}/><span>{t('nav.chat')}</span></Button>
    <Button variant="ghost" aria-current={mode === 'rooms' ? 'page' : undefined} onClick={() => onChange('rooms')}><UsersRound size={16}/><span>{t('rooms.listTitle')}</span></Button>
  </nav><Button variant="ghost" onClick={()=>{window.location.hash='#/extensions'}}><Plug size={16}/><span>{t('nav.extensions')}</span></Button><FeatureSearchButton/></>
}
