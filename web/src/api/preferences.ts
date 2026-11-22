export const koreanReasoningKey='dominion.chat.reasoningKo'
export function koreanReasoningEnabled(){
 try{return localStorage.getItem(koreanReasoningKey)!=='0'}catch{return true}
}
