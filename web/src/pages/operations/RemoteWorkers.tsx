import {useCallback,useEffect,useRef,useState} from 'react'
import {Cpu,RefreshCw,Send,Square,Wifi,WifiOff} from 'lucide-react'
import {ApiError,get,post} from '../../api'
import {formatDateTime,useLocale} from '../../i18n'
import {Action,Notice,Panel} from '../../components/UI'
import {StatusPill} from '../../components/StatusPill'

export type RemoteAvailability={enabled:boolean;online:number;loaded:boolean}
type Worker={workerId:string;model:string;capabilities:string[];lastSeenAt?:string;online:boolean}
type Task={id:string;status:string;phase?:string;workerId?:string|null;
 queuedAt?:string;startedAt?:string;finishedAt?:string;
 evidenceGrade?:string;result?:{answer?:string;usage?:unknown}|null}
const control='w-full min-w-0 rounded-lg border border-[#E7E0D5] bg-white px-3 py-2 text-[12px] outline-none focus:border-[#AB795C]'
const action='inline-flex items-center gap-1.5 rounded-lg border border-[#E7E0D5] px-3 py-2 text-[11px] font-semibold hover:bg-[#FAF7F1] disabled:opacity-40'
const unsettled=new Set(['queued','running'])

export function RemoteWorkersPanel({onAvailability}:{
 onAvailability:(next:RemoteAvailability)=>void
}){
 const en=useLocale()==='en'
 const t=(ko:string,english:string)=>en?english:ko
 const [expanded,setExpanded]=useState(false)
 const [workers,setWorkers]=useState<Worker[]>([])
 const [tasks,setTasks]=useState<Task[]>([])
 const [enabled,setEnabled]=useState(false)
 const [loaded,setLoaded]=useState(false)
 const [prompt,setPrompt]=useState('')
 const [tokens,setTokens]=useState(2048)
 const [selected,setSelected]=useState('')
 const [busy,setBusy]=useState(false)
 const [error,setError]=useState('')
 const [notice,setNotice]=useState('')
 const refreshSequence=useRef(0)
 const refresh=useCallback(async()=>{
  const version=++refreshSequence.current
  try{
   const machines=await get<Worker[]>('/api/remote/workers')
   const jobs=await get<Task[]>('/api/remote/tasks')
   if(version!==refreshSequence.current)return
   setWorkers(machines)
   setTasks(jobs)
   setEnabled(true)
   setLoaded(true)
   setError('')
   onAvailability({enabled:true,online:machines.filter(w=>w.online).length,loaded:true})
  }catch(cause){
   if(version!==refreshSequence.current)return
   if(cause instanceof ApiError && cause.status===503){
    setEnabled(false);setLoaded(true);setWorkers([]);setTasks([])
    onAvailability({enabled:false,online:0,loaded:true})
    setError('')
   }else{
    setLoaded(true)
    setError((cause as Error).message)
   }
  }
 },[onAvailability])
 useEffect(()=>{
  void refresh()
  const timer=window.setInterval(()=>{void refresh()},5000)
  return()=>{window.clearInterval(timer);refreshSequence.current++}
 },[refresh])
 const online=workers.filter(w=>w.online).length
 const validPrompt=prompt.trim().length>0 &&
  new TextEncoder().encode(prompt.trim()).length<=16384
 const create=async()=>{
  if(busy||!enabled||online===0||!validPrompt)return
  refreshSequence.current++
  setBusy(true);setError('');setNotice('')
  try{
   const result=await post<{id:string}>('/api/remote/tasks',{
    prompt:prompt.trim(),maxTokens:tokens
   })
   setSelected(result.id);setPrompt('')
   setNotice(t('원격 추론 작업을 등록했습니다. 실행할 장치가 작업을 가져갑니다.',
    'Remote task queued. An available worker will claim it.'))
   await refresh()
  }catch(cause){setError((cause as Error).message)}
  finally{setBusy(false)}
 }
 const cancel=async(id:string)=>{
  if(busy||!window.confirm(t('이 원격 작업을 취소하시겠습니까?',
   'Cancel this remote task?')))return
  refreshSequence.current++
  setBusy(true);setError('')
  try{
   await post('/api/remote/tasks/'+encodeURIComponent(id)+'/cancel',{})
   await refresh()
  }catch(cause){setError((cause as Error).message)}
  finally{setBusy(false)}
 }
 const current=tasks.find(task=>task.id===selected)
 return <Panel className="min-w-0 p-4 sm:p-5" >
  <div className="flex flex-wrap items-center justify-between gap-3">
   <div className="flex min-w-0 items-center gap-3">
    <div className="grid size-9 shrink-0 place-items-center rounded-xl bg-[#F3EEE5] text-[#976D4C]">
     <Cpu size={17}/></div>
    <div className="min-w-0">
     <strong className="block text-[13px]">{t('원격 장치 · 추론 워커','Remote devices · inference workers')}</strong>
     <p className="text-[11px] text-[#8A8176]">{!loaded?t('연결 상태 조회 중','Checking connections'):
      !enabled?t('기능 비활성','Feature disabled'):
      t('온라인 '+online+'대 / 등록 '+workers.length+'대','Online '+online+' / registered '+workers.length)}</p>
    </div>
   </div>
   <div className="flex gap-2">
    <button type="button" onClick={()=>void refresh()} className={action}
     aria-label={t('원격 연결 새로고침','Refresh remote devices')}><RefreshCw size={14}/></button>
    <button type="button" onClick={()=>setExpanded(v=>!v)} className={action}
     aria-expanded={expanded} aria-controls="dominion-remote-workers">
     {expanded?t('접기','Collapse'):t('장치 · 작업 보기','View devices and tasks')}</button>
   </div>
  </div>
  {expanded&&<div id="dominion-remote-workers" className="mt-4 grid gap-4 border-t border-[#EFE9DF] pt-4">
   {!enabled&&loaded&&<p className="rounded-lg border border-[#E7E0D5] bg-[#FCFAF7] p-3 text-[11px] text-[#746A5F]">
    {t('원격 게이트웨이는 기본 비활성입니다. 서버에서 DOMINION_REMOTE_WORKERS=1을 설정하고 비공개 HTTPS 또는 SSH 터널로 연결하세요. 인증 키는 화면에 입력하지 않습니다.',
     'The remote gateway is disabled by default. Set DOMINION_REMOTE_WORKERS=1 on the server and connect through private HTTPS or an SSH tunnel. Never enter worker secrets here.')}
   </p>}
   {error&&<Notice message={error}/>}
   {notice&&<p role="status" className="text-[11px] text-emerald-700">{notice}</p>}
   {enabled&&<>
    <div>
     <p className="mb-2 text-[11px] font-semibold">{t('연결된 장치','Registered machines')}</p>
     {workers.length===0?<p className="text-[11px] text-[#8A8176]">
      {t('등록된 장치가 없습니다. 다른 컴퓨터에서 remote-worker를 실행하면 여기에 나타납니다.',
       'No machines registered. Start remote-worker on another machine to see it here.')}</p>:
     <div className="grid gap-2 sm:grid-cols-2">
      {workers.map(worker=><div key={worker.workerId}
       className="flex min-w-0 items-start gap-2 rounded-lg border border-[#E7E0D5] bg-[#FCFAF7] p-3">
       {worker.online?<Wifi className="mt-0.5 shrink-0 text-emerald-700" size={15}/>:
        <WifiOff className="mt-0.5 shrink-0 text-[#A49B90]" size={15}/>}
       <div className="min-w-0 flex-1">
        <div className="flex flex-wrap items-center justify-between gap-2">
         <strong className="break-all text-[11px]">{worker.workerId}</strong>
         <StatusPill status={worker.online?'available':'unavailable'}>
          {worker.online?t('온라인','Online'):t('오프라인','Offline')}</StatusPill>
        </div>
        <p className="break-all text-[11px] text-[#746A5F]">{worker.model}</p>
        <p className="text-[10px] text-[#9C9388]">{t('마지막 신호','Last seen')}: {formatDateTime(worker.lastSeenAt)}</p>
       </div>
      </div>)}
     </div>}
     <p className="mt-2 text-[10px] text-[#8A8176]">
      {t('온라인 표시는 최근 30초 이내의 신호를 뜻합니다. 모델 인증 또는 실제 추론 성공을 보장하지 않습니다.',
       'Online means a heartbeat within 30 seconds; model authentication and inference are not yet verified.')}
     </p>
    </div>
    <div className="rounded-xl border border-[#E7E0D5] bg-[#FCFAF7] p-3">
     <label className="block text-[12px] font-semibold" htmlFor="remote-task-prompt">
      {t('원격 장치에 텍스트 추론 요청','Send text reasoning to a remote device')}</label>
     <textarea id="remote-task-prompt" className={'mt-2 '+control} rows={3}
      maxLength={16384} value={prompt} onChange={e=>setPrompt(e.target.value)}
      placeholder={t('원격 모델에 맡길 분석 또는 요약을 입력하세요.','Describe the reasoning or summary to delegate.')}/>
     <div className="mt-2 flex flex-wrap items-end justify-between gap-3">
      <label className="grid min-w-36 gap-1 text-[11px] text-[#746A5F]">
       {t('최대 출력 토큰','Max output tokens')}
       <select className={control} value={tokens} onChange={e=>setTokens(Number(e.target.value))}>
        {[512,1024,2048,4096].map(n=><option key={n} value={n}>{n.toLocaleString()}</option>)}
       </select></label>
      <Action disabled={busy||online===0||!validPrompt} onClick={()=>void create()}>
       <Send size={14}/>{t('원격 작업 요청','Queue remote task')}</Action>
     </div>
     {prompt.trim()&&!validPrompt&&<p role="alert" className="mt-2 text-[11px] text-rose-700">
      {t('요청은 UTF-8 기준 16,384바이트를 넘을 수 없습니다.',
       'The request must be at most 16,384 UTF-8 bytes.')}</p>}
     {online===0&&<p className="mt-2 text-[11px] text-[#8A8176]">
      {t('온라인 장치가 연결되어야 새 작업을 요청할 수 있습니다.',
       'Connect an online worker before queuing new work.')}</p>}
    </div>
    <div>
     <p className="mb-2 text-[11px] font-semibold">{t('원격 작업 기록','Remote task history')}</p>
     {tasks.length===0?<p className="text-[11px] text-[#8A8176]">{t('아직 요청한 작업이 없습니다.','No remote tasks yet.')}</p>:
     <div className="grid gap-2 md:grid-cols-[minmax(0,1fr)_minmax(0,1.4fr)]">
      <div className="grid max-h-64 content-start gap-1 overflow-y-auto">
       {tasks.slice(-30).reverse().map(task=><button type="button" key={task.id}
        aria-pressed={selected===task.id} onClick={()=>setSelected(task.id)}
        className={'flex min-w-0 flex-wrap items-center justify-between gap-2 rounded-lg border p-2 text-left text-[11px] '+
         (selected===task.id?'border-[#C7B18F] bg-[#F7F0E5]':'border-[#E7E0D5] bg-white hover:bg-[#FAF7F1]')}>
        <span className="min-w-0 truncate font-mono text-[10px]">{task.id}</span>
        <StatusPill status={task.status}/></button>)}
      </div>
      <div className="min-w-0 rounded-lg border border-[#E7E0D5] bg-white p-3">
       {!current?<p className="text-[11px] text-[#8A8176]">
        {t('작업을 선택하면 결과와 검토 상태가 표시됩니다.','Select a task to view its result and review state.')}</p>:<>
        <div className="flex flex-wrap items-center justify-between gap-2">
         <strong className="break-all font-mono text-[10px]">{current.id}</strong>
         <StatusPill status={current.status}/></div>
        <p className="mt-2 text-[11px] text-[#746A5F]">
         {t('담당 장치','Worker')}: {current.workerId||'—'} · {formatDateTime(current.queuedAt)}</p>
        {current.evidenceGrade&&<p className="mt-1 text-[11px] text-amber-800">
         {t('근거 등급','Evidence')}: {current.evidenceGrade} · {t('외부 추론, 별도 검증 전','External inference, not verified')}</p>}
        {current.result?.answer&&<pre className="mt-3 max-h-72 overflow-auto whitespace-pre-wrap break-words rounded-lg bg-[#FCFAF7] p-3 font-sans text-[12px]">{current.result.answer}</pre>}
        {unsettled.has(current.status)&&<button type="button" disabled={busy}
         className={'mt-3 '+action} onClick={()=>void cancel(current.id)}>
         <Square size={12}/>{t('이 작업 취소','Cancel this task')}</button>}
       </>}
      </div>
     </div>}
    </div>
   </>}
  </div>}
 </Panel>
}
