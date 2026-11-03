import React,{useEffect,useState} from 'react'
import {ArrowUpRight,LoaderCircle,Plus} from 'lucide-react'
import {get} from '../api'
import {Button} from './Primitives'

export function Panel({children,className='',...rest}:{children:React.ReactNode;className?:string;[key:string]:any}){
 return <section className={['min-w-0 rounded-2xl border border-slate-200 bg-white shadow-sm',className].join(' ')} {...rest}>{children}</section>
}
export function Headline({eyebrow,title,description,actions}:{eyebrow?:string;title:string;description?:string;actions?:React.ReactNode}){
 return <header className="mb-7 flex flex-wrap items-center justify-between gap-5">
  <div className="min-w-0"><p className="m-0 text-[11px] font-bold tracking-[.16em] text-blue-600">{eyebrow||'DOMINION WORKSPACE'}</p>
   <h1 className="my-2 text-2xl font-extrabold tracking-tight text-slate-900 sm:text-3xl">{title}</h1>
   {description&&<p className="m-0 max-w-3xl text-sm leading-6 text-slate-500">{description}</p>}</div>
  {actions&&<div className="flex flex-wrap items-center gap-2">{actions}</div>}
 </header>
}
export function Action({children,onClick,variant='primary',disabled=false,type='button',className=''}:{
 children:React.ReactNode;onClick?:()=>void;variant?:'primary'|'secondary'|'ghost'|'danger';disabled?:boolean;type?:'button'|'submit';className?:string
}){
 return <Button type={type} variant={variant} className={className.includes('full')?'w-full '+className:className} onClick={onClick} disabled={disabled}>{children}</Button>
}
export function Subheading({title,extra}:{title:string;extra?:React.ReactNode}){
 return <div className="mb-4 flex flex-wrap items-center justify-between gap-3"><h3 className="m-0 text-base font-bold tracking-tight text-slate-800">{title}</h3>{extra}</div>
}
export function FormField({label,children,hint}:{label:string;children:React.ReactNode;hint?:string}){
 return <label className="flex min-w-0 flex-col gap-2 text-sm font-medium text-slate-700">
  <span>{label}</span>{children}{hint&&<small className="text-xs font-normal leading-5 text-slate-500">{hint}</small>}
 </label>
}
export function Empty({icon:Icon=Plus,title,description}:{icon?:any;title:string;description?:string}){
 return <div className="flex min-h-44 flex-col items-center justify-center gap-2 p-7 text-center">
  <span className="mb-2 grid size-12 place-items-center rounded-2xl bg-blue-50 text-blue-500"><Icon size={24}/></span>
  <strong className="text-sm font-bold text-slate-700">{title}</strong>
  {description&&<p className="m-0 max-w-sm text-xs leading-5 text-slate-500">{description}</p>}
 </div>
}
export function StateTag({online,children}:{online?:boolean;children:React.ReactNode}){
 return <span className={['inline-flex items-center gap-2 rounded-lg px-2.5 py-1.5 text-xs font-semibold',online?'bg-emerald-50 text-emerald-700':'bg-slate-100 text-slate-600'].join(' ')}>
  <span className={['size-1.5 rounded-full',online?'bg-emerald-500':'bg-slate-400'].join(' ')}/>{children}
 </span>
}
export function useRemote<T>(endpoint:string,initial:T,refreshOnEvent=true){
 const [value,setValue]=useState<T>(initial),[loading,setLoading]=useState(true),[error,setError]=useState('');
 const reload=async()=>{try{setValue(await get<T>(endpoint));setError('')}catch(e){setError(String((e as Error).message))}finally{setLoading(false)}};
 useEffect(()=>{void reload();if(!refreshOnEvent)return;const h=()=>void reload();window.addEventListener('dominion-refresh',h);return()=>window.removeEventListener('dominion-refresh',h)},[endpoint,refreshOnEvent]);
 return {value,setValue,loading,error,reload};
}
export const refresh=()=>window.dispatchEvent(new Event('dominion-refresh'))
export function Notice({message}:{message:string}){
 return message?<div role="alert" className="my-3 rounded-xl border border-rose-200 bg-rose-50 px-4 py-3 text-sm text-rose-700">{message}</div>:null
}
export function InfoRow({label,value}:{label:string;value:React.ReactNode}){
 return <div className="flex flex-wrap items-center justify-between gap-3 border-b border-slate-100 py-2.5 text-sm">
  <span className="text-slate-500">{label}</span><strong className="font-semibold text-slate-800">{value}</strong>
 </div>
}
export function Loading(){return <span className="inline-flex items-center gap-2 text-xs text-slate-500"><LoaderCircle size={16} className="animate-spin"/> 처리 중</span>}
export function ArrowLink({children,onClick}:{children:React.ReactNode;onClick:()=>void}){
 return <Button variant="ghost" onClick={onClick}>{children}<ArrowUpRight size={15}/></Button>
}
