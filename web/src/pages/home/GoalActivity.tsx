import {useRef,useState,type FormEvent} from 'react'
import {ArrowRight,Send,Square,RotateCcw} from 'lucide-react'
import {post} from '../../api'
import {Button,TextArea} from '../../components/Primitives'

export type GoalSummary={
 id:string;goal:string;status:string;phase?:string;error?:string;
 iterations?:number;maxIterations?:number;createdAt?:string;planRevision?:number
}
type Props={goals:GoalSummary[];navigate:(page:'goals')=>void}
const labels:Record<string,string>={
 queued:'시작 대기',running:'진행 중',completed:'완료',
 needs_review:'확인 필요',failed:'실행 실패',cancelled:'중단됨'
}
const stages:Record<string,string>={
 planning:'계획 수립',plan:'계획 수립',execute:'작업 수행',
 executing:'작업 수행',review:'결과 검토',verify:'결과 검증',
 replan:'방법 재검토',repair:'오류 수정',resume:'이어서 실행'
}

/** A live owner control, not an optimistic fake "message sent" action. */
function ActiveGoalControls({goal}:{goal:GoalSummary}){
 const [instruction,setInstruction]=useState('')
 const [busy,setBusy]=useState(false)
 const [error,setError]=useState('')
 const [notice,setNotice]=useState('')
 // Preserve the request ID on ambiguous failures. An identical retry is
 // idempotent through the Rust SQLite goal revision + receipt CAS.
 const requestId=useRef<string|null>(null)
 const send=async(event:FormEvent<HTMLFormElement>)=>{
  event.preventDefault()
  if(busy||!instruction.trim())return
  const text=instruction.trim()
  const id=requestId.current??crypto.randomUUID()
  requestId.current=id
  setBusy(true);setError('');setNotice('')
  try{
   await post('/api/goals/'+encodeURIComponent(goal.id)+'/steer',{
    text,mode:'append',requestId:id
   })
   requestId.current=null;setInstruction('')
   setNotice('추가 지시가 저장됐습니다. 다음 안전한 실행 단계에서 반영됩니다.')
   window.dispatchEvent(new Event('dominion-refresh'))
  }catch(cause){
   setError('결과 확인에 실패했습니다. 같은 내용을 다시 보내면 동일 요청 번호로 중복 등록을 방지합니다. '+(cause as Error).message)
  }finally{setBusy(false)}
 }
 const cancel=async()=>{
  if(busy||!window.confirm('이 작업을 중단하시겠습니까? 이미 끝난 외부 작업은 되돌리지 않습니다.'))return
  setBusy(true);setError('');setNotice('')
  try{
   await post('/api/goals/'+encodeURIComponent(goal.id)+'/cancel',{})
   setNotice('중단 요청이 저장됐습니다.')
   window.dispatchEvent(new Event('dominion-refresh'))
  }catch(cause){setError((cause as Error).message)}
  finally{setBusy(false)}
 }
 return <div className="mt-3 border-t border-[#EEE8DF] pt-3">
  <form onSubmit={send} className="space-y-2">
   <label htmlFor={'home-goal-steer-'+goal.id} className="text-xs font-semibold text-[#62594E]">
    실행 중인 작업에 추가 지시하기
   </label>
   <div className="flex flex-col items-stretch gap-2 sm:flex-row sm:items-start">
    <TextArea id={'home-goal-steer-'+goal.id} rows={2} maxLength={6000}
     value={instruction} disabled={busy}
     onChange={e=>{setInstruction(e.target.value);requestId.current=null;setNotice('')}}
     placeholder="예: 다른 파일은 건드리지 말고 테스트부터 확인해줘"
     className="min-w-0 flex-1 !text-sm"/>
    <Button type="submit" size="sm" variant="primary" disabled={busy||!instruction.trim()}
     className="!min-h-10"><Send size={14}/> 지시 전달</Button>
   </div>
  </form>
  <div className="mt-2 flex items-center justify-between gap-2">
   <p className="text-xs leading-5 text-[#7C7469]">현재 실행 단계를 마친 뒤 새로운 지시가 적용됩니다.</p>
   <Button variant="ghost" size="sm" disabled={busy} onClick={()=>void cancel()}>
    <Square size={13}/> 작업 중단
   </Button>
  </div>
  {notice&&<p role="status" className="mt-2 text-xs text-emerald-700">{notice}</p>}
  {error&&<p role="alert" className="mt-2 text-xs leading-5 text-rose-700">{error}</p>}
 </div>
}

