import {PageHeader,EmptyState,SectionHeader} from './ui/WorkspaceUI'
import {t} from '../i18n'
import type React from 'react'
import {ArrowUpRight,LoaderCircle,Plus} from 'lucide-react'
import {Button} from './Primitives'
export {Surface as Panel} from './ui/WorkspaceUI'
export function Headline({eyebrow,title,description,actions}:{eyebrow?:string;title:string;description?:string;actions?:React.ReactNode}){return <div className="mb-6">{eyebrow&&<p className="mb-2 text-xs font-semibold tracking-wide text-muted">{eyebrow}</p>}<PageHeader title={title} description={description} actions={actions}/></div>}
export function Action({children,onClick,variant='primary',disabled=false,type='button',className=''}:{children:React.ReactNode;onClick?:()=>void;variant?:'primary'|'secondary'|'ghost'|'danger';disabled?:boolean;type?:'button'|'submit';className?:string}){return <Button type={type} variant={variant} className={className} onClick={onClick} disabled={disabled}>{children}</Button>}
export function Subheading({title,extra}:{title:string;extra?:React.ReactNode}){return <SectionHeader title={title} actions={extra}/>}
export function FormField({label,children,hint}:{label:string;children:React.ReactNode;hint?:string}){return <label className="flex min-w-0 flex-col gap-2 text-sm font-semibold text-ink"><span>{label}</span>{children}{hint&&<small className="text-xs font-normal leading-6 text-muted">{hint}</small>}</label>}
export function Empty({icon:Icon=Plus,title,description}:{icon?:any;title:string;description?:string}){return <EmptyState icon={Icon} title={title} description={description} framed={false}/>}
export function StateTag({online,children}:{online?:boolean;children:React.ReactNode}){return <span className={['inline-flex items-center gap-1.5 rounded-md px-2 py-1 text-[10px] font-semibold',online?'bg-success-soft text-success':'bg-canvas text-muted'].join(' ')}><i className={'size-1.5 rounded-full '+(online?'bg-success':'bg-muted')}/>{children}</span>}
export {useRemote} from '../hooks/useRemote'
export {refresh} from '../dataRefresh'
export function Notice({message,onRetry,retrying=false}:{message:string;onRetry?:()=>void;retrying?:boolean}){return message?<div role="alert" className="my-3 flex flex-wrap items-center gap-3 rounded-lg border border-danger/20 bg-danger-soft px-3 py-2 text-[12px] text-danger"><span className="min-w-0 flex-1">{message}</span>{onRetry&&<Button variant="ghost" size="sm" disabled={retrying} onClick={onRetry}>{retrying?<LoaderCircle size={14} className="animate-spin"/>:null}{t('history.retry')}</Button>}</div>:null}
export function InfoRow({label,value}:{label:string;value:React.ReactNode}){return <div className="flex flex-wrap items-center justify-between gap-3 border-b border-divider py-2 text-[12px]"><span className="text-muted">{label}</span><strong className="font-semibold">{value}</strong></div>}
export function Loading(){return <span className="inline-flex items-center gap-2 text-[12px] text-muted"><LoaderCircle size={14} className="animate-spin"/> {t('common.processing')}</span>}
export function ArrowLink({children,onClick}:{children:React.ReactNode;onClick:()=>void}){return <Button variant="ghost" onClick={onClick}>{children}<ArrowUpRight size={14}/></Button>}
