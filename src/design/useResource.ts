import {useCallback,useEffect,useRef,useState} from 'react'
import {get} from '../api'

export function notifyDataChanged(){window.dispatchEvent(new Event('dominion-refresh'))}
export function useResource<T>(url:string,initial:T){
 const [value,setValue]=useState<T>(initial)
 const [loading,setLoading]=useState(true)
 const [error,setError]=useState('')
 const alive=useRef(true)
 const reload=useCallback(async()=>{
  try{const data=await get<T>(url);if(alive.current){setValue(data);setError('')}}
  catch(e){if(alive.current)setError((e as Error).message)}
  finally{if(alive.current)setLoading(false)}
 },[url])
 useEffect(()=>{alive.current=true;void reload();const listen=()=>void reload();window.addEventListener('dominion-refresh',listen);return()=>{alive.current=false;window.removeEventListener('dominion-refresh',listen)}},[reload])
 return {value,setValue,loading,error,reload}
}
export function useOperation(){
 const [working,setWorking]=useState(false)
 const [error,setError]=useState('')
 const run=useCallback(async<T,>(fn:()=>Promise<T>):Promise<T|undefined>=>{
  setError('');setWorking(true)
  try{const result=await fn();notifyDataChanged();return result}
  catch(e){setError((e as Error).message);return undefined}
  finally{setWorking(false)}
 },[])
 return {working,error,setError,run}
}
