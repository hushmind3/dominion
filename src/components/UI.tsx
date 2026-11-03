import React,{useEffect,useState} from 'react'
import {ArrowUpRight,LoaderCircle,Plus} from 'lucide-react'
import {get} from '../api'
export function Panel({children,className='',...rest}:{children:React.ReactNode;className?:string;[key:string]:any}){return <section className={'panel '+className} {...rest}>{children}</section>}
export function Headline({eyebrow,title,description,actions}:{eyebrow?:string;title:string;description?:string;actions?:React.ReactNode}){return <header className="page-heading"><div><div className="eyebrow">{eyebrow||'DOMINION WORKSPACE'}</div><h1>{title}</h1>{description&&<p>{description}</p>}</div>{actions&&<div className="heading-actions">{actions}</div>}</header>}
export function Action({children,onClick,variant='primary',disabled=false,type='button',className=''}:{children:React.ReactNode;onClick?:()=>void;variant?:'primary'|'secondary'|'ghost'|'danger';disabled?:boolean;type?:'button'|'submit';className?:string}){return <button type={type} className={'action '+variant+' '+className} onClick={onClick} disabled={disabled}>{children}</button>}
export function Subheading({title,extra}:{title:string;extra?:React.ReactNode}){return <div className="subheading"><h3>{title}</h3>{extra}</div>}
export function FormField({label,children,hint}:{label:string;children:React.ReactNode;hint?:string}){return <label className="form-field"><span>{label}</span>{children}{hint&&<small>{hint}</small>}</label>}
export function Empty({icon:Icon=Plus,title,description}:{icon?:any;title:string;description?:string}){return <div className="empty"><span className="empty-icon"><Icon size={24}/></span><strong>{title}</strong><p>{description}</p></div>}
export function StateTag({online,children}:{online?:boolean;children:React.ReactNode}){return <span className={'state-tag '+(online?'online':'')}><i/>{children}</span>}
export function useRemote<T>(endpoint:string,initial:T,refreshOnEvent=true){
 const [value,setValue]=useState<T>(initial),[loading,setLoading]=useState(true),[error,setError]=useState('');
 const reload=async()=>{try{setValue(await get<T>(endpoint));setError('')}catch(e){setError(String((e as Error).message))}finally{setLoading(false)}};
 useEffect(()=>{void reload();if(!refreshOnEvent)return;const h=()=>void reload();window.addEventListener('dominion-refresh',h);return()=>window.removeEventListener('dominion-refresh',h)},[endpoint,refreshOnEvent]);
 return {value,setValue,loading,error,reload};
}
export const refresh=()=>window.dispatchEvent(new Event('dominion-refresh'))
export function Notice({message}:{message:string}){return message?<div className="notice">{message}</div>:null}
export function InfoRow({label,value}:{label:string;value:React.ReactNode}){return <div className="info-row"><span>{label}</span><strong>{value}</strong></div>}
export function Loading(){return <span className="loading"><LoaderCircle size={16} className="spin"/> 처리 중</span>}
export function ArrowLink({children,onClick}:{children:React.ReactNode;onClick:()=>void}){return <button className="arrow-link" onClick={onClick}>{children}<ArrowUpRight size={15}/></button>}
