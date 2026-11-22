import {useMemo,useState} from 'react'
import {AlertCircle,Clock3,FileCheck2,PlayCircle,RefreshCw,Search,Workflow} from 'lucide-react'

export type NativeRunListItem={
 id:string;status:string;instruction:string;
 checkpoint?:{rounds?:number;toolCalls?:number;verificationChecks?:{satisfied:boolean}[]}|null
}
type Filter='all'|'active'|'review'|'finished'
const active=new Set(['queued','running'])
const review=new Set(['awaiting_approval','in_doubt','needs_review','interrupted'])
const stateLabel:Record<string,[string,string]>={
 queued:['대기 중','Queued'],running:['실행 중','Running'],
 completed:['완료','Completed'],succeeded:['완료','Succeeded'],
 failed:['실패','Failed'],cancelled:['중단','Cancelled'],
 exhausted:['한도 도달','Limit reached'],awaiting_approval:['승인 대기','Approval required'],
 in_doubt:['확인 필요','Needs reconciliation'],
 interrupted:['재개 가능','Resumable'],denied:['거부됨','Denied'],
 needs_review:['검토 필요','Review required']
}
const filterMatch=(status:string,filter:Filter)=>{
 if(filter==='active')return active.has(status)
 if(filter==='review')return review.has(status)
 if(filter==='finished')return !active.has(status)&&!review.has(status)
 return true
}
export function NativeRunNavigator({runs,selectedId,onSelect,onRefresh,en}:{
 runs:NativeRunListItem[];selectedId?:string;onSelect:(id:string)=>void;
 onRefresh:()=>void;en:boolean
}){
 const [filter,setFilter]=useState<Filter>('all')
 const [query,setQuery]=useState('')
 const counts=useMemo(()=>({
  all:runs.length,active:runs.filter(r=>active.has(r.status)).length,
  review:runs.filter(r=>review.has(r.status)).length,
  finished:runs.filter(r=>filterMatch(r.status,'finished')).length
 }),[runs])
 const visible=useMemo(()=>runs.filter(r=>
  filterMatch(r.status,filter)&&
  (r.instruction+' '+r.id).toLocaleLowerCase().includes(query.trim().toLocaleLowerCase())
 ),[runs,filter,query])
 const filters=[
  {id:'all' as const,ko:'전체',english:'All',value:counts.all},
  {id:'active' as const,ko:'실행 중',english:'Running',value:counts.active},
  {id:'review' as const,ko:'확인 필요',english:'Review',value:counts.review},
  {id:'finished' as const,ko:'종료',english:'Finished',value:counts.finished}
 ]
 return <aside className="native-run-navigator flex min-h-0 min-w-0 flex-col bg-canvas text-ink" aria-label={en?'Saved task runs':'저장된 자동 작업'}>
  <div className="native-run-navigator-head flex items-center justify-between gap-2 p-4 pb-2">
   <div className="min-w-0">
    <span className="native-rail-kicker block text-[10px] font-semibold tracking-wide text-muted">{en?'RUN HISTORY':'실행 이력'}</span>
    <h3 className="mt-1 text-sm font-semibold text-ink">{en?'Your task queue':'작업 목록'} <small className="ml-1 text-xs font-normal text-muted">{runs.length}</small></h3>
   </div>
   <button type="button" aria-label={en?'Refresh task history':'작업 이력 새로고침'}
    title={en?'Refresh task history':'작업 이력 새로고침'}
    className="native-rail-refresh grid size-8 shrink-0 place-items-center rounded-lg border border-divider bg-surface text-muted hover:text-ink"
    onClick={onRefresh}><RefreshCw size={16}/></button>
  </div>
  {counts.review>0&&<button type="button" className="native-review-banner mx-3 mt-2 flex items-center gap-2 rounded-lg border border-amber-200 bg-amber-50 p-2.5 text-left text-xs text-amber-900"
   onClick={()=>setFilter('review')}>
   <AlertCircle size={16}/>
   <span className="flex-1">{en?'Tasks needing your attention':'확인할 작업'} <b className="float-right">{counts.review}</b></span>
  </button>}
  <div className="native-run-filters grid grid-cols-2 gap-1.5 px-3 py-3" role="group" aria-label={en?'Filter task runs':'작업 상태 필터'}>
   {filters.map(item=><button key={item.id} type="button"
    aria-pressed={filter===item.id} onClick={()=>setFilter(item.id)}
    className={'flex min-h-9 items-center justify-between gap-1 rounded-lg border px-2 py-1.5 text-[10px] font-medium transition-colors '+
     (filter===item.id?'border-workspace-selected bg-workspace-selected text-white':'border-divider bg-surface text-muted hover:text-ink')}>
    <span>{en?item.english:item.ko}</span><b>{item.value}</b>
   </button>)}
  </div>
  <label className="native-run-search mx-3 mb-2 flex items-center gap-2 rounded-lg border border-divider bg-surface px-2.5 text-muted focus-within:border-brand">
   <Search size={15}/><span className="sr-only">{en?'Search tasks':'작업 검색'}</span>
   <input value={query} onChange={e=>setQuery(e.target.value)}
    className="h-10 min-w-0 flex-1 bg-transparent text-xs text-ink outline-none"
    placeholder={en?'Search work or run ID':'작업·실행 ID 검색'}/>
  </label>
  <div className="native-run-list min-h-24 flex-1 space-y-1 overflow-y-auto px-2.5 pb-4" role="list">
   {visible.map((item,index)=>{
    const focused=item.id===selectedId
    const isActive=active.has(item.status)
    const isReview=review.has(item.status)
    const Icon=isReview?AlertCircle:isActive?PlayCircle:item.status==='completed'?FileCheck2:Clock3
    return <div role="listitem" key={item.id}>
     <button type="button" onClick={()=>onSelect(item.id)}
      aria-current={focused?'true':undefined}
      className={'native-run-item flex w-full min-w-0 flex-col gap-2 rounded-lg border p-3 text-left transition-colors '+
       (focused?'border-brand bg-surface shadow-sm':'border-transparent hover:border-divider hover:bg-surface')}>
      <span className="native-run-item-top flex w-full items-center justify-between gap-2">
       <span className="native-run-item-index text-[10px] text-muted">{String(index+1).padStart(2,'0')}</span>
       <span className={'native-run-status inline-flex items-center gap-1 text-[10px] font-medium '+
        (isReview?'text-amber-700':isActive?'text-sky-700':'text-muted')}>
        <Icon size={12}/>{en?stateLabel[item.status]?.[1]||item.status:stateLabel[item.status]?.[0]||item.status}
       </span>
      </span>
      <strong className="line-clamp-2 break-words text-xs font-medium leading-relaxed text-ink">{item.instruction||item.id}</strong>
      <span className="native-run-item-meta flex items-center gap-1.5 text-[10px] text-muted">
       <Workflow size={12}/>{item.checkpoint?.rounds??0} {en?'rounds':'회 판단'}
       <span>·</span>{item.checkpoint?.toolCalls??0} {en?'tool calls':'회 도구 실행'}
      </span>
     </button>
    </div>
   })}
   {!visible.length&&<div role="status" className="native-run-empty flex min-h-40 flex-col items-center justify-center gap-2 p-5 text-center text-muted">
    <Workflow size={20}/>
    <strong className="text-xs font-medium">{en?'No matching tasks':'표시할 작업이 없습니다'}</strong>
    <p className="text-[11px]">{runs.length
      ?(en?'Try another status or keyword.':'상태나 검색어를 변경해 보세요.')
      :(en?'Created tasks will appear here.':'실행한 작업이 여기에 표시됩니다.')}</p>
   </div>}
  </div>
 </aside>
}
