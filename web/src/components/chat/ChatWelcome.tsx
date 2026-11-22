import {ArrowRight, FileText, Lightbulb, MessageSquare, Sparkles} from 'lucide-react'
import {Button} from '../Primitives'
import {t, useLocale} from '../../i18n'

type Props = {
  ready: boolean
  loading: boolean
  error?: string
  onRetry: () => void
  onConnect: () => void
  onPrompt: (prompt: string) => void
}

export function ChatWelcome({ready, loading, error, onRetry, onConnect, onPrompt}: Props) {
  useLocale()
  const suggestions = [
    {icon: Lightbulb, title: 'welcome.idea', prompt: 'welcome.ideaPrompt'},
    {icon: FileText, title: 'welcome.write', prompt: 'welcome.writePrompt'},
    {icon: MessageSquare, title: 'welcome.think', prompt: 'welcome.thinkPrompt'},
  ] as const

  return <div className="chat-welcome">
    <div className="chat-welcome-mark" aria-hidden="true"><Sparkles size={27} strokeWidth={1.5}/></div>
    <span className="chat-welcome-eyebrow">{t('welcome.eyebrow')}</span>
    <h1>{t('welcome.title')}</h1>
    <p className="chat-welcome-description">{t('welcome.description')}</p>
    {error && <div className="chat-connect-card" role="alert"><div><strong>{t('welcome.connectionLost')}</strong><p>{error}</p></div><Button variant="secondary" onClick={onRetry}>{t('history.retry')}</Button></div>}
    {!ready && !loading && !error && <div className="chat-connect-card">
      <div><strong>{t('welcome.connectTitle')}</strong><p>{t('welcome.connectDescription')}</p></div>
      <Button variant="primary" onClick={onConnect}>{t('welcome.connect')}<ArrowRight size={16}/></Button>
    </div>}
    {loading && <p role="status" className="text-sm text-muted">{t('welcome.loading')}</p>}
    <div className="chat-suggestions" aria-label={t('chat.examples')}>
      {suggestions.map(({icon: Icon, title, prompt}) => <Button
        variant="bare" key={title} className="chat-suggestion" onClick={() => onPrompt(t(prompt))}
      >
        <Icon size={19} strokeWidth={1.6}/>
        <strong>{t(title)}</strong>
        <span>{t(prompt)}</span>
        <ArrowRight size={15} className="chat-suggestion-arrow"/>
      </Button>)}
    </div>
    <p className="chat-welcome-note">{t('welcome.note')}</p>
  </div>
}
