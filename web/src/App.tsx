import { ContextMenu } from './components/ContextMenu'
import {OwnerProfileProvider} from './components/OwnerProfile'
import {Component,lazy,Suspense,useCallback,useEffect,useLayoutEffect,useState,type ReactNode} from 'react'
import {useWorkspaceSignals} from './hooks/useWorkspaceSignals'
import {WorkspaceTopBar,SettingsHeader} from './components/WorkspaceShell'
import {isPageId,type PageId} from './navigation'
import {WorkspaceSearch} from './components/WorkspaceSearch'
const HomePage=lazy(()=>import('./pages/Home').then(module=>({default:module.HomePage})))
const CapabilitiesPage=lazy(()=>import('./pages/Capabilities').then(module=>({default:module.CapabilitiesPage})))
const MemoryPage=lazy(()=>import('./pages/Memory').then(module=>({default:module.MemoryPage})))
const ChatPage=lazy(()=>import('./pages/Chat').then(module=>({default:module.ChatPage})))
const ModelHub=lazy(()=>import('./pages/Models').then(module=>({default:module.ModelHub})))
const AgentsPage=lazy(()=>import('./pages/Agents').then(module=>({default:module.AgentsPage})))
const RoomsPage=lazy(()=>import('./pages/Rooms').then(module=>({default:module.RoomsPage})))
const ExtensionsPage=lazy(()=>import('./pages/Extensions').then(module=>({default:module.ExtensionsPage})))
const ConnectionsPage=lazy(()=>import('./pages/Connections').then(module=>({default:module.ConnectionsPage})))
const OperationsPage=lazy(()=>import('./pages/Operations').then(module=>({default:module.OperationsPage})))


// A running tab retains the entry bundle from the moment it was opened.
// After "npm run build", lazy-loaded page chunks have new hashed URLs, while
// this tab still points at the removed ones. React.lazy alone cannot recover.
const loadedEntry = document.querySelector<HTMLScriptElement>('script[type="module"][src]')?.getAttribute('src') || ''
const loadedBundle = /\/assets\/[^/?]+\.js(?:\?|$)/.test(loadedEntry)
 ? new URL(loadedEntry, document.baseURI).pathname : ''
let reloadScheduled = false
let lastVersionCheck = 0
let versionCheck: Promise<void> | undefined

function reloadUpdatedUi() {
 if (reloadScheduled) return
 // If a new bundle is itself broken, show the recovery button instead of
 // trapping the browser in an endless reload loop.
 const key = 'dominion.ui.autoreload'
 try {
  const last = JSON.parse(sessionStorage.getItem(key) || 'null') as {entry:string;at:number}|null
  if (last?.entry === loadedBundle && Date.now()-last.at < 15000) return
  sessionStorage.setItem(key,JSON.stringify({entry:loadedBundle,at:Date.now()}))
 } catch {}
 reloadScheduled = true
 window.location.reload()
}

function checkForUpdatedUi() {
 if (!loadedBundle || reloadScheduled) return Promise.resolve()
 if (versionCheck) return versionCheck
 if (Date.now() - lastVersionCheck < 1500) return Promise.resolve()
 lastVersionCheck=Date.now()
 versionCheck=(async()=>{
  try{
   // The Rust server already sends no-cache. Check the actual HTML entry,
   // not an API status flag or a timestamp that changes on every request.
   const response=await fetch('/',{cache:'no-store',headers:{Accept:'text/html'},signal:AbortSignal.timeout(2500)})
   if (!response.ok || !response.headers.get('content-type')?.includes('text/html')) return
   const next=new DOMParser().parseFromString(await response.text(),'text/html')
   const nextEntry=next.querySelector<HTMLScriptElement>('script[type="module"][src]')?.getAttribute('src')
   if (!nextEntry || !/\/assets\/[^/?]+\.js(?:\?|$)/.test(nextEntry)) return
   const nextBundle=new URL(nextEntry,window.location.origin).pathname
   if (nextBundle!==loadedBundle) reloadUpdatedUi()
  }catch{
   // The API or frontend may be restarting; navigation must remain usable.
  }
 })().finally(()=>{versionCheck=undefined})
 return versionCheck
}

// Vite emits this for missing lazy page assets after a rebuild.
window.addEventListener('vite:preloadError',event=>{
 event.preventDefault()
 reloadUpdatedUi()
})

