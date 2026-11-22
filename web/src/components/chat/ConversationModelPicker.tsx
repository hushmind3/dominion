import {useEffect,useRef,useState} from 'react'
import {createPortal} from 'react-dom'
import {ArrowRight,Check,ChevronDown,LoaderCircle,Search,Sparkles,X} from 'lucide-react'
import type {Provider} from '../../api'
import {t,useLocale} from '../../i18n'
import {useAnchoredPanel} from '../../hooks/useAnchoredPanel'
import type {useProviderModels} from '../../hooks/useProviderModels'
import {Button,TextInput} from '../Primitives'

type Props = {
  active?: boolean
  providers: Provider[]
  providerId: string
  model: string
  catalogue: ReturnType<typeof useProviderModels>
  onChange: (providerId: string, model: string) => void
  onManage: () => void
}

export function ConversationModelPicker({active=true, providers, providerId, model, catalogue, onChange, onManage}: Props) {
  useLocale()
  const [open, setOpen] = useState(false)
  const [query, setQuery] = useState('')
  const search = useRef<HTMLInputElement>(null)
  const panel = useAnchoredPanel(open&&active, () => setOpen(false), 380, 'top')
  useEffect(()=>{if(!active)setOpen(false)},[active])
  const provider = providers.find(item => item.id === providerId)
  const selectedModel = catalogue.value.find(item => item.id === model)
  const visibleModels = catalogue.value.filter(item => (item.name + ' ' + item.id).toLowerCase().includes(query.trim().toLowerCase()))
  useEffect(() => {
    if (open && panel.position) search.current?.focus({preventScroll: true})
  }, [open, Boolean(panel.position)])
  useEffect(() => {setQuery('')}, [providerId, open])
  const manage = () => {setOpen(false); onManage()}
  return <div className="chat-ai-picker">
    <Button variant="ghost" ref={panel.trigger} onClick={() => providers.length ? setOpen(value => !value) : manage()}
      aria-label={t('welcome.chooseAi')} aria-haspopup="dialog" aria-expanded={open} className="chat-ai-trigger">
      <Sparkles size={16}/><span>{selectedModel?.name || model || t('welcome.chooseAi')}</span><ChevronDown size={13}/>
    </Button>
    {open && active && panel.position && createPortal(<div ref={panel.panel} style={panel.position} role="dialog" aria-label={t('welcome.chooseAi')} className="chat-ai-panel">
      <div className="chat-ai-panel-heading"><div><h2>{t('welcome.chooseAi')}</h2><p>{t('welcome.chooseAiDescription')}</p></div><Button variant="ghost" size="icon" onClick={() => setOpen(false)} aria-label={t('rooms.close')}><X size={17}/></Button></div>
      <div className="chat-ai-providers">{providers.map(item => <Button variant="bare" key={item.id} aria-pressed={item.id === providerId} onClick={() => onChange(item.id, item.model || '')}>
        <span>{item.name}</span>{item.id === providerId && <Check size={14}/>}
      </Button>)}</div>
      <div className="chat-ai-model"><label className="chat-ai-search"><Search size={15}/><TextInput ref={search} value={query} onChange={event => setQuery(event.target.value)} aria-label={t('welcome.chooseModel')} placeholder={t('picker.searchPlaceholder')}/></label>
        <div className="chat-ai-options">
          {catalogue.loading && <p role="status"><LoaderCircle size={15} className="animate-spin"/>{t('welcome.loading')}</p>}
          {catalogue.error && <div role="alert"><p>{t('welcome.modelError')}</p><Button size="sm" onClick={() => void catalogue.reload()}>{t('history.retry')}</Button></div>}
          {!catalogue.loading && !catalogue.error && visibleModels.map(item => <Button key={item.id} variant="bare" aria-pressed={item.id === model} onClick={() => {onChange(providerId, item.id); setOpen(false)}}>
            <span>{item.name || item.id}</span>{item.id === model && <Check size={15}/>}
          </Button>)}
          {!catalogue.loading && !catalogue.error && !visibleModels.length && <p>{t(query ? 'history.noResults' : 'welcome.noModels')}</p>}
        </div>
        {provider && <p className="chat-ai-source">{provider.name}</p>}
      </div>
      <Button variant="ghost" onClick={manage} className="chat-ai-manage">{t('welcome.manageAi')}<ArrowRight size={14}/></Button>
    </div>, document.body)}
  </div>
}
