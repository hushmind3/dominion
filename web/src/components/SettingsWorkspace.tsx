import {useCallback, useState} from 'react'
import {DialogBackdrop} from './Dialog'
import {WorkspaceSidebar, SettingsHeader} from './WorkspaceShell'
import {AsyncPage} from './AsyncPage'
import {settingsPages, type PageId, type SettingsPage} from '../workspacePages'
import {t} from '../i18n'

type Props = {
  page: SettingsPage
  online: boolean
  connecting: boolean
  running: number
  pending: number
  onNavigate: (page: PageId, sessionId?: string) => void
  onClose: () => void
}

export function SettingsWorkspace({page, online, connecting, running, pending, onNavigate, onClose}: Props) {
  const [navOpen, setNavOpen] = useState(() => {
    try {return localStorage.getItem('dominion.settings.nav') !== '0'} catch {return true}
  })
  const toggleNav = useCallback(() => setNavOpen(previous => {
    const next = !previous
    try {localStorage.setItem('dominion.settings.nav', next ? '1' : '0')} catch {}
    return next
  }), [])
  return <DialogBackdrop onDismiss={onClose}>
    <div role="dialog" aria-modal="true" aria-label={t('nav.settings')} className="workspace-settings">
      <WorkspaceSidebar page={page} online={online} connecting={connecting} running={running} pending={pending} onNavigate={onNavigate} navOpen={navOpen} onToggleNav={toggleNav}/>
      <div className="workspace-settings-main">
        <SettingsHeader page={page} onNavigate={onNavigate} online={online} connecting={connecting} onClose={onClose}/>
        <div key={page} className="workspace-settings-content"><AsyncPage label={t('shell.loadingPage')}>{settingsPages[page].render(onNavigate)}</AsyncPage></div>
      </div>
    </div>
  </DialogBackdrop>
}
