import {useLayoutEffect, type KeyboardEvent, type ReactNode, type RefObject} from 'react'
import {Button,TextArea,cn,type ButtonProps} from '../Primitives'

type Props = {
 value: string; onChange: (value: string) => void; onSend: () => void | Promise<void>
 inputRef: RefObject<HTMLTextAreaElement | null>; label: string; placeholder: string
 options: ReactNode; actions: ReactNode; disabled?: boolean
 onKeyDown?: (event: KeyboardEvent<HTMLTextAreaElement>) => void
 slashOpen?: boolean
}

/** Direct and group chat use the same input, keyboard and responsive layout. */
export function ConversationComposer({value, onChange, onSend, inputRef, label, placeholder, options, actions, disabled, onKeyDown, slashOpen}: Props) {
 useLayoutEffect(() => {
  const input = inputRef.current
  if (!input) return
  input.style.height = 'auto'
  input.style.height = Math.min(input.scrollHeight, 180) + 'px'
 }, [value, inputRef])
 return <form className="conversation-composer rounded-2xl border border-divider bg-surface shadow-sm focus-within:border-brand focus-within:ring-4 focus-within:ring-brand/10" onSubmit={event => {event.preventDefault(); void onSend()}}>
  <TextArea ref={inputRef} rows={1} value={value} disabled={disabled} aria-label={label} placeholder={placeholder}
   aria-controls={slashOpen ? 'dominion-slash-menu' : undefined} aria-expanded={slashOpen}
   className="conversation-composer-input block w-full !min-h-14 !max-h-44 !resize-none !rounded-2xl !border-0 !bg-surface !px-5 !pb-0 !pt-4 !text-base !leading-7 !text-ink !shadow-none !ring-0" onChange={event => onChange(event.target.value)}
   onKeyDown={event => {
    if (event.nativeEvent.isComposing || event.keyCode === 229) return
    if (onKeyDown) {onKeyDown(event); return}
    if (event.key === 'Enter' && !event.shiftKey) {event.preventDefault(); void onSend()}
   }}/>
  <div className="flex items-end justify-between gap-2 px-3 pb-3 pt-2">
   <div className="conversation-composer-options flex min-w-0 flex-1 flex-wrap items-center gap-1 gap-y-2">{options}</div>
   <div className="flex shrink-0 items-center gap-2 self-end">{actions}</div>
  </div>
 </form>
}

export function ComposerSendButton({className,variant='primary',...props}:ButtonProps) {
 return <Button size="icon" variant={variant} className={cn('conversation-composer-send !size-10 !rounded-full !p-0',className)} {...props}/>
}
