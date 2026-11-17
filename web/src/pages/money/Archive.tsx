import {useEffect,useRef,useState} from 'react'
import {ChevronDown,RefreshCw,Search} from 'lucide-react'
import {MoneyEvidence,type MoneyEvidenceRow} from './Evidence'
import {get,post} from '../../api'

type Candidate={
 id:string;title:string;status?:string;origin?:string;customer?:string
 offer?:string;whyPay?:string;firstTest?:string;createdAt?:string
 evidence?:MoneyEvidenceRow[];implementationRunId?:string
}
type Archive={candidates:Candidate[];hasMore:boolean;offset:number;limit:number}
const labels:Record<string,string>={
 hypothesis:'추론 후보',researching:'재검토 중',testing:'실험 중',
 validated:'검증된 기록',deferred:'보류',rejected:'제외',
 launched:'출시 기록',implementation_requested:'구현 승인됨'
}

export function MoneyArchive({enabled}:{enabled:boolean}){
 const [expanded,setExpanded]=useState(false)
 const [query,setQuery]=useState('')
 const [status,setStatus]=useState('')
 const [rows,setRows]=useState<Candidate[]>([])
 const [hasMore,setHasMore]=useState(false)
 const [loading,setLoading]=useState(false)
 const [saving,setSaving]=useState('')
 const [error,setError]=useState('')
 const seq=useRef(0)
 const fetchPage=async(offset:number,q=query,filter=status)=>{
  const request=++seq.current
  setLoading(true);setError('')
  try{
   const params=new URLSearchParams({q:q.trim(),status:filter,limit:'20',offset:String(offset)})
   const response=await get<Archive>('/api/money/candidates/archive?'+params.toString())
   if(request!==seq.current)return
   setRows(old=>offset===0?response.candidates:[...old,...response.candidates])
   setHasMore(response.hasMore)
  }catch(e){if(request===seq.current)setError((e as Error).message)}
  finally{if(request===seq.current)setLoading(false)}
 }
 useEffect(()=>{
  if(!expanded)return
  void fetchPage(0)
  return()=>{seq.current++}
  // A user explicitly submits a search, so typing never triggers DB work.
  // eslint-disable-next-line react-hooks/exhaustive-deps
 },[expanded])
 const changeStatus=async(candidate:Candidate,target:'deferred'|'researching')=>{
  if(saving||!enabled)return
  setSaving(candidate.id);setError('')
  try{
   const updated=await post<Candidate>(
    '/api/money/candidates/'+encodeURIComponent(candidate.id),{status:target})
   setRows(items=>items.map(item=>item.id===candidate.id?{...item,...updated}:item))
  }catch(e){setError((e as Error).message)}
  finally{setSaving('')}
 }
 return <section className="mt-7 rounded-2xl border border-[#E4D8C9] bg-[#FFFDF9]">
  <button type="button" aria-expanded={expanded} onClick={()=>setExpanded(open=>!open)}
   className="flex w-full items-center justify-between gap-3 px-5 py-4 text-left">
   <span><strong className="block text-sm text-[#3B2E24]">전체 후보·보류 기록</strong>
    <small className="mt-1 block text-[11px] text-[#8D7B6C]">
     상위 3건 외의 후보와 제외한 가설도 다시 검색하고 검토합니다.
    </small></span>
   <ChevronDown size={18} className={'shrink-0 text-[#99775C] '+(expanded?'rotate-180':'')}/>
  </button>
  {expanded&&<div className="border-t border-[#EFE6DC] px-5 py-4">
   <form onSubmit={e=>{e.preventDefault();void fetchPage(0)}} className="flex flex-wrap items-center gap-2">
    <input aria-label="후보·구매자 검색" value={query} onChange={e=>setQuery(e.target.value)}
     placeholder="사업·구매자·수익구조 검색" maxLength={120}
     className="min-h-9 min-w-[160px] flex-1 rounded-lg border border-[#DAD0C3] bg-white px-3 text-xs"/>
    <select aria-label="후보 상태" value={status} onChange={e=>setStatus(e.target.value)}
     className="min-h-9 rounded-lg border border-[#DAD0C3] bg-white px-2 text-xs">
     <option value="">모든 상태</option>
     {Object.entries(labels).map(([id,label])=><option key={id} value={id}>{label}</option>)}
    </select>
    <button type="submit" disabled={loading}
     className="inline-flex min-h-9 items-center gap-1 rounded-lg bg-[#9B6A3F] px-3 text-xs font-bold text-white disabled:opacity-50">
     <Search size={14}/> 검색
    </button>
   </form>
   {error&&<p role="alert" className="mt-3 text-xs text-red-700">{error}</p>}
   <div className="mt-3 space-y-2">
    {rows.map(item=><article key={item.id} className="rounded-lg border border-[#EEE5D9] bg-[#FFFBF6] px-3 py-3">
     <div className="flex flex-wrap items-start justify-between gap-2">
      <strong className="text-xs text-[#403227]">{item.title||'제목 없는 기록'}</strong>
      <span className="text-[10px] font-semibold text-[#9D593D]">{labels[item.status||'hypothesis']||item.status}</span>
     </div>
     <p className="mt-1 text-[11px] text-[#786757]">{item.customer||'구매자 미상'} · {item.offer||'제공 결과 미상'}</p>
     <details className="mt-2 text-[11px] text-[#665548]">
      <summary className="cursor-pointer font-semibold">수익 이유·원문 확인</summary>
      <p className="mt-2 whitespace-pre-wrap">{item.whyPay||'추가 조사 필요'}</p>
      {item.firstTest&&<p className="mt-1">다음 확인: {item.firstTest}</p>}
      <MoneyEvidence items={item.evidence||[]} limit={4}/>
     </details>
     {enabled&&<div className="mt-2 flex flex-wrap gap-2">
      {item.status!=='deferred'&&item.status!=='rejected'&&
       <button type="button" disabled={!!saving} onClick={()=>void changeStatus(item,'deferred')}
        className="text-[11px] font-semibold text-[#8F604A] underline disabled:opacity-50">보류하기</button>}
      {['deferred','rejected'].includes(item.status||'')&&
       <button type="button" disabled={!!saving} onClick={()=>void changeStatus(item,'researching')}
        className="text-[11px] font-semibold text-[#8F604A] underline disabled:opacity-50">재검토로 이동</button>}
     </div>}
    </article>)}
    {!rows.length&&!loading&&<p className="py-6 text-center text-xs text-[#8D7B6C]">검색 조건에 맞는 후보가 없습니다.</p>}
   </div>
   {hasMore&&<button type="button" disabled={loading} onClick={()=>void fetchPage(rows.length)}
    className="mt-3 inline-flex items-center gap-2 text-xs font-semibold text-[#9D593D] disabled:opacity-50">
    <RefreshCw size={14}/> {loading?'불러오는 중…':'다음 20건 보기'}
   </button>}
  </div>}
 </section>
}
