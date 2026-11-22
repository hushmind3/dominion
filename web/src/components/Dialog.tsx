import {useLayoutEffect,useRef,type HTMLAttributes,type ReactNode} from 'react'
import {t} from '../i18n'

const dialogs: symbol[] = []
let previousOverflow = ''

export function DialogBackdrop({children,onDismiss,...props}:HTMLAttributes<HTMLDivElement>&{
 children:ReactNode;onDismiss:()=>void
}){
 const dismiss = useRef(onDismiss)
 const backdrop = useRef<HTMLDivElement>(null)
 dismiss.current = onDismiss
 useLayoutEffect(() => {
  const id = Symbol()
  const root = document.documentElement
  if (!dialogs.length) {
   previousOverflow = root.style.overflow
   root.style.overflow = 'hidden'
  }
  dialogs.push(id)
  const origin = document.activeElement
  const focusable = () => Array.from(
   backdrop.current?.querySelectorAll<HTMLElement>(
    'a[href],button:not([disabled]),input:not([disabled]):not([type="hidden"]),select:not([disabled]),textarea:not([disabled]),[tabindex]:not([tabindex="-1"])'
   ) || []
  ).filter(node => node.getClientRects().length>0 && !node.closest('[inert]'))
  // A modal must own keyboard focus, even when navigation opened it from
  // the underlying chat, which becomes inert. Nested dialogs take priority.
  const initial = focusable()[0] || backdrop.current?.querySelector<HTMLElement>('[role="dialog"]')
  if (initial) {
   if (!initial.hasAttribute('tabindex') && initial.getAttribute('role')==='dialog') initial.tabIndex=-1
   initial.focus({preventScroll:true})
  }
  const onKeyDown = (event: KeyboardEvent) => {
   if (dialogs.at(-1) !== id || event.isComposing) return
   if (event.key === 'Escape') {
    event.preventDefault()
    dismiss.current()
   } else if (event.key === 'Tab') {
    const items = focusable()
    if (!items.length) {
     event.preventDefault()
     backdrop.current?.querySelector<HTMLElement>('[role="dialog"]')?.focus({preventScroll:true})
     return
    }
    const first=items[0],last=items[items.length-1]
    if (!backdrop.current?.contains(document.activeElement) ||
      (event.shiftKey && document.activeElement===first) ||
      (!event.shiftKey && document.activeElement===last)) {
     event.preventDefault()
     ;(event.shiftKey?last:first).focus({preventScroll:true})
    }
   }
  }
  document.addEventListener('keydown', onKeyDown)
  return () => {
   document.removeEventListener('keydown', onKeyDown)
   const top = dialogs.at(-1) === id
   dialogs.splice(dialogs.indexOf(id), 1)
   if (!dialogs.length) root.style.overflow = previousOverflow
   if (top && origin instanceof HTMLElement && origin.isConnected) origin.focus({preventScroll: true})
  }
 }, [])
 return <div ref={backdrop} {...props} className="fixed inset-0 z-[60] flex items-center justify-center bg-ink/45/50 p-4 backdrop-blur-[2px]"
  onMouseDown={event=>{if(event.target===event.currentTarget)onDismiss()}}>{children}</div>
}
export function DialogPanel({children,size='default',className='',...props}:HTMLAttributes<HTMLDivElement>&{
 children:ReactNode;size?:'default'|'wide'
}){
 return <div role="dialog" aria-modal="true" aria-label={props['aria-label']||t('common.settingsWindow')} {...props}
  className={['relative max-h-[90dvh] w-full overflow-y-auto overscroll-contain rounded-2xl border border-divider bg-white shadow-2xl',
   size==='wide'?'max-w-3xl':'max-w-xl',className].join(' ')}>{children}</div>
}
export function DialogHeader({children}: {children:ReactNode}){
 return <header className="flex items-center justify-between gap-3 border-b border-divider p-5">{children}</header>
}
export function DialogBody({children}: {children:ReactNode}){
 return <div className="grid gap-4 p-5">{children}</div>
}
export function DialogActions({children}: {children:ReactNode}){
 return <footer className="flex flex-wrap items-center justify-end gap-2 border-t border-divider p-5">{children}</footer>
}
