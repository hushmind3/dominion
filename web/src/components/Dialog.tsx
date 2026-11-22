import {useLayoutEffect,useRef,type HTMLAttributes,type ReactNode} from 'react'
import {t} from '../i18n'

const dialogs: symbol[] = []
let previousOverflow = ''

export function DialogBackdrop({children,onDismiss,...props}:HTMLAttributes<HTMLDivElement>&{
 children:ReactNode;onDismiss:()=>void
}){
 const dismiss = useRef(onDismiss)
 const container = useRef<HTMLDivElement>(null)
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
  const onKeyDown = (event: KeyboardEvent) => {
   if(dialogs.at(-1)!==id)return
   if(event.key==='Escape'&&!event.isComposing){event.preventDefault();dismiss.current();return}
   if(event.key!=='Tab')return
   const root=container.current
   if(!root)return
   const focusable=Array.from(root.querySelectorAll<HTMLElement>(
    'button:not([disabled]),a[href],input:not([disabled]),select:not([disabled]),textarea:not([disabled]),[tabindex]:not([tabindex="-1"])'
   )).filter(element=>element.getClientRects().length>0&&element.getAttribute('aria-hidden')!=='true')
   if(!focusable.length){event.preventDefault();root.focus();return}
   const first=focusable[0],last=focusable[focusable.length-1],current=document.activeElement
   if(!root.contains(current)){event.preventDefault();(event.shiftKey?last:first).focus()}
   else if(event.shiftKey&&current===first){event.preventDefault();last.focus()}
   else if(!event.shiftKey&&current===last){event.preventDefault();first.focus()}
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
 return <div {...props} ref={container} tabIndex={-1} className="fixed inset-0 z-[60] flex items-center justify-center bg-slate-950/50 p-4 backdrop-blur-[2px]"
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
 return <header className="flex items-center justify-between gap-3 border-b border-[#EFE9DF] p-5">{children}</header>
}
export function DialogBody({children}: {children:ReactNode}){
 return <div className="grid gap-4 p-5">{children}</div>
}
export function DialogActions({children}: {children:ReactNode}){
 return <footer className="flex flex-wrap items-center justify-end gap-2 border-t border-[#EFE9DF] p-5">{children}</footer>
}
