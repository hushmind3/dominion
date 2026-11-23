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

/** Shared prompt editor, now surfaced as tabs inside Memory. */
export function PromptsPage({section,embedded=false}:{section?:View;embedded?:boolean}={}){
 const en=useLocale()==='en';const tr=(ko:string,english:string)=>en?english:ko
 const [route,setRoute]=useState(readRoute)
 const active=section??route.view
 const [visited,setVisited]=useState<Set<View>>(()=>new Set([section??route.view]))
 useEffect(()=>{if(section)setVisited(previous=>new Set([...previous,section]))},[section])
 useEffect(()=>{
  const sync=()=>{const next=readRoute();setRoute(next);setVisited(previous=>new Set([...previous,next.view]))}
  window.addEventListener('hashchange',sync);window.addEventListener('popstate',sync)
  return()=>{window.removeEventListener('hashchange',sync);window.removeEventListener('popstate',sync)}
 },[])
 const choose=(view:View)=>{window.location.hash='#/memory?view='+view}
 const content=<>
  {!embedded&&<ViewTabs value={active} onChange={choose} label={tr('프롬프트와 페르소나','Prompts and personas')} options={[
   {id:'shared',label:tr('공통·시스템 지침','Shared & system instructions'),icon:FileText},
   {id:'personas',label:tr('AI·페르소나','AIs & personas'),icon:UsersRound},
   {id:'groups',label:tr('그룹 지침','Group instructions'),icon:UsersRound},
  ]}/>}
  {visited.has('shared')&&<div hidden={active!=='shared'}><SystemPromptPanel/></div>}
  {visited.has('personas')&&<div hidden={active!=='personas'}><AgentsPage embedded active={active==='personas'} focusAgentId={route.agentId}/></div>}
  {visited.has('groups')&&<div hidden={active!=='groups'}><GroupPromptPanel initialRoomId={route.roomId}/></div>}
 </>
 return embedded?<div className="min-w-0">{content}</div>:<PageContainer>{content}</PageContainer>
}
