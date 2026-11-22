import {useState} from 'react'
import {Activity,ChevronDown,ChevronRight,LoaderCircle} from 'lucide-react'
import {useLocale} from '../i18n'
import {useRemote} from './UI'

type ModelCallUsage = {
 completedCallIndex?:number;
 promptTokens?:number|null;
 cachedPromptTokens?:number|null;
 newPromptTokens?:number|null;
 outputTokens?:number|null;
 estimatedPromptTokens?:number|null;
 estimatedOutputTokens?:number|null;
 usageExact?:boolean;
 providerId?:string;
 model?:string;
}
type InputContext = {
 policy?:string;
 systemTokens?:number|null;
 retrievalTokens?:number|null;
 historyTokens?:number|null;
 toolSchemaTokens?:number|null;
 totalInputTokens?:number|null;
 reservedOutputTokens?:number|null;
 requiredOverBudget?:boolean;
 providerPromptTokens?:number|null;
 providerCachedPromptTokens?:number|null;
 providerNewPromptTokens?:number|null;
 providerOutputTokens?:number|null;
 providerContextTokens?:number|null;
 modelCallUsage?:ModelCallUsage[];
}
type ExecutionEvidence = {
 id:string;status?:string;contextPolicy?:string;
 modelCalls?:number;toolCalls?:number;tokens?:number;
 discoveredTools?:string[];inputContext?:InputContext;
 modelAccounting?:{
  usageComplete?:boolean;tokenScope?:string;primaryTokens?:number|null;
  auxiliaryObservedTokens?:number|null;observedTotalTokens?:number|null;
  auxiliaryLogicalCalls?:number|null;primaryCalls?:number|null;reason?:string|null;
 };
 taskOutcome?:string|null;verified?:boolean;acceptancePassed?:boolean;
 failureClass?:string|null;reason?:string|null;
}
const fmt=(value?:number|null):string=>
 typeof value==='number'&&Number.isFinite(value)?value.toLocaleString():'—'

/** Inspect durable, provider-linked evidence on demand. Closed transcript
 * messages do not issue HTTP requests; this is not a second execution path. */
