import {useState} from 'react'
import {Activity,Clock3,RefreshCw,Square,Terminal} from 'lucide-react'
import {post} from '../api'
import {Button} from './Primitives'
import {Panel,useRemote} from './UI'

type Execution={
 id:string;sessionId:string;agentId?:string;providerId:string;model:string;
 status:string;startedAt:string;finishedAt?:string;reason?:string;
 iterations:number;modelCalls:number;toolCalls:number;tokens:number
}
type Operation={id:string;callId:string;tool:string;status:string;startedAt:string;reason?:string}
const labels:Record<string,string>={running:'실행 중',completed:'완료',cancelled:'취소',failed:'실패',
 interrupted:'중단됨',needs_review:'확인 필요',budget_exceeded:'예산 초과'}
const active=(s:string)=>s==='running'
function ToolHistory({id}:{id:string}){
 const log=useRemote<Operation[]>('/api/executions/'+id+'/operations',[])
 return <div className="mt-3 max-h-44 space-y-2 overflow-y-auto">
  {log.value.map(o=><div key={o.id} className="flex items-center gap-2 rounded-lg bg-slate-50 px-3 py-2 text-xs">
   <Terminal size={13} className="shrink-0 text-slate-500"/><span className="min-w-0 flex-1 truncate">{o.tool}</span>
   <span className="text-slate-500">{o.status}</span>
  </div>)}
  {!log.value.length&&<p className="text-xs text-slate-400">도구 실행 기록이 없습니다.</p>}
 </div>
}
export function ExecutionMonitor(){
 const runs=useRemote<Execution[]>('/api/executions',[])
 const [selected,setSelected]=useState<string|null>(null),[busy,setBusy]=useState(false),[error,setError]=useState('')
 const current=runs.value.find(x=>x.id===selected)||runs.value[0]
 const count=runs.value.filter(x=>active(x.status)).length
 const cancel=async()=>{
  if(!current)return
  setBusy(true);setError('')
  try{await post('/api/executions/'+current.id+'/cancel',{});await runs.reload()}
  catch(e){setError((e as Error).message)}
  finally{setBusy(false)}
 }
 return <Panel className="mb-8 overflow-hidden">
  <div className="flex flex-wrap items-center justify-between gap-3 border-b border-slate-200 px-5 py-4">
   <div className="flex items-center gap-3"><span className="grid size-10 place-items-center rounded-xl bg-slate-900 text-white"><Activity size={20}/></span>
    <div><h2 className="m-0 text-lg font-bold text-slate-900">실행 엔진</h2><p className="m-0 text-xs text-slate-500">모델 추론 · 도구 호출 · 실행 기록 · 취소</p></div></div>
   <div className="flex items-center gap-3"><span className="text-xs font-semibold text-slate-600">{count}개 실행 중</span>
    <Button size="sm" variant="ghost" onClick={()=>void runs.reload()}><RefreshCw size={14}/> 새로고침</Button></div>
  </div>
  <div className="grid min-w-0 gap-5 p-5 lg:grid-cols-2">
   <div className="max-h-64 space-y-2 overflow-y-auto">
    {runs.value.map(r=><Button key={r.id} variant="ghost" onClick={()=>setSelected(r.id)}
     className={'!flex !w-full !items-center !justify-between !gap-2 !rounded-lg !border !px-3 !py-3 !text-left '+(current?.id===r.id?'!border-blue-300 !bg-blue-50':'!border-slate-200')}>
      <span className="min-w-0"><strong className="block truncate text-xs text-slate-800">{r.model}</strong>
       <small className="block truncate text-[11px] text-slate-500">{r.sessionId}</small></span>
      <span className="shrink-0 text-xs font-medium text-slate-600">{labels[r.status]||r.status}</span>
    </Button>)}
    {!runs.value.length&&<p className="py-8 text-center text-sm text-slate-500">실행 기록이 없습니다. 대화나 에이전트 작업을 시작하면 기록됩니다.</p>}
   </div>
   <div className="min-w-0 border-t border-slate-200 pt-4 lg:border-l lg:border-t-0 lg:pl-5 lg:pt-0">
    {current?<><div className="flex items-center justify-between gap-2"><h3 className="m-0 text-sm font-bold text-slate-800">실행 상세</h3>
     <span className="text-xs font-semibold text-slate-600">{labels[current.status]||current.status}</span></div>
     <div className="mt-3 grid grid-cols-3 gap-2 rounded-lg bg-slate-50 p-3 text-center">
      <div><strong className="block text-lg">{current.modelCalls||0}</strong><small className="text-[10px]">모델 호출</small></div>
      <div><strong className="block text-lg">{current.toolCalls||0}</strong><small className="text-[10px]">도구 호출</small></div>
      <div><strong className="block text-lg">{current.tokens||0}</strong><small className="text-[10px]">토큰</small></div>
     </div>
     <p className="mt-3 flex items-center gap-2 text-xs text-slate-500"><Clock3 size={13}/>{current.startedAt?.replace('T',' ').slice(0,19)} UTC</p>
     {current.reason&&<p role="alert" className="mt-2 break-words rounded-lg bg-amber-50 p-3 text-xs text-amber-800">{current.reason}</p>}
     {active(current.status)&&<Button variant="danger" disabled={busy} onClick={()=>void cancel()}><Square size={14}/> 실행 취소</Button>}
     <ToolHistory key={current.id} id={current.id}/>
    </>:<p className="text-sm text-slate-500">실행 중인 세션과 도구 호출 이력이 여기에 표시됩니다.</p>}
   </div>
  </div>
  {error&&<p role="alert" className="px-5 pb-4 text-xs text-rose-600">{error}</p>}
 </Panel>
}
