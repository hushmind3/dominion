import type {ReactNode} from 'react'
import type {LucideIcon} from 'lucide-react'
import {Button} from './Primitives'

export type WorkspaceMetric={id:string;label:string;value:number|string;icon?:LucideIcon;loading?:boolean;onClick?:()=>void;attention?:boolean}

/** Presentational components receive live data and actions, never own API calls. */
export function WorkspaceHeader({title,description,actions,level=1}:{
 title:string;description?:string;actions?:ReactNode;level?:1|2|3
}){
 const Heading=level===3?'h3':level===2?'h2':'h1'
 return <header className="mb-5 flex flex-wrap items-center justify-between gap-4">
  <div className="min-w-0">
   <Heading className="m-0 text-2xl font-bold tracking-tight text-ink">{title}</Heading>
   {description&&<p className="mt-1 max-w-2xl text-sm text-muted">{description}</p>}
  </div>
  {actions&&<div className="flex flex-wrap items-center gap-2">{actions}</div>}
 </header>
}
export function WorkspaceMetrics({items,columns='four'}:{items:WorkspaceMetric[];columns?:'three'|'four'}){
 return <div className={columns==='three'?'grid grid-cols-3 gap-2 sm:gap-3':'grid grid-cols-2 gap-3 lg:grid-cols-4'}>
  {items.map(item=>{
   const Icon=item.icon
   const content=<>
    <span className="flex min-w-0 items-start justify-between gap-2 text-xs font-medium text-muted">
     <span className="min-w-0">{item.label}</span>
     {Icon&&<Icon size={17} className="shrink-0 text-brand" aria-hidden="true"/>}
    </span>
    {item.loading?<span className="h-7 w-14 animate-pulse rounded-md bg-canvas" aria-label="Loading"/>:
     <strong className={'mt-2 block text-2xl font-bold leading-none tabular-nums '+(item.attention?'text-amber-700':'text-ink')}>{item.value}</strong>}
   </>
   const style='flex min-h-24 min-w-0 flex-col items-start justify-between rounded-xl border border-divider bg-surface p-4 text-left shadow-sm'
   return item.onClick?
    <Button variant="bare" key={item.id} type="button" aria-busy={item.loading} onClick={item.onClick} className={style+' transition-colors hover:border-brand'}>{content}</Button>:
    <div key={item.id} aria-busy={item.loading} className={style}>{content}</div>
  })}
 </div>
}