export function ExecutionInsights({executionId,compact=false}:{executionId:string;compact?:boolean}){
 const locale=useLocale()
 const en=locale==='en'
 const [open,setOpen]=useState(false)
 const remote=useRemote<ExecutionEvidence|null>(
  open?'/api/executions/'+encodeURIComponent(executionId):null,null,false)
 const record=remote.value
 const ctx=record?.inputContext
 const calls=Array.isArray(ctx?.modelCallUsage)?ctx.modelCallUsage:[]
 const first=calls[0]
 const firstPrompt=first?.usageExact?first.promptTokens:
  first?.estimatedPromptTokens??ctx?.totalInputTokens
 const schemaCount=record?.discoveredTools?.length
 const accounting=record?.modelAccounting
 const exactCalls=calls.filter(call=>call.usageExact===true).length
 const estimatedCalls=calls.filter(call=>call.usageExact===false).length
 const totalCalls=calls.length
 const exactAggregate=totalCalls>0&&estimatedCalls===0&&exactCalls===totalCalls
 const cached=ctx?.providerCachedPromptTokens
 const promptTotal=ctx?.providerPromptTokens
 const outputTotal=ctx?.providerOutputTokens
 const uncached=ctx?.providerNewPromptTokens
 const verified=record?.verified===true||record?.taskOutcome==='succeeded'
 // Older finalizers classified ordinary, completed replies as execution_error.
 // Do not turn that stale learning label into a red failure on the read-only UI.
 const legacyNormalReply=record?.status==='completed' &&
  (record?.taskOutcome==='answered_unverified'||record?.taskOutcome==='succeeded')
 const visibleFailureClass=legacyNormalReply?null:record?.failureClass
 const entries=[
  [en?'System':'시스템',ctx?.systemTokens],
  [en?'Past conversation':'이전 대화',ctx?.historyTokens],
  [en?'Retrieved context':'검색된 기억·자료',ctx?.retrievalTokens],
  [en?'Tool schemas':'도구 스키마',ctx?.toolSchemaTokens],
  [en?'Reserved output':'출력 예약',ctx?.reservedOutputTokens],
 ] as const
 return <div className={compact?'mt-1':'mt-3'}>
  <button type="button" aria-expanded={open}
   onClick={()=>setOpen(v=>!v)}
   className="inline-flex min-h-7 items-center gap-1.5 rounded-lg px-2 py-1 text-[11px] font-medium text-muted hover:bg-canvas focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-[var(--color-brand)]">
   <Activity size={13}/>
   {en?'Execution & token details':'실행·토큰 상세'}
   {open?<ChevronDown size={13}/>:<ChevronRight size={13}/>}
  </button>
  {open&&<div className="mt-2 min-w-0 rounded-xl border border-divider bg-surface p-3 text-[11px] text-muted">
   {remote.loading?<p role="status" className="flex items-center gap-2"><LoaderCircle className="animate-spin" size={13}/>{en?'Loading execution…':'실행 기록을 불러오는 중…'}</p>:
   remote.error?<div role="alert"><p className="break-words text-danger">{remote.error}</p>
     <button type="button" className="mt-2 underline" onClick={()=>void remote.reload()}>{en?'Retry':'다시 시도'}</button></div>:
   !record?<p>{en?'No execution record available.':'연결된 실행 기록이 없습니다.'}</p>:
   <>
    <div className="flex flex-wrap items-center justify-between gap-2">
     <strong className="text-[12px] text-ink">{en?'Request footprint':'요청 구성'}</strong>
     <span className="text-[10px] text-muted">{record.status||'—'} · {ctx?.policy||record.contextPolicy||'—'}</span>
    </div>
    <div className="mt-3 grid grid-cols-3 gap-2">
     <div className="rounded-lg border border-divider bg-surface p-2"><span className="block text-muted">{en?'First input':'첫 입력'}</span><strong className="mt-1 block text-[14px]">{fmt(firstPrompt)}</strong><small>{first?.usageExact?(en?'Provider-reported':'공급자 실측'):(en?'Estimated':'추정')}</small></div>
     <div className="rounded-lg border border-divider bg-surface p-2"><span className="block text-muted">{en?'Model calls':'모델 호출'}</span><strong className="mt-1 block text-[14px]">{fmt(record.modelCalls)}</strong><small>{en?'This execution':'현재 실행'}</small></div>
     <div className="rounded-lg border border-divider bg-surface p-2"><span className="block text-muted">{en?'Exposed tools':'노출된 도구'}</span><strong className="mt-1 block text-[14px]">{fmt(schemaCount)}</strong><small>{en?'Available schemas':'모델 공개 스키마'}</small></div>
    </div>
    <div className="mt-3 grid grid-cols-2 gap-x-4 gap-y-1 rounded-lg bg-surface p-3 sm:grid-cols-3">
     {entries.map(([label,value])=><div key={label} className="flex min-w-0 justify-between gap-2 border-b border-canvas py-1">
      <span className="truncate text-muted">{label}</span><strong className="shrink-0">{fmt(value)}</strong>
     </div>)}
    </div>
    <div className="mt-3 rounded-lg border border-divider bg-surface p-3">
     <div className="flex flex-wrap items-center justify-between gap-2">
      <strong className="text-[11px] text-ink">{en?'Model token accounting':'모델 토큰 정산'}</strong>
      <span className={'rounded-md px-2 py-0.5 text-[10px] '+(exactAggregate?'bg-success-soft text-success':'bg-canvas text-muted')}>
       {exactAggregate?(en?'Provider measured':'전부 공급자 실측'):estimatedCalls>0?(en?'Includes estimates':'추정값 포함'):(en?'Measurement unavailable':'실측 여부 미상')}
      </span>
     </div>
     <div className="mt-2 grid grid-cols-2 gap-2 text-[11px] sm:grid-cols-4">
      <div><small className="block text-muted">{en?'Input total':'입력 합계'}</small><strong>{fmt(promptTotal)}</strong></div>
      <div><small className="block text-muted">{en?'Cached input':'캐시 입력'}</small><strong>{fmt(cached)}</strong></div>
      <div><small className="block text-muted">{en?'New input':'신규 입력'}</small><strong>{fmt(uncached)}</strong></div>
      <div><small className="block text-muted">{en?'Output total':'출력 합계'}</small><strong>{fmt(outputTotal)}</strong></div>
     </div>
     <p className="mt-2 text-[10px] text-muted">{en?'Provider calls measured':'공급자 실측 호출'} {exactCalls} / {totalCalls} · {en?'Estimated calls':'추정 호출'} {estimatedCalls}</p>
     {accounting&&<div className="mt-2 flex flex-wrap gap-3 border-t border-divider pt-2 text-[10px]">
      <span>{en?'Primary tokens':'주 모델 토큰'}: {fmt(accounting.primaryTokens)}</span>
      <span>{en?'Auxiliary tokens':'보조 모델 토큰'}: {fmt(accounting.auxiliaryObservedTokens)}</span>
      <span>{en?'Total observed':'기록된 전체'}: {fmt(accounting.observedTotalTokens)}</span>
      <span>{accounting.usageComplete?(en?'Complete receipt coverage':'호출 기록 완비'):(en?'Partial receipt coverage':'호출 기록 일부 누락')}</span>
     </div>}
     <p className="mt-2 text-[10px] text-muted">{en
      ?'KRW cost: unavailable without an attributable provider price. No price is fabricated.'
      :'원화 비용: 호출별 공급자 단가가 연결되지 않아 표시하지 않습니다. 임의 비용은 계산하지 않습니다.'}</p>
    </div>
    {(record.taskOutcome||record.reason||visibleFailureClass)&&<div className="mt-3 rounded-lg border border-divider bg-surface p-3 text-[11px]">
     <strong>{en?'Execution verification':'실행 검증·결과'}</strong>
     <div className="mt-1 flex flex-wrap gap-2">
      <span className={'rounded border px-2 py-0.5 '+(verified?'border-success/20 bg-success-soft text-success':'border-warning/20 bg-warning-soft text-warning')}>{verified?(en?'Verified success':'성공 검증'):(record.taskOutcome||'unverified')}</span>
      {record.acceptancePassed===true&&<span>{en?'Acceptance checks passed':'수락 조건 통과'}</span>}
     </div>
     {visibleFailureClass&&<p className="mt-1 text-danger">{visibleFailureClass}</p>}
     {record.reason&&<p className="mt-1 break-words text-danger">{record.reason}</p>}
    </div>}
    <p className="mt-2 text-[10px] leading-4 text-muted">{en
      ?'Breakdown values are pre-send estimates. Per-call values use provider reports when available; cache misses are never inferred from missing data.'
      :'구성별 숫자는 전송 전 추정치입니다. 호출별 수치는 가능한 경우 공급자 실측값을 사용하며, 캐시 정보가 없으면 0으로 표시하지 않습니다.'}</p>
    {ctx?.requiredOverBudget&&<p className="mt-2 text-[10px] text-warning">{en?'Pre-send compiler estimated a capacity overflow; the provider wire is checked separately.':'컨텍스트 컴파일 단계에서 한도 초과가 추정됐습니다. 실제 전송 요청은 별도로 검사합니다.'}</p>}
    {calls.length>0&&<div className="mt-3 border-t border-divider pt-2">
     <strong>{en?'Model call ledger':'모델 호출별 사용량'}</strong>
     <div className="mt-2 max-h-40 space-y-1 overflow-y-auto">
      {calls.map((call,index)=>{
       const providerInput=call.usageExact?call.promptTokens:call.estimatedPromptTokens
       const providerOutput=call.usageExact?call.outputTokens:call.estimatedOutputTokens
       return <div key={index} className="flex flex-wrap justify-between gap-2 rounded-lg bg-surface px-2 py-1.5">
        <span>{en?'Call':'호출'} {call.completedCallIndex??index+1} · {call.usageExact?(en?'Measured':'실측'):(en?'Estimated':'추정')}</span>
        <span>{en?'Input':'입력'} {fmt(providerInput)} · {en?'Cache':'캐시'} {fmt(call.cachedPromptTokens)} · {en?'Output':'출력'} {fmt(providerOutput)}</span>
       </div>
      })}
     </div>
    </div>}
   </>}
  </div>}
 </div>
}
