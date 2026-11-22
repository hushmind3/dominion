import {useEffect, useState, type ReactNode} from 'react'
import {createPortal} from 'react-dom'
import {MoreHorizontal} from 'lucide-react'
import type {ChatMessage} from '../../api'
import {useAnchoredPanel} from '../../hooks/useAnchoredPanel'
import {getLocale, t} from '../../i18n'
import {Button} from '../Primitives'
import {TokenUsage} from '../TokenUsage'

export function MessageInfo({message, children, active = true}: {message: ChatMessage; children?: ReactNode; active?: boolean}) {
 const [open, setOpen] = useState(false)
 const panel = useAnchoredPanel(open, () => setOpen(false), 320, 'top')
 useEffect(() => {
  if (!open) return
  const close = () => setOpen(false)
  window.addEventListener('hashchange', close)
  return () => window.removeEventListener('hashchange', close)
 }, [open])
 useEffect(() => { if (!active) setOpen(false) }, [active])
 const placed = Boolean(panel.position)
 useEffect(() => { if (open && placed) panel.panel.current?.focus({preventScroll: true}) }, [open, placed])
 const value = message.created_at
 const date = value ? new Date(value.includes('T') ? value : value.replace(' ', 'T') + 'Z') : undefined
 const timestamp = date && !Number.isNaN(date.getTime())
  ? date.toLocaleString(getLocale() === 'en' ? 'en-US' : 'ko-KR', {month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit'}) : ''
 return <>
  <Button ref={panel.trigger} variant="bare" className="inline-grid size-8 place-items-center rounded-lg align-middle text-muted hover:bg-canvas hover:text-ink aria-expanded:bg-canvas" aria-label={t('rooms.messageInfo')}
   title={t('rooms.messageInfo')} aria-haspopup="dialog" aria-expanded={open} onClick={() => setOpen(value => !value)}>
   <MoreHorizontal size={15}/>
  </Button>
  {active && open && panel.position && createPortal(
   <div ref={panel.panel} style={panel.position} role="dialog" tabIndex={-1} aria-label={t('rooms.messageInfo')} className="z-80 grid gap-3 overflow-y-auto rounded-xl border border-divider bg-surface p-4 text-xs leading-7 text-muted shadow-xl">
    {timestamp && <time dateTime={value}>{timestamp}</time>}
    <TokenUsage usage={message.metadata?.usage}/>
    {children}
   </div>, document.body)}
 </>
}
