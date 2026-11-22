// Browser-local defaults for private chat controls. Room tool/learning
// switches are saved separately by /api/rooms and never use these keys.
export type ConversationToggle='showReasoning'|'thinkingEnabled'|'toolsEnabled'|'autoLearn'
const defaults:Record<ConversationToggle,boolean>={
 showReasoning:false,thinkingEnabled:true,toolsEnabled:true,autoLearn:false,
}
export const conversationPreferenceEvent='dominion-conversation-preference-changed'
export const conversationToggleKey=(name:ConversationToggle)=>'dominion.chat.pref.'+name
export const conversationEffortKey='dominion.chat.pref.reasoningEffort'
const validEffort=(value:string)=>value===''||/^[a-z][a-z0-9_-]{0,63}$/.test(value)
function changed(){
 if(typeof window!=='undefined'&&typeof Event!=='undefined'){
  window.dispatchEvent(new Event(conversationPreferenceEvent))
 }
}
export function readConversationToggle(name:ConversationToggle):boolean {
 try{
  const value=localStorage.getItem(conversationToggleKey(name))
  if(value==='1')return true
  if(value==='0')return false
 }catch{}
 return defaults[name]
}
export function writeConversationToggle(name:ConversationToggle,value:boolean):boolean{
 try{
  localStorage.setItem(conversationToggleKey(name),value?'1':'0')
  changed()
  return true
 }catch{return false}
}
// Keep the owner's chosen tier across model switches, but only send it when
// the current model actually declares support for that tier.
export function compatibleConversationEffort(saved:string,supported:readonly string[]):string{
 return saved&&!supported.includes(saved)?'':saved
}
export function readConversationEffort():string{
 try{
  const value=localStorage.getItem(conversationEffortKey)||''
  return validEffort(value)?value:''
 }catch{return ''}
}
export function writeConversationEffort(value:string):boolean{
 if(!validEffort(value))return false
 try{
  localStorage.setItem(conversationEffortKey,value)
  changed()
  return true
 }catch{return false}
}
