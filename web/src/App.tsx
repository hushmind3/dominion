import {ContextMenu} from './components/ContextMenu'
import {OwnerProfileProvider} from './components/OwnerProfile'
import {WorkspaceSidebar,WorkspaceHeader,CommandPalette,navigation,type PageId} from './components/WorkspaceShell'
import {Suspense,lazy,useCallback,useEffect,useState,type ReactNode,Component} from 'react'
import {get} from './api'

const NativeDashboard=lazy(()=>import('./pages/NativeWorkspace').then(x=>({default:x.NativeDashboard})))
const NativeTasks=lazy(()=>import('./pages/NativeWorkspace').then(x=>({default:x.NativeTasks})))
const NativeGroups=lazy(()=>import('./pages/NativeWorkspace').then(x=>({default:x.NativeGroups})))
const NativeVideos=lazy(()=>import('./pages/NativeWorkspace').then(x=>({default:x.NativeVideos})))
const NativeImprovements=lazy(()=>import('./pages/NativeWorkspace').then(x=>({default:x.NativeImprovements})))
const NativeKnowledge=lazy(()=>import('./pages/NativeWorkspace').then(x=>({default:x.NativeKnowledge})))
const NativeConnections=lazy(()=>import('./pages/NativeWorkspace').then(x=>({default:x.NativeConnections})))
const NativeModels=lazy(()=>import('./pages/NativeSystem').then(x=>({default:x.NativeModels})))
const AgentsPage=lazy(()=>import('./pages/NativeAgents').then(x=>({default:x.NativeAgents})))
const WorkflowsPage=lazy(()=>import('./pages/NativeWorkflows').then(x=>({default:x.NativeWorkflows})))
const ExtensionsPage=lazy(()=>import('./pages/NativeSystem').then(x=>({default:x.NativeExtensions})))
const DeveloperPage=lazy(()=>import('./pages/NativeSystem').then(x=>({default:x.NativeDeveloper})))

