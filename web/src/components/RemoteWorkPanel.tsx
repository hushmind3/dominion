import {useEffect,useState} from 'react'
import {Check,ChevronDown,ChevronRight,Cloud,Copy,FileDiff,KeyRound,LoaderCircle,RefreshCw,ShieldAlert} from 'lucide-react'
import {get,post} from '../api'
import {useLocale} from '../i18n'
import {Panel,useRemote} from './UI'

type Worker={id:string;status:string;capabilities?:string[];lastSeenAt?:string|null;registeredAt?:string}
type RemoteJob={id:string;status:string;instruction?:string;verifyCommand?:string;createdAt?:string;
 workerId?:string|null;targetWorkerId?:string|null;requiredCapabilities?:string[];reviewReason?:string;
 workerReport?:unknown;artifactId?:string|null;artifactSha256?:string|null;nativeVerified?:boolean;
 attempts?:number;finishedAt?:string|null}
type Registration={worker:Worker;token:string}
type Manifest={id:string;jobId:string;workerId?:string;baseCommit?:string;patchSha256?:string;
 patchBytes?:number;status:string;nativeVerified?:boolean}
type PatchResponse={manifest:Manifest;patch:string}
const statusTone=(status:string)=>status==='needs_review'?'border-amber-200 bg-amber-50 text-amber-800':
 status==='owner_accepted'?'border-emerald-200 bg-emerald-50 text-emerald-800':
 status==='owner_rejected'||status==='cancelled'?'border-rose-200 bg-rose-50 text-rose-700':
 'border-divider bg-[var(--color-canvas)] text-[var(--color-muted)]'
const stamped=(value?:string|null)=>value?new Date(value).toLocaleString():'—'
function PatchReview({jobId,jobStatus,onChanged}:{jobId:string;jobStatus:string;onChanged:()=>void}){
 const en=useLocale()==='en'
 const [open,setOpen]=useState(false)
 const [patch,setPatch]=useState<PatchResponse|null>(null)
 const [loading,setLoading]=useState(false)
 const [busy,setBusy]=useState(false)
 const [error,setError]=useState('')
 const inspect=async()=>{
  setLoading(true);setError('')
  try{
   const evidence=await get<PatchResponse>('/api/remote/jobs/'+encodeURIComponent(jobId)+'/artifact')
   if(evidence.manifest.jobId!==jobId)throw Error(en?'Patch belongs to another job.':'패치의 원본 작업 ID가 일치하지 않습니다.')
   setPatch(evidence)
  }
  catch(e){setError((e as Error).message)}
  finally{setLoading(false)}
 }
 const decide=async(accept:boolean)=>{
  if(!patch||busy)return
  if(accept&&!window.confirm(en
   ?'Accept this patch for owner review? This does NOT apply or merge it.'
   :'이 패치를 소유자 검토에서 승인하시겠습니까? 파일 적용·병합은 수행되지 않습니다.'))return
  setBusy(true);setError('')
  try{
   const updated=await post<Manifest>('/api/remote/jobs/'+encodeURIComponent(jobId)+'/artifact/review',{accept})
   setPatch({...patch,manifest:updated})
   onChanged()
  }catch(e){setError((e as Error).message)}
  finally{setBusy(false)}
 }
 return <div className="mt-2 rounded-lg border border-divider bg-[var(--color-surface)]">
  <button type="button" aria-expanded={open} onClick={()=>{setOpen(v=>!v);if(!open&&!patch)void inspect()}}
   className="flex w-full items-center justify-between gap-2 px-3 py-2 text-[11px] font-semibold text-[var(--color-muted)]">
   <span className="flex items-center gap-1.5"><FileDiff size={13}/>{en?'Remote patch evidence':'원격 패치 검토'}</span>
   {open?<ChevronDown size={13}/>:<ChevronRight size={13}/>}
  </button>
  {open&&<div className="border-t border-divider p-3">
   {loading&&<p role="status" className="text-[11px]">{en?'Loading patch…':'패치 불러오는 중…'}</p>}
   {error&&<p role="alert" className="mb-2 break-words text-[11px] text-rose-700">{error}</p>}
   {!loading&&patch&&<>
    <div className="mb-2 flex flex-wrap gap-2 text-[10px] text-muted">
     <span>{patch.manifest.status}</span>
     <span>{patch.manifest.patchBytes?.toLocaleString()??'—'} bytes</span>
     <span>SHA256: {patch.manifest.patchSha256||'—'}</span>
     <span>Base: {patch.manifest.baseCommit||'—'}</span>
    </div>
    <pre tabIndex={0} className="max-h-80 overflow-auto whitespace-pre-wrap break-all rounded-lg border border-[var(--color-divider)] bg-white p-3 font-mono text-[11px] leading-5">{patch.patch}</pre>
    <p className="mt-2 text-[10px] leading-4 text-amber-800">{en
     ?'Untrusted external patch. Accepting the review does not apply changes, prove correctness or make the native verifier succeed.'
     :'외부 패치는 신뢰되지 않는 자료입니다. 승인해도 파일에 적용·병합되지 않으며 네이티브 검증 완료로 처리되지 않습니다.'}</p>
    {jobStatus==='owner_accepted'&&patch.manifest.status==='quarantined'&&<div className="mt-3 flex flex-wrap gap-2">
     <button type="button" disabled={busy} onClick={()=>void decide(true)}
      className="rounded-lg border border-[#D6B99D] bg-[var(--color-brand-soft)] px-3 py-2 text-[11px] font-semibold text-[var(--color-brand-hover)] disabled:opacity-50">{en?'Accept patch for review':'패치 검토 승인'}</button>
     <button type="button" disabled={busy} onClick={()=>void decide(false)}
      className="rounded-lg border border-rose-200 px-3 py-2 text-[11px] text-rose-700 disabled:opacity-50">{en?'Reject patch':'패치 거절'}</button>
    </div>}
   </>}
   {!loading&&!patch&&<button type="button" onClick={()=>void inspect()} className="text-[11px] underline">{en?'Retry loading':'다시 불러오기'}</button>}
  </div>}
 </div>
}
/** Owner-only remote control surface. All sensitive worker token receipts
 * exist in React memory only, are not logged, and are not written to storage. */
