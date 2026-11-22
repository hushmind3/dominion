import {useEffect,useRef,useState} from 'react'
import {Activity,ChevronDown,ChevronRight,LoaderCircle,RefreshCw} from 'lucide-react'
import {get} from '../api'
import {useLocale} from '../i18n'

type RunRef={executionId:string;sessionId?:string;agentId?:string;taskId?:string;jobId?:string;parentExecutionId?:string;externalRunId?:string;lifecycle?:string}
type EventRow={seq:number;type:string;runRef:RunRef;kind?:string;record?:Record<string,unknown>;value?:Record<string,unknown>;[key:string]:unknown}
type EventPage={executionId:string;events:EventRow[];nextAfter:number;hasMore:boolean;historyTruncated:boolean;earliestAvailableSeq?:number|null}
const friendly=(event:EventRow,en:boolean)=>{
 const kind=typeof event.kind==='string'?event.kind:''
 if(event.type==='record')return kind?kind+(en?' updated':' 기록'):(en?'Record updated':'기록 변경')
 return event.type.replaceAll('_',' ')
}
/** Read-only, cursor-safe native event replay. A run ID is never inferred
 * from a room, SDK or job ID. Each page uses the execution's global cursor. */
export function RunEventTimeline({executionId}:{executionId:string}){
 const en=useLocale()==='en'
 const [open,setOpen]=useState(false)
 const [rows,setRows]=useState<EventRow[]>([])
 const [cursor,setCursor]=useState(0)
 const [more,setMore]=useState(false)
 const [truncated,setTruncated]=useState(false)
 const [loading,setLoading]=useState(false)
 const [error,setError]=useState('')
 const epoch=useRef(0)
 const inFlight=useRef<AbortController|null>(null)
 useEffect(()=>{
  ++epoch.current
  inFlight.current?.abort()
  inFlight.current=null
  setOpen(false);setRows([]);setCursor(0);setMore(false);setTruncated(false);setError('');setLoading(false)
  return ()=>{++epoch.current;inFlight.current?.abort();inFlight.current=null}
 },[executionId])
 const load=async(reset=false)=>{
  if(inFlight.current)return
  const token=epoch.current
  const controller=new AbortController()
  inFlight.current=controller
  setLoading(true);setError('')
  try{
   const page=await get<EventPage>(
    '/api/executions/'+encodeURIComponent(executionId)+'/events?limit=80&after='+(reset?0:cursor),
    controller.signal
   )
   if(controller.signal.aborted||token!==epoch.current)return
   setRows(previous=>{
    const merged=reset?[]:previous
    const seen=new Set(merged.map(event=>event.seq))
    return [...merged,...page.events.filter(event=>!seen.has(event.seq))]
   })
   setCursor(page.nextAfter)
   setMore(page.hasMore)
   setTruncated(previous=>reset?page.historyTruncated:previous||page.historyTruncated)
  }catch(e){
   if(!controller.signal.aborted&&token===epoch.current)setError((e as Error).message)
  }finally{
   if(inFlight.current===controller){
    inFlight.current=null
    if(token===epoch.current)setLoading(false)
   }
  }
 }
 useEffect(()=>{
  if(!open)return
  const timer=window.setInterval(()=>{void load(false)},5000)
  return()=>window.clearInterval(timer)
 },[open,cursor,loading,executionId])
 return <section className="mt-3 rounded-xl border border-divider bg-white">
  <button type="button" aria-expanded={open} onClick={()=>{if(!open&&!rows.length)void load(true);setOpen(!open)}}
   className="flex w-full items-center justify-between gap-2 px-3 py-2.5 text-left text-[11px] font-semibold text-[#514940] hover:bg-[#FAF7F1]">
   <span className="inline-flex items-center gap-2"><Activity size={14}/>{en?'Durable execution events':'실행 이벤트 기록'} {rows.length>0?'('+rows.length+')':''}</span>
   {open?<ChevronDown size={14}/>:<ChevronRight size={14}/>}
  </button>
  {open&&<div className="border-t border-[#EFE9DF] px-3 py-2">
   <div className="flex justify-end">
    <button type="button" disabled={loading} onClick={()=>void load(true)}
     className="inline-flex items-center gap-1 text-[10px] text-muted disabled:opacity-50"><RefreshCw size={11}/>{en?'Refresh':'새로고침'}</button>
   </div>
   {truncated&&<p className="my-2 text-[11px] text-amber-800">{en?'Old events were truncated by retention; this history is incomplete.':'보관 기간으로 일부 과거 이벤트가 누락됐습니다.'}</p>}
   <div className="mt-1 max-h-72 space-y-1 overflow-y-auto">
    {rows.map(event=><details key={event.seq} className="rounded-lg border border-[#F1EBE2] bg-[#FCFAF6] px-2.5 py-2">
     <summary className="flex cursor-pointer flex-wrap items-center gap-2 text-[11px] text-[#514940]">
      <span className="font-mono text-[10px] text-[#9C9388]">#{event.seq}</span>
      <strong>{friendly(event,en)}</strong>
      {event.runRef.lifecycle&&<span className="rounded bg-[#F0EAE1] px-1.5 py-0.5 text-[10px]">{event.runRef.lifecycle}</span>}
     </summary>
     <div className="mt-2 text-[10px] text-muted">
      {event.runRef.agentId&&<p>Agent: {event.runRef.agentId}</p>}
      {event.runRef.taskId&&<p>Task: {event.runRef.taskId}</p>}
      {event.runRef.externalRunId&&<p>External: {event.runRef.externalRunId}</p>}
      <pre className="mt-1 max-h-40 overflow-auto whitespace-pre-wrap break-all rounded bg-white p-2 text-[10px]">{JSON.stringify(event,null,2)}</pre>
     </div>
    </details>)}
    {!rows.length&&!loading&&!error&&<p className="py-4 text-center text-[11px] text-[#8A8176]">{en?'No recorded events.':'기록된 이벤트가 없습니다.'}</p>}
   </div>
   {loading&&<p role="status" className="mt-2 flex items-center gap-1 text-[11px] text-[#8A8176]"><LoaderCircle size={12} className="animate-spin"/>{en?'Loading…':'불러오는 중…'}</p>}
   {error&&<p role="alert" className="mt-2 text-[11px] text-rose-700">{error}</p>}
   {more&&<button type="button" disabled={loading} onClick={()=>void load()} className="mt-2 rounded-lg border border-divider px-3 py-1.5 text-[11px] text-[#514940] disabled:opacity-50">{en?'Load more':'이어서 보기'}</button>}
  </div>}
 </section>
}
