import {useEffect,useState} from 'react'
import {
 conversationPreferenceEvent,conversationToggleKey,conversationEffortKey,
 readConversationToggle,writeConversationToggle,readConversationEffort,writeConversationEffort,
 type ConversationToggle,
} from '../conversationPreferences'

// Reloaded bundles, chat/room switching, and changes in another tab must
// display the same persisted owner choice; no server-side conversation data
// or existing Room settings are changed by these display preferences.
function usePreferenceSync<T>(key:string,read:()=>T):[T,(value:T)=>void] {
 const [value,setValue]=useState(read)
 useEffect(()=>{
  const sync=(event:Event)=>{
   if(event.type==='storage'){
    const changed=(event as StorageEvent).key
    if(changed!==null&&changed!==key)return
   }
   setValue(read())
  }
  window.addEventListener('storage',sync)
  window.addEventListener(conversationPreferenceEvent,sync)
  return()=>{
   window.removeEventListener('storage',sync)
   window.removeEventListener(conversationPreferenceEvent,sync)
  }
 },[key])
 return [value,setValue]
}

export function usePersistentConversationToggle(name:ConversationToggle):[boolean,(value:boolean)=>void] {
 const [value,setValue]=usePreferenceSync(conversationToggleKey(name),()=>readConversationToggle(name))
 return [value,(next:boolean)=>{setValue(next);writeConversationToggle(name,next)}]
}
export function usePersistentReasoningEffort():[string,(value:string)=>void] {
 const [value,setValue]=usePreferenceSync(conversationEffortKey,readConversationEffort)
 return [value,(next:string)=>{setValue(next);writeConversationEffort(next)}]
}
