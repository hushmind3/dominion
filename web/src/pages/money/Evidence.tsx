import {useState} from 'react'
import {ArrowUpRight} from 'lucide-react'
import {get} from '../../api'

export type MoneyEvidenceRow={
 url?:string;claim?:string;quote?:string;sourceId?:string
 sourceRevisionId?:string;quotationStatus?:string
}
type Revision={id:string;observedAt?:string;articleExcerpt?:string;contentHash?:string}
type RevisionResponse={currentRevisionId?:string;revisions:Revision[]}
const labels:Record<string,string>={
 archived_page_quote_match:'본문 문구 일치 · 사실·거래 미확인',
 source_quote_mismatch:'본문 인용 불일치',
 missing_exact_quote:'인용문 없음 · 원문 확인 필요',
 source_not_fetched:'원문 본문 미수집',
 source_not_archived:'출처 미보관',
 invalid_source_url:'출처 주소 확인 필요'
}
function EvidenceEntry({row,index}:{row:MoneyEvidenceRow;index:number}){
 const [opened,setOpened]=useState(false)
 const [history,setHistory]=useState<RevisionResponse|null>(null)
 const [loading,setLoading]=useState(false)
 const [error,setError]=useState('')
 async function openHistory(){
  if(!row.sourceId)return
  if(opened){setOpened(false);return}
  setOpened(true)
  if(history)return
  setLoading(true)
  try{
   const result=await get<RevisionResponse>(
    '/api/money/sources/'+encodeURIComponent(row.sourceId)+'/revisions')
   setHistory(result)
  }catch(e){setError((e as Error).message)}
  finally{setLoading(false)}
 }
 return <div className="rounded-lg border border-[#EEE5D9] bg-[#FFFBF6] px-3 py-2">
  <div className="flex flex-wrap items-center gap-2">
   {row.url?.startsWith('https://')?
    <a href={row.url} target="_blank" rel="noreferrer"
     className="inline-flex items-center gap-1 text-[11px] font-semibold text-[#9F583C] underline">
     원문 {index+1}<ArrowUpRight size={12}/>
    </a>:<span className="text-[11px] text-[#7B726A]">원문 주소 미확인</span>}
   <span className="text-[11px] text-[#786757]">{labels[row.quotationStatus||'']||'인용 검증 기록 없음'}</span>
   {row.sourceId&&<button type="button" onClick={()=>void openHistory()}
    className="text-[11px] font-semibold text-[#9F583C] underline">
    {opened?'리비전 닫기':'저장된 원문 버전 보기'}
   </button>}
  </div>
  {row.quote&&<p className="mt-1 text-[11px] leading-5 text-[#665548]">인용: “{row.quote}”</p>}
  {row.claim&&<p className="mt-1 text-[11px] leading-5 text-[#8D7B6C]">주장: {row.claim}</p>}
  {opened&&<div className="mt-2 space-y-2 border-t border-[#EEE5D9] pt-2">
   {loading&&<p className="text-[11px]">보관된 원문 확인 중…</p>}
   {error&&<p role="alert" className="text-[11px] text-red-700">{error}</p>}
   {history?.revisions.map(r=><details key={r.id} className="text-[11px] leading-5 text-[#665548]">
    <summary className="cursor-pointer font-semibold">
     {r.id===history.currentRevisionId?'현재 보관본 · ':'이전 보관본 · '}
     {r.observedAt||'수집 시각 미확인'} · {r.contentHash?.slice(0,10)}
    </summary>
    <p className="mt-1 max-h-48 overflow-auto whitespace-pre-wrap break-words">
     {(r.articleExcerpt||'본문 없음').slice(0,5000)}
    </p>
   </details>)}
   {history&&!history.revisions.length&&<p className="text-[11px]">
    기존 원문 기록에 별도 리비전이 없습니다.
   </p>}
  </div>}
 </div>
}
export function MoneyEvidence({items,limit=4}:{items:MoneyEvidenceRow[];limit?:number}){
 if(!items.length)return null
 return <div className="mt-3 space-y-2">
  <strong className="block text-[11px] text-[#8F604A]">
   저장된 근거·인용 상태 (거래 검증과 별개)
  </strong>
  {items.slice(0,limit).map((row,i)=><EvidenceEntry key={(row.sourceRevisionId||row.url||'source')+'-'+i}
   row={row} index={i}/>)}
 </div>
}
