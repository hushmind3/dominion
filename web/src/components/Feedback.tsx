import {CheckCircle2,Info,TriangleAlert,X} from 'lucide-react'
import type {ReactNode} from 'react'
import {t} from '../i18n'

export function Skeleton({lines=3}:{lines?:number}){
 return <div role="status" aria-label={t('common.loading')} className="animate-pulse space-y-3 p-3">
  {Array.from({length:Math.max(1,Math.min(8,lines))},(_,i)=>
   <div key={i} className={'h-3 rounded-full bg-canvas '+(i===lines-1?'w-2/3':'w-full')}/>)}
 </div>
}
export function Feedback({message,kind='info',onDismiss}:{
 message:string;kind?:'info'|'success'|'error';onDismiss?:()=>void
}){
 if(!message)return null;
 const colors=kind==='success'?'border-success/20 bg-success-soft text-success':
  kind==='error'?'border-danger/20 bg-danger-soft text-danger':'border-divider bg-brand-soft text-brand-hover';
 const Icon=kind==='success'?CheckCircle2:kind==='error'?TriangleAlert:Info;
 return <div role={kind==='error'?'alert':'status'}
   className={'flex items-start gap-3 rounded-xl border p-3 text-xs leading-6 '+colors}>
   <Icon size={17} className="mt-0.5 shrink-0"/>
   <span className="min-w-0 flex-1">{message}</span>
   {onDismiss&&<button type="button" onClick={onDismiss} aria-label={t('common.dismiss')}
    className="inline-grid size-6 shrink-0 place-items-center rounded-lg hover:bg-black/5"><X size={14}/></button>}
  </div>
}
