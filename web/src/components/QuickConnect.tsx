import {useState} from 'react'
import {ArrowRight, HardDrive, KeyRound, LoaderCircle} from 'lucide-react'
import {Button, SelectInput} from './Primitives'
import {t, useLocale} from '../i18n'

type Props = {
  busy: boolean
  pending: boolean
  loginUrl: string
  message: string
  onKey: (provider: string) => void
  onLocal: () => void
}

export function QuickConnect({busy, pending, loginUrl, message, onKey, onLocal}: Props) {
  useLocale()
  const [provider, setProvider] = useState('OpenAI')
  return <section className="quick-connect">
    <div className="quick-connect-intro"><h3>{t('connect.title')}</h3><p>{t('connect.description')}</p></div>
    <div className="quick-connect-grid">
<div className="quick-connect-option"><span className="quick-connect-icon"><KeyRound size={24}/></span><h4>{t('connect.keyTitle')}</h4><p>{t('connect.keyDescription')}</p>
        <SelectInput value={provider} onChange={event => setProvider(event.target.value)} aria-label={t('connect.provider')}><option>OpenAI</option><option>Anthropic</option><option>Google Gemini</option><option>OpenRouter</option><option>Groq</option><option>DeepSeek</option></SelectInput>
        <Button variant="secondary" onClick={() => onKey(provider)}>{t('connect.keyAction')}<ArrowRight size={15}/></Button>
      </div>
      <div className="quick-connect-option"><span className="quick-connect-icon"><HardDrive size={24}/></span><h4>{t('connect.localTitle')}</h4><p>{t('connect.localDescription')}</p>
        <Button variant="secondary" onClick={onLocal}>{t('connect.localAction')}<ArrowRight size={15}/></Button>
      </div>
    </div>
    {pending && <div className="quick-connect-progress" role="status"><LoaderCircle size={17} className="animate-spin"/><span>{t('connect.pendingDescription')}</span>{loginUrl && <a href={loginUrl} target="_blank" rel="noreferrer">{t('models.oauth.openBrowser')}<ArrowRight size={14}/></a>}</div>}
    {message && <p className="quick-connect-message" role="status">{message}</p>}
  </section>
}
