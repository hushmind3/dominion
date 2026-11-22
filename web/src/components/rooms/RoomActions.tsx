import {useEffect, useRef, useState, type ReactNode} from 'react'
import {createPortal} from 'react-dom'
import {MoreHorizontal} from 'lucide-react'
import {useAnchoredPanel} from '../../hooks/useAnchoredPanel'
import {t} from '../../i18n'
import {Button} from '../Primitives'

type Action = {label: string; icon: ReactNode; run: () => void}

export function RoomActions({actions, active}: {actions: Action[]; active: boolean}) {
 const [open, setOpen] = useState(false)
 const panel = useAnchoredPanel(open, () => setOpen(false), 232, 'top')
 const placed = Boolean(panel.position)
 const firstAction = useRef<HTMLButtonElement>(null)
 useEffect(() => { if (!active) setOpen(false) }, [active])
 useEffect(() => { if (open && placed) firstAction.current?.focus() }, [open, placed])
 return <>
  <Button ref={panel.trigger} variant="ghost" size="icon" className="room-actions-trigger"
   aria-label={t('workspace.roomActions')} aria-expanded={open} aria-haspopup="dialog"
   onClick={() => setOpen(value => !value)}><MoreHorizontal size={19}/></Button>
  {open && panel.position && createPortal(
   <div ref={panel.panel} style={panel.position} role="dialog" aria-label={t('workspace.roomActions')} className="room-actions-menu">
    {actions.map((action, index) => <Button key={action.label} ref={index === 0 ? firstAction : undefined} variant="ghost"
     onClick={() => {setOpen(false); action.run()}}>{action.icon}{action.label}</Button>)}
   </div>, document.body)}
 </>
}