export function GoalActivity({goals,navigate}:Props){
 const recent=[...goals].sort((a,b)=>String(b.createdAt||'').localeCompare(String(a.createdAt||''))).slice(0,3)
 return <section aria-label="내 작업 진행 상황" className="mb-5 rounded-2xl border border-[#E7E0D5] bg-white p-5 sm:p-6">
  <div className="mb-3 flex flex-wrap items-center justify-between gap-2">
   <div><h2 className="text-base font-bold">맡긴 작업은 어떻게 되고 있나요?</h2>
    <p className="mt-1 text-xs leading-5 text-[#7C7469]">실제 작업 기록입니다. 필요하면 지시를 바꾸거나 중단할 수 있습니다.</p>
   </div>
   <Button variant="secondary" size="sm" onClick={()=>navigate('goals')}>
    전체 작업 보기 <ArrowRight size={14}/>
   </Button>
  </div>
  {!recent.length&&<p className="rounded-xl border border-dashed border-[#E1D9CC] p-5 text-sm text-[#7C7469]">
   아직 맡긴 작업이 없습니다. 위에서 원하는 일을 적어 시작해 보세요.
  </p>}
  <div className="space-y-3">
   {recent.map(goal=>{
    const progress=goal.maxIterations&&goal.iterations!=null
     ?Math.min(100,Math.round(100*goal.iterations/Math.max(1,goal.maxIterations))):null
    const active=goal.status==='running'||goal.status==='queued'
    return <article key={goal.id} className="rounded-xl border border-[#EEE8DF] p-4">
     <div className="flex flex-wrap items-start justify-between gap-2">
      <h3 className="min-w-0 flex-1 break-words text-sm font-semibold leading-6">{goal.goal}</h3>
      <span className={'shrink-0 rounded-full px-2.5 py-1 text-xs font-semibold '+
       (goal.status==='completed'?'bg-emerald-50 text-emerald-700':
        goal.status==='needs_review'?'bg-amber-50 text-amber-800':
        goal.status==='failed'?'bg-rose-50 text-rose-700':'bg-[#F3EEE5] text-[#62594E]')}>
       {labels[goal.status]??'상태 확인'}
      </span>
     </div>
     <div className="mt-2 flex flex-wrap items-center gap-3 text-xs text-[#786E61]">
      <span>{stages[goal.phase??'']??(active?'진행 상황 기록 중':'실행 기록 보관됨')}</span>
      {progress!==null&&<span>사용한 작업 시도 {goal.iterations??0}/{goal.maxIterations}</span>}
      {goal.status==='needs_review'&&<Button variant="ghost" size="sm" onClick={()=>navigate('goals')}
       className="!p-0 !text-amber-800"><RotateCcw size={13}/> 검토하고 이어가기</Button>}
     </div>
     {active&&progress!==null&&<div aria-label="반복 실행 한도 사용량" className="mt-3 h-1.5 overflow-hidden rounded-full bg-[#EEE7DD]">
      <div className="h-full bg-[#C96442]" style={{width:progress+'%'}}/>
     </div>}
     {goal.error&&<p role="alert" className="mt-2 line-clamp-2 text-xs text-rose-700">{goal.error}</p>}
     {active&&<ActiveGoalControls goal={goal}/>}
    </article>
   })}
  </div>
 </section>
}
