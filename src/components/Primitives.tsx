import React,{forwardRef} from 'react'

const focus='focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-500 focus-visible:ring-offset-2'
const field='w-full min-w-0 rounded-xl border border-slate-200 bg-white px-3.5 py-2.5 text-sm text-slate-800 placeholder:text-slate-400 shadow-sm transition-colors hover:border-slate-300 focus:border-blue-500 focus:outline-none focus:ring-2 focus:ring-blue-500/20 disabled:cursor-not-allowed disabled:bg-slate-100 disabled:opacity-70'

export type ButtonProps=React.ButtonHTMLAttributes<HTMLButtonElement>&{
 variant?:'primary'|'secondary'|'ghost'|'danger'
 size?:'default'|'sm'|'icon'
}
export const Button=forwardRef<HTMLButtonElement,ButtonProps>(function Button({className='',variant='secondary',size='default',type='button',...props},ref){
 const tones={
  primary:'border-blue-600 bg-blue-600 text-white hover:bg-blue-700 shadow-sm',
  secondary:'border-slate-200 bg-white text-slate-700 hover:bg-slate-50',
  ghost:'border-transparent bg-transparent text-slate-600 hover:bg-slate-100',
  danger:'border-rose-200 bg-rose-50 text-rose-700 hover:bg-rose-100'
 }
 const isIcon=size==='icon'||/\b(icon-button|mobile-close)\b/.test(className)
 const base=isIcon
  ?'inline-flex size-9 shrink-0 items-center justify-center rounded-lg border border-transparent text-slate-600 hover:bg-slate-100'
  :size==='sm'?'inline-flex min-h-8 items-center justify-center gap-1.5 rounded-lg border px-2.5 py-1.5 text-xs font-semibold transition-colors'
  :'inline-flex min-h-9 items-center justify-center gap-2 rounded-xl border px-3.5 py-2 text-sm font-semibold transition-colors'
 return <button ref={ref} type={type} className={[base,!isIcon&&tones[variant],focus,'disabled:pointer-events-none disabled:opacity-50',className].filter(Boolean).join(' ')} {...props}/>
})
export const TextInput=forwardRef<HTMLInputElement,React.InputHTMLAttributes<HTMLInputElement>>(function TextInput({className='',type='text',...props},ref){
 const special=['checkbox','radio','range','color','file','hidden'].includes(type)
 return <input ref={ref} type={type} className={[!special&&field,className].filter(Boolean).join(' ')} {...props}/>
})
export const TextArea=forwardRef<HTMLTextAreaElement,React.TextareaHTMLAttributes<HTMLTextAreaElement>>(function TextArea({className='',...props},ref){
 return <textarea ref={ref} className={[field,'min-h-24 resize-y',className].join(' ')} {...props}/>
})
export const SelectInput=forwardRef<HTMLSelectElement,React.SelectHTMLAttributes<HTMLSelectElement>>(function SelectInput({className='',...props},ref){
 return <select ref={ref} className={[field,'cursor-pointer',className].join(' ')} {...props}/>
})
export const DataTable=forwardRef<HTMLTableElement,React.TableHTMLAttributes<HTMLTableElement>>(function DataTable({className='',...props},ref){
 return <table ref={ref} className={['w-full border-collapse text-left text-sm text-slate-700',className].join(' ')} {...props}/>
})
export function PageContainer({children,className=''}:{children:React.ReactNode;className?:string}){
 return <div className={['mx-auto w-full max-w-7xl min-w-0 px-4 py-6 sm:px-7 lg:px-10 lg:py-10',className].join(' ')}>{children}</div>
}
export function Card({children,className=''}:{children:React.ReactNode;className?:string}){
 return <section className={['min-w-0 rounded-2xl border border-slate-200 bg-white p-5 shadow-sm',className].join(' ')}>{children}</section>
}
export function Stack({children,className=''}:{children:React.ReactNode;className?:string}){
 return <div className={['flex min-w-0 flex-col gap-4',className].join(' ')}>{children}</div>
}
export function Inline({children,className=''}:{children:React.ReactNode;className?:string}){
 return <div className={['flex min-w-0 flex-wrap items-center gap-3',className].join(' ')}>{children}</div>
}
