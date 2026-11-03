import {useCallback,useEffect,useState} from 'react'
import {get} from './api'
import {AppShell,type PageId} from './design/AppShell'
import {HomePage} from './pages/Home'
import {ChatPage} from './pages/Chat'
import {ModelHub} from './pages/Models'
import {AgentsPage,RoomsPage} from './pages/Agents'
import {WorkflowsPage} from './pages/Workflows'
import {MemoryPage} from './pages/Memory'
import {CapabilitiesPage} from './pages/Capabilities'
import {ExtensionsPage} from './pages/Extensions'
import {ConnectionsPage} from './pages/Connections'
import {OperationsPage} from './pages/Operations'

export default function App(){
 const [page,setPage]=useState<PageId>('home')
 const [session,setSession]=useState<string|undefined>()
 const [online,setOnline]=useState(false)
 const [running,setRunning]=useState(0)
 const [pending,setPending]=useState(0)
 const navigate=useCallback((destination:PageId,selectedSession?:string)=>{
  setPage(destination)
  if(selectedSession!==undefined)setSession(selectedSession)
 },[])
 const newChat=useCallback(()=>{setSession(undefined);setPage('chat')},[])
 useEffect(()=>{
  let live=true
  const check=()=>get<{jobs:number}>('/api/status').then(x=>{if(live){setOnline(true);setRunning(x.jobs||0)}}).catch(()=>{if(live)setOnline(false)})
  void check()
  const interval=setInterval(check,12000)
  const event=new EventSource('/api/events')
  event.onmessage=e=>{try{
   const data=JSON.parse(e.data)
   if(['record','message','job_event','audit','approval','channel_status','room_event'].includes(data.type)){
    window.dispatchEvent(new Event('dominion-refresh'))
    if(data.type==='approval')void get<unknown[]>('/api/approvals').then(x=>{if(live)setPending(x.length)}).catch(()=>{})
    if(data.type==='job_event')void check()
   }
  }catch{}}
  return()=>{live=false;event.close();clearInterval(interval)}
 },[])
 return <AppShell page={page} online={online} running={running} pending={pending} onNavigate={navigate} onNewChat={newChat}>
  {page==='home'&&<HomePage navigate={navigate}/>}
  {page==='chat'&&<ChatPage sessionId={session} onSession={setSession} openModels={()=>navigate('models')}/>}
  {page==='models'&&<ModelHub openChat={newChat}/>}
  {page==='agents'&&<AgentsPage/>}
  {page==='rooms'&&<RoomsPage openSession={id=>navigate('chat',id)}/>}
  {page==='workflows'&&<WorkflowsPage openSession={id=>navigate('chat',id)}/>}
  {page==='memory'&&<MemoryPage/>}
  {page==='capabilities'&&<CapabilitiesPage navigate={navigate}/>}
  {page==='extensions'&&<ExtensionsPage/>}
  {page==='connections'&&<ConnectionsPage/>}
  {(page==='automations'||page==='activity'||page==='settings')&&<OperationsPage section={page}/>}
 </AppShell>
}
