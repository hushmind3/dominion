import {useEffect,useState} from 'react'
import {FileText,UsersRound} from 'lucide-react'
import {PageContainer} from '../components/Primitives'
import {ViewTabs} from '../components/ui/WorkspaceUI'
import {SystemPromptPanel} from './memory/SystemPromptPanel'
import {AgentsPage} from './Agents'
import {GroupPromptPanel} from '../features/prompts/GroupPromptPanel'
import {useLocale} from '../i18n'

type View='shared'|'personas'|'groups'
function readRoute(){
 const [path,query]=window.location.hash.split('?');const params=new URLSearchParams(query)
 const requested=path==='#/agents'?'personas':params.get('view')
 const view:View=requested==='personas'||requested==='groups'?requested:'shared'
 return {view,roomId:params.get('room')||undefined,agentId:params.get('agent')||undefined}
}

/** The only workspace that owns prompt and persona editing. */
export function PromptsPage(){
 const en=useLocale()==='en';const tr=(ko:string,english:string)=>en?english:ko
 const [route,setRoute]=useState(readRoute)
 const [visited,setVisited]=useState<Set<View>>(()=>new Set([route.view]))
 useEffect(()=>{
  const sync=()=>{const next=readRoute();setRoute(next);setVisited(previous=>new Set([...previous,next.view]))}
  window.addEventListener('hashchange',sync);window.addEventListener('popstate',sync)
  return()=>{window.removeEventListener('hashchange',sync);window.removeEventListener('popstate',sync)}
 },[])
 const choose=(view:View)=>{window.location.hash='#/prompts'+(view==='shared'?'':'?view='+view)}
 return <PageContainer>
  <ViewTabs value={route.view} onChange={choose} label={tr('프롬프트와 페르소나','Prompts and personas')} options={[
   {id:'shared',label:tr('공통·시스템 지침','Shared & system instructions'),icon:FileText},
   {id:'personas',label:tr('AI·페르소나','AIs & personas'),icon:UsersRound},
   {id:'groups',label:tr('그룹 지침','Group instructions'),icon:UsersRound},
  ]}/>
  {visited.has('shared')&&<div hidden={route.view!=='shared'}><SystemPromptPanel/></div>}
  {visited.has('personas')&&<div hidden={route.view!=='personas'}><AgentsPage embedded active={route.view==='personas'} focusAgentId={route.agentId}/></div>}
  {visited.has('groups')&&<div hidden={route.view!=='groups'}><GroupPromptPanel initialRoomId={route.roomId}/></div>}
 </PageContainer>
}
