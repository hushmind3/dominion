import {useEffect,useState} from 'react'
import {ArrowUpRight,CheckCircle2,Lightbulb,Loader2,Search} from 'lucide-react'
import {get,post,type Agent} from '../api'

type Evidence={claim?:string;url?:string;kind?:string}
type Draft={
 id:string
 title:string
 firstTest?:string
 killSignal?:string
 moneyMechanism?:string
 customer?:string
 problem?:string
 offer?:string
 whyPay?:string
 implementationPlan?:string
 implementationRunId?:string
 firstPaymentStep?:string
 evidenceStatus?:string
 evidence?:Evidence[]
 inferenceSteps?:string[]
 counterfactual?:string
 origin?:string
 status?:string
}
type Stats={
 signals:number
 candidates:number
 modelCandidates:number
 templateDrafts:number
 tensions:number
 lastIngest?:{capturedAt?:string;found?:number;new?:number}|null
 models?:{model:string;completed:number;failed:number}[]
 autopilot?:{enabled:boolean;rounds?:number;rotation?:number}
}
type FindResult={
 drafts:Draft[]
 modelCalls:number
 sourceCount?:number
 searchedAt?:string|null
 message?:string
 activeAutomationIds?:string[]
 inferenceJobId?:string|null
 inferenceStatus?:string
 inferenceError?:string|null
 inferenceTokenBudget?:number|null
 inferenceModel?:{providerId?:string;model?:string}|null
 selectedAgentId?:string
}
type SdkRun={
 id:string
 status:string
 source?:{kind:string;id:string}
 sessionId?:string
 verified?:boolean
 taskOutcome?:string
 answer?:string
 error?:string
 pendingApprovalId?:string
 checkpoint?:{pendingCall?:{name?:string;arguments?:unknown}}
}
const empty:FindResult={drafts:[],modelCalls:0,activeAutomationIds:[]}

