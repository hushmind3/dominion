import {useState} from 'react'
import {Activity,ArrowRight,CheckCircle2,Clock3,Play,RefreshCw,RotateCcw,ShieldAlert,Square,Workflow} from 'lucide-react'
import {post} from '../api'
import {useRemote,Notice,Panel} from './UI'
import {Button,TextArea,TextInput} from './Primitives'

type Usage={modelCalls:number;tokens:number;toolCalls:number}
type Step={id:string;name:string;status:string;attempts:number;output?:string;reason?:string;reviewMode?:string}
type GoalRun={id:string;goal:string;status:string;createdAt:string;updatedAt:string;reason?:string;summary?:string;reviewMode?:string;steps:Step[];usage:Usage;budget:{maxSteps:number;maxRepairs:number}}
type RunEvent={seq:number;type:string;createdAt:string;data:Record<string,unknown>}
const states:Record<string,string>={queued:'대기',planning:'계획',running:'실행',reviewing:'검수',repairing:'재작업',
 completed:'완료',failed:'실패',needs_review:'확인 필요',budget_exceeded:'예산 초과',interrupted:'중단',cancelled:'취소'}
const stateColor=(s:string)=>s==='completed'?'bg-emerald-50 text-emerald-700':s==='needs_review'?'bg-amber-50 text-amber-800':
 s==='failed'||s==='budget_exceeded'?'bg-rose-50 text-rose-700':'bg-blue-50 text-blue-700'
