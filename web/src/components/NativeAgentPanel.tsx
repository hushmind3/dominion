import {useEffect, useRef, useState} from 'react'
import {Check, Play, RefreshCw, ShieldAlert, Square, Sparkles, History, Plus} from 'lucide-react'
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
 const [historyOpen,setHistoryOpen]=useState(true)
 const [historyQuery,setHistoryQuery]=useState('')
 const [loadingRun,setLoadingRun]=useState(false)
 const [steering,setSteering]=useState('')
 const [notice,setNotice]=useState('')
 // Guard every async response against switching to a different run.
 const selectionGeneration=useRef(0)
 const [selectionRevision,setSelectionRevision]=useState(0)
 const selectedRunId=useRef<string|null>(null)
 const mutationRevision=useRef(0)
 const showRun=(next:SdkRun)=>{
  selectedRunId.current=next.id
  setRun(next)
  // Selecting the same ID/status still needs to restart its guarded poll.
  setSelectionRevision(value=>value+1)
 }
 const selectHistory=async(id:string)=>{
  if(busy||loadingRun)return
  setLoadingRun(true)
  const previousSelection=selectionGeneration.current
  const selected=++selectionGeneration.current
  setError('');setNotice('');setSteering('');setObservation('')
  try{
   const current=await get<SdkRun>(`/api/sdk/runs/${encodeURIComponent(id)}`)
   if(selected===selectionGeneration.current)showRun(current)
  }catch(e){
   if(selected===selectionGeneration.current){
    selectionGeneration.current=previousSelection
    setError((e as Error).message)
   }
  }finally{setLoadingRun(false);setSelectionRevision(value=>value+1)}
 }
 const [busy,setBusy]=useState(false)
 const [error,setError]=useState('')
 const refreshHistory=async()=>{const list=await get<SdkRun[]>('/api/sdk/runs')
  setHistory([...list].sort((a,b)=>(b.createdAt||'').localeCompare(a.createdAt||'')).slice(0,30))
  return list
 }
 useEffect(()=>{
  let alive=true
  const selection=selectionGeneration.current
  get<SdkRun[]>('/api/sdk/runs').then(list=>{
   if(!alive)return
   const sorted=[...list].sort((a,b)=>(b.createdAt||'').localeCompare(a.createdAt||''))
   setHistory(sorted.slice(0,30))
   // An initial history response must never replace a newer manual selection.
   if(sorted.length&&selection===selectionGeneration.current&&!selectedRunId.current)showRun(sorted[0])
  }).catch(e=>{if(alive&&selection===selectionGeneration.current)setError((e as Error).message)})
  return()=>{alive=false}
 },[])
 useEffect(()=>{
  if(!run?.id || stopPolling.has(run.status)) return
  let alive=true, reading=false
  const id=run.id, selection=selectionGeneration.current
  const mutation=mutationRevision.current
  const controller=new AbortController()
  const poll=async()=>{
   if(!alive||reading)return
   reading=true
   // Recover from a hanging poll without abandoning the active task.
   const signal=AbortSignal.any([controller.signal,AbortSignal.timeout(10_000)])
   try{
    const next=await get<SdkRun>(`/api/sdk/runs/${encodeURIComponent(id)}`,signal)
    if(alive&&selection===selectionGeneration.current&&mutation===mutationRevision.current&&selectedRunId.current===id)setRun(next)
   }catch(e){
    if(alive&&!signal.aborted&&selection===selectionGeneration.current&&mutation===mutationRevision.current)setError((e as Error).message)
   }finally{reading=false}
  }
  const timer=setInterval(()=>{void poll()},1500)
  return()=>{alive=false;clearInterval(timer);controller.abort()}
 },[run?.id,run?.status,selectionRevision])
 const start=async()=>{
  if(!instruction.trim()||busy||loadingRun)return
  const previousSelection=selectionGeneration.current
  const selection=++selectionGeneration.current
  setBusy(true);setError('')
  try{
   const created=await post<SdkRun>('/api/sdk/runs',{
    instruction:instruction.trim(),providerId:providerId||undefined,
    verifyCommand:verification.trim()||undefined
   })
   if(selection===selectionGeneration.current){showRun(created);setNotice('');setObservation('');setSteering('');setInstruction('')}
   void refreshHistory().catch(()=>{})
  }catch(e){
   if(selection===selectionGeneration.current){
    selectionGeneration.current=previousSelection
    setError((e as Error).message)
   }
  }
  finally{setBusy(false)}
 }
 const action=async(kind:'approve'|'deny'|'cancel'|'resume')=>{
  if(!run||busy||loadingRun)return
  const current=run, selection=selectionGeneration.current
  ++mutationRevision.current
  setBusy(true);setError('')
  try{
   const call=current.checkpoint?.pendingCall
   if((kind==='approve'||kind==='deny')&&!call?.id)throw Error(en?'Missing approval details':'승인할 도구 정보를 찾지 못했습니다.')
   if(kind==='deny'){
    await resolveApproval(call!.id,false)
   }else{
    await post(`/api/sdk/runs/${encodeURIComponent(current.id)}/${kind}`,
     kind==='approve'?{callId:call!.id}:{})
   }
   const refreshed=await get<SdkRun>(`/api/sdk/runs/${encodeURIComponent(current.id)}`)
   if(selection===selectionGeneration.current&&selectedRunId.current===current.id)showRun(refreshed)
   void refreshHistory().catch(()=>{})
  }catch(e){if(selection===selectionGeneration.current)setError((e as Error).message)}
  finally{setBusy(false);setSelectionRevision(value=>value+1)}
 }
 const reconcile=async(applied:boolean)=>{
  if(!run||busy||loadingRun||!observation.trim())return
  const current=run, selection=selectionGeneration.current
  ++mutationRevision.current
  setBusy(true);setError('')
  try{
   const updated=await post<SdkRun>(`/api/sdk/runs/${encodeURIComponent(current.id)}/reconcile`,
    {applied,details:observation.trim()})
   if(selection===selectionGeneration.current&&selectedRunId.current===current.id){showRun(updated);setObservation('')}
   void refreshHistory().catch(()=>{})
  }catch(e){if(selection===selectionGeneration.current)setError((e as Error).message)}
  finally{setBusy(false);setSelectionRevision(value=>value+1)}
 }
 const followUp=async()=>{
  if(!run||busy||!steering.trim())return
  const current=run, selection=selectionGeneration.current
  ++mutationRevision.current
  setBusy(true);setError('');setNotice('')
  try{
   await post(`/api/sdk/runs/${encodeURIComponent(current.id)}/steer`,{instruction:steering.trim(),mode:'append'})
   const refreshed=await get<SdkRun>(`/api/sdk/runs/${encodeURIComponent(current.id)}`)
   if(selection===selectionGeneration.current&&selectedRunId.current===current.id){
    setSteering('');setNotice(en?'Instruction queued for the current run.':'현재 작업에 추가 지시를 전달했습니다.')
    showRun(refreshed)
   }
  }catch(e){if(selection===selectionGeneration.current)setError((e as Error).message)}
  finally{setBusy(false);setSelectionRevision(value=>value+1)}
 }
 const status=run?.status||''
 const call=run?.checkpoint?.pendingCall
 const needsApproval=status==='awaiting_approval' && call
 const canRecheck=status==='needs_review'&&run?.failureClass==='completion_evidence_unavailable'
 const canResume=['interrupted','failed'].includes(status)||canRecheck
 const events=(run?.checkpoint?.events||[]).slice(-12)
 const visibleHistory=history.filter(item=>[item.instruction,item.id,item.status].some(value=>(value||'').toLocaleLowerCase().includes(historyQuery.trim().toLocaleLowerCase())))
 return <section className="native-workspace grid min-w-0 gap-4 xl:grid-cols-[minmax(230px,280px)_minmax(0,1fr)]">
  <aside className="min-w-0 rounded-xl border border-divider bg-surface xl:self-start">
   <header className="flex items-center gap-2 border-b border-divider px-4 py-3">
    <History size={17} className="text-muted"/><h2 className="min-w-0 flex-1 text-sm font-semibold">{en?'Run history':'작업 내역'} <span className="text-xs text-muted">({history.length})</span></h2>
    <button type="button" className="rounded-lg p-2 hover:bg-canvas" title={en?'Refresh history':'목록 새로고침'} aria-label={en?'Refresh history':'목록 새로고침'} onClick={()=>void refreshHistory().catch(e=>setError((e as Error).message))}><RefreshCw size={16}/></button>
    <button type="button" className="rounded-lg px-2 py-1 text-xs text-muted xl:hidden" onClick={()=>setHistoryOpen(v=>!v)} aria-expanded={historyOpen}>{historyOpen?(en?'Hide':'접기'):(en?'Show':'펼치기')}</button>
   </header>
   <div className={(historyOpen?'block':'hidden')+' p-3 xl:block'}>
    <input value={historyQuery} onChange={event=>setHistoryQuery(event.target.value)} aria-label={en?'Search runs':'작업 내역 검색'} placeholder={en?'Find a task':'작업 검색'} className="mb-3 min-h-10 w-full rounded-lg border border-divider bg-surface px-3 text-sm"/>
    <div className="max-h-[min(48dvh,560px)] space-y-1 overflow-y-auto">
     {visibleHistory.map(item=><button type="button" key={item.id} disabled={busy||loadingRun}
      aria-pressed={run?.id===item.id} onClick={()=>void selectHistory(item.id)}
      className={'flex min-h-16 w-full flex-col items-start justify-center gap-1 rounded-lg border border-transparent px-3 py-2 text-left hover:bg-canvas disabled:opacity-60 '+(run?.id===item.id?'bg-selected !border-divider':'')}>
      <span className="line-clamp-2 w-full break-words text-sm font-medium">{item.instruction||item.id}</span><span className="text-xs text-muted">{en?stateNames[item.status]?.[1]||item.status:stateNames[item.status]?.[0]||item.status}</span>
     </button>)}
     {!visibleHistory.length&&<p className="px-2 py-6 text-center text-sm text-muted">{historyQuery?(en?'No matching tasks':'검색 결과 없음'):(en?'No previous runs':'이전 실행이 없습니다.')}</p>}
    </div>
   </div>
  </aside>
  <div className="min-w-0 space-y-4">
   <section className="rounded-xl border border-divider bg-surface">
    <header className="flex items-center gap-2 border-b border-divider px-4 py-3"><Plus size={18}/><h2 className="text-base font-semibold">{en?'New task':'새 작업'}</h2></header>
  <div className="grid gap-3 p-4">
   <label className="text-[12px] font-semibold" htmlFor="sdk-goal">{en?'What would you like done?':'무엇을 해드릴까요?'}</label>
   <textarea id="sdk-goal" value={instruction} onChange={e=>setInstruction(e.target.value)}
    rows={5} maxLength={64000}
    placeholder={en?'Describe a task, such as inspecting a project or fixing an issue.':'프로젝트를 분석하거나 문제를 해결하는 등 필요한 작업을 적어주세요.'}
    className="w-full min-h-32 resize-y rounded-xl border border-divider bg-surface p-4 text-sm leading-7 outline-none focus:border-brand"/>
   <div className="flex flex-wrap items-end gap-3">
    <label className="flex min-w-44 flex-1 flex-col gap-1 text-[11px] text-muted">
     {en?'AI model':'사용할 AI 모델'}
     <select value={providerId} onChange={e=>setProviderId(e.target.value)}
      className="h-9 rounded-lg border border-divider bg-surface px-3 text-[12px]">
      <option value="">{en?'Automatic selection':'자동 선택'}</option>
      {providers.filter(p=>p.enabled).map(p=><option key={p.id} value={p.id}>{p.name}{p.model?` · ${p.model}`:''}</option>)}
     </select>
    </label>
    <button type="button" onClick={()=>void start()} disabled={busy||loadingRun||!instruction.trim()}
     className="flex h-9 items-center gap-2 rounded-lg bg-brand-hover px-4 text-[12px] font-semibold text-white disabled:opacity-40">
     <Play size={14}/>{busy?(en?'Please wait…':'처리 중…'):(en?'Start task':'작업 시작')}
    </button>
   </div>
   <details className="text-[11px] text-muted">
    <summary className="cursor-pointer select-none">{en?'Optional: verify the result with a command':'선택 사항: 명령어로 결과 검증'}</summary>
    <label className="mt-2 block" htmlFor="sdk-verify">
     {en?'Verification command (runs after the agent finishes)':'검증 명령어 (작업 결과를 확인할 때 실행)'}
    </label>
    <input id="sdk-verify" value={verification} onChange={e=>setVerification(e.target.value)}
     maxLength={2048} placeholder="cargo test"
     className="mt-1 h-9 w-full rounded-lg border border-divider px-3 text-[12px]"/>
   </details>
    </div>
   </section>
   {error&&<p role="alert" className="rounded-lg bg-danger-soft p-3 text-sm text-danger">{error}</p>}
   {loadingRun&&<p role="status" className="p-3 text-sm text-muted">{en?'Loading task…':'작업 불러오는 중…'}</p>}
   {!run&&!loadingRun&&<section className="rounded-xl border border-dashed border-divider px-6 py-10 text-center text-sm text-muted">{en?'Start a task or choose a previous run to see its progress and results here.':'작업을 시작하거나 내역을 선택하면 이곳에 진행 상황과 결과가 표시됩니다.'}</section>}
   {run&&!loadingRun&&<section className="rounded-xl border border-divider bg-surface p-4 sm:p-6" aria-label={en?'Task detail':'작업 상세'}>
    <h2 className="mb-4 flex items-center gap-2 text-base font-semibold"><Sparkles size={18}/>{en?'Task detail':'작업 상세'}</h2>
    <div className="flex flex-wrap items-center justify-between gap-2">
     <strong className="text-[12px]">{en?stateNames[status]?.[1]||status:stateNames[status]?.[0]||status}</strong>
     <span className="text-[11px] text-muted">{run.checkpoint?.rounds??0} {en?'rounds':'회 판단'} · {run.checkpoint?.toolCalls??0} {en?'actions':'회 도구 실행'}</span>
    {run.checkpoint?.usage&&<p className="mt-2 text-[11px] text-muted">
     {run.checkpoint.usage.estimated?(en?'Estimated tokens':'추정 토큰'):(en?'Token usage':'토큰 사용량')}: {fmtTokens(run.checkpoint.usage.prompt_tokens)} {en?'input':'입력'} · {fmtTokens(run.checkpoint.usage.completion_tokens)} {en?'output':'출력'} · {fmtTokens(run.checkpoint.usage.cached_prompt_tokens)} {en?'cached input':'캐시 입력'}
    </p>}
    </div>
    <p className="mt-3 whitespace-pre-wrap break-words text-sm font-medium leading-6">{run.instruction}</p>
    {events.length>0&&<div className="mt-3 space-y-1 border-t border-divider pt-2">
     {events.map((event,i)=><p key={i} className="text-[11px] text-muted">
      {event.kind==='tool_started'?(en?'Running tool':'도구 실행'):
       event.kind==='model_started'?(en?'Model is working':'모델 판단 중'):
       event.kind==='verified'?(en?'Result verification':'결과 검증'):event.kind}
     </p>)}
    </div>}
    {needsApproval&&<div className="mt-3 rounded-lg border border-divider bg-surface p-3">
     <div className="flex items-center gap-2 text-[12px] font-semibold"><ShieldAlert size={16}/>
      {en?'Review before allowing this action':'아래 작업을 확인하고 승인하세요'}
     </div>
     <div className="mt-2 text-[11px] font-semibold">{call.name}</div>
     <pre className="mt-2 max-h-56 overflow-auto whitespace-pre-wrap break-all rounded-lg bg-canvas p-3 text-[11px]">{JSON.stringify(call.arguments,null,2)}</pre>
     <button type="button" disabled={busy} onClick={()=>void action('approve')}
      className="mt-3 flex items-center gap-2 rounded-lg bg-brand px-3 py-2 text-[11px] font-semibold text-white">
      <Check size={14}/>{en?'Approve this action only':'이 작업 1회 승인'}
     </button>
     <button type="button" disabled={busy} onClick={()=>void action('deny')}
      className="ml-2 mt-3 rounded-lg border border-danger/20 px-3 py-2 text-[11px] font-semibold text-danger disabled:opacity-50">
      {en?'Reject this action':'이 작업 거절'}
     </button>
    </div>}
    {run.answer||run.checkpoint?.answer?<div className="mt-4 border-t border-divider pt-4"><h3 className="mb-3 text-sm font-semibold">{en?'Result':'작업 결과'}</h3><div className="whitespace-pre-wrap break-words text-sm leading-7">{run.answer||run.checkpoint?.answer}</div></div>:null}
    {run.error&&<p className="mt-2 text-[11px] text-brand-hover">{run.error}</p>}
    {notice&&<p role="status" className="mt-2 text-[11px] text-success">{notice}</p>}
    {(['queued','running'].includes(status))&&!run.readOnly&&<form onSubmit={event=>{event.preventDefault();void followUp()}} className="mt-3 flex flex-wrap gap-2">
     <input type="text" maxLength={4000} value={steering} disabled={busy} onChange={e=>setSteering(e.target.value)}
      aria-label={en?'Steer current task':'실행 중 추가 지시'} placeholder={en?'Add a follow-up instruction…':'진행 중인 작업에 추가 지시…'}
      className="min-w-40 flex-1 rounded-lg border border-divider bg-surface px-3 py-2 text-[11px]"/>
     <button type="submit" disabled={busy||!steering.trim()} className="rounded-lg border border-brand px-3 py-2 text-[11px] text-brand disabled:opacity-40">{en?'Submit':'전달'}</button>
    </form>}
    {(run.runRef?.executionId||run.checkpoint?.executionId)&&<>
      <ExecutionInsights executionId={(run.runRef?.executionId||run.checkpoint?.executionId)!} compact/>
      <RunEventTimeline executionId={(run.runRef?.executionId||run.checkpoint?.executionId)!}/>
    </>}
    <div className="mt-3 flex gap-2">
     {running.has(status)&&<button type="button" disabled={busy} onClick={()=>void action('cancel')} className="flex items-center gap-1 rounded-lg border border-divider px-3 py-2 text-[11px]">
      <Square size={13}/>{en?'Stop':'중단'}
     </button>}
     {canResume&&<button type="button" disabled={busy} onClick={()=>void action('resume')} className="flex items-center gap-1 rounded-lg border border-divider px-3 py-2 text-[11px]">
      <RefreshCw size={13}/>{canRecheck?(en?'Recheck completion evidence':'완료 증거 재확인'):(en?'Resume':'재개')}
     </button>}
    </div>
    {status==='in_doubt'&&<div className="mt-3 rounded-lg border border-divider bg-surface p-3">
     <p className="text-[11px] text-brand-hover">
      {en?'The interrupted tool may have changed files. Inspect the workspace before continuing.':'중단 직전 도구가 파일을 변경했을 수 있습니다. 실제 상태를 확인한 뒤 기록하세요.'}
     </p>
     <label htmlFor="sdk-observation" className="mt-3 block text-[11px] font-semibold">
      {en?'What did you observe?':'확인한 작업 결과'}
     </label>
     <textarea id="sdk-observation" rows={2} maxLength={8192}
      value={observation} onChange={e=>setObservation(e.target.value)}
      placeholder={en?'Describe the files or outcomes you inspected.':'실제로 확인한 파일 상태나 실행 결과를 입력하세요.'}
      className="mt-1 w-full rounded-lg border border-divider p-2 text-[11px]" />
     <div className="mt-2 flex flex-wrap gap-2">
      <button type="button" disabled={busy||!observation.trim()} onClick={()=>void reconcile(true)}
       className="rounded-lg border border-divider px-3 py-2 text-[11px] disabled:opacity-40">
       {en?'Effect already applied':'변경이 적용됐음'}
      </button>
      <button type="button" disabled={busy||!observation.trim()} onClick={()=>void reconcile(false)}
       className="rounded-lg border border-divider px-3 py-2 text-[11px] disabled:opacity-40">
       {en?'Effect not applied':'변경이 적용되지 않았음'}
      </button>
     </div>
    </div>}
   </section>}
  </div>
 </section>
}