export function MoneyPage(props:{openSession:(id:string)=>void}){
 const [result,setResult]=useState<FindResult>(empty)
 const [loading,setLoading]=useState(true)
 const [searching,setSearching]=useState(false)
 const [stopping,setStopping]=useState(false)
 const [error,setError]=useState('')
 const [agents,setAgents]=useState<Agent[]>([])
 const [agentsLoading,setAgentsLoading]=useState(true)
 const [stats,setStats]=useState<Stats|null>(null)
 const [toggling,setToggling]=useState(false)
 const [agentId,setAgentId]=useState('db')
 const [implementations,setImplementations]=useState<Record<string,SdkRun>>({})
 const [implementationBusy,setImplementationBusy]=useState('')
 const reasoning=['queued','running'].includes(result.inferenceStatus||'')
 useEffect(()=>{
  let mounted=true
  void get<Stats>('/api/money/stats').then(data=>{if(mounted)setStats(data)}).catch(()=>{})
  void Promise.all([get<FindResult>('/api/money/find'),get<Agent[]>('/api/agents')])
   .then(([saved,list])=>{
    if(!mounted)return
    const previous=saved.selectedAgentId
    setResult(saved)
    setAgents(list)
    setAgentId(list.some(a=>a.id===previous)?previous!:(list[0]?.id||'db'))
   }).catch(err=>{if(mounted)setError((err as Error).message)})
   .finally(()=>{if(mounted){setLoading(false);setAgentsLoading(false)}})
  return()=>{mounted=false}
 },[])
 useEffect(()=>{
  if(!reasoning)return
  let live=true
  const interval=window.setInterval(()=>{
   void get<FindResult>('/api/money/find').then(data=>{if(live)setResult(data)}).catch(()=>{})
  },3500)
  return()=>{live=false;window.clearInterval(interval)}
 },[reasoning])
 const selectedAgent=agents.find(a=>a.id===agentId)
 useEffect(()=>{
  let mounted=true
  void get<SdkRun[]>('/api/sdk/runs/source/money_candidate')
   .then(runs=>{
    if(!mounted)return
    const mapped:Record<string,SdkRun>={}
    for(const run of runs){
     const candidateId=run.source?.id
     if(candidateId&&!mapped[candidateId])mapped[candidateId]=run
    }
    setImplementations(previous=>({...mapped,...previous}))
   }).catch(()=>{})
  return()=>{mounted=false}
 },[])
 // The candidate owns the durable link, even when more than 40 historical
 // SDK runs have accumulated and the generic recent-run fallback omits it.
 const missingRunLinks=JSON.stringify((result.drafts||[])
  .filter(d=>d.implementationRunId&&!implementations[d.id])
  .map(d=>[d.id,d.implementationRunId]))
 useEffect(()=>{
  const links=JSON.parse(missingRunLinks) as [string,string][]
  if(!links.length)return
  let live=true
  void Promise.all(links.map(async([draftId,runId])=>({
   draftId,run:await get<SdkRun>('/api/sdk/runs/'+encodeURIComponent(runId))
  }))).then(rows=>{
   if(!live)return
   setImplementations(previous=>{
    const next={...previous}
    for(const {draftId,run} of rows)next[draftId]=run
    return next
   })
  }).catch(()=>{})
  return()=>{live=false}
 },[missingRunLinks])
 const activeImplementationIds=Object.values(implementations)
  .filter(run=>['queued','running','awaiting_approval'].includes(run.status))
  .map(run=>run.id).sort().join('|')
 useEffect(()=>{
  if(!activeImplementationIds)return
  let mounted=true
  const ids=activeImplementationIds.split('|')
  const refresh=()=>{void Promise.all(ids.map(id=>get<SdkRun>('/api/sdk/runs/'+encodeURIComponent(id))))
   .then(runs=>{
    if(!mounted)return
    setImplementations(previous=>{
     const next={...previous}
     for(const run of runs){if(run.source?.id)next[run.source.id]=run}
     return next
    })
   }).catch(()=>{})}
  refresh()
  const timer=window.setInterval(refresh,4000)
  return()=>{mounted=false;window.clearInterval(timer)}
 },[activeImplementationIds])
 async function implementDraft(draft:Draft){
  const agent=agents.find(item=>item.id===agentId)
  if(!agent?.providerId||!agent.model||implementationBusy||draft.implementationRunId)return
  setImplementationBusy(draft.id)
  setError('')
  try{
   // The server builds instructions from its saved candidate; client-side
   // research text is never an authorization or execution plan.
   const run=await post<SdkRun>('/api/money/candidates/'+encodeURIComponent(draft.id)+'/implement',{
    approved:true,agentId:agent.id,providerId:agent.providerId,model:agent.model,
    tokenBudget:24000,maxRounds:12
   })
   setImplementations(previous=>({...previous,[draft.id]:run}))
   setResult(previous=>({...previous,drafts:previous.drafts.map(item=>
    item.id===draft.id?{...item,implementationRunId:run.id}:item)}))
  }catch(err){setError((err as Error).message)}
  finally{setImplementationBusy('')}
 }
 async function implementationAction(draftId:string,run:SdkRun,action:'cancel'|'resume'|'approve'){
  if(implementationBusy)return
  setImplementationBusy(draftId)
  setError('')
  try{
   const id=encodeURIComponent(run.id)
   await post('/api/sdk/runs/'+id+'/'+action,action==='approve'
    ?{callId:run.pendingApprovalId}:{})
   const saved=await get<SdkRun>('/api/sdk/runs/'+id)
   setImplementations(previous=>({...previous,[draftId]:saved}))
  }catch(err){setError((err as Error).message)}
  finally{setImplementationBusy('')}
 }
 async function toggleAutopilot(){
  if(toggling||!stats)return
  setToggling(true)
  setError('')
  try{
   const body=stats.autopilot?.enabled?{enabled:false}
    :{enabled:true,continuous:true,intervalMinutes:10,maxRounds:12,tokenBudget:24000,
      freeRotation:true,agentId:agentId==='db'?undefined:agentId}
   await post('/api/money/campaigns/money%3Aauto/autopilot',body)
   setStats(await get<Stats>('/api/money/stats'))
  }catch(err){setError((err as Error).message)}
  finally{setToggling(false)}
 }
 async function findMoney(infer=false){
  setSearching(true)
  setError('')
  try{
   const payload=agentId==='db'?{mode:'db'}:{mode:'agent',agentId,infer}
   const data=await post<FindResult>('/api/money/find',payload)
   setResult(data)
   void get<Stats>('/api/money/stats').then(setStats).catch(()=>{})
  }catch(err){setError((err as Error).message)}
  finally{setSearching(false)}
 }
 async function stopPreviousAI(){
  const ids=result.activeAutomationIds||[]
  if(!ids.length)return
  setStopping(true)
  setError('')
  try{
   await Promise.all(ids.map(id=>post('/api/money/campaigns/'+encodeURIComponent(id)+'/autopilot',{enabled:false})))
   setResult(prev=>({...prev,activeAutomationIds:[]}))
  }catch(err){setError((err as Error).message)}
  finally{setStopping(false)}
 }
 const drafts=(result.drafts||[]).filter(d=>d.status!=='rejected')
 return <main className="mx-auto w-full max-w-[1040px] px-4 pb-16 pt-8 text-[#33291F] sm:px-7">
  <div className="mb-6 flex flex-wrap items-center justify-between gap-5">
   <div>
    <p className="mb-2 text-[11px] font-extrabold tracking-[.2em] text-[#A46A4F]">DOMINION MONEY</p>
    <h1 className="text-[30px] font-extrabold tracking-tight sm:text-[38px]">돈벌이 찾기</h1>
    <p className="mt-2 text-[13px] leading-6 text-[#817368]">
     쌓아둔 자료에서 기회를 찾고, 돈 버는 방법까지 초안으로 정리합니다.
    </p>
   </div>
  </div>

  <section aria-label="누적 현황" className="mb-6">
   <div className="grid grid-cols-2 gap-2 sm:grid-cols-4">
    <div className="rounded-xl border border-[#E6D9C9] bg-[#FFFDF9] px-4 py-3">
     <p className="text-[10px] font-bold text-[#9A8B7E]">수집 신호</p>
     <strong className="mt-1 block text-xl tabular-nums text-[#3B2E24]">{stats?stats.signals.toLocaleString():'—'}</strong>
    </div>
    <div className="rounded-xl border border-[#E6D9C9] bg-[#FFFDF9] px-4 py-3">
     <p className="text-[10px] font-bold text-[#9A8B7E]">후보</p>
     <strong className="mt-1 block text-xl tabular-nums text-[#3B2E24]">{stats?stats.candidates.toLocaleString():'—'}</strong>
     {stats&&<small className="text-[10px] text-[#9A8B7E]">모델 {stats.modelCandidates} · 템플릿 {stats.templateDrafts}</small>}
    </div>
    <div className="rounded-xl border border-[#E6D9C9] bg-[#FFFDF9] px-4 py-3">
     <p className="text-[10px] font-bold text-[#9A8B7E]">갈등쌍</p>
     <strong className="mt-1 block text-xl tabular-nums text-[#3B2E24]">{stats?stats.tensions.toLocaleString():'—'}</strong>
    </div>
    <div className="rounded-xl border border-[#E6D9C9] bg-[#FFFDF9] px-4 py-3">
     <p className="text-[10px] font-bold text-[#9A8B7E]">자동 순환</p>
     <strong className="mt-1 block text-xl tabular-nums text-[#3B2E24]">{stats?.autopilot?.enabled?`${stats.autopilot.rounds??0}회`:'꺼짐'}</strong>
     <small className="mt-1 block text-[10px] text-[#9A8B7E]">{stats?.autopilot?.enabled?`모델 ${stats.autopilot.rotation??0}개 순환`:'꺼짐 상태'}</small>
    </div>
   </div>
   {stats?.lastIngest?.capturedAt&&<p className="mt-2 text-[11px] text-[#8D7B6C]">마지막 수집 {new Date(stats.lastIngest.capturedAt).toLocaleString('ko-KR')} · 발견 {stats.lastIngest.found??0}건 · 신규 {stats.lastIngest.new??0}건</p>}
   {!!stats?.models?.length&&<details className="mt-1 text-[11px] text-[#8D7B6C]">
    <summary className="cursor-pointer">모델별 성공/실패</summary>
    <ul className="mt-1 grid gap-0.5">
     {stats.models.map(m=><li key={m.model} className="flex justify-between gap-3"><span className="min-w-0 truncate">{m.model}</span><span className="tabular-nums">성공 {m.completed} · 실패 {m.failed}</span></li>)}
    </ul>
   </details>}
  </section>

  <section aria-label="실행" className="mb-6 rounded-xl border border-[#E6D9C9] bg-[#FFFDF9] p-3 sm:p-4">
   <div className="flex flex-wrap items-center gap-2">
    <button type="button" onClick={()=>void findMoney(false)} disabled={searching||loading||reasoning}
     className="inline-flex min-h-10 items-center gap-2 rounded-lg bg-[#C96442] px-4 text-xs font-extrabold text-white disabled:opacity-60">
     {searching?<Loader2 size={15} className="animate-spin"/>:<Search size={15}/>} 조사 · 0토큰
    </button>
    <select id="money-agent-choice" value={agentId} disabled={searching||reasoning||agentsLoading} onChange={e=>setAgentId(e.target.value)}
     className="min-h-10 min-w-0 flex-1 rounded-lg border border-[#DAD0C3] bg-white px-3 text-xs font-semibold text-[#392F26] disabled:opacity-60">
     {agents.map(agent=><option value={agent.id} key={agent.id}>{agent.name}{agent.model?' · '+agent.model:''}</option>)}
    </select>
    <button type="button" onClick={()=>void findMoney(true)} disabled={searching||loading||reasoning||agentsLoading||agentId==='db'}
     className="inline-flex min-h-10 items-center gap-2 rounded-lg bg-[#9B6A3F] px-4 text-xs font-extrabold text-white disabled:opacity-50">
     AI 선별 · 1회
    </button>
    <button type="button" onClick={()=>void toggleAutopilot()} disabled={toggling||!stats||agentsLoading}
     className={'inline-flex min-h-10 items-center rounded-lg px-4 text-xs font-extrabold text-white disabled:opacity-50 '+(stats?.autopilot?.enabled?'bg-[#8B6A4B]':'bg-[#6E7A5A]')}>
     {toggling?'처리 중…':stats?.autopilot?.enabled?'자동 순환 끄기':'자동 순환 켜기'}
    </button>
   </div>
   <p className="mt-2 text-[11px] leading-5 text-[#8D7B6C]">
    조사 = 쌓인 자료에서 초안 추출(0토큰) · AI 선별 = 선택한 모델로 1회 추론 · 자동 순환 = 선택한 에이전트 역할을 유지하며 10분마다 연결된 무료 모델을 순환
   </p>
  </section>

  {!!result.activeAutomationIds?.length&&<div className="mb-5 flex flex-wrap items-center justify-between gap-3 rounded-xl border border-[#F0D4B7] bg-[#FFF6EC] px-4 py-3 text-xs text-[#81512F]">
   <span>이전에 켠 AI 자동 분석이 별도로 실행 중입니다.</span>
   <button disabled={stopping} onClick={()=>void stopPreviousAI()} className="font-bold underline underline-offset-4 disabled:opacity-50">
    {stopping?'중지 중…':'기존 AI 자동 분석 중지'}
   </button>
  </div>}

  {error&&<div role="alert" className="mb-5 rounded-xl border border-red-200 bg-red-50 px-4 py-3 text-xs text-red-800">
   {error}
  </div>}

  {reasoning&&<div role="status" className="mb-4 rounded-xl border border-[#E5D3BC] bg-[#FFF7E9] px-4 py-3 text-xs leading-6 text-[#765337]">
   자료 수집과 DB 검색을 마쳤습니다. 지금 AI가 <strong>새 수익구조를 추측·추론</strong>하고 있습니다. 1회 실행, 토큰 예산 상한 {result.inferenceTokenBudget?.toLocaleString()||'24,000'}토큰. 아래는 먼저 만든 조사 초안입니다.
  </div>}
  {(result.inferenceStatus==='failed'||result.inferenceStatus==='needs_review'||result.inferenceError)&&!reasoning&&<div role="alert" className="mb-4 rounded-xl border border-amber-200 bg-amber-50 px-4 py-3 text-xs leading-5 text-amber-900">
   심층 추론 실패 · {result.inferenceError||'모델 응답을 후보로 변환하지 못했습니다.'} DB 추측 초안은 그대로 확인할 수 있습니다.
  </div>}
  <section aria-label="자동 발굴 결과">
   <div className="mb-4 flex flex-wrap items-center justify-between gap-2">
    <h2 className="text-lg font-extrabold">발굴한 돈벌이 초안</h2>
    <span className="inline-flex items-center gap-1 text-[11px] font-semibold text-[#8D7C6D]">
     <CheckCircle2 size={13}/> {drafts.length}건 표시 · 전체 후보 {(stats?.candidates??0).toLocaleString()}건 · {result.inferenceStatus==='queued'||result.inferenceStatus==='running'?'AI 추론 중':result.inferenceStatus==='completed'?'AI 추론 완료':result.inferenceStatus==='failed'?'AI 추론 실패':result.inferenceStatus==='needs_review'?'AI 검토 필요':'DB 초안'}
    </span>
   </div>
   {drafts.length?
    <div className="space-y-4">{drafts.map((draft,index)=><article key={draft.id}
     className="overflow-hidden rounded-2xl border border-[#E6D9C9] bg-[#FFFDF9] shadow-[0_3px_14px_rgba(67,48,29,.045)]">
     <div className="border-b border-[#EEE5D9] px-5 py-4 sm:px-6">
      <p className="mb-2 text-[10px] font-extrabold tracking-[.12em] text-[#B96848]">발굴 후보 {index+1}</p>
      <h3 className="text-base font-extrabold leading-6 text-[#30251E] sm:text-lg">{draft.title}</h3>
      <p className="mt-2 text-xs leading-6 text-[#645547]">{draft.offer}</p>
     </div>
     <div className="grid gap-2 px-5 py-4 sm:px-6">
      <div className="flex flex-wrap gap-x-2 text-xs leading-6"><strong className="text-[#A46144]">누가 돈을 내나</strong><span className="min-w-0 flex-1 text-[#5E534A]">{draft.customer||'구매자 조사 필요'}</span></div>
      <div className="flex flex-wrap gap-x-2 text-xs leading-6"><strong className="text-[#A46144]">수익 구조</strong><span className="min-w-0 flex-1 text-[#5E534A]">{draft.moneyMechanism||draft.whyPay||'지불 구조 추가 조사 필요'}</span></div>
      {!!draft.firstTest&&<div className="flex flex-wrap gap-x-2 text-xs leading-6"><strong className="text-[#A46144]">다음 검증</strong><span className="min-w-0 flex-1 text-[#5E534A]">{draft.firstTest}</span></div>}
     </div>
     <details className="border-t border-[#EEE5D9] bg-[#FFFBF3] px-5 py-3 sm:px-6">
      <summary className="cursor-pointer text-xs font-bold text-[#9D593D]">상세 보기 · 지불 이유·구현·첫 매출{draft.inferenceSteps?.length?'·추론 단계':''}</summary>
      <div className="mt-3 grid gap-3 sm:grid-cols-2 sm:gap-x-8">
       <div><strong className="text-xs text-[#A46144]">지불 이유</strong>
        <p className="mt-1 text-xs leading-6 text-[#5E534A]">{draft.whyPay||'추가 조사 필요'}</p>
       </div>
       <div><strong className="text-xs text-[#A46144]">구현</strong>
        <p className="mt-1 text-xs leading-6 text-[#5E534A]">{draft.implementationPlan||'추가 조사 필요'}</p>
       </div>
       <div><strong className="text-xs text-[#A46144]">첫 매출 경로</strong>
        <p className="mt-1 text-xs leading-6 text-[#5E534A]">{draft.firstPaymentStep||'추가 조사 필요'}</p>
       </div>
       <div><strong className="text-xs text-[#A46144]">폐기 조건</strong>
        <p className="mt-1 text-xs leading-6 text-[#5E534A]">{draft.killSignal||'추가 조사 필요'}</p>
       </div>
      </div>
      {!!draft.inferenceSteps?.length&&<ol className="mt-3 space-y-1.5">{draft.inferenceSteps.slice(0,5).map((step,i)=><li key={i} className="text-xs leading-5 text-[#665548]">{i+1}. {step}</li>)}</ol>}
      {draft.counterfactual&&<p className="mt-2 text-[11px] leading-5 text-[#8B6D51]"><strong>가설이 틀리는 경우:</strong> {draft.counterfactual}</p>}
     </details>
     <div className="border-t border-[#EEE5D9] px-5 py-3 sm:px-6">
      {(()=>{
       const run=implementations[draft.id]
       const active=run&&['queued','running','awaiting_approval'].includes(run.status)
       return <div className="space-y-2">
        <div className="flex flex-wrap items-center gap-2">
         {!run&&!draft.implementationRunId&&<button type="button" onClick={()=>void implementDraft(draft)}
          disabled={!!implementationBusy||agentsLoading||!selectedAgent?.providerId||!selectedAgent?.model}
          className="min-h-9 rounded-lg border border-[#A96743] px-3 text-xs font-bold text-[#9D593D] disabled:opacity-50">
          {implementationBusy===draft.id?'작업 생성 중…':'이 후보 MVP 구현 승인'}
         </button>}
         {draft.implementationRunId&&!run&&<span className="text-xs font-semibold text-[#81512F]">
          승인된 SDK 작업 기록 불러오는 중…
         </span>}
         {run&&<span className="text-xs font-semibold text-[#81512F]">
          SDK 구현 작업 · {run.status==='completed'?(run.verified?'검증 완료':'완료 · 검증 필요'):
           run.status==='awaiting_approval'?'도구 승인 대기':
           run.status==='in_doubt'?'외부 작업 결과 확인 필요':
           run.status==='running'?'실행 중':run.status==='queued'?'대기 중':
           run.status==='cancelled'?'중단됨':run.status==='failed'?'실패':run.status}
         </span>}
         {run?.sessionId&&<button type="button" onClick={()=>props.openSession(run.sessionId!)}
          className="text-xs font-semibold text-[#9D593D] underline underline-offset-2">작업 세션 보기</button>}
         {run&&active&&<button type="button" onClick={()=>void implementationAction(draft.id,run,'cancel')}
          disabled={!!implementationBusy} className="text-xs font-bold text-[#9D593D] underline disabled:opacity-50">작업 중단</button>}
         {run&&['failed','interrupted'].includes(run.status)&&<button type="button"
          onClick={()=>void implementationAction(draft.id,run,'resume')}
          disabled={!!implementationBusy} className="text-xs font-bold text-[#9D593D] underline disabled:opacity-50">기존 작업 재개</button>}
        </div>
        {run?.status==='awaiting_approval'&&run.pendingApprovalId&&<details className="rounded-lg border border-[#E5D3BC] bg-[#FFF8EB] px-3 py-2">
         <summary className="cursor-pointer text-xs font-bold text-[#765337]">승인 요청된 도구와 실행 인수 확인</summary>
         <p className="mt-2 text-xs text-[#765337]">{run.checkpoint?.pendingCall?.name||'도구 이름 확인 필요'}</p>
         <pre className="mt-1 max-h-40 overflow-auto whitespace-pre-wrap break-all text-[11px] text-[#665548]">{JSON.stringify(run.checkpoint?.pendingCall?.arguments??{},null,2).slice(0,4000)}</pre>
         <button type="button" onClick={()=>void implementationAction(draft.id,run,'approve')}
          disabled={!!implementationBusy} className="mt-2 rounded-lg bg-[#9B6A3F] px-3 py-2 text-xs font-bold text-white disabled:opacity-50">
          표시된 도구 실행 승인
         </button>
        </details>}
        {run?.error&&<p className="text-xs text-amber-800">{run.error}</p>}
        {run?.answer&&<details className="rounded-lg border border-[#E5D3BC] px-3 py-2">
         <summary className="cursor-pointer text-xs font-bold text-[#765337]">구현 결과 보고</summary>
         <p className="mt-2 whitespace-pre-wrap text-xs leading-5 text-[#665548]">{run.answer.slice(0,6000)}</p>
        </details>}
        {!run&&!draft.implementationRunId&&<p className="text-[11px] text-[#8D7B6C]">선택한 에이전트로 검증용 MVP만 구현합니다. 외부 판매·계약·지출·배포는 승인 범위에 포함되지 않습니다.</p>}
       </div>
      })()}
     </div>
     <div className="flex flex-wrap items-center gap-x-4 gap-y-2 border-t border-[#EEE5D9] bg-[#FCF9F4] px-5 py-3 sm:px-6">
      <span className="text-[10px] text-[#9A8B7E]">
       {draft.origin==='model'?'AI 추론 · 검증 전':draft.evidenceStatus==='two_source_research_question_only'?'독립 출처 2건 기반':
        draft.evidenceStatus==='single_source_only_not_validated'?'원문 1건 기반':
        (draft.evidence||[]).length?'기존 근거 기반':'기존 사업 가설'} · 고객 결제 미검증
      </span>
      {(draft.evidence||[]).filter(e=>typeof e.url==='string'&&e.url.startsWith('https://'))
       .slice(0,2).map((e,i)=><a key={i} href={e.url} target="_blank" rel="noreferrer"
        className="inline-flex items-center gap-1 text-[11px] font-semibold text-[#9F583C] hover:underline">
        자료 {i+1} <ArrowUpRight size={13}/>
       </a>)}
     </div>
    </article>)}</div>:
    <div className="flex min-h-52 flex-col items-center justify-center rounded-2xl border border-dashed border-[#E5D8C8] bg-[#FFFCF8] px-6 py-10 text-center">
     <Lightbulb size={28} className="mb-3 text-[#BD9479]"/>
     <strong className="text-sm text-[#584637]">{loading?'기존 발굴 결과 불러오는 중…':result.message||'돈벌이 찾기를 눌러 초안을 만들어 보세요.'}</strong>
     <p className="mt-2 text-xs leading-6 text-[#95877A]">기존 DB에서 돈벌이 후보를 추리고, 선택한 에이전트가 있으면 심층 추론합니다.</p>
    </div>}
  </section>
  {!!result.inferenceModel?.model&&<p className="mt-4 text-right text-[11px] font-medium text-[#786755]">
   추론 모델 · {result.inferenceModel.providerId||'기본 제공자'} / {result.inferenceModel.model}
  </p>}
  {result.searchedAt&&<p className="mt-5 text-right text-[10px] text-[#9B8E81]">
   마지막 발굴 {new Date(result.searchedAt).toLocaleString('ko-KR')}
  </p>}
 </main>
}
