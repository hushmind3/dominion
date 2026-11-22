import {useMemo,useState} from 'react'
import {AlertCircle,ArrowRight,ChevronDown,ChevronRight,Clock3,RefreshCw,Search,Workflow} from 'lucide-react'
import {Button,TextInput,SelectInput} from './Primitives'
import {Empty,Panel} from './UI'
import {StatusPill} from './StatusPill'
import {formatDateTime,jobKindLabel,useLocale} from '../i18n'
import {translateError} from '../i18n/errors'

type Job={
 id:string;kind?:string;status?:string;createdAt?:string;updatedAt?:string;
 sessionId?:string;leadSessionId?:string;error?:string;
 input?:{prompt?:string;goal?:string;[key:string]:unknown};
}
type Filter='all'|'active'|'attention'|'finished'
const active=new Set(['queued','running','launching','assigned','planning','reviewing','repairing'])
const attention=new Set(['needs_review','failed','interrupted','budget_exceeded','approval_pending','outcome_unknown'])
const finished=new Set(['completed','cancelled','succeeded','success'])
const priority=(status:string)=>attention.has(status)?0:active.has(status)?1:finished.has(status)?3:2
const label=(item:Job)=>String(item.input?.goal||item.input?.prompt||item.kind||item.id)
const isFilter=(job:Job,filter:Filter)=>filter==='all'
 ||filter==='attention'&&attention.has(job.status||'')
 ||filter==='active'&&active.has(job.status||'')
 ||filter==='finished'&&finished.has(job.status||'')

/** One consistent work lifecycle, shared by Home and the Activity screen.
 * These are backend job records: the UI never invents progress or completion. */
