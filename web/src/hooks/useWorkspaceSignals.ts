// Central shell health, scoped invalidation and keyboard shortcuts.
// Pages own their actual server state; only one app-level SSE connection exists.
import {useEffect,useState,type Dispatch,type SetStateAction} from 'react'
import {get} from '../api'
import {classifyRefreshEvent,emptyRefreshScope,mergeRefreshScopes} from '../refreshEvents'
import type {PageId} from '../components/WorkspaceShell'

export function useWorkspaceSignals(
 navigate:(target:PageId,sessionId?:string)=>void,
 setSearchOpen:Dispatch<SetStateAction<boolean>>,
){
 const [online,setOnline]=useState(false)
 const [running,setRunning]=useState(0)
 const [pending,setPending]=useState(0)
 const [engineBuild,setEngineBuild]=useState<{branch:string;commit:string}|null>(null)
 useEffect(()=>{
  let live=true
  const load=()=>{void get<{branch:string;commit:string}>('/api/health')
   .then(info=>{if(live)setEngineBuild(info)}).catch(()=>{if(live)setEngineBuild(null)})}
  load()
  const focus=()=>{if(document.visibilityState==='visible')load()}
  window.addEventListener('focus',focus)
  return()=>{live=false;window.removeEventListener('focus',focus)}
 },[])

 useEffect(()=>{
  let mounted=true
  const check=async()=>{
   try{
    const status=await get<{jobs?:number}>('/api/status')
    if(mounted){setOnline(true);setRunning(Number(status.jobs)||0)}
   }catch{if(mounted){setOnline(false);setRunning(0)}}
  }
  const approvals=async()=>{
   try{const data=await get<unknown[]>('/api/approvals');if(mounted)setPending(Array.isArray(data)?data.length:0)}
   catch{if(mounted)setPending(0)}
  }
  void check();void approvals()
  const poll=setInterval(()=>{void check()},12000)
  const stream=new EventSource('/api/events')
  let refreshTimer:ReturnType<typeof setTimeout>|undefined
  let statusTimer:ReturnType<typeof setTimeout>|undefined
  let approvalsTimer:ReturnType<typeof setTimeout>|undefined
  let pendingRefresh=emptyRefreshScope()
  const scheduleStatus=()=>{
   if(statusTimer)return
   statusTimer=setTimeout(()=>{statusTimer=undefined;void check()},250)
  }
  const scheduleApprovals=()=>{
   if(approvalsTimer)return
   approvalsTimer=setTimeout(()=>{approvalsTimer=undefined;void approvals()},250)
  }
  stream.onmessage=event=>{
   try{
    const item=JSON.parse(event.data)
    if(item.type==='room_event'){
     window.dispatchEvent(new CustomEvent('dominion-room-event',{detail:item}))
     return
    }
    // Keep SSE records scoped to their consumers. A busy tool journal should
    // not re-fetch agents, providers or account connections on every receipt.
    const next=classifyRefreshEvent(item)
    if(next.targets.length||next.rooms.length||next.sessions.length){
     pendingRefresh=mergeRefreshScopes(pendingRefresh,next)
     refreshTimer ??= setTimeout(()=>{
      refreshTimer=undefined
      const batch=pendingRefresh
      pendingRefresh=emptyRefreshScope()
      window.dispatchEvent(new CustomEvent('dominion-data-change',{detail:batch}))
     },100)
    }
    if(item.type==='approval')scheduleApprovals()
    if(item.type==='job_event'||item.type==='goal_event'||item.type==='execution_event'
      ||(item.type==='record'&&item.kind==='job'))scheduleStatus()
   }catch{}
  }
  // Inactive tabs pause request polling; catch up once the owner returns.
  const onVisibility=()=>{if(document.visibilityState==='visible'){void check();void approvals()}}
  stream.onerror=()=>{if(document.visibilityState==='visible')scheduleStatus()}
  document.addEventListener('visibilitychange',onVisibility)
  const shortcuts=(e:KeyboardEvent)=>{
   if((e.metaKey||e.ctrlKey)&&e.key===','){e.preventDefault();navigate('settings')}
   if((e.metaKey||e.ctrlKey)&&e.key.toLowerCase()==='k'){
    e.preventDefault()
    setSearchOpen(value=>!value)
   }
  }
  window.addEventListener('keydown',shortcuts)
  return()=>{mounted=false;clearInterval(poll);clearTimeout(refreshTimer);clearTimeout(statusTimer);clearTimeout(approvalsTimer);stream.close();document.removeEventListener('visibilitychange',onVisibility);window.removeEventListener('keydown',shortcuts)}
 },[navigate])
 return {online,running,pending,engineBuild}
}
