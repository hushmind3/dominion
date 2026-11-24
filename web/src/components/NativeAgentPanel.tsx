import {useEffect, useRef, useState} from 'react'
import {Check, Play, RefreshCw, ShieldAlert, Square, Sparkles} from 'lucide-react'
import {get, post, resolveApproval, type Provider} from '../api'
import {useLocale} from '../i18n'
import {ExecutionInsights} from './ExecutionInsights'
import {RunEventTimeline} from './RunEventTimeline'

type SdkState = {state: string; detail?: {call_id?: string; tool?: string}}
type SdkRun = {
 id: string; status: string; instruction: string; answer?: string; error?: string; createdAt?:string; model?:string; sessionId?:string; runRef?:{executionId:string}; readOnly?:boolean; failureClass?:string|null;
 checkpoint?: {
  state?: SdkState; rounds?: number; toolCalls?: number; answer?: string; executionId?:string;
  usage?: {prompt_tokens?:number|null; completion_tokens?:number|null; cached_prompt_tokens?:number|null; estimated?:boolean};
  pendingCall?: {id:string; name:string; arguments:unknown} | null;
  events?: {kind:string; detail:unknown}[];
 } | null
}
const fmtTokens=(value?:number|null)=>typeof value==='number'&&Number.isFinite(value)?value.toLocaleString():'—'
const running = new Set(['queued', 'running', 'awaiting_approval'])
const stopPolling = new Set(['completed','failed','cancelled','exhausted','denied','awaiting_approval','interrupted','in_doubt','needs_review'])
const stateNames:Record<string,[string,string]>={
 queued:['대기 중','Queued'], running:['작업 진행 중','Working'],
 completed:['완료','Completed'], failed:['실패','Failed'],
 cancelled:['중단됨','Stopped'], exhausted:['실행 한도 도달','Limit reached'],
 awaiting_approval:['실행 승인 필요','Needs approval'],
 in_doubt:['수동 확인 필요','Needs manual review'],
 needs_review:['검증 결과 재확인 필요','Needs evidence review'],
 interrupted:['재개 필요','Resume required'], denied:['실행 거부됨','Denied']
}
export function NativeAgentPanel({providers}:{providers:Provider[]}){
 const locale=useLocale()
 const en=locale==='en'
 const [instruction,setInstruction]=useState('')
 const [providerId,setProviderId]=useState('')
 const [verification,setVerification]=useState('')
 const [observation,setObservation]=useState('')
 const [run,setRun]=useState<SdkRun|null>(null)
 const [history,setHistory]=useState<SdkRun[]>([])
 const [historyOpen,setHistoryOpen]=useState(false)
 const [showComposer,setShowComposer]=useState(true)
 const goalRef=useRef<HTMLTextAreaElement>(null)
 const [steering,setSteering]=useState('')
 const [notice,setNotice]=useState('')
 const selectionGeneration=useRef(0)
 const selectHistory=async(id:string)=>{
  const selected=++selectionGeneration.current
  setError('');setNotice('');setSteering('')
  try{
   const current=await get<SdkRun>(`/api/sdk/runs/${encodeURIComponent(id)}`)
   if(selected===selectionGeneration.current){setRun(current);setShowComposer(false)}
  }catch(e){
   if(selected===selectionGeneration.current)setError((e as Error).message)
  }
 }
 const [busy,setBusy]=useState(false)
 const [error,setError]=useState('')
 const refreshHistory=async()=>{const list=await get<SdkRun[]>('/api/sdk/runs')
  setHistory([...list].sort((a,b)=>(b.createdAt||'').localeCompare(a.createdAt||'')).slice(0,30))
  return list
 }
 useEffect(()=>{
  let alive=true
  get<SdkRun[]>('/api/sdk/runs').then(list=>{
   if(!alive)return
   const sorted=[...list].sort((a,b)=>(b.createdAt||'').localeCompare(a.createdAt||''))
   setHistory(sorted.slice(0,30))
   if(sorted.length)setRun(sorted[0])
  }).catch(e=>{if(alive)setError((e as Error).message)})
  return()=>{alive=false}
 },[])
 useEffect(()=>{
  if(!run?.id || stopPolling.has(run.status)) return
  let alive=true
  const timer=setInterval(()=>{
   get<SdkRun>(`/api/sdk/runs/${encodeURIComponent(run.id)}`)
    .then(next=>{if(alive)setRun(next)})
    .catch(e=>{if(alive)setError((e as Error).message)})
  },1500)
  return()=>{alive=false;clearInterval(timer)}
 },[run?.id,run?.status])
 const start=async()=>{
  if(!instruction.trim()||busy)return
  setBusy(true);setError('')
  try{
   const created=await post<SdkRun>('/api/sdk/runs',{
    instruction:instruction.trim(),providerId:providerId||undefined,
    verifyCommand:verification.trim()||undefined
   })
   ++selectionGeneration.current
   setRun(created);setHistoryOpen(false);setNotice('');setShowComposer(false);setInstruction('')
   void refreshHistory().catch(()=>{})
  }catch(e){setError((e as Error).message)}
  finally{setBusy(false)}
 }
 const action=async(kind:'approve'|'deny'|'cancel'|'resume')=>{
  if(!run||busy)return
  setBusy(true);setError('')
  try{
   const call=run.checkpoint?.pendingCall
   if((kind==='approve'||kind==='deny')&&!call?.id)throw Error(en?'Missing approval details':'승인할 도구 정보를 찾지 못했습니다.')
   if(kind==='deny'){
    await resolveApproval(call!.id,false)
   }else{
    await post(`/api/sdk/runs/${encodeURIComponent(run.id)}/${kind}`,
     kind==='approve'?{callId:call!.id}:{})
   }
   const refreshed=await get<SdkRun>(`/api/sdk/runs/${encodeURIComponent(run.id)}`)
   setRun(refreshed);void refreshHistory().catch(()=>{})
  }catch(e){setError((e as Error).message)}
  finally{setBusy(false)}
 }
 const reconcile=async(applied:boolean)=>{
  if(!run||busy||!observation.trim())return
  setBusy(true);setError('')
  try{
   const updated=await post<SdkRun>(`/api/sdk/runs/${encodeURIComponent(run.id)}/reconcile`,
    {applied,details:observation.trim()})
   setRun(updated);setObservation('');void refreshHistory().catch(()=>{})
  }catch(e){setError((e as Error).message)}
  finally{setBusy(false)}
 }
 const followUp=async()=>{
  if(!run||busy||!steering.trim())return
  setBusy(true);setError('');setNotice('')
  try{
   await post(`/api/sdk/runs/${encodeURIComponent(run.id)}/steer`,{instruction:steering.trim(),mode:'append'})
   setSteering('');setNotice(en?'Instruction queued for the current run.':'현재 작업에 추가 지시를 전달했습니다.')
   setRun(await get<SdkRun>(`/api/sdk/runs/${encodeURIComponent(run.id)}`))
  }catch(e){setError((e as Error).message)}
  finally{setBusy(false)}
 }
 const status=run?.status||''
 const call=run?.checkpoint?.pendingCall
 const needsApproval=status==='awaiting_approval' && call
 const canRecheck=status==='needs_review'&&run?.failureClass==='completion_evidence_unavailable'
 const canResume=['interrupted','failed'].includes(status)||canRecheck
 const events=(run?.checkpoint?.events||[]).slice(-4)
 return <section className="mt-4 rounded-xl border border-divider bg-surface">
  <div className="flex items-center gap-2 border-b border-canvas px-4 py-3">
   <Sparkles size={17} className="text-brand-hover"/>
   <div><h2 className="text-[13px] font-bold">{en?'Autonomous work':'자동 작업'}</h2>
    <p className="mt-0.5 text-xs text-muted">{en?'Describe what you need. Dominion’s own Rust engine handles the steps.':'할 일을 말씀하시면 도미니언 Rust 엔진이 작업을 실행합니다.'}</p>
   </div>
   {run&&!showComposer&&<button type="button" onClick={()=>{setShowComposer(true);requestAnimationFrame(()=>goalRef.current?.focus())}} className="ml-auto shrink-0 rounded-lg bg-brand px-3 py-2 text-xs font-semibold text-white hover:bg-brand-hover">{en?'New task':'새 작업'}</button>}
  </div>
  <div className="border-b border-canvas px-4 py-2">
   <button type="button" onClick={()=>setHistoryOpen(v=>!v)} aria-expanded={historyOpen}
    className="flex w-full items-center justify-between text-xs text-muted">
    <span>{en?'Previous SDK runs':'기존 SDK 실행 기록'} ({history.length})</span>
    {historyOpen?'▴':'▾'}
   </button>
   {historyOpen&&<div className="mt-2 max-h-44 space-y-1 overflow-y-auto">
    <button type="button" className="mb-1 text-xs underline" onClick={()=>void refreshHistory().catch(e=>setError((e as Error).message))}>{en?'Refresh history':'목록 새로고침'}</button>
    {history.map(item=><button type="button" key={item.id}
      aria-pressed={run?.id===item.id} onClick={()=>void selectHistory(item.id)}
      className={'flex w-full items-center justify-between gap-2 rounded-lg px-3 py-2 text-left text-xs '+(run?.id===item.id?'bg-brand-soft':'bg-canvas hover:bg-canvas')}>
      <span className="min-w-0 flex-1 truncate">{item.instruction||item.id}</span>
      <span className="shrink-0 text-muted">{en?stateNames[item.status]?.[1]||item.status:stateNames[item.status]?.[0]||item.status}</span>
     </button>)}
    {!history.length&&<p className="text-xs text-muted">{en?'No previous runs':'이전 실행이 없습니다.'}</p>}
   </div>}
  </div>
  <div className="grid gap-3 p-4">
   {showComposer&&<>
   <label className="text-sm font-semibold" htmlFor="sdk-goal">{en?'What would you like done?':'무엇을 해드릴까요?'}</label>
   <textarea ref={goalRef} id="sdk-goal" value={instruction} onChange={e=>setInstruction(e.target.value)}
    rows={3} maxLength={64000}
    placeholder={en?'Describe a task, such as inspecting a project or fixing an issue.':'프로젝트를 분석하거나 문제를 해결하는 등 필요한 작업을 적어주세요.'}
    className="w-full resize-y rounded-xl border border-divider bg-surface p-3 text-sm outline-none focus:border-brand"/>
   <div className="flex flex-wrap items-end gap-3">
    <label className="flex min-w-44 flex-1 flex-col gap-1 text-xs text-muted">
     {en?'AI model':'사용할 AI 모델'}
     <select value={providerId} onChange={e=>setProviderId(e.target.value)}
      className="h-9 rounded-lg border border-divider bg-surface px-3 text-sm">
      <option value="">{en?'Automatic selection':'자동 선택'}</option>
      {providers.filter(p=>p.enabled).map(p=><option key={p.id} value={p.id}>{p.name}{p.model?` · ${p.model}`:''}</option>)}
     </select>
    </label>
    <button type="button" onClick={()=>void start()} disabled={busy||!instruction.trim()}
     className="flex h-9 items-center gap-2 rounded-lg bg-brand-hover px-4 text-sm font-semibold text-white disabled:opacity-40">
     <Play size={14}/>{busy?(en?'Please wait…':'처리 중…'):(en?'Start task':'작업 시작')}
    </button>
   </div>
   <details className="text-xs text-muted">
    <summary className="cursor-pointer select-none">{en?'Optional: verify the result with a command':'선택 사항: 명령어로 결과 검증'}</summary>
    <label className="mt-2 block" htmlFor="sdk-verify">
     {en?'Verification command (runs after the agent finishes)':'검증 명령어 (작업 결과를 확인할 때 실행)'}
    </label>
    <input id="sdk-verify" value={verification} onChange={e=>setVerification(e.target.value)}
     maxLength={2048} placeholder="cargo test"
     className="mt-1 h-9 w-full rounded-lg border border-divider px-3 text-sm"/>
   </details>
   </>}
   {error&&<p role="alert" className="rounded-lg bg-danger-soft p-3 text-xs text-brand-hover">{error}</p>}
   {run&&<div className="rounded-xl border border-divider bg-surface p-3">
    <div className="flex flex-wrap items-center justify-between gap-2">
     <strong className="text-sm">{en?stateNames[status]?.[1]||status:stateNames[status]?.[0]||status}</strong>
     <span className="text-xs text-muted">{run.checkpoint?.rounds??0} {en?'rounds':'회 판단'} · {run.checkpoint?.toolCalls??0} {en?'actions':'회 도구 실행'}</span>
    {run.checkpoint?.usage&&<p className="mt-2 text-xs text-muted">
     {run.checkpoint.usage.estimated?(en?'Estimated tokens':'추정 토큰'):(en?'Token usage':'토큰 사용량')}: {fmtTokens(run.checkpoint.usage.prompt_tokens)} {en?'input':'입력'} · {fmtTokens(run.checkpoint.usage.completion_tokens)} {en?'output':'출력'} · {fmtTokens(run.checkpoint.usage.cached_prompt_tokens)} {en?'cached input':'캐시 입력'}
    </p>}
    </div>
    <p className="mt-2 line-clamp-2 text-xs text-muted">{run.instruction}</p>
    {events.length>0&&<div className="mt-3 space-y-1 border-t border-divider pt-2">
     {events.map((event,i)=><p key={i} className="text-xs text-muted">
      {event.kind==='tool_started'?(en?'Running tool':'도구 실행'):
       event.kind==='model_started'?(en?'Model is working':'모델 판단 중'):
       event.kind==='verified'?(en?'Result verification':'결과 검증'):event.kind}
     </p>)}
    </div>}
    {needsApproval&&<div className="mt-3 rounded-lg border border-divider bg-surface p-3">
     <div className="flex items-center gap-2 text-sm font-semibold"><ShieldAlert size={16}/>
      {en?'Review before allowing this action':'아래 작업을 확인하고 승인하세요'}
     </div>
     <div className="mt-2 text-xs font-semibold">{call.name}</div>
     <pre className="mt-2 max-h-56 overflow-auto whitespace-pre-wrap break-all rounded-lg bg-canvas p-3 text-xs">{JSON.stringify(call.arguments,null,2)}</pre>
     <button type="button" disabled={busy} onClick={()=>void action('approve')}
      className="mt-3 flex items-center gap-2 rounded-lg bg-brand px-3 py-2 text-xs font-semibold text-white">
      <Check size={14}/>{en?'Approve this action only':'이 작업 1회 승인'}
     </button>
     <button type="button" disabled={busy} onClick={()=>void action('deny')}
      className="ml-2 mt-3 rounded-lg border border-danger/20 px-3 py-2 text-xs font-semibold text-danger disabled:opacity-50">
      {en?'Reject this action':'이 작업 거절'}
     </button>
    </div>}
    {run.answer||run.checkpoint?.answer?<div className="mt-3 whitespace-pre-wrap break-words border-t border-divider pt-3 text-sm">{run.answer||run.checkpoint?.answer}</div>:null}
    {run.error&&<p className="mt-2 text-xs text-brand-hover">{run.error}</p>}
    {notice&&<p role="status" className="mt-2 text-xs text-success">{notice}</p>}
    {(['queued','running'].includes(status))&&!run.readOnly&&<form onSubmit={event=>{event.preventDefault();void followUp()}} className="mt-3 flex flex-wrap gap-2">
     <input type="text" maxLength={4000} value={steering} disabled={busy} onChange={e=>setSteering(e.target.value)}
      aria-label={en?'Steer current task':'실행 중 추가 지시'} placeholder={en?'Add a follow-up instruction…':'진행 중인 작업에 추가 지시…'}
      className="min-w-40 flex-1 rounded-lg border border-divider bg-surface px-3 py-2 text-xs"/>
     <button type="submit" disabled={busy||!steering.trim()} className="rounded-lg border border-brand px-3 py-2 text-xs text-brand disabled:opacity-40">{en?'Submit':'전달'}</button>
    </form>}
    {(run.runRef?.executionId||run.checkpoint?.executionId)&&<details className="mt-3 rounded-lg border border-divider p-3">
      <summary className="cursor-pointer select-none text-xs font-semibold">{en?'Execution details & evidence':'실행 기록 · 검증 자료'}</summary>
      <ExecutionInsights executionId={(run.runRef?.executionId||run.checkpoint?.executionId)!} compact/>
      <RunEventTimeline executionId={(run.runRef?.executionId||run.checkpoint?.executionId)!}/>
    </details>}
    <div className="mt-3 flex gap-2">
     {running.has(status)&&<button type="button" disabled={busy} onClick={()=>void action('cancel')} className="flex items-center gap-1 rounded-lg border border-divider px-3 py-2 text-xs">
      <Square size={13}/>{en?'Stop':'중단'}
     </button>}
     {canResume&&<button type="button" disabled={busy} onClick={()=>void action('resume')} className="flex items-center gap-1 rounded-lg border border-divider px-3 py-2 text-xs">
      <RefreshCw size={13}/>{canRecheck?(en?'Recheck completion evidence':'완료 증거 재확인'):(en?'Resume':'재개')}
     </button>}
    </div>
    {status==='in_doubt'&&<div className="mt-3 rounded-lg border border-divider bg-surface p-3">
     <p className="text-xs text-brand-hover">
      {en?'The interrupted tool may have changed files. Inspect the workspace before continuing.':'중단 직전 도구가 파일을 변경했을 수 있습니다. 실제 상태를 확인한 뒤 기록하세요.'}
     </p>
     <label htmlFor="sdk-observation" className="mt-3 block text-xs font-semibold">
      {en?'What did you observe?':'확인한 작업 결과'}
     </label>
     <textarea id="sdk-observation" rows={2} maxLength={8192}
      value={observation} onChange={e=>setObservation(e.target.value)}
      placeholder={en?'Describe the files or outcomes you inspected.':'실제로 확인한 파일 상태나 실행 결과를 입력하세요.'}
      className="mt-1 w-full rounded-lg border border-divider p-2 text-xs" />
     <div className="mt-2 flex flex-wrap gap-2">
      <button type="button" disabled={busy||!observation.trim()} onClick={()=>void reconcile(true)}
       className="rounded-lg border border-divider px-3 py-2 text-xs disabled:opacity-40">
       {en?'Effect already applied':'변경이 적용됐음'}
      </button>
      <button type="button" disabled={busy||!observation.trim()} onClick={()=>void reconcile(false)}
       className="rounded-lg border border-divider px-3 py-2 text-xs disabled:opacity-40">
       {en?'Effect not applied':'변경이 적용되지 않았음'}
      </button>
     </div>
    </div>}
   </div>}
  </div>
 </section>
}
