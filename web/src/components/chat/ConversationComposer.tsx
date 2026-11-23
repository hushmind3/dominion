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
 return <form className="conversation-composer group min-w-0 overflow-hidden rounded-[20px] border border-[#DFD8CD] bg-[#FFFDF9] shadow-[0_3px_18px_rgba(54,43,32,.06)] transition-[border-color,box-shadow] focus-within:border-[#C9A185] focus-within:shadow-[0_5px_28px_rgba(54,43,32,.11)]" onSubmit={event => {event.preventDefault(); void onSend()}}>
  <TextArea ref={inputRef} rows={1} value={value} disabled={disabled} aria-label={label} placeholder={placeholder}
   aria-controls={slashOpen ? 'dominion-slash-menu' : undefined} aria-expanded={slashOpen}
   className="conversation-composer-input block w-full !min-h-[72px] !max-h-44 !resize-none !rounded-none !border-0 !bg-transparent !px-5 !pb-2 !pt-5 !text-[15px] !leading-7 !text-[#44382E] !shadow-none !ring-0 placeholder:!text-[#AA9D8D]" onChange={event => onChange(event.target.value)}
   onKeyDown={event => {
    if (event.nativeEvent.isComposing || event.keyCode === 229) return
    if (onKeyDown) {onKeyDown(event); return}
    if (event.key === 'Enter' && !event.shiftKey) {event.preventDefault(); void onSend()}
   }}/>
  <div className="flex min-w-0 items-end justify-between gap-2 border-t border-[#F0E9DD] bg-[#FBF9F4]/80 px-3 py-2.5 sm:px-4">
   <div className="conversation-composer-options flex min-w-0 flex-1 flex-wrap items-center gap-1 gap-y-1.5">{options}</div>
   <div className="flex shrink-0 items-center gap-2 self-end">{actions}</div>
  </div>
 </form>
}

export function ComposerSendButton({className,variant='primary',...props}:ButtonProps) {
 return <Button size="icon" variant={variant} className={cn('conversation-composer-send !size-10 !rounded-full !p-0',className)} {...props}/>
}