export function WorkQueue({jobs,loading,error='',compact=false,onRefresh,onOpenActivity}:{
 jobs:Job[];loading:boolean;error?:string;compact?:boolean;
 onRefresh?:()=>void;onOpenActivity?:()=>void
}){
 const en=useLocale()==='en'
 const [query,setQuery]=useState('')
 const [filter,setFilter]=useState<Filter>('all')
 const [expanded,setExpanded]=useState<string|null>(null)
 const [showAll,setShowAll]=useState(false)
 const records=useMemo(()=>[...jobs].sort((a,b)=>{
  const rank=priority(a.status||'')-priority(b.status||'')
  return rank||String(b.updatedAt||b.createdAt||'').localeCompare(String(a.updatedAt||a.createdAt||''))
 }),[jobs])
 const matched=records.filter(job=>isFilter(job,filter)
  &&[label(job),job.id,job.kind,job.status].join(' ').toLowerCase().includes(query.trim().toLowerCase()))
 const shown=compact&&!showAll?matched.slice(0,6):matched
 const counts={active:records.filter(job=>active.has(job.status||'')).length,
  attention:records.filter(job=>attention.has(job.status||'')).length}
 return <section aria-label={en?'Work history':'작업 기록'} className="min-w-0">
  {!compact&&<div className="mb-3 grid grid-cols-2 gap-2 sm:grid-cols-3">
   <div className="rounded-xl border border-[#E7E0D5] bg-white p-3"><span className="block text-[11px] text-[#7C7469]">{en?'Total recorded':'기록된 작업'}</span><strong className="text-xl tabular-nums">{loading||error?'—':records.length}</strong></div>
   <div className="rounded-xl border border-[#E7E0D5] bg-white p-3"><span className="block text-[11px] text-[#7C7469]">{en?'In progress':'진행 중'}</span><strong className="text-xl tabular-nums">{loading||error?'—':counts.active}</strong></div>
   <div className="col-span-2 rounded-xl border border-[#E7E0D5] bg-white p-3 sm:col-span-1"><span className="block text-[11px] text-[#7C7469]">{en?'Needs attention':'확인 필요'}</span><strong className="text-xl tabular-nums text-amber-800">{loading||error?'—':counts.attention}</strong></div>
  </div>}
  {!compact&&<div className="mb-3 flex flex-col gap-2 sm:flex-row">
   <label className="relative min-w-0 flex-1"><Search size={15} className="pointer-events-none absolute left-3 top-3 text-[#8A8176]"/>
    <TextInput aria-label={en?'Search work':'작업 검색'} value={query} onChange={event=>setQuery(event.target.value)}
     placeholder={en?'Find by request, ID or state':'작업 내용·ID·상태 검색'} className="pl-9"/>
   </label>
   <SelectInput value={filter} aria-label={en?'Filter work':'작업 상태 필터'} className="sm:w-44"
    onChange={event=>setFilter(event.target.value as Filter)}>
    <option value="all">{en?'All work':'전체 작업'}</option>
    <option value="active">{en?'In progress':'진행 중'}</option>
    <option value="attention">{en?'Needs attention':'확인 필요'}</option>
    <option value="finished">{en?'Finished':'종료됨'}</option>
   </SelectInput>
  </div>}
  {compact&&counts.attention>0&&!loading&&!error&&<div className="mb-3 flex flex-wrap items-center gap-2 rounded-lg border border-amber-200 bg-amber-50 px-3 py-2 text-xs text-amber-900"><AlertCircle size={14}/>{en?counts.attention+' work items need review':counts.attention+'건의 작업 결과 확인 필요'}
   {onOpenActivity&&<Button variant="ghost" size="sm" onClick={onOpenActivity}>{en?'Inspect':'확인'}<ArrowRight size={12}/></Button>}</div>}
  {loading&&<div role="status" aria-live="polite" className="space-y-3 p-4">
   {[0,1,2].map(index=><div key={index} className="flex items-center gap-3"><span className="size-8 animate-pulse rounded-lg bg-[#F3EEE5]"/><span className="h-3 flex-1 animate-pulse rounded bg-[#F3EEE5]"/><span className="h-5 w-16 animate-pulse rounded bg-[#F3EEE5]"/></div>)}
  </div>}
  {!loading&&error&&<Panel className="flex flex-wrap items-center gap-2 p-4 text-xs text-rose-700" role="alert">
   <AlertCircle size={16}/><span className="min-w-0 flex-1">{en?'Could not load work records. The list is not necessarily empty.':'작업 기록을 불러오지 못했습니다. 작업이 없다는 뜻은 아닙니다.'}</span>
   {onRefresh&&<Button variant="secondary" size="sm" onClick={onRefresh}><RefreshCw size={13}/>{en?'Retry':'다시 시도'}</Button>}
  </Panel>}
  {!loading&&!error&&<div className="divide-y divide-[#F0EAE1]">
   {shown.map(job=>{
    const open=expanded===job.id
    const sessionId=job.sessionId||job.leadSessionId
    return <article key={job.id} className="min-w-0">
     <Button variant="bare" type="button" aria-expanded={open}
      onClick={()=>setExpanded(open?null:job.id)}
      className="flex min-h-16 w-full items-start gap-3 px-4 py-3 text-left hover:bg-[#FAF7F1] focus-visible:rounded-lg">
      <span className="mt-0.5 grid size-8 shrink-0 place-items-center rounded-lg bg-[#F7F2EA] text-[#A94E30]"><Workflow size={16}/></span>
      <span className="min-w-0 flex-1">
       <strong className="block truncate text-[12px] text-[#3A342D]">{label(job)}</strong>
       <small className="mt-1 block break-words text-[10px] text-[#8A8176]">{job.kind?jobKindLabel(job.kind):en?'Work':'작업'} · {job.createdAt?formatDateTime(job.createdAt):en?'Time unknown':'시간 미상'}</small>
       {job.error&&<small className="mt-1 line-clamp-2 text-[11px] text-rose-700">{translateError(job.error)}</small>}
      </span><span className="flex shrink-0 items-center gap-1.5"><StatusPill status={job.status||'unknown'}/>{open?<ChevronDown size={14}/>:<ChevronRight size={14}/>}</span>
     </Button>
     {open&&<div className="mx-4 mb-3 ml-15 rounded-lg border border-[#E7E0D5] bg-[#FAF7F1] p-3 text-[11px]">
      <div className="flex flex-wrap gap-2"><span className="break-all font-mono text-[#7C7469]">ID: {job.id}</span></div>
      <p className="mt-2 whitespace-pre-wrap break-words text-[#3A342D]">{label(job)}</p>
      {job.error&&<p role="alert" className="mt-2 whitespace-pre-wrap break-words text-rose-700">{translateError(job.error)}</p>}
      <div className="mt-3 flex flex-wrap gap-2">
       {sessionId&&<a href={'#/chat?session='+encodeURIComponent(sessionId)} className="inline-flex min-h-9 items-center gap-1.5 rounded-lg border border-[#E1D9CC] bg-white px-3 py-2 font-semibold text-[#3A342D] hover:bg-[#F7F2EA]">{en?'Open conversation':'대화에서 이어보기'}<ArrowRight size={13}/></a>}
       {onOpenActivity&&<Button size="sm" variant="secondary" onClick={onOpenActivity}>{en?'Open activity':'활동에서 보기'}<ArrowRight size={13}/></Button>}
      </div>
     </div>}
    </article>
   })}
   {!shown.length&&<Empty icon={Clock3} title={compact?(en?'No recent work':'최근 작업이 없습니다'):(query||filter!=='all'?(en?'No matching work':'조건에 맞는 작업이 없습니다'):(en?'No work recorded':'기록된 작업이 없습니다'))}
    description={query||filter!=='all'?(en?'Change the search or filter.':'검색어 또는 상태 필터를 변경해 보세요.'):undefined}/>}
  </div>}
  {compact&&!loading&&!error&&matched.length>6&&<Button type="button" variant="ghost" className="mx-4 my-2"
   onClick={()=>setShowAll(value=>!value)}>{showAll?(en?'Show less':'접기'):(en?'Show all work':'작업 더 보기')}<ArrowRight size={13}/></Button>}
 </section>
}
