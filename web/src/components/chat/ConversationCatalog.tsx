import {createContext,useContext,type ReactNode} from 'react'
import type {Agent,Provider} from '../../api'
import {useRemote} from '../UI'

function useCatalog(){
 const agents=useRemote<Agent[]>('/api/agents',[])
 const providers=useRemote<Provider[]>('/api/providers',[])
 return {agents,providers}
}
const Context=createContext<ReturnType<typeof useCatalog>|null>(null)
/** One catalog subscription across both retained conversation pages. */
export function ConversationCatalogProvider({children}:{children:ReactNode}){
 const catalog=useCatalog()
 return <Context.Provider value={catalog}>{children}</Context.Provider>
}
export function useConversationCatalog(){
 const value=useContext(Context)
 if(!value)throw new Error('ConversationCatalogProvider missing')
 return value
}
