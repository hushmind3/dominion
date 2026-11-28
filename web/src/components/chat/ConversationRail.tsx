import {MessageSquare, PanelLeftOpen, Plus, Plug, Settings, UsersRound} from 'lucide-react'
import {Button} from '../Primitives'
import {t, useLocale} from '../../i18n'

export function ConversationRail({onExpand, onNew, onSwitchMode, onSettings, mode='chat', sidebarId='conversation-sidebar'}: {
  mode?: 'chat' | 'rooms'
  sidebarId?: string
  onExpand: () => void
  onNew: () => void
  onSwitchMode: (mode: 'chat' | 'rooms') => void
  onSettings: () => void
}) {
  useLocale()
  return <aside className="conversation-rail" aria-label={t('shell.mainNav')}>
    <Button variant="ghost" size="icon" onClick={onExpand} title={t('chat.listExpand')} aria-label={t('chat.listExpand')} aria-controls={sidebarId} aria-expanded="false"><PanelLeftOpen size={19}/></Button>
    <Button variant="ghost" size="icon" onClick={onNew} title={t(mode==='chat'?'chat.newChat':'rooms.create')} aria-label={t(mode==='chat'?'chat.newChat':'rooms.create')}><Plus size={19}/></Button>
    <Button variant="ghost" size="icon" aria-current={mode==='chat'?'page':undefined} onClick={() => onSwitchMode('chat')} title={t('nav.chat')} aria-label={t('nav.chat')}><MessageSquare size={18}/></Button>
    <Button variant="ghost" size="icon" aria-current={mode==='rooms'?'page':undefined} onClick={() => onSwitchMode('rooms')} title={t('rooms.listTitle')} aria-label={t('rooms.listTitle')}><UsersRound size={18}/></Button>
    <Button variant="ghost" size="icon" onClick={()=>{window.location.hash='#/extensions'}} title={t('nav.extensions')} aria-label={t('nav.extensions')}><Plug size={18}/></Button>
    <Button variant="ghost" size="icon" className="mt-auto" onClick={onSettings} title={t('nav.settings')} aria-label={t('nav.settings')}><Settings size={18}/></Button>
  </aside>
}
