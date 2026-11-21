import {useCallback,useState,type SetStateAction} from 'react'

// Drafts are isolated per conversation and per browser tab. An unsent owner
// message must never silently move to another chat when the history changes.
const PREFIX='dominion.chat.draft:'
const keyOf=(sessionId?:string)=>PREFIX+(sessionId||'new')
const read=(key:string)=>{try{return sessionStorage.getItem(key)||''}catch{return ''}}
const write=(key:string,value:string)=>{
 try {
  if(value)sessionStorage.setItem(key,value)
  else sessionStorage.removeItem(key)
 } catch {
  // Private browsing/storage restrictions must not prevent chatting.
 }
}

export function useChatDraft(sessionId?:string):[string,(next:SetStateAction<string>)=>void]{
 const key=keyOf(sessionId)
 const [current,setCurrent]=useState<{key:string;text:string}>(()=>({key,text:read(key)}))
 // Derive from scope in render; switching sessions never flashes the
 // previous session's unsent text even before an effect can run.
 const input=current.key===key?current.text:read(key)
 const setInput=useCallback((next:SetStateAction<string>)=>{
  const value=typeof next==='function'?next(read(key)):next
  write(key,value)
  setCurrent({key,text:value})
 },[key])
 return [input,setInput]
}
