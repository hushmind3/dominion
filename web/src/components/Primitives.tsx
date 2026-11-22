import {forwardRef, type ButtonHTMLAttributes, type HTMLAttributes, type InputHTMLAttributes, type ReactNode, type SelectHTMLAttributes, type TableHTMLAttributes, type TextareaHTMLAttributes} from 'react'

export const cn=(...classes:(string|false|null|undefined)[])=>classes.filter(Boolean).join(' ')
const focus='focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand focus-visible:ring-offset-2 focus-visible:ring-offset-surface'
const field='w-full min-w-0 rounded-xl border border-divider bg-surface px-3.5 py-2.5 text-sm text-ink placeholder:text-muted/65 transition-colors hover:border-muted/40 focus:border-brand focus:outline-none focus:ring-4 focus:ring-brand/10 disabled:cursor-not-allowed disabled:bg-canvas disabled:opacity-60'

export type ButtonProps=ButtonHTMLAttributes<HTMLButtonElement>&{variant?:'primary'|'secondary'|'ghost'|'danger'|'bare';size?:'default'|'sm'|'icon'}
const tones={primary:'border-brand bg-brand text-white shadow-sm hover:bg-brand-hover',secondary:'border-divider bg-surface text-ink hover:bg-canvas',ghost:'border-transparent bg-transparent text-muted hover:bg-canvas hover:text-ink',danger:'border-danger/20 bg-danger-soft text-danger hover:bg-danger/10',bare:''}
const sizes={default:'inline-flex min-h-10 items-center justify-center gap-2 rounded-xl border px-3.5 py-2 text-sm font-semibold',sm:'inline-flex min-h-9 items-center justify-center gap-1.5 rounded-lg border px-3 py-1.5 text-xs font-semibold',icon:'inline-flex size-10 shrink-0 items-center justify-center rounded-xl border'}
export const Button=forwardRef<HTMLButtonElement,ButtonProps>(function Button({className='',variant='secondary',size='default',type='button',...props},ref){
 return <button ref={ref} type={type} className={cn(variant!=='bare'&&sizes[size],tones[variant],focus,'transition-colors disabled:pointer-events-none disabled:opacity-45',className)} {...props}/>
})
export const TextInput=forwardRef<HTMLInputElement,InputHTMLAttributes<HTMLInputElement>>(function TextInput({className='',type='text',...props},ref){
 const special=['checkbox','radio','range','color','file','hidden'].includes(type)
 return <input ref={ref} type={type} className={cn(!special&&field,special&&'accent-brand',className)} {...props}/>
})
export const TextArea=forwardRef<HTMLTextAreaElement,TextareaHTMLAttributes<HTMLTextAreaElement>>(function TextArea({className='',...props},ref){return <textarea ref={ref} className={cn(field,'min-h-20 resize-y',className)} {...props}/>})
export const SelectInput=forwardRef<HTMLSelectElement,SelectHTMLAttributes<HTMLSelectElement>>(function SelectInput({className='',...props},ref){return <select ref={ref} className={cn(field,'cursor-pointer',className)} {...props}/>})
export const DataTable=forwardRef<HTMLTableElement,TableHTMLAttributes<HTMLTableElement>>(function DataTable({className='',...props},ref){return <table ref={ref} className={cn('w-full border-collapse text-left text-sm text-ink',className)} {...props}/>})
export function PageContainer({children,className=''}:{children:ReactNode;className?:string}){return <div className={cn('mx-auto w-full min-w-0 max-w-6xl px-4 py-6 sm:px-8',className)}>{children}</div>}
export function Card({children,className='',...props}:HTMLAttributes<HTMLElement>){return <section className={cn('min-w-0 rounded-2xl border border-divider bg-surface p-5',className)} {...props}>{children}</section>}
export function Stack({children,className=''}:{children:ReactNode;className?:string}){return <div className={cn('flex min-w-0 flex-col gap-4',className)}>{children}</div>}
export function Inline({children,className=''}:{children:ReactNode;className?:string}){return <div className={cn('flex min-w-0 flex-wrap items-center gap-2',className)}>{children}</div>}
