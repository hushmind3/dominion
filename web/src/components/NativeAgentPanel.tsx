import {useEffect, useState} from 'react'
import {Check, Play, RefreshCw, ShieldAlert, Square, Sparkles} from 'lucide-react'
import {get, post, type Provider} from '../api'
import {useLocale} from '../i18n'

// Main branch presentation, wired to sub3's single Rust job/SDK runtime.
type NativeMeasuredUsage = {
 inputTokens:number;outputTokens:number;totalTokens:number;
 cacheReadTokens:number;cacheWriteTokens:number;
 reportedCalls:number;unreportedCalls:number;cacheReportedCalls:number;
 configuredCostKRW?:number;pricedCalls?:number;unpricedCalls?:number;
 source:string;estimated:false;
}
type RoutingPolicy={mode:'default'|'quality_cost';minQuality:number;maxQualityDrop:number}
type PinnedNativeRoute={providerId:string;model:string;
 selection?:{mode?:string;requestedMode?:string;reason?:string;estimatedPerTurnKRW?:number;selectedQuality?:number;qualifiedCandidates?:number}}
type AgentJob = {
 id:string; kind:string; status:string; sessionId?:string; createdAt?:string; queuedAt?:string;
 input?:{prompt?:string;providerId?:string;checks?:{type?:string;tool?:string;contains?:string}[];limits?:{maxEstimatedTokens?:number};nativeRoutingPolicy?:RoutingPolicy};
 nativeState?:{step?:number;modelCalls?:number;chargedTokens?:number;receipts?:{tool?:string;success?:boolean}[];error?:string};
 nativeStream?:{stage?:string;protocol?:string;events?:number;visibleTail?:string;pendingCalls?:{id:string;name:string}[];executable?:boolean};
 nativeModelUsage?:NativeMeasuredUsage;
 nativeRoute?:PinnedNativeRoute;
 result?:{answer?:string;reason?:string;toolCalls?:number;modelCalls?:number};error?:string;
}
type Approval = {id:string;status:string;sessionId?:string;tool?:string;args?:unknown}
type SdkRun = {
 id:string;status:string;instruction:string;providerId?:string;answer?:string;error?:string;verification?:string;tokenBudget?:number;
 routingPolicy?:RoutingPolicy;selectedRoute?:PinnedNativeRoute;
 stream?:AgentJob['nativeStream'];
 checkpoint?:{rounds?:number;toolCalls?:number;answer?:string;estimatedTokens?:number;
  usage?:NativeMeasuredUsage;
  pendingCall?:{id:string;name:string;arguments:unknown}|null;
  events?:{kind:string;detail:unknown}[]}
}
const running=new Set(['queued','running'])
const stopPolling=new Set(['completed','failed','cancelled','exhausted','denied','awaiting_approval','interrupted','in_doubt'])
const stateNames:Record<string,[string,string]>={
 queued:['대기 중','Queued'],running:['작업 진행 중','Working'],
 completed:['완료','Completed'],failed:['실패','Failed'],
 cancelled:['중단됨','Stopped'],exhausted:['실행 한도 도달','Limit reached'],
 awaiting_approval:['실행 승인 필요','Needs approval'],
 in_doubt:['수동 확인 필요','Needs manual review'],
 interrupted:['재개 필요','Resume required'],denied:['실행 거부됨','Denied']
}
function present(job:AgentJob,approvals:Approval[]=[]):SdkRun{
 const pending=approvals.find(a=>a.status==='pending'&&!!job.sessionId&&a.sessionId===job.sessionId)
 const status=pending?'awaiting_approval':job.status==='needs_review'?'in_doubt':job.status
 const receipts=job.nativeState?.receipts??[]
 return {
  id:job.id,status,instruction:job.input?.prompt??'',providerId:job.input?.providerId,
  tokenBudget:job.input?.limits?.maxEstimatedTokens,
  routingPolicy:job.input?.nativeRoutingPolicy,selectedRoute:job.nativeRoute,
  verification:job.input?.checks?.find(c=>c.type==='tool_succeeded'&&c.tool==='shell_exec')?.contains,
  answer:job.result?.answer,error:job.error??job.result?.reason??job.nativeState?.error,
  stream:job.nativeStream,
  checkpoint:{
   rounds:job.nativeState?.step??job.nativeState?.modelCalls??job.result?.modelCalls??0,
   toolCalls:job.result?.toolCalls??receipts.length,
   answer:job.result?.answer,
   usage:job.nativeModelUsage,
   estimatedTokens:job.nativeState?.chargedTokens,
   pendingCall:pending?{id:pending.id,name:pending.tool??'tool',arguments:pending.args??{}}:null,
   events:receipts.slice(-4).map(x=>({kind:x.success?'verified':'tool_started',detail:x.tool??''}))
  }
 }
}
async function loadRun(id:string):Promise<SdkRun>{
 const [job,approvals]=await Promise.all([
  get<AgentJob>('/api/jobs/'+encodeURIComponent(id)),
  get<Approval[]>('/api/approvals')
 ])
 return present(job,approvals)
}
export function NativeAgentPanel({providers}:{providers:Provider[]}){
 const locale=useLocale()
 const en=locale==='en'
 const [instruction,setInstruction]=useState('')
 const [providerId,setProviderId]=useState('')
 const [verification,setVerification]=useState('')
 const [budgetInput,setBudgetInput]=useState('100000')
 const [routingMode,setRoutingMode]=useState<'default'|'quality_cost'>('default')
 const [qualityInput,setQualityInput]=useState('0.80')
 const [qualityDropInput,setQualityDropInput]=useState('0.10')
 const [observation,setObservation]=useState('')
 const [run,setRun]=useState<SdkRun|null>(null)
 const [busy,setBusy]=useState(false)
 const [error,setError]=useState('')
 useEffect(()=>{
  let alive=true
  get<AgentJob[]>('/api/jobs').then(async jobs=>{
   const recent=jobs.filter(j=>j.kind==='prompt')
    .sort((a,b)=>String(b.queuedAt??b.createdAt??'').localeCompare(String(a.queuedAt??a.createdAt??'')))[0]
   if(recent){
    const current=await loadRun(recent.id)
    if(alive){setRun(current);if(current.tokenBudget)setBudgetInput(String(current.tokenBudget));
     if(current.routingPolicy){setRoutingMode(current.routingPolicy.mode);
      setQualityInput(String(current.routingPolicy.minQuality));
      setQualityDropInput(String(current.routingPolicy.maxQualityDrop))}
    }
   }
  }).catch(()=>{})
  return()=>{alive=false}
 },[])
 useEffect(()=>{
  if(!run?.id||stopPolling.has(run.status))return
  let alive=true
  const timer=setInterval(()=>{
   loadRun(run.id).then(next=>{if(alive)setRun(next)})
    .catch(e=>{if(alive)setError((e as Error).message)})
  },1500)
  return()=>{alive=false;clearInterval(timer)}
 },[run?.id,run?.status])
 const budget=()=>Math.max(1000,Math.min(20000000,Math.trunc(Number(budgetInput)||100000)))
 const launch=async(prompt:string,extraInstructions?:string,verifyCommand?:string,
  limit=budget(),previous?:SdkRun)=>{
  const policy=previous?.routingPolicy??{
   mode:routingMode,minQuality:Number(qualityInput),maxQualityDrop:Number(qualityDropInput)
  }
  if(!Number.isFinite(policy.minQuality)||policy.minQuality<0||policy.minQuality>1
   ||!Number.isFinite(policy.maxQualityDrop)||policy.maxQualityDrop<0||policy.maxQualityDrop>0.5)
   throw Error(en?'Quality must be 0–1 and quality loss 0–0.5':'품질 하한은 0~1, 허용 차이는 0~0.5로 입력하세요.')
  const created=await post<AgentJob>('/api/jobs',{kind:'prompt',input:{
   prompt,providerId:previous?previous.providerId||undefined:providerId||undefined,
   tokenBudget:limit,routingMode:policy.mode,
   minQuality:policy.minQuality,maxQualityDrop:policy.maxQualityDrop,
   ...(extraInstructions?{ownerInstructions:extraInstructions}:{}),
   ...(verifyCommand?{acceptanceChecks:[{type:'command_succeeded',command:verifyCommand}]}:{})
  }})
  setRun(present(created))
 }
 const start=async()=>{
  if(!instruction.trim()||busy)return
  setBusy(true);setError('')
  try{
   const verify=verification.trim()
   await launch(instruction.trim(),undefined,verify||undefined)
  }catch(e){setError((e as Error).message)}
  finally{setBusy(false)}
 }
 const action=async(kind:'approve'|'cancel'|'resume')=>{
  if(!run||busy)return
  setBusy(true);setError('')
  try{
   if(kind==='approve'){
    const call=run.checkpoint?.pendingCall
    if(!call?.id)throw Error(en?'Missing approval details':'승인할 도구 정보를 찾지 못했습니다.')
    await post('/api/approvals',{id:call.id,allow:true})
    setRun({...run,status:'running'})
   }else if(kind==='cancel'){
    await post('/api/jobs/'+encodeURIComponent(run.id)+'/cancel',{})
    setRun({...run,status:'cancelled'})
   }else{
    // This SDK does not replay uncertain external effects from a failed job.
    // A retry is a distinct job with its own fresh journal.
    await launch(run.instruction,
     'This is a fresh retry after an earlier run failed. Inspect the workspace before editing, and never assume earlier tools did or did not take effect.',
     run.verification||verification.trim()||undefined,run.tokenBudget??budget(),run)
   }
  }catch(e){setError((e as Error).message)}
  finally{setBusy(false)}
 }
 const reconcile=async(applied:boolean)=>{
  if(!run||busy||!observation.trim())return
  setBusy(true);setError('')
  try{
   const details=observation.trim()
   await launch(run.instruction,
    'Previous execution needs manual review. The operator inspected the effects and reports: '+(applied?'effect applied':'effect not applied')+
    '. Observation: '+details+'. Verify the actual workspace state before taking any further action; do not blindly replay prior changes.',
    run.verification||verification.trim()||undefined,run.tokenBudget??budget(),run)
   setObservation('')
  }catch(e){setError((e as Error).message)}
  finally{setBusy(false)}
 }
 const status=run?.status||''
 const call=run?.checkpoint?.pendingCall
 const needsApproval=status==='awaiting_approval' && call
 const canResume=['interrupted','failed'].includes(status)
 const events=(run?.checkpoint?.events||[]).slice(-4)
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
      className="h-9 rounded-lg border border-[#E7E0D5] bg-white px-3 text-[12px]">
      <option value="">{en?'Automatic selection':'자동 선택'}</option>
      {providers.filter(p=>p.enabled).map(p=><option key={p.id} value={p.id}>{p.name}{p.model?` · ${p.model}`:''}</option>)}
     </select>
    </label>
    <button type="button" onClick={()=>void start()} disabled={busy||!instruction.trim()}
     className="flex h-9 items-center gap-2 rounded-lg bg-[#AE5337] px-4 text-[12px] font-semibold text-white disabled:opacity-40">
     <Play size={14}/>{busy?(en?'Please wait…':'처리 중…'):(en?'Start task':'작업 시작')}
    </button>
   </div>
   <details className="text-[11px] text-[#746A5F]">
    <summary className="cursor-pointer select-none">{en?'Advanced: task token budget and result verification':'고급 설정: 작업 토큰 예산·결과 검증'}</summary>
    <label className="mt-2 block" htmlFor="sdk-token-budget">
     {en?'Rust agent token budget (estimated accounting; 1,000–20,000,000)':'Rust 에이전트 토큰 예산 (추정 기준 · 1,000~20,000,000)'}
    </label>
    <input id="sdk-token-budget" type="number" min={1000} max={20000000} step={1000}
     value={budgetInput} onChange={e=>setBudgetInput(e.target.value)}
     className="mt-1 h-9 w-56 max-w-full rounded-lg border border-[#E7E0D5] px-3 text-[12px]"/>
    <p className="mt-1">{en?'The Rust kernel enforces this estimate-based limit. Provider-measured usage is displayed separately.':'Rust 커널이 추정 기준 예산을 적용합니다. 제공자 실측 토큰은 별도로 표시합니다.'}</p>
    <label className="mt-3 block" htmlFor="sdk-routing-mode">{en?'Native model routing':'Rust 엔진 모델 선택'}</label>
    <select id="sdk-routing-mode" value={routingMode}
     onChange={e=>setRoutingMode(e.target.value as 'default'|'quality_cost')}
     className="mt-1 h-9 w-full rounded-lg border border-[#E7E0D5] bg-white px-3 text-[12px]">
     <option value="default">{en?'Use selected/default provider':'선택한 모델 또는 기본 모델 사용'}</option>
     <option value="quality_cost">{en?'Quality floor + configured KRW cost':'품질 하한 유지 + 등록 원화 단가 비교'}</option>
    </select>
    {routingMode==='quality_cost'&&<div className="mt-2 grid grid-cols-2 gap-2">
     <label className="text-[11px]">{en?'Minimum quality (0–1)':'최소 품질 (0~1)'}
      <input type="number" min={0} max={1} step={0.05} value={qualityInput}
       onChange={e=>setQualityInput(e.target.value)}
       className="mt-1 h-9 w-full rounded-lg border border-[#E7E0D5] bg-white px-3"/>
     </label>
     <label className="text-[11px]">{en?'Maximum drop from best quality (0–0.5)':'최고 품질 대비 허용 차이 (0~0.5)'}
      <input type="number" min={0} max={0.5} step={0.05} value={qualityDropInput}
       onChange={e=>setQualityDropInput(e.target.value)}
       className="mt-1 h-9 w-full rounded-lg border border-[#E7E0D5] bg-white px-3"/>
     </label>
    </div>}
    {routingMode==='quality_cost'&&<p className="mt-1">{en?'Only applies when AI model is Automatic; requires owner-entered model quality and KRW rates in Model Settings.':'AI 모델을 자동 선택으로 둔 경우에만 적용됩니다. 모델 설정에서 품질·원화 단가를 등록한 제공자만 비교합니다.'}</p>}
    <label className="mt-2 block" htmlFor="sdk-verify">
     {en?'Required verification command (must pass)':'필수 검증 명령어 (성공해야 완료)'}
    </label>
    <input id="sdk-verify" value={verification} onChange={e=>setVerification(e.target.value)}
     maxLength={2048} placeholder="cargo test"
     className="mt-1 h-9 w-full rounded-lg border border-[#E7E0D5] px-3 text-[12px]"/>
   </details>
   {error&&<p role="alert" className="rounded-lg bg-[#FFF3EE] p-3 text-[11px] text-[#A54027]">{error}</p>}
   {run&&<div className="rounded-xl border border-[#E7E0D5] bg-[#FCFBF9] p-3">
    <div className="flex flex-wrap items-center justify-between gap-2">
     <strong className="text-[12px]">{en?stateNames[status]?.[1]||status:stateNames[status]?.[0]||status}</strong>
     <span className="text-[11px] text-[#82796D]">{run.checkpoint?.rounds??0} {en?'rounds':'회 판단'} · {run.checkpoint?.toolCalls??0} {en?'actions':'회 도구 실행'}</span>
    {run.tokenBudget&&<p className="mt-2 text-[11px] text-[#82796D]">
     {en?'Kernel token budget':'커널 토큰 예산'}: {(run.checkpoint?.estimatedTokens??0).toLocaleString()} / {run.tokenBudget.toLocaleString()}
     <span className="ml-1">{en?'(measured when available, otherwise estimated)':'(제공자 보고값 우선·누락 시 추정)'}</span>
    </p>}
    {run.checkpoint?.usage&&<div className="mt-2 text-[11px] text-[#82796D]" aria-label={en?'Measured provider token usage':'제공자 측정 토큰 사용량'}>
     <p>{en?'Provider-reported (measured)':'제공자 보고값 (실측)'}: {run.checkpoint.usage.inputTokens.toLocaleString()} {en?'input':'입력'} · {run.checkpoint.usage.outputTokens.toLocaleString()} {en?'output':'출력'} · {run.checkpoint.usage.reportedCalls} {en?'reported calls':'회 보고'}</p>
     {run.checkpoint.usage.cacheReportedCalls>0&&<p className="mt-1">
      {en?'Cache read':'캐시 읽기'} {run.checkpoint.usage.cacheReadTokens.toLocaleString()} · {en?'Cache write':'캐시 쓰기'} {run.checkpoint.usage.cacheWriteTokens.toLocaleString()} {en?'tokens':'토큰'}
     </p>}
     {run.checkpoint.usage.pricedCalls!==undefined&&<p className="mt-1">
      {en?'KRW estimate (owner tariffs × reported usage)':'등록 단가 × 제공자 사용량 (원화 추산)'}: {(run.checkpoint.usage.configuredCostKRW??0).toLocaleString('ko-KR',{maximumFractionDigits:3})}원
      {(run.checkpoint.usage.unpricedCalls??0)>0&&<span className="ml-1 text-[#A54027]">· {run.checkpoint.usage.unpricedCalls} {en?'calls unpriced':'회 비용 미산정'}</span>}
     </p>}
     {run.checkpoint.usage.unreportedCalls>0&&<p className="mt-1 text-[#A54027]">
      {run.checkpoint.usage.unreportedCalls} {en?'calls did not report token usage; totals exclude unknown usage':'회 호출에서 사용량 미보고 · 위 합계에서 제외'}
     </p>}
    </div>}
    </div>
    {run.selectedRoute&&<div className="mt-2 text-[11px] text-[#746A5F]">
     <p>{en?'Pinned model':'작업 고정 모델'}: <strong>{run.selectedRoute.providerId} · {run.selectedRoute.model}</strong></p>
     {run.selectedRoute.selection?.mode==='quality_cost'&&
      <p>{en?'Owner-rate projected cost per turn':'등록 단가 기준 턴 예상 비용'}: {run.selectedRoute.selection.estimatedPerTurnKRW?.toLocaleString('ko-KR',{maximumFractionDigits:3})??'—'}원 · {en?'declared quality':'등록 품질'} {run.selectedRoute.selection.selectedQuality}</p>}
     {run.selectedRoute.selection?.requestedMode==='quality_cost'&&<p className="text-[#A54027]">
      {en?'No qualified priced models; used default route':'품질·단가를 갖춘 후보가 없어 기존 기본 모델 사용'}
     </p>}
    </div>}
    <p className="mt-2 line-clamp-2 text-[11px] text-[#766F67]">{run.instruction}</p>
    {run.stream&&['receiving','interrupted'].includes(run.stream.stage??'')&&<div role="status" aria-live="polite" className="mt-3 rounded-lg border border-[#E7E0D5] bg-white p-3 text-[11px]">
     <p className="font-semibold text-[#726B64]">{en?'Model is generating':'모델 응답 생성 중'} · {run.stream.protocol??'AI'} · {run.stream.events??0} {en?'updates':'회 갱신'}</p>
     {run.stream.visibleTail&&<pre className="mt-2 max-h-36 overflow-y-auto whitespace-pre-wrap break-words text-[#655D54]">{run.stream.visibleTail}</pre>}
     {(run.stream.pendingCalls?.length??0)>0&&<p className="mt-2 text-[#9B6D4E]">
      {en?'Pending model tool calls (not executed yet)':'모델이 준비 중인 도구 호출 (실행 전)'}: {run.stream.pendingCalls?.map(c=>c.name).join(', ')}
     </p>}
     {run.stream.stage==='interrupted'&&<p className="mt-2 text-[#A54027]">{en?'Generation interrupted; review before retrying':'생성 중단 · 재실행 전 확인 필요'}</p>}
    </div>}
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
     <div className="mt-2 text-[11px] font-semibold">{call.name}</div>
     <pre className="mt-2 max-h-56 overflow-auto whitespace-pre-wrap break-all rounded-lg bg-[#F5F2EE] p-3 text-[11px]">{JSON.stringify(call.arguments,null,2)}</pre>
     <button type="button" disabled={busy} onClick={()=>void action('approve')}
      className="mt-3 flex items-center gap-2 rounded-lg bg-[#A4583E] px-3 py-2 text-[11px] font-semibold text-white">
      <Check size={14}/>{en?'Approve this action only':'이 작업 1회 승인'}
     </button>
    </div>}
    {run.answer||run.checkpoint?.answer?<div className="mt-3 whitespace-pre-wrap break-words border-t border-[#E7E0D5] pt-3 text-[12px]">{run.answer||run.checkpoint?.answer}</div>:null}
    {run.error&&<p className="mt-2 text-[11px] text-[#A54027]">{run.error}</p>}
    <div className="mt-3 flex gap-2">
     {running.has(status)&&<button type="button" disabled={busy} onClick={()=>void action('cancel')} className="flex items-center gap-1 rounded-lg border border-[#E7E0D5] px-3 py-2 text-[11px]">
      <Square size={13}/>{en?'Stop':'중단'}
     </button>}
     {canResume&&<button type="button" disabled={busy} onClick={()=>void action('resume')} className="flex items-center gap-1 rounded-lg border border-[#E7E0D5] px-3 py-2 text-[11px]">
      <RefreshCw size={13}/>{en?'Run again':'다시 실행'}
     </button>}
    </div>
    {status==='in_doubt'&&<div className="mt-3 rounded-lg border border-[#E5C4AB] bg-white p-3">
     <p className="text-[11px] text-[#A54027]">
      {en?'Inspect prior effects; a fresh job will verify the result before continuing.':'이전 작업의 변경 사항을 확인하세요. 새 작업에서 상태를 재검사한 뒤 진행합니다.'}
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
       className="rounded-lg border border-[#E7E0D5] px-3 py-2 text-[11px] disabled:opacity-40">
       {en?'Effect already applied':'변경이 적용됐음'}
      </button>
      <button type="button" disabled={busy||!observation.trim()} onClick={()=>void reconcile(false)}
       className="rounded-lg border border-[#E7E0D5] px-3 py-2 text-[11px] disabled:opacity-40">
       {en?'Effect not applied':'변경이 적용되지 않았음'}
      </button>
     </div>
    </div>}
   </div>}
  </div>
 </section>
}
