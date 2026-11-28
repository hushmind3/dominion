import {useEffect,useState} from 'react'
import {get,post} from '../../../api'
export type InstallActivity={label:string;at:number;stage?:string;status?:string}
export type Installation={id:string;packageName:string;version:string;status:'running'|'completed'|'failed';startedAt:number;finishedAt?:number;activities:InstallActivity[];error?:string}
// One persisted installation receipt survives navigation; UI never invents progress.
const receipts=new Map<string,Installation>()
export function usePluginInstall(onChanged:()=>void){
 const [entries,setEntries]=useState(()=>new Map(receipts)),[error,setError]=useState('')
 useEffect(()=>{let live=true;void get<Installation[]>('/api/plugins/operations').then(rows=>{if(!live)return;for(const row of rows){const previous=receipts.get(row.packageName);if(!previous||row.startedAt>previous.startedAt||(row.id===previous.id&&row.status!=='running'))receipts.set(row.packageName,row)}setEntries(new Map(receipts))}).catch(e=>{if(live)setError(e.message)});return()=>{live=false}},[])
 useEffect(()=>{
  if(![...entries.values()].some(row=>row.status==='running'))return
  let live=true
  const timer=setInterval(()=>{void Promise.all([...receipts.values()].filter(row=>row.status==='running').map(async row=>{
   try{const next=await get<Installation>('/api/plugins/operations/'+encodeURIComponent(row.id));if(!live)return;receipts.set(next.packageName,next);setEntries(new Map(receipts));if(next.status!=='running')onChanged()}
   catch(e){if(live)setError((e as Error).message)}
  }))},1000)
  return()=>{live=false;clearInterval(timer)}
 },[entries,onChanged])
 const install=async(name:string,version:string)=>{
  if(receipts.get(name)?.status==='running')return
  setError('')
  try{const row=await post<Installation>('/api/plugins/install',{packageName:name,version});receipts.set(name,row);setEntries(new Map(receipts))}catch(e){setError((e as Error).message)}
 }
 return {entries,error,install}
}
