import {Activity,LoaderCircle,RefreshCw} from 'lucide-react'
import {useLocale} from '../../i18n'
import {useRemote} from '../UI'
import {TokenUsage} from '../TokenUsage'
import {executionTokenDetails,numberOrNull,type ExecutionEvidence,type MessageUsage} from '../../utils/executionTokenEvidence'

const fmt=(value:number|null|undefined)=>
 typeof value==='number'&&Number.isFinite(value)?value.toLocaleString():'—'

/** Source-derived Sub4 execution footprint, adapted to Sub2 providerRounds.
 * The caller mounts this only after the owner explicitly opens token details. */
export function ExecutionTokenDetails({executionId,usage,createdAt}:{
 executionId?:string;usage?:MessageUsage;createdAt?:string
}){
 const locale=useLocale()
 const en=locale==='en'
 const remote=useRemote<ExecutionEvidence|null>(
  executionId?'/api/executions/'+encodeURIComponent(executionId):null,null,false)
 const record=remote.value
 const d=executionTokenDetails(record,usage)
 const timestamp=createdAt?new Date(createdAt.includes('T')?createdAt:createdAt.replace(' ','T')+'Z'):null
 const stamp=timestamp&&!Number.isNaN(timestamp.getTime())
  ?timestamp.toLocaleString(en?'en-US':'ko-KR',{month:'short',day:'numeric',hour:'2-digit',minute:'2-digit'}):''
 const hasEvidence=Boolean(record)
 const numericEntries=[
  [en?'System':'시스템',d.system],
  [en?'Conversation & user':'사용자·이전 대화',d.history],
  [en?'Retrieved context':'검색된 기억·자료',d.retrieval],
  [en?'Tool schemas':'도구 스키마',d.schema],
  [en?'Reserved output':'출력 예약',d.reserved],
 ] as const
 const verified=record?.verified===true||record?.taskOutcome==='succeeded'
 const normalCompletion=record?.status==='completed'&&
  (record?.taskOutcome==='answered_unverified'||record?.taskOutcome==='succeeded')
 const failure=normalCompletion?null:record?.failureClass
 return <div className="execution-token-details text-[11px] text-[#5C544A]" aria-label="실행·토큰 상세">
  {stamp&&<time className="block pb-3 text-[11px] text-[#938675]" dateTime={createdAt}>{stamp}</time>}
  {usage?.totalTokens!=null&&<div className="mb-3 text-[12px] text-[#7B6C59]">
   <TokenUsage usage={usage} className="inline-flex flex-wrap gap-1"/>
  </div>}
  {remote.loading?<div role="status" className="flex items-center gap-2 py-4"><LoaderCircle size={15} className="animate-spin"/>{en?'Loading execution details…':'실행 기록을 불러오는 중…'}</div>:
   remote.error?<div role="alert" className="rounded-lg border border-rose-200 bg-rose-50 p-3">
    {en?'Execution details unavailable':'실행 상세 기록을 불러오지 못했습니다.'}
    <div className="mt-1 break-words text-rose-700">{remote.error}</div>
    <button type="button" className="mt-2 inline-flex items-center gap-1 underline" onClick={()=>void remote.reload()}><RefreshCw size={12}/>{en?'Retry':'다시 시도'}</button>
   </div>:
   !hasEvidence?<p className="rounded-lg bg-[#F6F3EE] p-3 text-[#8A8176]">{en?'No linked execution evidence for this reply.':'이 답변에 연결된 실행 상세 기록이 없습니다.'}</p>:
   <>
    <div className="flex flex-wrap items-center justify-between gap-2">
     <strong className="text-[12px] text-[#3A342D]">{en?'Request footprint':'요청 구성'}</strong>
     <span className="max-w-full break-words text-[10px] text-[#948879]">{d.overallStatus||'—'} · {d.policy||'—'}</span>
    </div>
    <div className="mt-3 grid grid-cols-3 gap-2">
     <div className="rounded-lg border border-[#EFE9DF] bg-white p-2"><span className="block text-[#8A8176]">{en?'First input':'첫 입력'}</span><strong className="mt-1 block text-[14px]">{fmt(d.firstInput)}</strong><small className="text-[#8A8176]">{d.firstInputExact?(en?'Provider measured':'공급자 실측'):(en?'Estimated / unavailable':'추정·미확인')}</small></div>
     <div className="rounded-lg border border-[#EFE9DF] bg-white p-2"><span className="block text-[#8A8176]">{en?'Model calls':'모델 호출'}</span><strong className="mt-1 block text-[14px]">{fmt(d.modelCalls)}</strong><small className="text-[#8A8176]">{en?'Current execution':'현재 실행'}</small></div>
     <div className="rounded-lg border border-[#EFE9DF] bg-white p-2"><span className="block text-[#8A8176]">{en?'Exposed tools':'노출된 도구'}</span><strong className="mt-1 block text-[14px]">{fmt(d.tools)}</strong><small className="text-[#8A8176]">{en?'Advertised schemas':'모델 공개 스키마'}</small></div>
    </div>
    <div className="mt-3 grid grid-cols-2 gap-x-3 gap-y-1 rounded-lg bg-white p-3">
     {numericEntries.map(([label,value])=><div key={label} className="flex min-w-0 justify-between gap-2 border-b border-[#F3EEE5] py-1">
      <span className="min-w-0 text-[#8A8176]">{label}</span><strong className="shrink-0">{fmt(value)}</strong>
     </div>)}
    </div>
    <div className="mt-3 rounded-lg border border-[#EFE9DF] bg-white p-3">
     <div className="flex flex-wrap items-center justify-between gap-2">
      <strong className="text-[11px] text-[#3A342D]">{en?'Model token accounting':'모델 토큰 정산'}</strong>
      <span className={'rounded px-2 py-0.5 text-[10px] '+(d.allExact?'bg-emerald-50 text-emerald-800':'bg-[#F3EEE5] text-[#7C7469]')}>
       {d.allExact?(en?'Provider measured':'전부 공급자 실측'):d.estimatedCalls?(en?'Includes estimates':'추정값 포함'):(en?'Measurement unknown':'실측 여부 미상')}
      </span>
     </div>
     <div className="mt-3 grid grid-cols-2 gap-2 sm:grid-cols-4">
      <div><small className="block text-[#8A8176]">{en?'Input total':'입력 합계'}</small><strong>{fmt(d.input)}</strong></div>
      <div><small className="block text-[#8A8176]">{en?'Cached input':'캐시 입력'}</small><strong>{fmt(d.cached)}</strong></div>
      <div><small className="block text-[#8A8176]">{en?'New input':'신규 입력'}</small><strong>{fmt(d.newInput)}</strong></div>
      <div><small className="block text-[#8A8176]">{en?'Output total':'출력 합계'}</small><strong>{fmt(d.output)}</strong></div>
     </div>
     <p className="mt-2 text-[10px] text-[#8A8176]">{en?'Provider calls measured':'공급자 실측 호출'} {d.exactCalls} / {d.calls.length} · {en?'Estimated calls':'추정 호출'} {d.estimatedCalls}</p>
     <div className="mt-2 flex flex-wrap gap-2 border-t border-[#EFE9DF] pt-2 text-[10px]">
      <span>{en?'Primary tokens':'주 모델 토큰'}: {fmt(numberOrNull(d.accounting?.primaryTokens)??d.recordedTotal)}</span>
      <span>{en?'Auxiliary tokens':'보조 모델 토큰'}: {fmt(numberOrNull(d.accounting?.auxiliaryObservedTokens))}</span>
      <span>{en?'Total observed':'기록된 전체'}: {fmt(numberOrNull(d.accounting?.observedTotalTokens)??d.recordedTotal)}</span>
      {d.accounting&&<span>{d.accounting.usageComplete?(en?'Complete receipts':'호출 기록 완비'):(en?'Partial receipts':'호출 기록 일부 누락')}</span>}
     </div>
     <p className="mt-2 text-[10px] text-[#8A8176]">{en
      ?'KRW cost unavailable without verified provider rates.'
      :'원화 비용은 공급자 단가가 확인된 경우에만 계산합니다.'}</p>
    </div>
    {(record?.taskOutcome||record?.reason||failure)&&<div className="mt-3 rounded-lg border border-[#EFE9DF] bg-white p-3">
     <strong>{en?'Execution verification':'실행 검증·결과'}</strong>
     <div className="mt-2"><span className={'rounded border px-2 py-0.5 '+(verified?'border-emerald-200 bg-emerald-50 text-emerald-800':'border-amber-200 bg-amber-50 text-amber-800')}>{verified?(en?'Verified success':'성공 검증'):record?.taskOutcome||'unverified'}</span></div>
     {record?.acceptancePassed&&<p className="mt-2">{en?'Acceptance checks passed':'수락 조건 통과'}</p>}
     {failure&&<p className="mt-1 break-words text-rose-700">{failure}</p>}
     {record?.reason&&<p className="mt-1 break-words text-rose-700">{record.reason}</p>}
    </div>}
    <p className="mt-3 text-[10px] leading-5 text-[#8A8176]">{en
     ?'Context figures are pre-send estimates. Per-call receipts are measured only when the provider reports usage; missing cache reports are unknown.'
     :'요청 구성 수치는 전송 전 추정입니다. 호출별 실측은 공급자가 보고한 경우만 표시하며, 캐시 보고가 없으면 0으로 계산하지 않습니다.'}</p>
    {d.calls.length>0&&<section className="mt-3 border-t border-[#E7E0D5] pt-3" aria-label="모델 호출별 사용량">
     <strong>{en?'Model call ledger':'모델 호출별 사용량'}</strong>
     <div className="mt-2 max-h-44 space-y-1 overflow-y-auto">
      {d.calls.map((call,i)=><div key={i} className="flex flex-wrap items-center justify-between gap-2 rounded-lg bg-white px-2 py-2">
       <span>{en?'Call':'호출'} {call.index} · {call.exact?(en?'Measured':'실측'):call.estimated?(en?'Estimated':'추정'):(en?'Unknown':'미확인')}</span>
       <span>{en?'Input':'입력'} {fmt(call.prompt)} · {en?'Cache':'캐시'} {fmt(call.cached)} · {en?'Output':'출력'} {fmt(call.output)}</span>
      </div>)}
     </div>
    </section>}
   </>}
 </div>
}
