import {useState} from 'react'
import {Activity,Clock3,Play,RefreshCw,Square,Terminal} from 'lucide-react'
import {post} from '../api'
import {Button,SelectInput,TextArea} from './Primitives'
import {Panel,useRemote} from './UI'
import {t,useLocale,jobKindLabel,type LocaleKey} from '../i18n'
import {translateError} from '../i18n/errors'

type Execution={
 id:string;sessionId:string;agentId?:string;providerId:string;model:string;
 status:string;startedAt:string;finishedAt?:string;reason?:string;
 iterations:number;modelCalls:number;toolCalls:number;tokens:number
}
type Agent={id:string;name:string;providerId:string;model:string}
type Operation={id:string;callId:string;tool:string;status:string;startedAt:string;reason?:string}
const labels:Record<string,LocaleKey>={running:'em.status.running',completed:'em.status.completed',cancelled:'em.status.cancelled',failed:'em.status.failed',interrupted:'em.status.interrupted',needs_review:'em.status.needs_review',budget_exceeded:'em.status.budget_exceeded'}
const active=(s:string)=>s==='running'
function ToolHistory({id}:{id:string}){
 const log=useRemote<Operation[]>('/api/executions/'+id+'/operations',[])
 return <div className="mt-3 max-h-44 space-y-2 overflow-y-auto">
  {log.value.map(o=><div key={o.id} className="flex items-center gap-2 rounded-lg bg-[#FAF7F1] px-3 py-2 text-xs">
   <Terminal size={13} className="shrink-0 text-[#7C7469]"/><span className="min-w-0 flex-1 truncate">{o.tool}</span>
   <span className="text-[#7C7469]">{o.status}</span>
  </div>)}
  {!log.value.length&&<p className="text-xs text-[#9C9388]">{t('em.noTools')}</p>}
 </div>
}
export function ExecutionMonitor(){
 useLocale()
 const runs=useRemote<Execution[]>('/api/executions',[])
 const jobs=useRemote<any[]>('/api/jobs',[])
 const [autonomous,setAutonomous]=useState(true)
 const [attempts,setAttempts]=useState(3)
 const [strategy,setStrategy]=useState<'focused'|'conservative'>('focused')
 const agents=useRemote<Agent[]>('/api/agents',[])
 const [prompt,setPrompt]=useState(''),[agentChoice,setAgentChoice]=useState(''),[info,setInfo]=useState('')
 const [selected,setSelected]=useState<string|null>(null),[busy,setBusy]=useState(false),[error,setError]=useState('')
 const availableAgents=agents.value.filter(x=>x.providerId&&x.model)
 const executorId=availableAgents.some(x=>x.id===agentChoice)?agentChoice:availableAgents[0]?.id||''
 const current=runs.value.find(x=>x.id===selected)||runs.value[0]
 const count=runs.value.filter(x=>active(x.status)).length
 const execute=async()=>{
  if(!prompt.trim()||!executorId||busy)return
  setBusy(true);setError('');setInfo('')
  try{
   await post('/api/jobs',{kind:'prompt',input:{
    prompt:prompt.trim(),agentId:executorId,autonomous,
    maxAutonomousAttempts:attempts,executionStrategy:strategy
   }})
   setPrompt('');setInfo(t('em.queued'))
   await runs.reload();await jobs.reload()
  }catch(e){setError((e as Error).message)}
  finally{setBusy(false)}
 }
 const cancel=async()=>{
  if(!current)return
  setBusy(true);setError('')
  try{await post('/api/executions/'+current.id+'/cancel',{});await runs.reload()}
  catch(e){setError((e as Error).message)}
  finally{setBusy(false)}
 }
 return <Panel className="mb-8 overflow-hidden">
  <div className="flex flex-wrap items-center justify-between gap-3 border-b border-[#E7E0D5] px-5 py-4">
   <div className="flex items-center gap-3"><span className="grid size-10 place-items-center rounded-xl bg-[#24211D] text-white"><Activity size={20}/></span>
    <div><h2 className="m-0 text-lg font-bold text-[#24211D]">{t('em.title')}</h2><p className="m-0 text-xs text-[#7C7469]">{t('em.subtitle')}</p></div></div>
   <div className="flex items-center gap-3"><span className="text-xs font-semibold text-[#5C544A]">{t('em.count',{count})}</span>
    <Button size="sm" variant="ghost" onClick={()=>{void runs.reload();void jobs.reload()}}><RefreshCw size={14}/> {t('em.refresh')}</Button></div>
  </div>
  <div className="border-b border-[#E7E0D5] p-5">
   <label className="mb-2 block text-sm font-semibold text-[#3A342D]">{t('em.instruction')}</label>
   <TextArea rows={3} value={prompt} onChange={e=>setPrompt(e.target.value)} placeholder={t('em.placeholder')}/>
   <div className="mt-3 flex flex-wrap items-center gap-2">
    <SelectInput value={executorId} onChange={e=>setAgentChoice(e.target.value)} aria-label={t('em.agentAria')}>
     {!availableAgents.length&&<option value="">{t('em.noAgents')}</option>}
     {availableAgents.map(a=><option key={a.id} value={a.id}>{a.name} · {a.model}</option>)}
    </SelectInput>
    <Button variant="primary" disabled={busy||!executorId||!prompt.trim()} onClick={()=>void execute()}><Play size={15}/> {t('em.run')}</Button>
   </div>
   <details className="mt-3 rounded-lg border border-[#E7E0D5] bg-[#FAF7F1] px-3 py-2 text-xs">
    <summary className="cursor-pointer font-medium text-[#5C544A]">자율 실행 · 검증 및 수정 설정</summary>
    <div className="mt-3 flex flex-wrap items-center gap-4">
     <label className="flex items-center gap-2"><input type="checkbox" checked={autonomous} onChange={e=>setAutonomous(e.target.checked)}/> 검증 후 자동 수정</label>
     <label className="flex items-center gap-2">최대 시도 <select className="rounded border bg-white px-2 py-1" value={attempts} disabled={!autonomous} onChange={e=>setAttempts(Number(e.target.value))}>{[1,2,3,4,5,6].map(n=><option key={n} value={n}>{n}회</option>)}</select></label>
     <label className="flex items-center gap-2">실행 전략 <select className="rounded border bg-white px-2 py-1" value={strategy} disabled={!autonomous} onChange={e=>setStrategy(e.target.value as 'focused'|'conservative')}><option value="focused">검증 기반 수정</option><option value="conservative">보수적 복구</option></select></label>
    </div>
    <p className="mt-2 text-[#7C7469]">검증할 결과가 남으면 Rust 엔진이 수정 과정을 이어갑니다. 외부 작업 결과가 불확실하면 자동 반복하지 않습니다.</p>
   </details>
   {info&&<p className="mt-2 text-xs text-emerald-700">{info}</p>}
  </div>
  {jobs.value.some(j=>j.kind==='prompt')&&<div className="border-b border-[#E7E0D5] px-5 py-3">
   <h3 className="mb-2 text-xs font-bold text-[#3A342D]">최근 자율 작업 · 실제 검증 기록</h3>
   <div className="grid gap-2">{jobs.value.filter(j=>j.kind==='prompt').slice(-5).reverse().map(j=><details key={j.id} className="rounded-lg border border-[#E7E0D5] bg-white px-3 py-2 text-xs">
    <summary className="flex cursor-pointer flex-wrap items-center gap-2"><span className="min-w-0 flex-1 truncate font-medium">{j.input?.prompt||j.id}</span><span>{j.status}</span><span className="text-[#7C7469]">{j.autonomousState?.attempt||0} / {j.input?.maxAutonomousAttempts||3}회</span></summary>
    <div className="mt-2 space-y-1 text-[#5C544A]">
     {(j.autonomousState?.receipts||j.result?.receipts||[]).map((r:any)=><div key={r.attempt} className="rounded bg-[#FAF7F1] p-2">
      <strong>{r.attempt}차</strong> · {r.outcome==='accepted'?'결과 수락':r.outcome==='repair'?'재검증 후 수정':'검토 필요'} · 성공 도구 {r.toolSuccess?.length||0}개
      {(r.gaps||[]).map((gap:string,i:number)=><p key={i} className="mt-1 break-words text-[#A94E30]">{gap}</p>)}
     </div>)}
     {j.result?.verified===true&&<p className="text-emerald-700">독립 검증 조건 통과</p>}
     {j.result?.status==='needs_review'&&<p className="text-amber-800">검토가 필요한 작업입니다: {j.result?.reason||'검증 미완료'}</p>}
     {['queued','running'].includes(j.status)&&<button type="button" className="rounded border border-rose-200 px-2 py-1 text-rose-700" onClick={async()=>{await post('/api/jobs/'+encodeURIComponent(j.id)+'/cancel',{});await jobs.reload()}}>작업 중단</button>}
    </div>
   </details>)}</div>
  </div>}
  <div className="grid min-w-0 gap-4 p-5 lg:grid-cols-2">
   <div className="max-h-64 space-y-2 overflow-y-auto">
    {runs.value.map(r=><Button key={r.id} variant="ghost" onClick={()=>setSelected(r.id)}
     className={'!flex !w-full !items-center !justify-between !gap-2 !rounded-lg !border !px-3 !py-3 !text-left '+(current?.id===r.id?'!border-blue-300 !bg-[#F7E8E1]':'!border-[#E7E0D5]')}>
      <span className="min-w-0"><strong className="block truncate text-xs text-[#3A342D]">{r.model}</strong>
       <small className="block truncate text-[11px] text-[#7C7469]">{r.sessionId}</small></span>
      <span className="shrink-0 text-xs font-medium text-[#5C544A]">{labels[r.status]?t(labels[r.status]):r.status}</span>
    </Button>)}
    {!runs.value.length&&<p className="py-8 text-center text-sm text-[#7C7469]">{t('em.empty')}</p>}
   </div>
   <div className="min-w-0 border-t border-[#E7E0D5] pt-4 lg:border-l lg:border-t-0 lg:pl-5 lg:pt-0">
    {current?<><div className="flex items-center justify-between gap-2"><h3 className="m-0 text-sm font-bold text-[#3A342D]">{t('em.detail')}</h3>
     <span className="text-xs font-semibold text-[#5C544A]">{labels[current.status]?t(labels[current.status]):current.status}</span></div>
     <div className="mt-3 grid grid-cols-3 gap-2 rounded-lg bg-[#FAF7F1] p-3 text-center">
      <div><strong className="block text-lg">{current.modelCalls||0}</strong><small className="text-[10px]">{t('em.modelCalls')}</small></div>
      <div><strong className="block text-lg">{current.toolCalls||0}</strong><small className="text-[10px]">{t('em.toolCalls')}</small></div>
      <div><strong className="block text-lg">{current.tokens||0}</strong><small className="text-[10px]">{t('em.tokens')}</small></div>
     </div>
     <p className="mt-3 flex items-center gap-2 text-xs text-[#7C7469]"><Clock3 size={13}/>{current.startedAt?.replace('T',' ').slice(0,19)} UTC</p>
     {current.reason&&<p role="alert" className="mt-2 break-words rounded-lg bg-amber-50 p-3 text-xs text-amber-800">{translateError(current.reason)}</p>}
     {active(current.status)&&<Button variant="danger" disabled={busy} onClick={()=>void cancel()}><Square size={14}/> {t('em.cancel')}</Button>}
     <ToolHistory key={current.id} id={current.id}/>
    </>:<p className="text-sm text-[#7C7469]">{t('em.idle')}</p>}
   </div>
  </div>
  {error&&<p role="alert" className="px-5 pb-4 text-xs text-rose-600">{translateError(error)}</p>}
 </Panel>
}