class PageLoadBoundary extends Component<{children:ReactNode},{failed:boolean}>{
 state={failed:false}
 static getDerivedStateFromError(){return {failed:true}}
 componentDidCatch(error:Error){
  if (/dynamically imported module|importing a module script|loading chunk|failed to fetch.*module|module script/i.test(error.message)) {
   reloadUpdatedUi()
  }
 }
 render(){
  if (!this.state.failed) return this.props.children
  return <div role="alert" className="m-6 max-w-lg rounded-xl border border-[#E7DDCC] bg-white p-6 text-sm text-[#5C544A]">
   <p className="mb-3 font-semibold">화면을 불러오지 못했습니다. 업데이트 중이었을 수 있습니다.</p>
   <button type="button" className="rounded-lg bg-[#C96442] px-4 py-2 font-semibold text-white" onClick={()=>window.location.reload()}>최신 화면으로 다시 열기</button>
  </div>
 }
}

function locationState(){
 const raw=window.location.hash.replace(/^#\/?/,'')
 const [name,query]=raw.split('?')
 const valid=isPageId(name)
 const params=new URLSearchParams(query)
 return {page:(valid?name:'chat') as PageId,session:params.get('session')||undefined,
  agent:params.get('agent')||undefined,provider:params.get('provider')||undefined,model:params.get('model')||undefined}
}
export default function App(){
 const [{page,session,agent,provider,model},setRoute]=useState(locationState)
 const [chatRoute,setChatRoute]=useState(()=>{const route=locationState();return route.page==='chat'?route:{page:'chat' as PageId,session:undefined,agent:undefined,provider:undefined,model:undefined}})
 const [mode,setMode]=useState<'chat'|'rooms'>(()=>locationState().page==='rooms'?'rooms':'chat')
 const [visited,setVisited]=useState(()=>({chat:locationState().page!=='rooms',rooms:locationState().page==='rooms'}))
 const [chatDraft,setChatDraft]=useState(0)
 const [searchOpen,setSearchOpen]=useState(false)
 const settingsOpen=page!=='chat'&&page!=='rooms'
 const navigate=useCallback((target:PageId,sessionId?:string)=>{
  const next='#/'+target+(sessionId?'?session='+encodeURIComponent(sessionId):'')
  if(window.location.hash!==next)window.history.pushState(null,'',next)
  setRoute(locationState())
  // Hash navigation stays immediate. Reload only when the server's built
  // entry changed; never block menu clicks on a network request.
  void checkForUpdatedUi()
 },[])
 const {online,running,pending,engineBuild}=useWorkspaceSignals(navigate,setSearchOpen)
 useEffect(()=>{
  const onKey=(event:KeyboardEvent)=>{
   if((event.metaKey||event.ctrlKey)&&event.key.toLowerCase()==='k'){
    event.preventDefault();setSearchOpen(open=>!open)
   }
  }
  window.addEventListener('keydown',onKey)
  return()=>window.removeEventListener('keydown',onKey)
 },[])
 useEffect(()=>{
  const sync=()=>setRoute(locationState())
  window.addEventListener('hashchange',sync)
  window.addEventListener('popstate',sync)
  return()=>{window.removeEventListener('hashchange',sync);window.removeEventListener('popstate',sync)}
 },[])
 useLayoutEffect(()=>{
  if(page==='chat'){setChatRoute({page,session,agent,provider,model});setMode('chat');setVisited(previous=>previous.chat?previous:{...previous,chat:true})}
  if(page==='rooms'){setMode('rooms');setVisited(previous=>previous.rooms?previous:{...previous,rooms:true})}
 },[page,session,agent,provider,model])
 const returnToChat=useCallback(()=>{
  const params=new URLSearchParams()
  for(const key of ['session','agent','provider','model'] as const)if(chatRoute[key])params.set(key,chatRoute[key]!)
  const next='#/chat'+(params.size?'?'+params.toString():'')
  if(window.location.hash!==next)window.history.pushState(null,'',next)
  setRoute(locationState())
  void checkForUpdatedUi()
 },[chatRoute])
 const switchMode=useCallback((next:'chat'|'rooms')=>{if(next==='chat')returnToChat();else navigate('rooms')},[returnToChat,navigate])
 const closeSettings=useCallback(()=>switchMode(mode),[switchMode,mode])
 useEffect(()=>{
  if(!settingsOpen||searchOpen)return
  const onEscape=(event:KeyboardEvent)=>{
   if(event.key==='Escape'&&!(event.target instanceof HTMLInputElement)&&!(event.target instanceof HTMLTextAreaElement)){
    event.preventDefault()
    closeSettings()
   }
  }
  window.addEventListener('keydown',onEscape)
  return()=>window.removeEventListener('keydown',onEscape)
 },[settingsOpen,searchOpen,closeSettings])
 useEffect(()=>{
  const check=()=>{if(document.visibilityState==='visible')void checkForUpdatedUi()}
  window.addEventListener('focus',check)
  document.addEventListener('visibilitychange',check)
  return()=>{window.removeEventListener('focus',check);document.removeEventListener('visibilitychange',check)}
 },[])

 return <OwnerProfileProvider><div className="relative grid h-dvh min-h-0 w-full grid-rows-[3.5rem_minmax(0,1fr)] overflow-hidden bg-canvas text-ink">
  <ContextMenu/>
  {/* One light chrome owns the whole product. Chat and room trees stay mounted
      below it so drafts and active turns survive every menu switch. */}
  <WorkspaceTopBar page={page} online={online} running={running} pending={pending}
   onNavigate={target=>target==='chat'?returnToChat():navigate(target)}
   onSearch={()=>setSearchOpen(true)}/>
  <div className="min-h-0 min-w-0 overflow-hidden bg-canvas">
   <main hidden={settingsOpen} inert={settingsOpen||searchOpen} className="dominion-studio-conversations h-full min-h-0 min-w-0 overflow-hidden">
    {visited.chat&&<div hidden={mode!=='chat'} className="h-full page-chat">
     <PageLoadBoundary><Suspense fallback={<div role="status" className="p-8 text-sm">대화 불러오는 중…</div>}>
      <ChatPage key={chatDraft} active={mode==='chat'&&!settingsOpen}
       sessionId={chatRoute.session} initialAgent={chatRoute.agent}
       initialProvider={chatRoute.provider} initialModel={chatRoute.model}
       onSession={id=>{
        setChatRoute(previous=>({...previous,session:id}))
        if(locationState().page==='chat')navigate('chat',id)
       }} openModels={()=>navigate('models')} openSettings={()=>navigate('settings')}
       switchMode={switchMode}/>
     </Suspense></PageLoadBoundary>
    </div>}
    {visited.rooms&&<div hidden={mode!=='rooms'} className="h-full page-rooms">
     <PageLoadBoundary><Suspense fallback={<div role="status" className="p-8 text-sm">Room 불러오는 중…</div>}>
      <RoomsPage active={mode==='rooms'&&!settingsOpen} openSession={id=>navigate('chat',id)}
       openSettings={()=>navigate('settings')} switchMode={switchMode}/>
     </Suspense></PageLoadBoundary>
    </div>}
   </main>
   {settingsOpen&&<section aria-label="DOMINION workspace" inert={searchOpen}
      className="workspace-settings flex h-full min-h-0 min-w-0 flex-col overflow-hidden bg-canvas">
    <SettingsHeader page={page} online={online} build={engineBuild} onClose={closeSettings}/>
    <div className="workspace-settings-content min-h-0 min-w-0 flex-1 overflow-y-auto bg-canvas p-4 md:p-7">
     <PageLoadBoundary key={page}><Suspense fallback={<div role="status" className="p-8 text-sm">화면 불러오는 중…</div>}>
      {page==='home'&&<HomePage navigate={navigate}/>}
      {page==='capabilities'&&<CapabilitiesPage navigate={navigate}/>}
      {page==='memory'&&<MemoryPage/>}
      {page==='models'&&<ModelHub/>}
      {page==='agents'&&<AgentsPage/>}
      {page==='extensions'&&<ExtensionsPage/>}
      {page==='connections'&&<ConnectionsPage/>}
      {(page==='activity'||page==='settings')&&<OperationsPage section={page}/>}
     </Suspense></PageLoadBoundary>
    </div>
   </section>}
  </div>
  {searchOpen&&<WorkspaceSearch onNavigate={navigate} onDismiss={()=>setSearchOpen(false)}/>}
 </div></OwnerProfileProvider>
}
