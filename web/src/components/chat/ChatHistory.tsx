import {useMemo, useState} from 'react'
import {ArrowRight, LoaderCircle, MessageSquare, Plus, Search, Settings, X} from 'lucide-react'
import type {ChatSession} from '../../api'
import {t, useLocale} from '../../i18n'
import {Button, TextInput} from '../Primitives'
import {OwnerProfileEditor} from '../OwnerProfile'

type Props = {
  sessions: ChatSession[]
  selected?: string
  loading: boolean
  error: string
  onSelect: (session: ChatSession) => void
  onNew: () => void
  onSettings: () => void
  onClose: () => void
  onRetry: () => void
}

export function ChatHistory({sessions, selected, loading, error, onSelect, onNew, onSettings, onClose, onRetry}: Props) {
  useLocale()
  const [query, setQuery] = useState('')
  const groups = useMemo(() => {
    const needle = query.trim().toLocaleLowerCase()
    const today = new Date()
    today.setHours(0, 0, 0, 0)
    const week = new Date(today)
    week.setDate(week.getDate() - 7)
    const result: [string, ChatSession[]][] = [
      ['history.today', []], ['history.week', []], ['history.older', []],
    ]
    for (const session of sessions) {
      if (session.roomId || !(session.title || '').toLocaleLowerCase().includes(needle)) continue
      const time = new Date(session.updatedAt || session.createdAt || '').getTime()
      result[time >= today.getTime() ? 0 : time >= week.getTime() ? 1 : 2][1].push(session)
    }
    return result.filter(([, entries]) => entries.length)
  }, [sessions, query])

  return <>
    <div className="chat-history-brand"><span className="workspace-logo">D</span><strong>dominion<span> / </span></strong><Button variant="ghost" size="icon" className="ml-auto md:hidden" aria-label={t('chat.listCollapse')} onClick={onClose}><X size={18}/></Button></div>
    <Button className="chat-new-button" variant="primary" onClick={onNew}><Plus size={17}/>{t('chat.newChat')}</Button>
    <label className="chat-history-search">
      <Search size={15}/>
      <TextInput value={query} onChange={event => setQuery(event.target.value)} placeholder={t('history.search')} aria-label={t('history.search')}/>
      {query && <Button variant="bare" onClick={() => setQuery('')} aria-label={t('history.clear')}><X size={14}/></Button>}
    </label>
    <nav className="chat-history-list" aria-label={t('chat.recent')}>
      {loading && !sessions.length && <div className="chat-history-empty" role="status"><LoaderCircle size={17} className="animate-spin"/>{t('history.loading')}</div>}
      {error && <div className="chat-history-empty" role="alert"><p>{t('history.error')}</p><Button size="sm" onClick={onRetry}>{t('history.retry')}</Button></div>}
      {groups.map(([label, entries]) => <section key={label}>
        <h2>{t(label as 'history.today' | 'history.week' | 'history.older')}</h2>
        {entries.map(session => <Button variant="bare" key={session.id} onClick={() => onSelect(session)}
          aria-current={selected === session.id ? 'page' : undefined} title={session.title} className="chat-history-item">
          <MessageSquare size={15}/><span>{session.title || t('chat.sessionTitle')}</span><ArrowRight size={13}/>
        </Button>)}
      </section>)}
      {!loading && !error && !groups.length && <div className="chat-history-empty"><MessageSquare size={21}/><p>{t(query ? 'history.noResults' : 'history.empty')}</p></div>}
    </nav>
    <div className="chat-history-footer">
      <OwnerProfileEditor compact/>
      <Button variant="ghost" onClick={onSettings}><Settings size={16}/>{t('nav.settings')}</Button>
    </div>
  </>
}
