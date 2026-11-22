import {useModalFocus} from '../hooks/useModalFocus'
import {useRef,type HTMLAttributes,type ReactNode} from 'react'
import {t} from '../i18n'

export function DialogBackdrop({children,onDismiss,...props}:HTMLAttributes<HTMLDivElement>&{
 children:ReactNode;onDismiss:()=>void
}){
 const container = useRef<HTMLDivElement>(null)
 useModalFocus(container,onDismiss)
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
 return <header className="flex items-center justify-between gap-3 border-b border-[var(--color-divider)] p-5">{children}</header>
}
export function DialogBody({children}: {children:ReactNode}){
 return <div className="grid gap-4 p-5">{children}</div>
}
export function DialogActions({children}: {children:ReactNode}){
 return <footer className="flex flex-wrap items-center justify-end gap-2 border-t border-[var(--color-divider)] p-5">{children}</footer>
}