export function RemoteWorkPanel(){
 const en=useLocale()==='en'
 const [open,setOpen]=useState(false)
 const workers=useRemote<Worker[]>(open?'/api/remote/workers':null,[],false)
 const jobs=useRemote<RemoteJob[]>(open?'/api/remote/jobs':null,[],false)
 const [workerName,setWorkerName]=useState('')
 const [workerCaps,setWorkerCaps]=useState('sdk')
 const [secret,setSecret]=useState<Registration|null>(null)
 const [instruction,setInstruction]=useState('')
 const [verifyCommand,setVerifyCommand]=useState('')
 const [target,setTarget]=useState('')
 const [requiredCaps,setRequiredCaps]=useState('')
 const [selected,setSelected]=useState('')
 const [busy,setBusy]=useState(false)
 const [error,setError]=useState('')
 const [notice,setNotice]=useState('')
 const [showWorkers,setShowWorkers]=useState(false)
 const [showCreate,setShowCreate]=useState(false)
 const selectedJob=jobs.value.find(job=>job.id===selected)||jobs.value[0]
 const refreshAll=()=>{void Promise.all([workers.reload(),jobs.reload()]).catch(()=>{})}
 useEffect(()=>{
  if(!open)return
  const timer=window.setInterval(()=>{void workers.reload();void jobs.reload()},8000)
  return()=>window.clearInterval(timer)
 },[open,workers.reload,jobs.reload])
 const act=async(task:()=>Promise<unknown>,message:string)=>{
  if(busy)return
  setBusy(true);setError('');setNotice('')
  try{await task();setNotice(message);refreshAll()}
  catch(e){setError((e as Error).message)}
  finally{setBusy(false)}
 }
 const register=()=>{
  const caps=workerCaps.split(',').map(s=>s.trim()).filter(Boolean)
  void act(async()=>{
   const record=await post<Registration>('/api/remote/workers',{workerId:workerName.trim(),capabilities:caps})
   setSecret(record);setWorkerName('')
  },en?'Worker registered; copy the one-time credential.':'워커 등록 완료. 1회 발급된 인증키를 복사하세요.')
 }
 const rotate=(worker:Worker)=>{
  if(!window.confirm(en?'Rotate this worker credential? Its previous token will stop working.':'이 워커의 인증키를 재발급하시겠습니까? 기존 키는 즉시 무효화됩니다.'))return
  void act(async()=>{
   const response=await post<Registration>('/api/remote/workers/'+encodeURIComponent(worker.id)+'/rotate',{})
   setSecret(response)
  },en?'Credential rotated. Copy the new token now.':'인증키 재발급 완료. 새 키를 지금 복사하세요.')
 }
 const submit=()=>{
  void act(async()=>{
   await post('/api/remote/jobs',{
    instruction:instruction.trim(),verifyCommand:verifyCommand.trim()||undefined,
    targetWorkerId:target||undefined,
    requiredCapabilities:requiredCaps.split(',').map(s=>s.trim()).filter(Boolean)
   })
   setInstruction('');setVerifyCommand('');setSelected('')
  },en?'Remote task queued.':'원격 작업을 대기열에 등록했습니다.')
 }
 const review=(job:RemoteJob,accept:boolean)=>{
  if(accept&&!window.confirm(en?'Accept this remote worker report as owner-reviewed? It is NOT native-verified.':'원격 워커 보고서를 소유자 검토로 승인하시겠습니까? 네이티브 검증으로 인정되지는 않습니다.'))return
  void act(()=>post('/api/remote/jobs/'+encodeURIComponent(job.id)+'/review',{accept}),
   en?'Review recorded.':'검토 결과를 기록했습니다.')
 }
 const cancel=(job:RemoteJob)=>{
  if(!window.confirm(en?'Cancel this remote task?':'이 원격 작업을 중단하시겠습니까?'))return
  void act(()=>post('/api/remote/jobs/'+encodeURIComponent(job.id)+'/cancel',{}),
   en?'Remote cancellation requested.':'원격 작업 중단을 요청했습니다.')
 }
 return <Panel className="mt-5 min-w-0 overflow-hidden">
  <button type="button" aria-expanded={open} onClick={()=>setOpen(v=>!v)}
   className="flex w-full items-center justify-between gap-3 p-4 text-left hover:bg-[var(--color-canvas)]">
   <span className="flex items-center gap-3"><Cloud size={18} className="text-[var(--color-brand)]"/>
    <span><strong className="block text-[13px]">{en?'Remote workers & evidence':'원격 워커·작업·결과물'}</strong>
     <small className="mt-1 block text-[11px] text-[var(--color-muted)]">{en?'Review off-device execution from the existing activity screen.':'기존 작업 화면에서 외부 장치의 실행과 결과물을 관리합니다.'}</small></span>
   </span>
   {open?<ChevronDown size={16}/>:<ChevronRight size={16}/>}
  </button>
  {open&&<div className="space-y-4 border-t border-divider p-4">
   <div className="flex flex-wrap items-center justify-between gap-2">
    <p className="text-[11px] text-[var(--color-muted)]">{en?'Owner-only access. Workers cannot grant themselves native verification.':'소유자 권한이 필요합니다. 워커 보고가 네이티브 검증을 대신하지 않습니다.'}</p>
    <button type="button" onClick={refreshAll} disabled={busy}
     className="inline-flex items-center gap-1 rounded-lg border border-divider px-2.5 py-1.5 text-[11px]"><RefreshCw size={13}/>{en?'Refresh':'새로고침'}</button>
   </div>
   <details className="rounded-xl border border-[var(--color-divider)] bg-[var(--color-surface)] p-3">
    <summary className="cursor-pointer text-[11px] font-semibold text-[var(--color-muted)]">{en?'How to connect a remote machine':'원격 장치 연결 방법'}</summary>
    <p className="mt-2 text-[11px] leading-5 text-muted">{en
     ?'Set DOMINION_ADMIN_TOKEN on the coordinator and enter the same owner token in Settings → Advanced → Admin token. Register a worker here, then configure its one-time worker credential only on that remote machine. Use HTTPS for a network coordinator.'
     :'코디네이터에 DOMINION_ADMIN_TOKEN을 설정하고, 기존 설정 → 고급 설정 → 관리자 토큰에 같은 값을 입력하세요. 여기서 워커를 등록한 다음, 1회 발급된 워커 키는 원격 장치에만 설정합니다. 외부 네트워크 연결은 HTTPS를 사용합니다.'}</p>
    <pre className="mt-2 overflow-x-auto rounded-lg bg-white p-3 text-[10px] leading-5">{'export DOMINION_REMOTE_COORDINATOR="https://your-coordinator.example"\nexport DOMINION_REMOTE_WORKER_ID="registered-worker-id"\nexport DOMINION_REMOTE_WORKER_TOKEN="the-one-time-worker-token"\nexport DOMINION_WORKER_LOCAL_URL="http://127.0.0.1:4317"\ncargo run --locked --bin dominion-worker'}</pre>
    <p className="mt-1 text-[10px] text-[var(--color-muted)]">{en?'The remote machine must already be running its own DOMINION Rust server and configured model.':'원격 장치에는 별도의 도미니언 Rust 서버와 사용할 모델이 준비돼 있어야 합니다.'}</p>
   </details>
   {workers.error&&<p role="alert" className="text-[11px] text-rose-700">{en?'Worker records: ':'워커 기록: '}{workers.error}</p>}
   {jobs.error&&<p role="alert" className="text-[11px] text-rose-700">{en?'Remote jobs: ':'원격 작업: '}{jobs.error}</p>}
   {error&&<p role="alert" className="break-words text-[11px] text-rose-700">{error}</p>}
   {notice&&<p role="status" className="flex items-center gap-1 text-[11px] text-emerald-700"><Check size={13}/>{notice}</p>}
   {secret&&<div className="rounded-xl border border-amber-300 bg-amber-50 p-3">
    <div className="flex gap-2 text-[12px] font-semibold text-amber-900"><KeyRound size={15}/>{en?'One-time worker credential':'워커 인증키 · 1회 발급'} ({secret.worker.id})</div>
    <p className="mt-1 text-[10px] text-amber-800">{en?'Copy now and provision it on the worker. It will not be shown again.':'지금 복사해 워커 장치에 설정하세요. 이 키는 다시 조회할 수 없습니다.'}</p>
    <div className="mt-2 flex flex-wrap gap-2">
     <code className="min-w-0 flex-1 select-all break-all rounded-lg bg-white p-2 text-[11px]">{secret.token}</code>
     <button type="button" onClick={()=>void navigator.clipboard.writeText(secret.token).catch(e=>setError(String(e)))}
      className="flex items-center gap-1 rounded-lg border border-amber-300 bg-white px-3 py-2 text-[11px]"><Copy size={12}/>{en?'Copy':'복사'}</button>
     <button type="button" onClick={()=>setSecret(null)}
      className="rounded-lg border border-amber-300 px-3 py-2 text-[11px]">{en?'Dismiss':'확인·닫기'}</button>
    </div>
   </div>}
   <section className="rounded-xl border border-divider bg-white p-3">
    <button type="button" className="flex w-full justify-between text-left text-[12px] font-bold" aria-expanded={showWorkers} onClick={()=>setShowWorkers(v=>!v)}>
     {en?'Registered workers':'등록된 워커'} ({workers.value.length}) {showWorkers?'▴':'▾'}
    </button>
    {showWorkers&&<div className="mt-3 space-y-2">
     <form onSubmit={event=>{event.preventDefault();register()}} className="flex flex-wrap gap-2">
      <input value={workerName} maxLength={96} onChange={e=>setWorkerName(e.target.value)}
       placeholder={en?'Worker ID (unique)':'워커 ID (고유)'} aria-label={en?'Worker ID':'워커 ID'}
       className="min-w-40 flex-1 rounded-lg border border-divider p-2 text-[11px]"/>
      <input value={workerCaps} onChange={e=>setWorkerCaps(e.target.value)}
       placeholder="sdk,gpu" aria-label={en?'Worker capabilities':'워커 기능 태그'}
       className="min-w-28 flex-1 rounded-lg border border-divider p-2 text-[11px]"/>
      <button type="submit" disabled={!workerName.trim()||busy}
       className="rounded-lg bg-[var(--color-brand)] px-3 py-2 text-[11px] font-semibold text-white disabled:opacity-50">{en?'Register':'워커 등록'}</button>
     </form>
     {workers.value.map(worker=><div key={worker.id} className="flex min-w-0 flex-wrap items-center justify-between gap-2 rounded-lg bg-[var(--color-canvas)] px-3 py-2 text-[11px]">
      <div className="min-w-0"><strong className="block break-all">{worker.id}</strong>
       <p className="text-[10px] text-[var(--color-muted)]">{worker.status} · {(worker.capabilities||[]).join(', ')||'—'} · {en?'Last seen':'최근 연결'}: {stamped(worker.lastSeenAt)}</p>
      </div>
      <div className="flex flex-wrap gap-1.5">
       <button type="button" disabled={busy} onClick={()=>void act(
        ()=>post('/api/remote/workers/'+encodeURIComponent(worker.id)+'/state',{enabled:worker.status!=='enabled'}),
        en?'Worker state updated.':'워커 상태를 변경했습니다.')}
        className="rounded-lg border border-divider px-2 py-1.5 disabled:opacity-50">{worker.status==='enabled'?(en?'Disable':'비활성화'):(en?'Enable':'활성화')}</button>
       <button type="button" disabled={busy} onClick={()=>rotate(worker)}
        className="rounded-lg border border-divider px-2 py-1.5 disabled:opacity-50">{en?'Rotate key':'키 재발급'}</button>
      </div>
     </div>)}
     {!workers.value.length&&!workers.loading&&<p className="text-[11px] text-[var(--color-muted)]">{en?'No workers registered.':'등록된 워커가 없습니다.'}</p>}
    </div>}
   </section>
   <section className="rounded-xl border border-divider bg-white p-3">
    <button type="button" className="flex w-full justify-between text-left text-[12px] font-bold" aria-expanded={showCreate} onClick={()=>setShowCreate(v=>!v)}>
     {en?'Submit remote task':'원격 작업 제출'} {showCreate?'▴':'▾'}
    </button>
    {showCreate&&<form onSubmit={e=>{e.preventDefault();submit()}} className="mt-3 grid gap-2">
     <textarea rows={3} maxLength={16000} value={instruction} onChange={e=>setInstruction(e.target.value)}
      aria-label={en?'Remote instruction':'원격 작업 지시'} placeholder={en?'Describe a read-only remote task…':'원격 워커에 맡길 작업을 입력하세요.'}
      className="w-full rounded-lg border border-divider p-3 text-[11px]"/>
     <div className="grid gap-2 sm:grid-cols-2">
      <label className="text-[11px]">{en?'Target worker':'대상 워커'}
       <select value={target} onChange={e=>setTarget(e.target.value)} className="mt-1 block w-full rounded-lg border border-divider p-2">
        <option value="">{en?'Any eligible worker':'조건에 맞는 워커 자동 선택'}</option>
        {workers.value.filter(w=>w.status==='enabled').map(w=><option key={w.id} value={w.id}>{w.id}</option>)}
       </select>
      </label>
      <label className="text-[11px]">{en?'Capability tags (comma-separated)':'필요 기능 태그 (쉼표로 구분)'}
       <input value={requiredCaps} onChange={e=>setRequiredCaps(e.target.value)} placeholder="sdk,gpu" className="mt-1 block w-full rounded-lg border border-divider p-2"/>
      </label>
     </div>
     <input value={verifyCommand} maxLength={2048} onChange={e=>setVerifyCommand(e.target.value)}
      placeholder={en?'Optional verification command':'선택: 검증 명령어'} aria-label={en?'Verification command':'검증 명령어'}
      className="rounded-lg border border-divider p-2 text-[11px]"/>
     <button type="submit" disabled={busy||!instruction.trim()}
      className="justify-self-start rounded-lg bg-[var(--color-brand)] px-4 py-2 text-[11px] font-semibold text-white disabled:opacity-50">{en?'Queue remote task':'작업 예약'}</button>
    </form>}
   </section>
   <section className="rounded-xl border border-divider bg-white p-3">
    <div className="flex flex-wrap items-center justify-between gap-2">
     <strong className="text-[12px]">{en?'Remote job ledger':'원격 작업 기록'} ({jobs.value.length})</strong>
     <button type="button" disabled={busy} onClick={()=>void act(()=>post('/api/remote/reconcile',{}),en?'Expired leases reconciled.':'만료된 임대를 검토 상태로 반영했습니다.')}
      className="rounded-lg border border-divider px-2 py-1.5 text-[10px] disabled:opacity-50">{en?'Check expired leases':'만료 임대 확인'}</button>
    </div>
    <div className="mt-2 grid max-h-44 gap-1 overflow-y-auto">
     {jobs.value.map(job=><button type="button" key={job.id} aria-pressed={selectedJob?.id===job.id} onClick={()=>setSelected(job.id)}
      className={'flex min-w-0 justify-between gap-2 rounded-lg border px-2.5 py-2 text-left text-[11px] '+(selectedJob?.id===job.id?'border-[#B78C70] bg-[var(--color-brand-soft)]':'border-divider bg-white hover:bg-[var(--color-canvas)]')}>
      <span className="min-w-0 flex-1 truncate">{job.instruction||job.id}</span>
      <span className="shrink-0 text-[10px]">{job.status}</span>
     </button>)}
     {!jobs.value.length&&!jobs.loading&&<p className="py-3 text-[11px] text-[var(--color-muted)]">{en?'No remote tasks.':'원격 작업 기록이 없습니다.'}</p>}
    </div>
    {selectedJob&&<div className="mt-3 border-t border-[var(--color-divider)] pt-3">
     <div className="flex flex-wrap items-center gap-2">
      <strong className="break-all text-[11px]">{selectedJob.id}</strong>
      <span className={'rounded border px-2 py-0.5 text-[10px] '+statusTone(selectedJob.status)}>{selectedJob.status}</span>
     </div>
     <p className="mt-2 whitespace-pre-wrap break-words text-[12px]">{selectedJob.instruction}</p>
     <div className="mt-2 text-[10px] text-[var(--color-muted)]">{en?'Worker':'워커'}: {selectedJob.workerId||selectedJob.targetWorkerId||'—'} · {en?'Created':'생성'}: {stamped(selectedJob.createdAt)}
      {selectedJob.reviewReason&&<p className="mt-1 text-amber-800">{selectedJob.reviewReason}</p>}</div>
     {selectedJob.workerReport!=null&&<details className="mt-2 rounded-lg border border-divider p-2">
      <summary className="cursor-pointer text-[11px] font-semibold">{en?'Worker-reported evidence (not verified)':'워커 보고 증거 (미검증)'}</summary>
      <pre className="mt-2 max-h-40 overflow-auto whitespace-pre-wrap break-all text-[10px]">{JSON.stringify(selectedJob.workerReport,null,2)}</pre>
     </details>}
     <div className="mt-2 flex flex-wrap gap-2">
      {selectedJob.status==='needs_review'&&<>
       <button type="button" disabled={busy||!selectedJob.workerReport} onClick={()=>review(selectedJob,true)}
        className="rounded-lg border border-[var(--color-brand)] px-3 py-1.5 text-[11px] text-[var(--color-brand-hover)] disabled:opacity-50">{en?'Accept report':'보고서 승인'}</button>
       <button type="button" disabled={busy} onClick={()=>review(selectedJob,false)}
        className="rounded-lg border border-rose-200 px-3 py-1.5 text-[11px] text-rose-700 disabled:opacity-50">{en?'Reject report':'보고서 거절'}</button>
      </>}
      {['queued','leased','needs_review'].includes(selectedJob.status)&&<button type="button" disabled={busy} onClick={()=>cancel(selectedJob)}
       className="rounded-lg border border-divider px-3 py-1.5 text-[11px] disabled:opacity-50">{en?'Cancel':'작업 중단'}</button>}
     </div>
     {selectedJob.artifactId&&<PatchReview key={selectedJob.id} jobId={selectedJob.id} jobStatus={selectedJob.status} onChanged={refreshAll}/>}
    </div>}
   </section>
  </div>}
 </Panel>
}