const entry=document.querySelector<HTMLScriptElement>('script[type="module"][src]')?.getAttribute('src')||''
const entryPath=/\/assets\/[^/?]+\.js/.test(entry)?new URL(entry,document.baseURI).pathname:''
let recovering=false
function recoverPage(){
 if(recovering)return
 const key='dominion.ui.recovery'
 try{
  const previous=JSON.parse(sessionStorage.getItem(key)||'null') as {entry:string;when:number}|null
  if(previous?.entry===entryPath&&Date.now()-previous.when<15000)return
  sessionStorage.setItem(key,JSON.stringify({entry:entryPath,when:Date.now()}))
 }catch{}
 recovering=true
 window.location.reload()
}
window.addEventListener('vite:preloadError',event=>{event.preventDefault();recoverPage()})
class Boundary extends Component<{children:ReactNode},{failed:boolean}>{
 state={failed:false}
 static getDerivedStateFromError(){return {failed:true}}
 componentDidCatch(error:Error){
  if(/dynamically imported|loading chunk|failed to fetch.*module|module script/i.test(error.message))recoverPage()
 }
 render(){return this.state.failed?<div role="alert" className="m-6 rounded-xl bg-white p-5 text-[13px]">화면을 불러올 수 없습니다. <button className="ml-3 font-semibold text-[#C96442]" onClick={()=>window.location.reload()}>다시 열기</button></div>:this.props.children}
}
function locationState(){
 const raw=window.location.hash.replace(/^#\/?/,'').split('?')[0]
 return navigation.some(group=>group.items.some(item=>item.id===raw))?raw as PageId:'home'
}
export default function App(){
 const [page,setPage]=useState<PageId>(locationState)
 const [mobile,setMobile]=useState(false)
 const [collapsed,setCollapsed]=useState(()=>{try{return localStorage.getItem('dominion.nav.collapsed')==='1'}catch{return false}})
 const [palette,setPalette]=useState(false)
 const [online,setOnline]=useState(false)
 const [running,setRunning]=useState(0)
 const [build,setBuild]=useState<{branch:string;commit:string}|null>(null)
 const navigate=useCallback((next:PageId,taskId?:string)=>{
  const hash='#/'+next+(taskId?'?task='+encodeURIComponent(taskId):'')
  if(window.location.hash!==hash)window.history.pushState(null,'',hash)
  setPage(next);setMobile(false);setPalette(false)
 },[])
 useEffect(()=>{const sync=()=>setPage(locationState());window.addEventListener('hashchange',sync);window.addEventListener('popstate',sync);return()=>{window.removeEventListener('hashchange',sync);window.removeEventListener('popstate',sync)}},[])
 useEffect(()=>{
  let alive=true
  const poll=async()=>{
   try{
    const [health,tasks]=await Promise.all([get<{ok:boolean}>('/api/health'),get<{status:string}[]>('/api/tasks')])
    if(alive){setOnline(health.ok===true);setRunning(tasks.filter(t=>['queued','running'].includes(t.status)).length)}
   }catch{if(alive){setOnline(false);setRunning(0)}}
  }
  void poll()
  const timer=setInterval(()=>{if(document.visibilityState==='visible')void poll()},7000)
  const onFocus=()=>{void poll()}
  window.addEventListener('focus',onFocus)
  return()=>{alive=false;clearInterval(timer);window.removeEventListener('focus',onFocus)}
 },[])
 useEffect(()=>{const shortcut=(event:KeyboardEvent)=>{
  if((event.ctrlKey||event.metaKey)&&event.key.toLowerCase()==='k'){event.preventDefault();setPalette(x=>!x)}
  if(event.key==='Escape')setPalette(false)
 };window.addEventListener('keydown',shortcut);return()=>window.removeEventListener('keydown',shortcut)},[])
 const toggle=()=>{if(window.innerWidth<520){setMobile(x=>!x);return}
  const next=!collapsed;setCollapsed(next);try{localStorage.setItem('dominion.nav.collapsed',next?'1':'0')}catch{}
 }
 return <OwnerProfileProvider><div className="min-h-dvh bg-[#F6F3EC] font-sans text-[#24211D]">
   <ContextMenu/>
   <WorkspaceSidebar page={page} open={mobile} collapsed={collapsed} online={online} running={running} pending={0}
    onNavigate={navigate} onNewChat={()=>navigate('chat')} onClose={()=>setMobile(false)}/>
   <main className={'min-h-dvh min-w-0 transition-[margin] duration-200 '+(collapsed?'ml-0':'ml-[216px] max-[519px]:ml-0')}>
    <WorkspaceHeader page={page} online={online} collapsed={collapsed} build={build} onMenu={toggle} onSearch={()=>setPalette(true)}/>
    <div className={'min-w-0 page-'+page}>
     <Boundary key={page}><Suspense fallback={<div className="p-8 text-[13px] text-[#7C7469]">화면 불러오는 중…</div>}>
      {page==='home'&&<NativeDashboard navigate={navigate}/>}
      {page==='chat'&&<NativeTasks/>}
      {page==='rooms'&&<NativeGroups/>}
      {page==='video'&&<NativeVideos/>}
      {page==='rsi'&&<NativeImprovements/>}
      {page==='memory'&&<NativeKnowledge/>}
      {page==='models'&&<NativeModels/>}
      {(['connections','settings'] as PageId[]).includes(page)&&<NativeConnections/>}
      {page==='activity'&&<NativeTasks/>}
      {page==='agents'&&<AgentsPage/>}
      {page==='workflows'&&<WorkflowsPage openSession={id=>navigate('chat',id)}/>}
      {page==='extensions'&&<ExtensionsPage/>}
      {page==='developer'&&<DeveloperPage/>}
      {page==='automations'&&<NativeTasks/>}
      {page==='capabilities'&&<NativeConnections/>}
     </Suspense></Boundary>
    </div>
   </main>
   {palette&&<CommandPalette onNavigate={navigate} onClose={()=>setPalette(false)}/>}
 </div></OwnerProfileProvider>
}
