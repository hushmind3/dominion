import {useLayoutEffect,useRef,useState,type Dispatch,type SetStateAction} from 'react'

const conversationDrafts = new Map<string,string>()

/**
 * Keep unsent text attached to its conversation, not to the currently visible
 * textarea. An in-memory map avoids copying private drafts to local storage.
 */
export function useChatDraft(scope:string|undefined):[
 string, Dispatch<SetStateAction<string>>, (sessionId:string,submitted:string)=>void
] {
 const key=scope??''
 const drafts=useRef(conversationDrafts)
 const activeScope=useRef(key)
 const latest=useRef(conversationDrafts.get(key)??'')
 const migration=useRef<string|undefined>(undefined)
 const [value,updateValue]=useState(()=>conversationDrafts.get(key)??'')

 useLayoutEffect(()=>{
  if(activeScope.current===key)return
  const from=activeScope.current
  activeScope.current=key
  const restored=drafts.current.get(key)??''
  if(from===''&&migration.current===key){
   // Sending the first turn adopted the unsent follow-up into its new session.
   drafts.current.delete('')
   migration.current=undefined
  }
  latest.current=restored
  updateValue(restored)
 },[key])

 const setDraft:Dispatch<SetStateAction<string>>=update=>{
  const next=typeof update==='function'?update(latest.current):update
  latest.current=next
  drafts.current.set(activeScope.current,next)
  updateValue(next)
 }

 const adoptNewSession=(sessionId:string,submitted:string)=>{
  if(activeScope.current!=='')return
  const remainder=latest.current.trim()===submitted?'':latest.current
  drafts.current.set(sessionId,remainder)
  migration.current=sessionId
 }

 return [value,setDraft,adoptNewSession]
}
