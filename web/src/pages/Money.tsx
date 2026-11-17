import {useEffect,useState} from 'react'
import {ArrowUpRight,CheckCircle2,Lightbulb,Loader2,Search,Square,Code2} from 'lucide-react'
import {get,post,type Agent} from '../api'

type Evidence={claim?:string;url?:string;kind?:string;publishedAt?:string;contentStatus?:string}
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
 firstPaymentStep?:string
 evidenceStatus?:string
 evidence?:Evidence[]
 inferenceSteps?:string[]
 counterfactual?:string
 origin?:string
 status?:string
 reviewReason?:string
 reviewRevision?:number
 implementationJobId?:string
 implementationJob?:{id:string;status:string;error?:string;sessionId?:string|null}
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
 inferenceEstimatedTokens?:number|null
 inferenceConfiguredCostKRW?:number|null
 inferenceModel?:{providerId?:string;model?:string}|null
 selectedAgentId?:string
}
type WorkJob={id:string;status:string;error?:string;sessionId?:string;result?:{sessionId?:string;result?:{sessionId?:string}}}
type ArchiveResult={candidates:Draft[];modelCalls:number}
type SourcePreview={source:{title?:string;description?:string;articleExcerpt?:string;articleBody?:string;publishedAt?:string;capturedAt?:string;sourceName?:string;contentStatus?:string;relationship?:string;canonicalSourceId?:string};revisions?:{id:string;reason?:string;capturedAt?:string;content?:{articleExcerpt?:string;articleBody?:string;description?:string}}[];sourceStatus?:string}
type ReviewHistory={reviews:{id:string;fromStatus?:string;toStatus?:string;reason?:string;reviewedAt?:string}[]}
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
 const [cancellingInference,setCancellingInference]=useState(false)
 const [implementingId,setImplementingId]=useState<string|null>(null)
 const [archiveOpen,setArchiveOpen]=useState(false)
 const [archiveStatus,setArchiveStatus]=useState('deferred')
 const [archiveOffset,setArchiveOffset]=useState(0)
 const [archiveInput,setArchiveInput]=useState('')
 const [archiveQuery,setArchiveQuery]=useState('')
 const [archiveItems,setArchiveItems]=useState<Draft[]>([])
 const [archiveLoading,setArchiveLoading]=useState(false)
 const [archiveError,setArchiveError]=useState('')
 const [archiveRevision,setArchiveRevision]=useState(0)
 const [sourceViews,setSourceViews]=useState<Record<string,SourcePreview>>({})
 const [reviewHistories,setReviewHistories]=useState<Record<string,ReviewHistory>>({})
 const [sourceLoading,setSourceLoading]=useState<string|null>(null)
 const reasoning=['queued','running'].includes(result.inferenceStatus||'')
 const activeImplementation=result.drafts?.some(d=>['queued','running'].includes(d.implementationJob?.status||''))||false
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
  if(!reasoning&&!activeImplementation)return
  let live=true
  const interval=window.setInterval(()=>{
   void get<FindResult>('/api/money/find').then(data=>{if(live)setResult(data)}).catch(()=>{})
  },3500)
  return()=>{live=false;window.clearInterval(interval)}
 },[reasoning,activeImplementation])
 async function cancelInference(){
  const id=result.inferenceJobId
  if(!id||cancellingInference)return
  setCancellingInference(true);setError('')
  try{
   const job=await get<WorkJob>('/api/jobs/'+encodeURIComponent(id))
   if(['queued','running'].includes(job.status))await post('/api/jobs/'+encodeURIComponent(id)+'/cancel',{})
   setResult(await get<FindResult>('/api/money/find'))
  }catch(err){setError((err as Error).message)}
  finally{setCancellingInference(false)}
 }
 useEffect(()=>{
  if(!archiveOpen)return
  let active=true
  setArchiveLoading(true);setArchiveError('')
  const query=new URLSearchParams({status:archiveStatus,q:archiveQuery,limit:'24',offset:String(archiveOffset)})
  void get<ArchiveResult>('/api/money/archive?'+query.toString())
   .then(data=>{if(active)setArchiveItems(data.candidates)})
   .catch(err=>{if(active)setArchiveError((err as Error).message)})
   .finally(()=>{if(active)setArchiveLoading(false)})
  return()=>{active=false}
 },[archiveOpen,archiveStatus,archiveQuery,archiveOffset,archiveRevision])
 async function reviewCandidate(draft:Draft,status:'deferred'|'rejected'|'hypothesis'){
  setError('')
  const reason=window.prompt(status==='hypothesis'?'다시 검토하는 이유 (선택)':status==='deferred'?'보류 이유 (선택)':'제외 이유 (선택)','')
  if(reason===null)return
  try{
   await post('/api/money/candidates/'+encodeURIComponent(draft.id)+'/review',{status,reason})
   setResult(await get<FindResult>('/api/money/find'))
   setArchiveRevision(n=>n+1)
   setReviewHistories(prev=>{const copy={...prev};delete copy[draft.id];return copy})
  }catch(err){setError((err as Error).message)}
 }
 async function loadReviewHistory(id:string){
  try{
   const data=await get<ReviewHistory>('/api/money/candidates/'+encodeURIComponent(id)+'/reviews')
   setReviewHistories(prev=>({...prev,[id]:data}))
  }catch(err){setArchiveError((err as Error).message)}
 }
 async function revealStoredSource(url:string){
  if(sourceViews[url]||sourceLoading)return
  setSourceLoading(url)
  try{
   const data=await get<SourcePreview>('/api/money/source?url='+encodeURIComponent(url))
   setSourceViews(prev=>({...prev,[url]:data}))
  }catch(err){setArchiveError((err as Error).message)}
  finally{setSourceLoading(null)}
 }
 async function approveImplementation(draft:Draft){
  if(agentId==='db'||implementingId||draft.implementationJobId){
   setError('구현할 에이전트를 선택하고 후보의 기존 작업 상태를 확인하세요.')
   return
  }
  if(!window.confirm('선택한 후보의 로컬 프로토타입 구현을 승인하시겠습니까?\\n모델 호출·파일 수정·테스트가 수행될 수 있습니다. 별도 결제·배포·외부 연락은 승인하지 않습니다.'))return
  setImplementingId(draft.id);setError('')
  try{
   await post<WorkJob>('/api/money/candidates/'+encodeURIComponent(draft.id)+'/implement',{
    approved:true,agentId,tokenBudget:40000
   })
   setResult(await get<FindResult>('/api/money/find'))
  }catch(err){setError((err as Error).message)}
  finally{setImplementingId(null)}
 }
 async function cancelImplementation(draft:Draft){
  const jobId=draft.implementationJob?.id
  if(!jobId)return
  try{
   await post('/api/jobs/'+encodeURIComponent(jobId)+'/cancel',{})
   setResult(await get<FindResult>('/api/money/find'))
  }catch(err){setError((err as Error).message)}
 }
 const selectedAgent=agents.find(a=>a.id===agentId)
 async function toggleAutopilot(){
  if(toggling||!stats)return
  setToggling(true)
  setError('')
  try{
   const body=stats.autopilot?.enabled?{enabled:false}
    :{enabled:true,continuous:true,intervalMinutes:10,maxRounds:12,tokenBudget:24000,freeRotation:true}
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
 const drafts=(result.drafts||[]).filter(d=>!['rejected','deferred'].includes(d.status||''))
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
    <button type="button" onClick={()=>void toggleAutopilot()} disabled={toggling||!stats}
     className={'inline-flex min-h-10 items-center rounded-lg px-4 text-xs font-extrabold text-white disabled:opacity-50 '+(stats?.autopilot?.enabled?'bg-[#8B6A4B]':'bg-[#6E7A5A]')}>
     {toggling?'처리 중…':stats?.autopilot?.enabled?'자동 순환 끄기':'자동 순환 켜기'}
    </button>
   </div>
   <p className="mt-2 text-[11px] leading-5 text-[#8D7B6C]">
    조사 = 쌓인 자료에서 초안 추출(0토큰) · AI 선별 = 선택한 모델로 1회 추론 · 자동 순환 = 10분마다 무료 모델을 바꿔 계속 쌓기
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

  {reasoning&&<div role="status" className="mb-4 flex flex-wrap items-center justify-between gap-3 rounded-xl border border-[#E5D3BC] bg-[#FFF7E9] px-4 py-3 text-xs leading-6 text-[#765337]">
   <span>DB 근거 기반 <strong>새 수익구조 추론 중</strong>입니다. 신규 수집은 별도 작업으로 진행될 수 있습니다. 토큰 상한 {result.inferenceTokenBudget?.toLocaleString()||'24,000'}.</span>
   <button type="button" disabled={cancellingInference} onClick={()=>void cancelInference()}
    className="inline-flex items-center gap-1 rounded-lg border border-[#CBA88D] px-3 py-1 font-bold disabled:opacity-50"><Square size={12}/>{cancellingInference?'중단 중…':'이번 추론 중단'}</button>
  </div>}
  {(result.inferenceStatus==='failed'||result.inferenceStatus==='needs_review'||result.inferenceError)&&!reasoning&&<div role="alert" className="mb-4 rounded-xl border border-amber-200 bg-amber-50 px-4 py-3 text-xs leading-5 text-amber-900">
   심층 추론 실패 · {result.inferenceError||'모델 응답을 후보로 변환하지 못했습니다.'} DB 추측 초안은 그대로 확인할 수 있습니다.
  </div>}
  {result.inferenceJobId&&<p className="mb-4 text-[11px] text-[#877566]">
   모델 호출 {result.modelCalls.toLocaleString()}회
   {typeof result.inferenceEstimatedTokens==='number'&&` · 커널 추정 누적 ${result.inferenceEstimatedTokens.toLocaleString()}토큰`}
   {typeof result.inferenceConfiguredCostKRW==='number'&&` · 설정 단가 기준 ${result.inferenceConfiguredCostKRW.toLocaleString('ko-KR')}원 (청구 확인 전)`}
  </p>}
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
      <div className="ml-auto flex flex-wrap gap-2">
       <button type="button" disabled={!!implementingId||!!draft.implementationJobId} onClick={()=>void reviewCandidate(draft,'deferred')}
        className="text-[11px] font-semibold text-[#8B8076] hover:text-[#A46144] disabled:opacity-50">보류</button>
       <button type="button" disabled={!!implementingId||!!draft.implementationJobId} onClick={()=>void reviewCandidate(draft,'rejected')}
        className="text-[11px] font-semibold text-[#8B8076] hover:text-red-700 disabled:opacity-50">제외</button>
       {draft.implementationJobId&&<span className="text-[11px] font-semibold text-[#6E7A5A]">
        구현 · {draft.implementationJob?.status||'기록 조회 필요'}
       </span>}
       {['queued','running'].includes(draft.implementationJob?.status||'')&&<button type="button"
        onClick={()=>void cancelImplementation(draft)} className="text-[11px] font-bold underline">구현 중단</button>}
       {draft.implementationJob?.status==='completed'&&draft.implementationJob.sessionId&&<button type="button"
        onClick={()=>props.openSession(draft.implementationJob!.sessionId!)}
        className="text-[11px] font-bold underline">결과 보기</button>}
       <button type="button" disabled={agentId==='db'||!!implementingId||!!draft.implementationJobId}
        onClick={()=>void approveImplementation(draft)}
        title={agentId==='db'?'에이전트를 먼저 선택하세요':'승인 후 로컬 프로토타입을 구현합니다'}
        className="inline-flex items-center gap-1 rounded-lg bg-[#6E7A5A] px-3 py-2 text-[11px] font-extrabold text-white disabled:opacity-40">
        <Code2 size={13}/>{implementingId===draft.id?'승인 처리 중…':draft.implementationJobId?'승인 기록됨':'승인 후 구현'}
       </button>
      </div>
     </div>
    </article>)}</div>:
    <div className="flex min-h-52 flex-col items-center justify-center rounded-2xl border border-dashed border-[#E5D8C8] bg-[#FFFCF8] px-6 py-10 text-center">
     <Lightbulb size={28} className="mb-3 text-[#BD9479]"/>
     <strong className="text-sm text-[#584637]">{loading?'기존 발굴 결과 불러오는 중…':result.message||'돈벌이 찾기를 눌러 초안을 만들어 보세요.'}</strong>
     <p className="mt-2 text-xs leading-6 text-[#95877A]">기존 DB에서 돈벌이 후보를 추리고, 선택한 에이전트가 있으면 심층 추론합니다.</p>
    </div>}
  </section>
  <section aria-label="보존 후보 및 원문 검토" className="mt-8 rounded-xl border border-[#E6D9C9] bg-[#FFFDF9] p-4 sm:p-5">
   <div className="flex flex-wrap items-center justify-between gap-2">
    <div><h2 className="text-lg font-extrabold">보류·과거 후보 다시 보기</h2>
     <p className="mt-1 text-[11px] leading-5 text-[#847366]">낮은 순위·제외 후보도 삭제하지 않고 남깁니다. 사용자 판단 이력과 수집된 원문을 검토할 수 있습니다.</p>
    </div>
    <button type="button" aria-expanded={archiveOpen} onClick={()=>setArchiveOpen(x=>!x)}
     className="rounded-lg border border-[#D8CAB9] px-3 py-2 text-xs font-bold">{archiveOpen?'목록 닫기':'보존 후보 열기'}</button>
   </div>
   {archiveOpen&&<>
    <form onSubmit={e=>{e.preventDefault();setArchiveOffset(0);setArchiveQuery(archiveInput.trim())}}
     className="mt-4 flex flex-wrap items-center gap-2">
     <select aria-label="보존 후보 상태" value={archiveStatus} onChange={e=>{setArchiveStatus(e.target.value);setArchiveOffset(0)}}
      className="min-h-10 rounded-lg border border-[#DAD0C3] bg-white px-2 text-xs">
      <option value="">전체 후보</option><option value="deferred">보류</option>
      <option value="rejected">제외</option><option value="hypothesis">가설</option>
      <option value="implementation_requested">구현 승인</option>
     </select>
     <input aria-label="보존 후보 검색어" value={archiveInput} onChange={e=>setArchiveInput(e.target.value)}
      placeholder="업종·고객·아이디어 검색" className="min-h-10 min-w-0 flex-1 rounded-lg border border-[#DAD0C3] bg-white px-3 text-xs"/>
     <button type="submit" className="min-h-10 rounded-lg bg-[#6E7A5A] px-4 text-xs font-bold text-white">검색</button>
    </form>
    {archiveError&&<p role="alert" className="mt-3 text-xs text-red-700">{archiveError}</p>}
    <p className="my-3 text-[11px] text-[#8D7C6D]">{archiveLoading?'보존 후보 조회 중…':`현재 페이지 ${archiveItems.length}건 · ${archiveOffset+1}번째부터 · 모델 호출 0회`}</p>
    <div className="space-y-3">
     {archiveItems.map(item=><article key={item.id} className="rounded-xl border border-[#EAE0D2] bg-[#FFFCF7] p-3">
      <div className="flex flex-wrap items-start justify-between gap-2">
       <div className="min-w-0 flex-1"><strong className="text-sm">{item.title||'제목 없는 가설'}</strong>
        <p className="mt-1 text-[11px] leading-5 text-[#6E6054]">{item.offer||'제안 내용 조사 필요'}</p>
       </div>
       <span className="text-[11px] font-semibold text-[#9D593D]">{item.status==='deferred'?'보류':item.status==='rejected'?'제외':item.status==='hypothesis'?'가설':item.status||'미분류'}</span>
      </div>
      {!!item.reviewReason&&<p className="mt-2 text-[11px] text-[#8E6F56]">마지막 판단: {item.reviewReason}</p>}
      <div className="mt-3 flex flex-wrap gap-3">
       <button type="button" onClick={()=>void loadReviewHistory(item.id)}
        className="text-[11px] font-bold text-[#8E6F56] underline">판단 이력 열기</button>
       {['deferred','rejected'].includes(item.status||'')&&<button type="button" onClick={()=>void reviewCandidate(item,'hypothesis')}
        className="text-[11px] font-bold text-[#6E7A5A] underline">다시 검토</button>}
       {item.status==='hypothesis'&&<button type="button" onClick={()=>void reviewCandidate(item,'deferred')}
        className="text-[11px] font-bold text-[#8E6F56] underline">보류</button>}
      </div>
      {reviewHistories[item.id]&&<div className="mt-2 rounded-lg bg-white p-2 text-[11px] leading-5">
       <strong>판단 변경 {reviewHistories[item.id].reviews.length}건 (최근 80건)</strong>
       {reviewHistories[item.id].reviews.map(r=><p key={r.id} className="mt-1 border-t border-[#E9DFD1] pt-1">
        {r.fromStatus||'가설'} → {r.toStatus||'가설'} · {r.reviewedAt||'시각 미상'} · {r.reason||'별도 사유 없음'}</p>)}
      </div>}
      <details className="mt-3 text-xs">
       <summary className="cursor-pointer font-bold text-[#8E6F56]">출처·원문·판단 내역</summary>
       <div className="mt-3 space-y-2">
        {(item.evidence||[]).slice(0,6).map((e,i)=>e.url?.startsWith('https://')&&<div key={i}
         className="rounded-lg border border-[#E9DFD1] bg-white p-3">
         <p className="text-[11px] leading-5">{e.claim||'제목·출처 연결 자료'} · {e.kind||'자료'} · {e.contentStatus||'원문 보존 상태 조회 필요'}</p>
         <div className="mt-1 flex flex-wrap gap-3">
          <a href={e.url} target="_blank" rel="noreferrer" className="text-[11px] font-semibold text-[#9D593D] underline">원본 링크 열기</a>
          <button type="button" onClick={()=>void revealStoredSource(e.url!)}
           className="text-[11px] font-semibold text-[#6E7A5A] underline">{sourceLoading===e.url?'조회 중…':'DB 보관 원문 보기'}</button>
         </div>
         {sourceViews[e.url]&&<div className="mt-2 rounded-lg bg-[#F6F0E6] p-2 text-[11px] leading-5">
          <p>보관 출처: {sourceViews[e.url!].source.sourceName||'미상'} · {sourceViews[e.url!].source.publishedAt||'발행일 미상'}</p>
          <p>저장 상태: {sourceViews[e.url!].sourceStatus}</p>
          <p className="mt-1 whitespace-pre-wrap">{sourceViews[e.url!].source.articleBody||sourceViews[e.url!].source.articleExcerpt||sourceViews[e.url!].source.description||'기사 본문 미보관 · 제목 또는 메타데이터만 저장됨'}</p>
          {!!sourceViews[e.url!].revisions?.length&&<details className="mt-2">
           <summary className="cursor-pointer">원문 리비전 {sourceViews[e.url!].revisions!.length}개</summary>
           {sourceViews[e.url!].revisions!.map(r=><p key={r.id} className="mt-1 whitespace-pre-wrap border-t border-[#DFD3C3] pt-1">
            {r.reason||'원문 기록'} · {r.capturedAt||'시각 미상'}: {r.content?.articleBody||r.content?.articleExcerpt||r.content?.description||'제목만 보관'}</p>)}
          </details>}
         </div>}
        </div>)}
        {!(item.evidence||[]).length&&<p className="text-[11px] text-[#8D7C6D]">이 후보에 연결된 원문 근거는 아직 없습니다.</p>}
        <p className="text-[11px] text-[#8D7C6D]">저장된 원문은 자료의 기록이며, 구매·거래 사실 확인을 의미하지 않습니다.</p>
       </div>
      </details>
     </article>)}
     {!archiveLoading&&!archiveItems.length&&<p className="py-3 text-center text-xs text-[#8D7C6D]">해당 조건의 보존 후보가 없습니다.</p>}
    </div>
    <div className="mt-4 flex items-center justify-end gap-3 text-xs">
     <button type="button" disabled={archiveOffset===0||archiveLoading} onClick={()=>setArchiveOffset(v=>Math.max(0,v-24))}
      className="rounded-lg border border-[#D8CAB9] px-3 py-2 disabled:opacity-40">이전 24건</button>
     <button type="button" disabled={archiveItems.length<24||archiveLoading} onClick={()=>setArchiveOffset(v=>v+24)}
      className="rounded-lg border border-[#D8CAB9] px-3 py-2 disabled:opacity-40">다음 24건</button>
    </div>
   </>}
  </section>
  {!!result.inferenceModel?.model&&<p className="mt-4 text-right text-[11px] font-medium text-[#786755]">
   추론 모델 · {result.inferenceModel.providerId||'기본 제공자'} / {result.inferenceModel.model}
  </p>}
  {result.searchedAt&&<p className="mt-5 text-right text-[10px] text-[#9B8E81]">
   마지막 발굴 {new Date(result.searchedAt).toLocaleString('ko-KR')}
  </p>}
 </main>
}
