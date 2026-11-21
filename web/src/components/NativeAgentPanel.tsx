import {useEffect, useRef, useState} from 'react'
import {Check, Play, RefreshCw, ShieldAlert, Square, Sparkles, MessageSquareText} from 'lucide-react'
import {get, post, type Provider} from '../api'
import {useLocale} from '../i18n'

type SdkState = {state: string; detail?: {call_id?: string; tool?: string}}
type SdkRun = {
 id: string; status: string; instruction: string; answer?: string; error?: string;
 algorithmId?:string; algorithmRevision?:string;
 checkpoint?: {
  state?: SdkState; rounds?: number; toolCalls?: number; answer?: string;
  usage?: {prompt_tokens:number; completion_tokens:number; cached_prompt_tokens:number};
  pendingCall?: {id:string; name:string; arguments:unknown} | null;
  events?: {kind:string; detail:unknown}[];
  verificationChecks?: {satisfied:boolean;explanation?:string}[];
  toolReceipts?: {id:string;tool:string;success:boolean;step:number}[];
  toolReceiptsOmitted?:number;
  artifactEvidence?: {captureComplete:boolean;omitted:number;
   entries:{path:string;state:string;bytes?:number|null;sha256?:string|null}[]} | null;
 } | null
}
const running = new Set(['queued', 'running'])
const stopPolling = new Set(['completed','failed','cancelled','exhausted','denied'])
const stateNames:Record<string,[string,string]>={
 queued:['대기 중','Queued'], running:['작업 진행 중','Working'],
 completed:['완료','Completed'], failed:['실패','Failed'],
 cancelled:['중단됨','Stopped'], exhausted:['실행 한도 도달','Limit reached'],
 awaiting_approval:['실행 승인 필요','Needs approval'],
 in_doubt:['수동 확인 필요','Needs manual review'],
 interrupted:['재개 가능','Resumable'], denied:['실행 거부됨','Denied'],
 needs_review:['검토 필요 · 자동 재실행 중지','Review required · replay blocked']
}
export function NativeAgentPanel({providers}:{providers:Provider[]}){
 const locale=useLocale()
 const en=locale==='en'
 const [instruction,setInstruction]=useState('')
 const [providerId,setProviderId]=useState('')
 const [verification,setVerification]=useState('')
 const [algorithmId,setAlgorithmId]=useState<'standard-v1'|'focused-v1'>('standard-v1')
 const [maxRounds,setMaxRounds]=useState(24)
 const [steering,setSteering]=useState('')
 const [runs,setRuns]=useState<SdkRun[]>([])
 const [observation,setObservation]=useState('')
 const [run,setRun]=useState<SdkRun|null>(null)
 const [busy,setBusy]=useState(false)
 const [error,setError]=useState('')
 const [disconnected,setDisconnected]=useState(false)
 // A pre-action poll must not revive a refused/approved tool after the
 // server has accepted the exact owner decision.
 const pollEpoch=useRef(0)
 const mutationInProgress=useRef(false)
 // A late response from a different selected task must never replace the active run.
 const updateRun=(next:SdkRun)=>{
  setRun(current=>current?.id===next.id?next:current)
  setRuns(rows=>rows.some(row=>row.id===next.id)
   ?rows.map(row=>row.id===next.id?next:row):[next,...rows].slice(0,30))
 }
 useEffect(()=>{
  let alive=true
  get<SdkRun[]>('/api/sdk/runs').then(list=>{
   if(alive){setRuns(list);if(list.length)setRun(list[0])}
  }).catch(()=>{})
  return()=>{alive=false}
 },[])
 useEffect(()=>{
  if(!run?.id)return
  let alive=true
  let fetching=false
  const selectedId=run.id
  const refresh=()=>{
   if(!alive||fetching||mutationInProgress.current||document.visibilityState==='hidden')return
   fetching=true
   const epoch=pollEpoch.current
   get<SdkRun>(`/api/sdk/runs/${encodeURIComponent(selectedId)}`)
    .then(next=>{
     if(alive&&epoch===pollEpoch.current&&!mutationInProgress.current){
      updateRun(next);setDisconnected(false)
     }
    })
    .catch(()=>{
     if(alive&&epoch===pollEpoch.current&&!mutationInProgress.current)setDisconnected(true)
    })
    .finally(()=>{fetching=false})
  }
  // Approval may be resolved on another tab, and an interrupted run may
  // change after reconnection. Keep server state authoritative on focus.
  const timer=window.setInterval(refresh,stopPolling.has(run.status)?15000:2500)
  window.addEventListener('focus',refresh)
  document.addEventListener('visibilitychange',refresh)
  return()=>{
   alive=false;window.clearInterval(timer)
   window.removeEventListener('focus',refresh)
   document.removeEventListener('visibilitychange',refresh)
  }
 },[run?.id,run?.status])
 const start=async()=>{
  if(!instruction.trim()||busy)return
  setBusy(true);setError('')
  try{
   const created=await post<SdkRun>('/api/sdk/runs',{
    instruction:instruction.trim(),providerId:providerId||undefined,
    verifyCommand:verification.trim()||undefined,algorithmId,maxRounds
   })
   setRun(created)
   setRuns(rows=>[created,...rows.filter(row=>row.id!==created.id)].slice(0,30))
   setSteering('')
  }catch(e){setError((e as Error).message)}
  finally{setBusy(false)}
 }
 const action=async(kind:'approve'|'deny'|'cancel'|'resume')=>{
  if(!run||busy)return
  mutationInProgress.current=true
  pollEpoch.current+=1
  setBusy(true);setError('')
  try{
   const call=run.checkpoint?.pendingCall
   if(kind==='approve'||kind==='deny'){
    if(run.status!=='awaiting_approval'||!call?.id)
     throw Error(en?'Missing approval details':'승인 대상 도구 정보를 찾지 못했습니다.')
    // Confirm the exact live call against the server before allowing effects.
    const latest=await get<SdkRun>(`/api/sdk/runs/${encodeURIComponent(run.id)}`)
    if(latest.status!=='awaiting_approval'||latest.checkpoint?.pendingCall?.id!==call.id){
     updateRun(latest)
     throw Error(en?'The approval target changed. Review the new request.':'승인 대상이 변경됐습니다. 새 요청을 확인해 주세요.')
    }
   }
   const updated=await post<SdkRun>(`/api/sdk/runs/${encodeURIComponent(run.id)}/${kind}`,
    kind==='approve'||kind==='deny'?{callId:call!.id}:{})
   // The host, not the button click, determines the actual next phase.
   updateRun(updated)
  }catch(e){setError((e as Error).message)}
  finally{mutationInProgress.current=false;setBusy(false)}
 }
 const reconcile=async(applied:boolean)=>{
  if(!run||busy||!observation.trim())return
  mutationInProgress.current=true
  pollEpoch.current+=1
  setBusy(true);setError('')
  try{
   const updated=await post<SdkRun>(`/api/sdk/runs/${encodeURIComponent(run.id)}/reconcile`,
    {applied,details:observation.trim()})
   updateRun(updated);setObservation('')
  }catch(e){setError((e as Error).message)}
  finally{mutationInProgress.current=false;setBusy(false)}
 }
 const steer=async()=>{
  if(!run||busy||!running.has(run.status)||!steering.trim())return
  mutationInProgress.current=true
  pollEpoch.current+=1
  setBusy(true);setError('')
  try{
   const updated=await post<SdkRun>(`/api/sdk/runs/${encodeURIComponent(run.id)}/steer`,{
    instruction:steering.trim(),mode:'append'
   })
   updateRun(updated);setSteering('')
  }catch(e){setError((e as Error).message)}
  finally{mutationInProgress.current=false;setBusy(false)}
 }
 const reload=async()=>{
  setError('')
  try{
   const list=await get<SdkRun[]>('/api/sdk/runs')
   setRuns(list)
   setDisconnected(false)
   if(run?.id){
    const selected=list.find(row=>row.id===run.id)
    if(selected)setRun(selected)
   }
  }catch(e){setError((e as Error).message)}
 }
 const status=run?.status||''
 const call=run?.checkpoint?.pendingCall
 const needsApproval=status==='awaiting_approval' && call
 const canResume=status==='interrupted'
 const events=(run?.checkpoint?.events||[]).slice(-4)
 const toolReceipts=run?.checkpoint?.toolReceipts||[]
 return <section className="mt-4 rounded-xl border border-[#E7E0D5] bg-white">
  <div className="flex items-center gap-2 border-b border-[#F0EAE1] px-4 py-3">
   <Sparkles size={17} className="text-[#AF5B3A]"/>
   <div><h2 className="text-[13px] font-bold">{en?'Autonomous work':'자동 작업'}</h2>
    <p className="mt-0.5 text-[11px] text-[#8A8176]">{en?'Describe what you need. Dominion’s own Rust engine handles the steps.':'할 일을 말씀하시면 도미니언 Rust 엔진이 작업을 실행합니다.'}</p>
   </div>
  </div>
  <div className="grid gap-3 p-4">
   <label className="text-[12px] font-semibold" htmlFor="sdk-goal">{en?'What would you like done?':'무엇을 해드릴까요?'}</label>
   <textarea id="sdk-goal" value={instruction} onChange={e=>setInstruction(e.target.value)}
    rows={3} maxLength={64000}
    placeholder={en?'Describe a task, such as inspecting a project or fixing an issue.':'프로젝트를 분석하거나 문제를 해결하는 등 필요한 작업을 적어주세요.'}
    className="w-full resize-y rounded-xl border border-[#E7E0D5] bg-[#FCFBF9] p-3 text-[12px] outline-none focus:border-[#A36A4A]"/>
   <div className="flex flex-wrap items-end gap-3">
    <label className="flex min-w-44 flex-1 flex-col gap-1 text-[11px] text-[#746A5F]">
     {en?'AI model':'사용할 AI 모델'}
     <select value={providerId} onChange={e=>setProviderId(e.target.value)}
      className="min-h-11 rounded-lg border border-[#E7E0D5] bg-white px-3 text-[12px] focus-visible:outline-2 focus-visible:outline-offset-2">
      <option value="">{en?'Automatic selection':'자동 선택'}</option>
      {providers.filter(p=>p.enabled).map(p=><option key={p.id} value={p.id}>{p.name}{p.model?` · ${p.model}`:''}</option>)}
     </select>
    </label>
    <button type="button" onClick={()=>void start()} disabled={busy||!instruction.trim()}
     className="flex min-h-11 items-center gap-2 rounded-lg bg-[#AE5337] px-4 text-[12px] font-semibold text-white focus-visible:outline-2 focus-visible:outline-offset-2 disabled:opacity-40">
     <Play size={14}/>{busy?(en?'Please wait…':'처리 중…'):(en?'Start task':'작업 시작')}
    </button>
   </div>
   <details className="text-[11px] text-[#746A5F]">
    <summary className="cursor-pointer select-none">{en?'Advanced: strategy, budget and verification':'고급 설정: 전략 · 실행 한도 · 결과 검증'}</summary>
    <div className="mt-3 grid gap-3 sm:grid-cols-2">
     <label className="grid gap-1" htmlFor="sdk-strategy">{en?'Execution strategy':'실행 전략'}
      <select id="sdk-strategy" value={algorithmId} onChange={e=>setAlgorithmId(e.target.value as 'standard-v1'|'focused-v1')}
       className="min-h-11 w-full rounded-lg border border-[#E7E0D5] bg-white px-3 text-[12px] focus-visible:outline-2 focus-visible:outline-offset-2">
       <option value="standard-v1">{en?'Standard · balanced':'기본 · 균형'}</option>
       <option value="focused-v1">{en?'Focused · compact context':'집중 · 압축 문맥'}</option>
      </select></label>
     <label className="grid gap-1" htmlFor="sdk-rounds">{en?'Maximum model rounds':'최대 모델 판단 회수'}
      <select id="sdk-rounds" value={maxRounds} onChange={e=>setMaxRounds(Number(e.target.value))}
       className="min-h-11 w-full rounded-lg border border-[#E7E0D5] bg-white px-3 text-[12px] focus-visible:outline-2 focus-visible:outline-offset-2">
       {[12,24,40,80].map(n=><option key={n} value={n}>{n}</option>)}
      </select></label>
    </div>
    <p className="mt-2 text-[10px]">{en?
     'These limits are enforced by the Rust kernel, not by the model.':
     '실행 한도는 모델 요청이 아니라 Rust 커널에서 적용합니다.'}</p>
    <label className="mt-2 block" htmlFor="sdk-verify">
     {en?'Verification command (runs after the agent finishes)':'검증 명령어 (작업 결과를 확인할 때 실행)'}
    </label>
    <input id="sdk-verify" value={verification} onChange={e=>setVerification(e.target.value)}
     maxLength={2048} placeholder="cargo test"
     className="mt-1 min-h-11 w-full rounded-lg border border-[#E7E0D5] px-3 text-[12px] focus-visible:outline-2 focus-visible:outline-offset-2"/>
   </details>
   {runs.length>0&&<div className="flex min-w-0 flex-wrap items-end gap-2 rounded-lg bg-[#FAF7F1] p-2">
    <label htmlFor="sdk-history" className="grid min-w-0 flex-1 gap-1 text-[11px] text-[#746A5F]">
     {en?'Previous tasks':'이전 자동 작업'}
     <select id="sdk-history" className="min-h-11 min-w-0 rounded-lg border border-[#E7E0D5] bg-white px-2 text-[12px] focus-visible:outline-2 focus-visible:outline-offset-2"
      value={run?.id||''} onChange={e=>{
       const item=runs.find(row=>row.id===e.target.value)
       if(item){setRun(item);setSteering('');setObservation('');setError('')}
      }}>
      {runs.map(item=><option key={item.id} value={item.id}>
       {item.status} · {item.instruction.slice(0,75)}</option>)}
     </select></label>
    <button type="button" className="flex size-11 items-center justify-center rounded-lg border border-[#E7E0D5] bg-white focus-visible:outline-2 focus-visible:outline-offset-2"
     aria-label={en?'Refresh task history':'작업 이력 새로고침'}
     onClick={()=>void reload()}><RefreshCw size={16}/></button>
   </div>}
   {disconnected&&<p role="status" className="rounded-lg border border-amber-200 bg-amber-50 p-3 text-[11px] text-amber-900">{en?'Connection lost. Actions may have changed; reconnecting to the server.':'서버 연결이 끊겼습니다. 실행 상태가 바뀌었을 수 있어 재연결 후 다시 확인합니다.'}</p>}
   {error&&<p role="alert" className="rounded-lg bg-[#FFF3EE] p-3 text-[11px] text-[#A54027]">{error}</p>}
   {run&&<div className="rounded-xl border border-[#E7E0D5] bg-[#FCFBF9] p-3">
    <div className="flex flex-wrap items-center justify-between gap-2" aria-live="polite" aria-atomic="true">
     <strong className="text-[12px]">{en?stateNames[status]?.[1]||status:stateNames[status]?.[0]||status}</strong>
     <span className="text-[11px] text-[#82796D]">{run.checkpoint?.rounds??0} {en?'rounds':'회 판단'} · {run.checkpoint?.toolCalls??0} {en?'actions':'회 도구 실행'}</span>
    {run.checkpoint?.usage&&<p className="mt-2 text-[11px] text-[#82796D]">
     {en?'Provider-reported tokens':'제공자 보고 토큰'}: {run.checkpoint.usage.prompt_tokens.toLocaleString()} {en?'input':'입력'} · {run.checkpoint.usage.completion_tokens.toLocaleString()} {en?'output':'출력'} · {run.checkpoint.usage.cached_prompt_tokens.toLocaleString()} {en?'cached input':'캐시 입력'}
    </p>}
    </div>
    <p className="mt-2 break-words text-[11px] text-[#766F67]" title={run.instruction}>{run.instruction}</p>
    <p className="mt-1 text-[10px] text-[#8A8176]">{en?'Strategy':'실행 전략'}: {run.algorithmId||'standard-v1'}</p>
    {events.length>0&&<div className="mt-3 space-y-1 border-t border-[#E7E0D5] pt-2">
     {events.map((event,i)=><p key={i} className="text-[11px] text-[#82796D]">
      {event.kind==='tool_started'?(en?'Running tool':'도구 실행'):
       event.kind==='model_started'?(en?'Model is working':'모델 판단 중'):
       event.kind==='verified'?(en?'Result verification':'결과 검증'):event.kind}
     </p>)}
    </div>}
    {needsApproval&&<div className="mt-3 rounded-lg border border-[#E5C4AB] bg-white p-3">
     <div className="flex items-center gap-2 text-[12px] font-semibold"><ShieldAlert size={16}/>
      {en?'Review before allowing this action':'아래 작업을 확인하고 승인하세요'}
     </div>
     <div className="mt-2 break-words text-[11px] font-semibold">{call.name}</div>
     <p className="mt-1 break-all text-[10px] text-[#766F67]">{en?'Exact call ID':'정확한 호출 ID'}: {call.id}</p>
     <pre className="mt-2 max-h-56 overflow-auto whitespace-pre-wrap break-all rounded-lg bg-[#F5F2EE] p-3 text-[11px]">{JSON.stringify(call.arguments,null,2)}</pre>
     <div className="mt-3 flex flex-wrap gap-2">
      <button type="button" disabled={busy} onClick={()=>void action('approve')}
       className="flex min-h-11 items-center gap-2 rounded-lg bg-[#A4583E] px-4 py-2 text-[12px] font-semibold text-white focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[#A4583E] disabled:opacity-40">
       <Check size={14}/>{en?'Approve this action only':'이 작업 1회 승인'}
      </button>
      <button type="button" disabled={busy} onClick={()=>void action('deny')}
       className="flex min-h-11 items-center gap-2 rounded-lg border border-rose-200 bg-white px-4 py-2 text-[12px] font-semibold text-rose-700 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-rose-700 disabled:opacity-40">
       <Square size={13}/>{en?'Refuse this action':'이 도구 실행 거부'}
      </button>
     </div>
    </div>}
    {run.answer||run.checkpoint?.answer?<div className="mt-3 whitespace-pre-wrap break-words border-t border-[#E7E0D5] pt-3 text-[12px]">{run.answer||run.checkpoint?.answer}</div>:null}
    {toolReceipts.length>0&&<details className="mt-3 min-w-0 rounded-lg border border-[#E7E0D5] bg-white p-3 text-[11px]">
     <summary className="cursor-pointer font-semibold">
      {en?'Recorded tool calls':'도구 실행 기록'} · {run.checkpoint?.toolCalls??toolReceipts.length}
     </summary>
     <p className="mt-2 text-[10px] text-[#82796D]">{en?
      'These are persisted tool receipts, not independent proof that the requested result was achieved.':
      '저장된 도구 실행 영수증입니다. 요청 결과의 독립 검증과는 구분됩니다.'}</p>
     {Boolean(run.checkpoint?.toolReceiptsOmitted)&&<p className="mt-1 text-[10px] text-[#82796D]">
      {en?`${run.checkpoint?.toolReceiptsOmitted} earlier entries not shown`:
       `이전 기록 ${run.checkpoint?.toolReceiptsOmitted}건은 표시 생략`}
     </p>}
     <ol className="mt-2 max-h-64 space-y-1 overflow-y-auto" aria-label={en?'Persisted tool receipts':'저장된 도구 영수증'}>
      {toolReceipts.map((receipt,i)=><li key={receipt.id||i}
       className="flex min-w-0 flex-wrap items-start justify-between gap-2 rounded-lg bg-[#F8F6F2] p-2">
       <span className="min-w-0 break-all">
        <strong className="block text-[#51483F]">{receipt.tool}</strong>
        <span className="text-[10px] text-[#82796D]">{en?'Call ID':'호출 ID'}: {receipt.id} · {en?'Step':'회차'} {receipt.step}</span>
       </span>
       <span className={receipt.success?'font-semibold text-emerald-700':'font-semibold text-rose-700'}>
        {receipt.success?(en?'Tool succeeded':'도구 성공'):(en?'Tool failed':'도구 실패')}
       </span>
      </li>)}
     </ol>
    </details>}
    {Boolean(run.checkpoint?.artifactEvidence?.entries?.length)&&<details
     className="mt-3 min-w-0 rounded-lg border border-[#E7E0D5] bg-white p-3 text-[11px]">
     <summary className="cursor-pointer font-semibold">
      {en?'Captured file evidence':'파일 검증 스냅샷'} · {run.checkpoint?.artifactEvidence?.entries.length}
     </summary>
     <p className="mt-2 text-[10px] text-[#82796D]">{en?
      'Workspace file states captured when this run ended. Not a live scan or a claim that all checks passed.':
      '실행 종료 시 관찰한 파일 상태입니다. 현재 파일과의 일치 또는 전체 검증 통과를 뜻하지 않습니다.'}</p>
     {Boolean(run.checkpoint?.artifactEvidence?.omitted)&&<p className="mt-1 text-[10px] text-[#82796D]">
      {en?`${run.checkpoint?.artifactEvidence?.omitted} earlier files omitted`:
       `파일 ${run.checkpoint?.artifactEvidence?.omitted}건 표시 생략`}
     </p>}
     <ul className="mt-2 max-h-64 space-y-1 overflow-y-auto" aria-label={en?'Host-captured file evidence':'호스트 관찰 파일 증거'}>
      {run.checkpoint?.artifactEvidence?.entries.map((artifact,i)=><li key={artifact.path||i}
       className="min-w-0 rounded-lg bg-[#F8F6F2] p-2">
       <strong className="block break-all text-[#51483F]">{artifact.path}</strong>
       <span className="mt-1 block text-[10px] text-[#766F67]">
        {en?`Observed state: ${artifact.state}`:`관찰 상태: ${artifact.state}`}
        {artifact.bytes!=null?` · ${artifact.bytes.toLocaleString()} bytes`:''}
       </span>
       {artifact.sha256&&<code className="mt-1 block break-all text-[10px] text-[#766F67]">
        SHA-256: {artifact.sha256}
       </code>}
      </li>)}
     </ul>
    </details>}
    {run.checkpoint?.verificationChecks?.length?
     <details className="mt-3 rounded-lg border border-[#E7E0D5] bg-white p-3 text-[11px]">
      <summary className="cursor-pointer font-semibold">{en?'Actual acceptance checks':'실제 결과 검증'} · {run.checkpoint.verificationChecks.filter(c=>c.satisfied).length}/{run.checkpoint.verificationChecks.length}</summary>
      <ul className="mt-2 grid gap-1">
       {run.checkpoint.verificationChecks.map((check,i)=><li key={i}
        className={check.satisfied?'text-emerald-700':'text-amber-800'}>
        {check.satisfied?'✓':'!'} {check.explanation||'—'}</li>)}
      </ul>
     </details>:null}
    {status==='needs_review'&&<p role="status" className="mt-2 text-[11px] text-[#8A4C16]">{en?'The server requires review. No paid call or uncertain effect will be replayed automatically.':'서버가 검토를 요구하고 있습니다. 결과가 불분명한 유료 호출이나 변경 작업은 자동으로 재실행하지 않습니다.'}</p>}
    {run.error&&<p className="mt-2 text-[11px] text-[#A54027]">{run.error}</p>}
    <div className="mt-3 flex gap-2">
     {running.has(status)&&<button type="button" disabled={busy} onClick={()=>void action('cancel')} className="flex min-h-11 items-center gap-1 rounded-lg border border-[#E7E0D5] px-4 py-2 text-[12px] focus-visible:outline-2 focus-visible:outline-offset-2">
      <Square size={13}/>{en?'Stop':'중단'}
     </button>}
     {canResume&&<button type="button" disabled={busy} onClick={()=>void action('resume')} className="flex min-h-11 items-center gap-1 rounded-lg border border-[#E7E0D5] px-4 py-2 text-[12px] focus-visible:outline-2 focus-visible:outline-offset-2">
      <RefreshCw size={13}/>{en?'Resume':'재개'}
     </button>}
    </div>
    {running.has(status)&&<div className="mt-3 rounded-lg border border-[#E7E0D5] bg-white p-3">
     <div className="flex items-center gap-2 text-[11px] font-semibold"><MessageSquareText size={14}/>
      {en?'Correct the task while it is running':'진행 중 작업 지시 추가'}</div>
     <textarea rows={2} maxLength={16000} value={steering}
      onChange={e=>setSteering(e.target.value)}
      placeholder={en?'Add a correction or new constraint for the next model turn.':'다음 모델 판단에 반영할 정정이나 추가 조건을 입력하세요.'}
      className="mt-2 w-full rounded-lg border border-[#E7E0D5] bg-[#FCFBF9] p-2 text-[11px]"/>
     <button type="button" disabled={busy||!steering.trim()} onClick={()=>void steer()}
      className="mt-2 rounded-lg bg-[#A4583E] px-3 py-2 text-[11px] font-semibold text-white disabled:opacity-40">
      {en?'Send correction':'수정 지시 전달'}</button>
    </div>}
    {status==='in_doubt'&&<div className="mt-3 rounded-lg border border-[#E5C4AB] bg-white p-3">
     <p className="text-[11px] text-[#A54027]">
      {en?'The interrupted tool may have changed files. Inspect the workspace before continuing.':'중단 직전 도구가 파일을 변경했을 수 있습니다. 실제 상태를 확인한 뒤 기록하세요.'}
     </p>
     <label htmlFor="sdk-observation" className="mt-3 block text-[11px] font-semibold">
      {en?'What did you observe?':'확인한 작업 결과'}
     </label>
     <textarea id="sdk-observation" rows={2} maxLength={8192}
      value={observation} onChange={e=>setObservation(e.target.value)}
      placeholder={en?'Describe the files or outcomes you inspected.':'실제로 확인한 파일 상태나 실행 결과를 입력하세요.'}
      className="mt-1 w-full rounded-lg border border-[#E7E0D5] p-2 text-[11px]" />
     <div className="mt-2 flex flex-wrap gap-2">
      <button type="button" disabled={busy||!observation.trim()} onClick={()=>void reconcile(true)}
       className="min-h-11 rounded-lg border border-[#E7E0D5] px-4 py-2 text-[12px] focus-visible:outline-2 focus-visible:outline-offset-2 disabled:opacity-40">
       {en?'Effect already applied':'변경이 적용됐음'}
      </button>
      <button type="button" disabled={busy||!observation.trim()} onClick={()=>void reconcile(false)}
       className="min-h-11 rounded-lg border border-[#E7E0D5] px-4 py-2 text-[12px] focus-visible:outline-2 focus-visible:outline-offset-2 disabled:opacity-40">
       {en?'Effect not applied':'변경이 적용되지 않았음'}
      </button>
     </div>
    </div>}
   </div>}
  </div>
 </section>
}