function GoalHistory({id}:{id:string}){
 const history=useRemote<RunEvent[]>('/api/goals/'+id+'/events',[])
 return <div className="mt-4 max-h-64 overflow-y-auto rounded-xl border border-slate-200 bg-slate-50 p-3" aria-label="실행 이벤트 로그">
  {history.value.map(e=><div key={e.seq} className="flex items-start gap-3 border-b border-slate-200 py-2 text-xs last:border-0">
   <span className="min-w-9 font-mono text-slate-400">#{e.seq}</span>
   <span className="min-w-0 flex-1 break-words font-medium text-slate-700">{e.type}</span>
   <span className="text-slate-400">{e.createdAt?.slice(11,19)}</span>
  </div>)}
  {!history.value.length&&<p className="text-xs text-slate-500">기록이 없습니다.</p>}
 </div>
}
export function GoalRunner(){
 const runs=useRemote<GoalRun[]>('/api/goals',[])
 const [query,setQuery]=useState(''),[active,setActive]=useState<string|null>(null)
 const [steps,setSteps]=useState(8),[repairs,setRepairs]=useState(2)
 const [busy,setBusy]=useState(false),[error,setError]=useState(''),[info,setInfo]=useState('')
 const selected=runs.value.find(x=>x.id===active)||runs.value[0]
 const submit=async()=>{
  if(!query.trim()||busy)return
  setBusy(true);setError('');setInfo('')
  try{
   const r=await post<GoalRun>('/api/goals',{goal:query.trim(),budget:{maxSteps:steps,maxRepairs:repairs}})
   setActive(r.id);setQuery('');setInfo('목표가 실행 대기열에 등록되었습니다.');await runs.reload()
  }catch(e){setError((e as Error).message)}finally{setBusy(false)}
 }
 const act=async(action:'resume'|'cancel',id:string)=>{
  setBusy(true);setError('');setInfo('')
  try{await post('/api/goals/'+id+'/'+action,{});await runs.reload();setInfo(action==='resume'?'저장된 작업에서 다시 시작합니다.':'중단 요청을 기록했습니다.')}
  catch(e){setError((e as Error).message)}finally{setBusy(false)}
 }
 return <Panel className="mb-8 overflow-hidden">
  <div className="border-b border-slate-200 bg-gradient-to-r from-slate-900 to-slate-800 px-5 py-5 text-white sm:px-6">
   <div className="flex flex-wrap items-center justify-between gap-3">
    <div className="flex items-center gap-3"><span className="grid size-10 place-items-center rounded-xl bg-white/10"><Workflow size={21}/></span>
     <div><p className="m-0 text-[10px] font-bold tracking-widest text-blue-200">DURABLE AGENT EXECUTION</p>
      <h2 className="m-0 text-xl font-bold">자율 목표 실행</h2></div></div>
    <span className="rounded-full border border-white/20 px-3 py-1 text-xs text-slate-200">상태 · 이벤트 · 검수 · 재작업</span>
   </div>
  </div>
  <div className="grid gap-6 p-5 lg:grid-cols-[minmax(0,1.2fr)_minmax(0,1fr)] lg:p-6">
   <div className="min-w-0">
    <label htmlFor="new-goal" className="mb-2 block text-sm font-semibold text-slate-800">수행할 목표</label>
    <TextArea id="new-goal" value={query} onChange={e=>setQuery(e.target.value)} rows={4} placeholder="예: 현재 프로젝트의 API 문제를 조사하고, 코드를 수정한 뒤 실제 테스트 결과를 보고해 줘."/>
    <div className="mt-4 flex flex-wrap items-end gap-3">
     <label className="flex flex-col gap-1 text-xs font-semibold text-slate-600">최대 계획 단계
      <TextInput className="w-24" type="number" min={1} max={30} value={steps} onChange={e=>setSteps(Math.max(1,Math.min(30,Number(e.target.value)||1)))}/></label>
     <label className="flex flex-col gap-1 text-xs font-semibold text-slate-600">단계별 재작업
      <TextInput className="w-24" type="number" min={0} max={8} value={repairs} onChange={e=>setRepairs(Math.max(0,Math.min(8,Number(e.target.value)||0)))}/></label>
     <Button variant="primary" disabled={busy||!query.trim()} onClick={submit}><Play size={15}/> {busy?'처리 중':'목표 실행'}</Button>
    </div>
    {error&&<Notice message={error}/>}
    {info&&<p className="mt-3 text-xs text-emerald-700" role="status">{info}</p>}
    <div className="mt-6 flex items-center justify-between border-b border-slate-100 pb-2">
     <h3 className="m-0 text-sm font-bold text-slate-800">실행 내역 <span className="text-slate-400">{runs.value.length}</span></h3>
     <Button variant="ghost" className="px-2 py-1 text-xs" onClick={()=>void runs.reload()}><RefreshCw size={13}/> 새로고침</Button>
    </div>
    <div className="mt-2 max-h-72 space-y-2 overflow-y-auto">
     {runs.value.map(r=><button key={r.id} type="button" onClick={()=>setActive(r.id)} className={['flex w-full items-center gap-3 rounded-xl border px-3 py-3 text-left transition',
      selected?.id===r.id?'border-blue-300 bg-blue-50':'border-slate-200 bg-white hover:bg-slate-50'].join(' ')}>
      <span className={['rounded-md px-2 py-1 text-[11px] font-semibold',stateColor(r.status)].join(' ')}>{states[r.status]||r.status}</span>
      <span className="min-w-0 flex-1 truncate text-sm text-slate-700">{r.goal}</span><ArrowRight size={14} className="shrink-0 text-slate-400"/>
     </button>)}
     {!runs.value.length&&<p className="py-5 text-center text-xs text-slate-500">실행 기록이 없습니다.</p>}
    </div>
   </div>
   <div className="min-w-0 border-t border-slate-200 pt-5 lg:border-l lg:border-t-0 lg:pl-6 lg:pt-0">
    <div className="mb-3 flex flex-wrap items-center justify-between gap-2">
     <h3 className="m-0 flex items-center gap-2 text-sm font-bold text-slate-800"><Activity size={17}/> 실행 상태</h3>
     {selected&&<span className={['rounded-lg px-2.5 py-1 text-xs font-semibold',stateColor(selected.status)].join(' ')}>{states[selected.status]||selected.status}</span>}
    </div>
    {selected?<>
     <p className="break-words text-sm leading-6 text-slate-700">{selected.goal}</p>
     <div className="mt-3 grid grid-cols-3 gap-2 rounded-xl bg-slate-50 p-3 text-center">
      <div><strong className="block text-lg text-slate-800">{selected.steps.length}</strong><small className="text-[10px] text-slate-500">계획 단계</small></div>
      <div><strong className="block text-lg text-slate-800">{selected.usage?.modelCalls||0}</strong><small className="text-[10px] text-slate-500">모델 호출</small></div>
      <div><strong className="block text-lg text-slate-800">{selected.usage?.toolCalls||0}</strong><small className="text-[10px] text-slate-500">도구 실행</small></div>
     </div>
     <div className="mt-4 max-h-64 space-y-2 overflow-auto">
      {selected.steps.map(s=><div key={s.id} className="flex gap-3 rounded-xl border border-slate-100 px-3 py-2.5">
       {s.status==='completed'?<CheckCircle2 className="shrink-0 text-emerald-600" size={17}/>:s.status==='failed'?<ShieldAlert className="shrink-0 text-rose-500" size={17}/>:<Clock3 className="shrink-0 text-blue-500" size={17}/>}
       <div className="min-w-0"><strong className="block text-xs text-slate-800">{s.name}</strong><small className="text-[11px] text-slate-500">{states[s.status]||s.status} · 시도 {s.attempts}회 {s.reviewMode?'· '+s.reviewMode:''}</small>
       {s.reason&&<p className="m-0 mt-1 text-xs text-rose-600">{s.reason}</p>}</div>
      </div>)}
     </div>
     {selected.reason&&<p role="alert" className="mt-3 rounded-lg bg-amber-50 p-3 text-xs text-amber-800">{selected.reason}</p>}
     {selected.summary&&<details className="mt-3 rounded-xl border border-slate-200 p-3"><summary className="cursor-pointer text-xs font-semibold">완료 결과 보기</summary>
      <pre className="mt-3 max-h-60 overflow-auto whitespace-pre-wrap break-words text-xs leading-5">{selected.summary}</pre></details>}
     <div className="mt-3 flex flex-wrap gap-2">
      {selected.status==='needs_review'&&<Button variant="primary" disabled={busy} onClick={()=>act('resume',selected.id)}><RotateCcw size={14}/> 확인 후 재개</Button>}
      {['queued','planning','running','reviewing','repairing'].includes(selected.status)&&<Button variant="danger" disabled={busy} onClick={()=>act('cancel',selected.id)}><Square size={13}/> 중단</Button>}
     </div>
     <GoalHistory id={selected.id}/>
    </>:<p className="py-12 text-center text-sm text-slate-500">왼쪽에서 목표를 실행하면 계획과 실행 기록이 표시됩니다.</p>}
   </div>
  </div>
 </Panel>
}
