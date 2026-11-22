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
 return <aside className="native-run-navigator" aria-label={en?'Saved task runs':'저장된 자동 작업'}>
  <div className="native-run-navigator-head">
   <div>
    <span className="native-rail-kicker">{en?'02 / RUN HISTORY':'02 / 실행 이력'}</span>
    <h3>{en?'Your task queue':'작업 목록'} <small>{runs.length}</small></h3>
   </div>
   <button type="button" aria-label={en?'Refresh task history':'작업 이력 새로고침'}
    title={en?'Refresh task history':'작업 이력 새로고침'}
    className="native-rail-refresh" onClick={onRefresh}><RefreshCw size={16}/></button>
  </div>
  {counts.review>0&&<button type="button" className="native-review-banner"
   onClick={()=>setFilter('review')}>
   <AlertCircle size={16}/>
   <span>{en?'Tasks needing your attention':'확인할 작업'} <b>{counts.review}</b></span>
  </button>}
  <div className="native-run-filters" role="group" aria-label={en?'Filter task runs':'작업 상태 필터'}>
   {filters.map(item=><button key={item.id} type="button"
    aria-pressed={filter===item.id} onClick={()=>setFilter(item.id)}>
    <span>{en?item.english:item.ko}</span><b>{item.value}</b>
   </button>)}
  </div>
  <label className="native-run-search">
   <Search size={15}/><span className="sr-only">{en?'Search tasks':'작업 검색'}</span>
   <input value={query} onChange={e=>setQuery(e.target.value)}
    placeholder={en?'Search work or run ID':'작업·실행 ID 검색'}/>
  </label>
  <div className="native-run-list" role="list">
   {visible.map((item,index)=>{
    const focused=item.id===selectedId
    const isActive=active.has(item.status)
    const isReview=review.has(item.status)
    const Icon=isReview?AlertCircle:isActive?PlayCircle:item.status==='completed'?FileCheck2:Clock3
    return <div role="listitem" key={item.id}>
     <button type="button" onClick={()=>onSelect(item.id)}
      aria-current={focused?'true':undefined}
      className={'native-run-item '+(focused?'is-selected':'')+(isReview?' needs-attention':'')}>
      <span className="native-run-item-top">
       <span className="native-run-item-index">{String(index+1).padStart(2,'0')}</span>
       <span className={'native-run-status '+(isActive?'is-active':isReview?'is-review':'is-final')}>
        <Icon size={12}/>{en?stateLabel[item.status]?.[1]||item.status:stateLabel[item.status]?.[0]||item.status}
       </span>
      </span>
      <strong>{item.instruction||item.id}</strong>
      <span className="native-run-item-meta">
       <Workflow size={12}/>
       {item.checkpoint?.rounds??0} {en?'rounds':'회 판단'}
       <span>·</span>
       {item.checkpoint?.toolCalls??0} {en?'tool calls':'회 도구 실행'}
      </span>
     </button>
    </div>
   })}
   {!visible.length&&<div role="status" className="native-run-empty">
    <Workflow size={20}/>
    <strong>{en?'No matching tasks':'표시할 작업이 없습니다'}</strong>
    <p>{runs.length
      ?(en?'Try another status or keyword.':'상태나 검색어를 변경해 보세요.')
      :(en?'Created tasks will appear here.':'실행한 작업이 여기에 표시됩니다.')}</p>
   </div>}
  </div>
 </aside>
}
