import {useCallback,useEffect,useState} from 'react'
import {GitBranch,Plus,RefreshCw,ShieldCheck,Square,Workflow} from 'lucide-react'
import {get,post,type Provider} from '../../api'
import {formatDateTime,useLocale} from '../../i18n'
import {Action,Notice,Panel,Subheading} from '../../components/UI'
import {StatusPill} from '../../components/StatusPill'
import {RemoteWorkersPanel,type RemoteAvailability} from './RemoteWorkers'

type Runtime='native'|'codex'|'claude'|'qwen'|'remote'
type Mode='read'|'write'
type Draft={
 id:string;prompt:string;runtime:Runtime;mode:Mode;isolation:'shared'|'worktree';
 providerId:string;model:string;dependsOn:string[];resumeId:string;
 checkPath:string;checkText:string;
}
type TeamTask={taskId:string;mode:Mode;runtime:Runtime;isolation:string;
 dependsOn:string[];status?:string|null;verified?:boolean; evidenceGrade?:string|null;
 answerExcerpt?:string;observedOperations?:{operationId?:string;tool?:string;success?:boolean}[];
 worktree?:{id?:string;status?:string;mergeStatus?:string}|null}
type Team={id:string;goal:string;status:string;tasks:TeamTask[];error?:string|null;
 createdAt?:string;teamState?:{phase?:string}}
type Preview={taskId:string;diffSha256:string;files:string[];
 patchBytes:number;fileCount:number}
type Bridge={enabled:boolean;binaryAvailable:boolean;authentication:string}
const draft=(id:string):Draft=>({id,prompt:'',runtime:'native',mode:'read',
 isolation:'shared',providerId:'',model:'',dependsOn:[],resumeId:'',
 checkPath:'',checkText:''})
const field='w-full min-w-0 rounded-lg border border-[#E7E0D5] bg-white px-3 py-2 text-[12px]'
const btn='rounded-lg border border-[#E7E0D5] px-3 py-2 text-[11px] font-semibold hover:bg-[#FAF7F1] disabled:opacity-40'
const active=new Set(['queued','running'])
const recoverable=new Set(['failed','interrupted','needs_review'])
const apiPath=(id:string,task?:string)=>'/api/task-teams/'+encodeURIComponent(id)+
 (task?'/worktrees/'+encodeURIComponent(task):'')
export function TaskTeamsPanel({providers}:{providers:Provider[]}){
 const en=useLocale()==='en'
 const t=(ko:string,english:string)=>en?english:ko
 const [open,setOpen]=useState(true),[editing,setEditing]=useState(false)
 const [remoteAvailability,setRemoteAvailability]=useState<RemoteAvailability>({
  enabled:false,online:0,loaded:false
 })
 const [goal,setGoal]=useState(''),[steps,setSteps]=useState<Draft[]>([draft('inspect')])
 const [parallel,setParallel]=useState(2),[retries,setRetries]=useState(0)
 const [teams,setTeams]=useState<Team[]>([]),[selected,setSelected]=useState('')
 const [details,setDetails]=useState<Team|null>(null)
 const [bridges,setBridges]=useState<Record<string,Bridge>>({})
 const [preview,setPreview]=useState<Preview|null>(null)
 const [consent,setConsent]=useState(false),[busy,setBusy]=useState(false)
 const [error,setError]=useState(''),[notice,setNotice]=useState('')
 const refresh=useCallback(async()=>{
  const list=await get<Team[]>('/api/task-teams')
  setTeams(list)
  if(selected){
   const team=list.find(v=>v.id===selected)
   if(team)setDetails(team)
  }
 },[selected])
 useEffect(()=>{
  if(!open)return
  let alive=true
  void Promise.all([refresh(),get<{bridges:Record<string,Bridge>}>('/api/task-teams/bridges')])
   .then(([,result])=>{if(alive)setBridges(result.bridges||{})})
   .catch(err=>{if(alive)setError((err as Error).message)})
  const timer=setInterval(()=>{void refresh().catch(()=>{})},3000)
  return()=>{alive=false;clearInterval(timer)}
 },[open,refresh])
 const patch=(id:string,value:Partial<Draft>)=>setSteps(rows=>
  rows.map(v=>v.id===id?{...v,...value}:v))
 const add=(mode:Mode)=>{
  if(steps.length>=12)return
  const seen=new Set(steps.map(v=>v.id));let i=steps.length+1
  while(seen.has('step_'+i))i++
  const next=draft('step_'+i)
  next.mode=mode;next.isolation=mode==='write'?'worktree':'shared'
  // Independent reads can actually execute in parallel. A writer waits for
  // all predecessors rather than accidentally racing their observations.
  if(mode==='write')next.dependsOn=steps.map(step=>step.id)
  setSteps(rows=>[...rows,next])
 }
 const invalid=(()=>{
  if(!goal.trim())return t('팀 목표를 입력하세요.','Enter a team goal.')
  if(!steps.length||steps.some(v=>!v.prompt.trim() && steps.length>1))
   return t('여러 작업을 만들 때는 각 Worker의 지시문이 필요합니다.','Give each worker instructions when adding multiple tasks.')
  if(new Set(steps.map(v=>v.id)).size!==steps.length)return t('작업 ID가 중복됩니다.','Duplicate task IDs.')
  if(steps.some(step=>new TextEncoder().encode(step.prompt.trim()||goal.trim()).length>
   (step.runtime==='remote'?16384:32000)))
   return t('작업 지시문의 UTF-8 용량 제한을 초과했습니다.','Worker prompt exceeds the UTF-8 size limit.')
  for(const step of steps){
   if(step.mode==='write'&&!step.checkPath.trim())return t('수정 작업에 파일 검증 경로가 필요합니다.','Write jobs require an artifact check.')
   if(step.runtime!=='native'){
    if(step.mode==='write')return t('외부 실행기는 읽기 전용입니다.','External runtimes are read-only.')
    if(step.runtime==='remote'){
     if(!remoteAvailability.enabled)return t('원격 게이트웨이를 먼저 활성화하세요.','Enable the remote gateway first.')
     if(remoteAvailability.online===0)return t('온라인 원격 워커가 없습니다.','No remote worker is online.')
    }else if(!bridges[step.runtime]?.enabled||!bridges[step.runtime]?.binaryAvailable)
     return t('외부 CLI를 활성화하고 설치하세요.','Enable and install the external CLI.')
   }
   if(step.resumeId&& !/^[A-Za-z0-9_-]{4,128}$/.test(step.resumeId))
    return t('세션 ID 형식을 확인하세요.','Invalid session ID format.')
  }
  return ''
 })()
 const create=async()=>{
  if(invalid||busy)return
  setBusy(true);setError('');setNotice('')
  try{
   const tasks=steps.map(step=>{
    const item:Record<string,unknown>={id:step.id,
     prompt:step.prompt.trim()||goal.trim(),
     mode:step.mode,runtime:step.runtime,isolation:step.isolation,dependsOn:step.dependsOn}
    if(step.providerId&&step.runtime==='native')item.providerId=step.providerId
    if(step.model.trim()&&step.runtime!=='remote')item.model=step.model.trim()
    if(step.resumeId.trim()&&step.runtime!=='native'&&step.runtime!=='remote')item.resumeId=step.resumeId.trim()
    if(step.mode==='write')item.checks=[step.checkText.trim()?
     {type:'file_contains',path:step.checkPath.trim(),contains:step.checkText.trim()}:
     {type:'file_exists',path:step.checkPath.trim()}]
    return item
   })
   const created=await post<Team>('/api/task-teams',{
    goal:goal.trim(),maxParallel:parallel,maxReadRetries:retries,tasks})
   setEditing(false);setSelected(created.id);setDetails(created)
   setNotice(t('팀이 실행 대기열에 등록됐습니다.','Team queued for execution.'))
   await refresh()
  }catch(err){setError((err as Error).message)}
  finally{setBusy(false)}
 }
 const act=async(kind:'cancel'|'retry')=>{
  if(!details||busy)return
  if(!window.confirm(kind==='retry'?
   t('새 팀으로 처음부터 실행합니다. 추가 모델 비용이 발생할 수 있습니다. 진행하시겠습니까?',
    'Replay from scratch as a new team? Additional model costs may occur.'):
   t('이 팀과 하위 Worker를 취소하시겠습니까?','Cancel this team and all workers?')))return
  setBusy(true);setError('');setNotice('')
  try{
   const result=await post<any>(apiPath(details.id)+'/'+kind,kind==='retry'?{confirm:true}:{})
   if(kind==='retry'&&result.newTeamId){
    setSelected(result.newTeamId)
    setNotice(t('기존 기록은 유지되고 새 팀이 생성됐습니다.','Old records preserved; new team created.'))
   }else setDetails(result)
   setPreview(null);setConsent(false);await refresh()
  }catch(err){setError((err as Error).message)}
  finally{setBusy(false)}
 }
 const inspect=async(taskId:string)=>{
  if(!details||busy)return
  setError('');setConsent(false);setBusy(true)
  try{setPreview(await get<Preview>(apiPath(details.id,taskId)+'/preview'))}
  catch(err){setError((err as Error).message)}
  finally{setBusy(false)}
 }
 const stage=async()=>{
  if(!details||!preview||!consent||busy)return
  if(!window.confirm(t('검토한 변경을 원본 Git 작업공간에 스테이징할까요? 자동 커밋은 하지 않습니다.',
    'Stage reviewed changes in source Git workspace? No auto-commit.')))return
  setBusy(true);setError('')
  try{
   await post(apiPath(details.id,preview.taskId)+'/promote',
    {confirm:true,expectedDiffSha256:preview.diffSha256})
   setPreview(null);setConsent(false)
   setNotice(t('변경사항을 스테이징했습니다. Git에서 다시 확인하세요.',
    'Changes staged. Review them in Git before committing.'))
   await refresh()
  }catch(err){setError((err as Error).message)}
  finally{setBusy(false)}
 }
 return <div className="mt-8" aria-label={t('작업 팀','Task Teams')}>
  <div className="mb-3 flex flex-wrap items-center justify-between gap-2">
   <Subheading title={t('작업 팀 · 다중 에이전트','Task Teams · Multi-agent')}/>
   <div className="flex flex-wrap gap-2">
    <button type="button" className={btn} onClick={()=>setOpen(v=>!v)} aria-expanded={open}>
     <Workflow size={14} className="mr-1 inline"/>{open?t('접기','Collapse'):t('열기','Open')}</button>
    {open&&<button type="button" className={btn} onClick={()=>setEditing(v=>!v)} aria-expanded={editing}>
     <Plus size={14} className="mr-1 inline"/>{t('팀 만들기','New team')}</button>}
    {open&&<button type="button" className={btn} onClick={()=>void refresh().catch(err=>setError((err as Error).message))}>
     <RefreshCw size={14} className="mr-1 inline"/>{t('새로고침','Refresh')}</button>}
   </div>
  </div>
  {open&&<div className="grid gap-4">
   {error&&<Notice message={error}/>}
   {notice&&<p className="text-[12px] text-emerald-700" role="status">{notice}</p>}
   {editing&&<Panel className="p-4 sm:p-5">
    <p className="mb-4 text-[12px] text-[#746A5F]">{t(
     '목표를 입력하고 바로 실행하거나 조사·수정 Worker를 추가하세요. 독립 조사 작업은 병렬 실행되고 파일 수정은 Git Worktree로 격리할 수 있습니다.',
     'Enter a goal to run one worker, or add investigation and editing workers. Independent research runs in parallel; edits can use isolated Git worktrees.')}</p>
    <label className="grid gap-1 text-[12px] font-semibold">{t('팀 목표','Team goal')}
     <textarea className={field} rows={2} maxLength={16000} value={goal}
      onChange={e=>setGoal(e.target.value)}/></label>
    <div className="my-4 flex flex-wrap gap-3">
     <label className="text-[11px]">{t('동시 Worker 수','Concurrent workers')}
      <select className={field} value={parallel} onChange={e=>setParallel(Number(e.target.value))}>
       {[1,2,3].map(n=><option key={n} value={n}>{n}</option>)}</select></label>
     <label className="text-[11px]">{t('모델 실행 전 읽기 실패 재시도','Pre-model read retries')}
      <select className={field} value={retries} onChange={e=>setRetries(Number(e.target.value))}>
       {[0,1,2].map(n=><option key={n} value={n}>{n}</option>)}</select></label>
    </div>
    <div className="grid gap-3">
     {steps.map((step,index)=><div key={step.id} className="rounded-xl border border-[#E7E0D5] bg-[#FCFAF7] p-3">
      <div className="mb-3 flex items-center justify-between gap-2">
       <strong className="text-[12px]">{index+1}. {step.id}</strong>
       <button type="button" className={btn} disabled={steps.length===1||busy} onClick={()=>
        setSteps(rows=>rows.filter(v=>v.id!==step.id).map(v=>({
         ...v,dependsOn:v.dependsOn.filter(dep=>dep!==step.id)})))}>
        {t('제거','Remove')}</button>
      </div>
      <div className="grid gap-2 sm:grid-cols-3">
       <label className="grid gap-1 text-[11px]">{t('모드','Mode')}
        <select className={field} value={step.mode} onChange={e=>{
         const mode=e.target.value as Mode
         patch(step.id,{mode,runtime:mode==='write'?'native':step.runtime,
          isolation:mode==='write'?'worktree':step.isolation})
        }}><option value="read">{t('읽기·조사','Read')}</option>
         <option value="write">{t('파일 수정','Write')}</option></select></label>
       <label className="grid gap-1 text-[11px]">{t('실행 엔진','Runtime')}
        <select className={field} disabled={step.mode==='write'} value={step.runtime}
         onChange={e=>{
          const runtime=e.target.value as Runtime
          patch(step.id,{runtime,providerId:'',resumeId:'',model:'',
           isolation:runtime==='remote'?'shared':step.isolation})
         }}>
         <option value="native">DOMINION Rust</option>
         <option value="codex">Codex</option><option value="claude">Claude Code</option>
         <option value="qwen">Qwen Code</option>
         <option value="remote">{t('원격 장치 · 텍스트 추론','Remote device · text inference')}</option>
        </select></label>
       <label className="grid gap-1 text-[11px]">{t('작업공간','Workspace')}
        <select className={field} value={step.isolation}
         disabled={step.runtime==='remote'}
         onChange={e=>patch(step.id,{isolation:e.target.value as Draft['isolation']})}>
         <option value="shared">{t('공유','Shared')}</option>
         <option value="worktree">{t('Git Worktree 격리','Isolated Git worktree')}</option>
        </select></label>
      </div>
      <label className="mt-3 grid gap-1 text-[11px]">{t('작업 지시문','Worker instructions')}
       <textarea className={field} maxLength={32000} rows={2}
        placeholder={steps.length===1?t('생략하면 팀 목표를 사용합니다.','Leave empty to use the team goal.'):
         t('이 Worker가 맡을 구체적인 작업','Specify this worker’s responsibility')}
        value={step.prompt} onChange={e=>patch(step.id,{prompt:e.target.value})}/></label>
      {step.runtime==='native'?<label className="mt-3 grid gap-1 text-[11px]">
       {t('모델 제공자','Model provider')}
       <select className={field} value={step.providerId}
        onChange={e=>patch(step.id,{providerId:e.target.value,model:''})}>
        <option value="">{t('자동 선택','Automatic')}</option>
        {providers.filter(p=>p.enabled).map(p=><option key={p.id} value={p.id}>
         {p.name}{p.model?' · '+p.model:''}</option>)}
       </select></label>:<p className="mt-3 text-[11px] text-[#746A5F]">
       {step.runtime}: {step.runtime==='remote'?
        t('독립 실행 장치의 읽기 전용 추론','Remote text-only reasoning'):
        bridges[step.runtime]?.enabled&&bridges[step.runtime]?.binaryAvailable?
        t('CLI 설치 감지 · 인증은 별도','CLI detected · authentication unverified'):
        t('CLI가 비활성화됐거나 설치되지 않았습니다.','CLI disabled or unavailable')}</p>}
      {step.runtime==='remote'&&<p className="mt-2 text-[11px] text-[#746A5F]">
       {remoteAvailability.online>0?
        t('온라인 워커 '+remoteAvailability.online+'대 · 별도 장치 모델 사용 · 외부 추론 결과는 독립 검증 전',
         remoteAvailability.online+' worker(s) online · uses each device model · results are unverified'):
        t('아래 원격 장치에서 연결 상태를 확인하세요.','Check remote device status below.')}
      </p>}
      <details className="mt-3 text-[11px]">
       <summary className="cursor-pointer text-[#746A5F]">{t('모델 · 의존성 · 세션 설정','Model · dependencies · session')}</summary>
       {step.runtime!=='remote'&&<label className="mt-2 grid gap-1">{t('모델 ID (선택)','Model ID (optional)')}
        <input className={field} value={step.model} onChange={e=>patch(step.id,{model:e.target.value})}/></label>}
       {step.runtime!=='native'&&step.runtime!=='remote'&&<label className="mt-2 grid gap-1">
        {t('기존 세션 ID (선택)','Resume session ID (optional)')}
        <input className={field} value={step.resumeId} onChange={e=>patch(step.id,{resumeId:e.target.value})}/></label>}
       {steps.slice(0,index).length>0&&<fieldset className="mt-2">
        <legend>{t('선행 작업','Dependencies')}</legend>
        {steps.slice(0,index).map(previous=><label key={previous.id} className="mr-3 inline-flex items-center gap-2">
         <input type="checkbox" checked={step.dependsOn.includes(previous.id)}
          onChange={e=>patch(step.id,{dependsOn:e.target.checked?
           [...step.dependsOn,previous.id]:step.dependsOn.filter(x=>x!==previous.id)})}/>
         {previous.id}</label>)}
       </fieldset>}
      </details>
      {step.mode==='write'&&<div className="mt-3 grid gap-2 sm:grid-cols-2">
       <label className="grid gap-1 text-[11px]">{t('검증 파일 경로','Acceptance file')}
        <input className={field} value={step.checkPath} placeholder="src/feature.rs"
         onChange={e=>patch(step.id,{checkPath:e.target.value})}/></label>
       <label className="grid gap-1 text-[11px]">{t('포함할 문자열 (선택)','Must contain (optional)')}
        <input className={field} value={step.checkText}
         onChange={e=>patch(step.id,{checkText:e.target.value})}/></label>
      </div>}
     </div>)}
    </div>
    <div className="mt-3 flex flex-wrap gap-2">
     <button className={btn} type="button" disabled={busy||steps.length>=12}
      onClick={()=>add('read')}><Plus size={12} className="mr-1 inline"/>{t('조사 Worker','Read worker')}</button>
     <button className={btn} type="button" disabled={busy||steps.length>=12}
      onClick={()=>add('write')}><Plus size={12} className="mr-1 inline"/>{t('수정 Worker','Write worker')}</button>
    </div>
    <div className="mt-4 flex flex-wrap items-center justify-between gap-3 border-t border-[#EFE9DF] pt-3">
     <small className="text-[11px] text-[#8A8176]">{invalid||t('쓰기 작업은 검증 후 수동 병합합니다.','Writes require verified manual promotion.')}</small>
     <Action disabled={Boolean(invalid)||busy} onClick={()=>void create()}>
      <Workflow size={14}/>{t('팀 실행','Start team')}</Action>
    </div>
   </Panel>}
   <div className="grid gap-4 xl:grid-cols-[minmax(0,1fr)_minmax(0,1.5fr)]">
    <Panel className="min-w-0 p-3">
     <strong className="text-[12px]">{t('최근 팀','Recent teams')}</strong>
     {teams.length===0?<p className="my-3 text-[11px] text-[#8A8176]">{t('아직 팀이 없습니다.','No teams yet.')}</p>:
     <div className="mt-3 grid max-h-[430px] gap-2 overflow-y-auto">
      {teams.slice(0,35).map(team=><button key={team.id} type="button"
       aria-pressed={selected===team.id}
       onClick={()=>{setSelected(team.id);setDetails(team);setPreview(null);setConsent(false)}}
       className={'rounded-lg border p-3 text-left '+(selected===team.id?
        'border-[#C7B18F] bg-[#F7F0E5]':'border-[#E7E0D5] bg-white hover:bg-[#FAF7F1]')}>
       <div className="flex justify-between gap-2"><strong className="min-w-0 truncate text-[12px]">
        {team.goal}</strong><StatusPill status={team.status}/></div>
       <small className="text-[10px] text-[#8A8176]">{team.tasks?.length||0} Workers · {formatDateTime(team.createdAt)}</small>
      </button>)}
     </div>}
    </Panel>
    <Panel className="min-w-0 p-3">
     {!details?<p className="p-4 text-center text-[11px] text-[#8A8176]">
      {t('팀을 선택해 작업과 결과를 확인하세요.','Select a team to inspect workers and evidence.')}</p>:<>
      <div className="flex flex-wrap items-start justify-between gap-2">
       <div className="min-w-0"><strong className="block break-words text-[13px]">{details.goal}</strong>
        <small className="break-all text-[10px] text-[#8A8176]">{details.id}</small></div>
       <StatusPill status={details.status}/>
      </div>
      <div className="my-3 rounded-lg border border-[#E7E0D5] bg-[#FCFAF7] p-3">
       <div className="flex flex-wrap items-center justify-between gap-2 text-[11px]">
        <span className="font-semibold">{t('작업 진행','Task progress')}</span>
        <span>{details.tasks?.filter(task=>task.status==='completed').length||0} /
         {details.tasks?.length||0} {t('완료','completed')}</span>
       </div>
       <progress className="mt-2 h-2 w-full accent-[#A4583E]"
        max={Math.max(details.tasks?.length||0,1)}
        value={details.tasks?.filter(task=>task.status==='completed').length||0}
        aria-label={t('완료된 Worker 수','Completed workers')}/>
       <p className="mt-1 text-[10px] text-[#82796D]">
        {t('진행 단계','Phase')}: {details.teamState?.phase||'—'} ·
        {t(' 증거는 작업 완료와 별도로 검증됩니다.',' Evidence is checked separately from task completion.')}
       </p>
      </div>
      <div className="flex flex-wrap gap-2">
       {active.has(details.status)&&<button className={btn} type="button" disabled={busy} onClick={()=>void act('cancel')}>
        <Square size={13} className="mr-1 inline"/>{t('취소','Cancel')}</button>}
       {recoverable.has(details.status)&&<button className={btn} type="button" disabled={busy} onClick={()=>void act('retry')}>
        <RefreshCw size={13} className="mr-1 inline"/>{t('새 팀으로 재실행','Replay as new team')}</button>}
      </div>
      {details.error&&<Notice message={details.error}/>}
      <div className="mt-3 grid gap-2">
       {details.tasks?.map(task=><div key={task.taskId} className="rounded-lg border border-[#E7E0D5] bg-white p-3">
        <div className="flex flex-wrap items-center justify-between gap-2">
         <strong className="text-[12px]">{task.taskId}</strong>
         <StatusPill status={task.status||'queued'}/>
        </div>
        <p className="mt-1 text-[10px] text-[#746A5F]">{task.runtime} · {task.mode} · {task.isolation}</p>
        {task.dependsOn?.length>0&&<p className="text-[10px] text-[#8A8176]">
         {t('선행 작업','Dependencies')}: {task.dependsOn.join(', ')}</p>}
        <p className="mt-1 text-[10px] text-[#746A5F]">
         {t('검증','Evidence')}: {task.evidenceGrade||'—'}
         {task.verified&&<ShieldCheck size={13} className="ml-2 inline text-emerald-600"/>}</p>
        {task.answerExcerpt&&<p className="mt-2 whitespace-pre-wrap break-words text-[11px]">{task.answerExcerpt}</p>}
        {Boolean(task.observedOperations?.length)&&<details className="mt-2 rounded-lg bg-[#FCFAF7] p-2 text-[11px]">
         <summary className="cursor-pointer font-semibold text-[#746A5F]">
          {t('실제 도구 실행 근거','Observed tool receipts')} · {task.observedOperations?.length}</summary>
         <ul className="mt-2 space-y-1">
          {task.observedOperations?.map((operation,index)=><li key={operation.operationId||index}
           className="break-all text-[#746A5F]">{operation.tool||'tool'} · {operation.operationId||'—'}</li>)}
         </ul>
        </details>}
        {task.evidenceGrade==='external_unverified'&&<p className="mt-1 text-[10px] text-amber-800">
         {t('외부 모델의 응답이며 파일 변경 검증이 아닙니다.','External model result; not verified file changes.')}
        </p>}
        {task.worktree?.id&&<div className="mt-2 flex flex-wrap items-center gap-2">
         <GitBranch size={13}/><small>{task.worktree.status} · {task.worktree.mergeStatus||'pending'}</small>
         {details.status==='completed'&&task.mode==='write'&&task.worktree.mergeStatus!=='applied'&&
          <button type="button" className={btn} disabled={busy} onClick={()=>void inspect(task.taskId)}>
           {t('변경 미리보기','Preview changes')}</button>}
        </div>}
       </div>)}
      </div>
      {preview&&<div className="mt-3 rounded-lg border border-amber-200 bg-[#FFF9ED] p-3">
       <strong className="text-[12px]">{t('Git 변경 검토','Git review')} · {preview.taskId}</strong>
       <p className="mt-2 text-[11px]">{preview.fileCount} files · {preview.patchBytes} bytes</p>
       <ul className="my-2 max-h-28 list-inside list-disc overflow-y-auto text-[11px]">
        {preview.files.map(name=><li key={name} className="break-all">{name}</li>)}</ul>
       <p className="break-all font-mono text-[10px]">SHA256: {preview.diffSha256}</p>
       <label className="mt-3 flex items-start gap-2 text-[11px]">
        <input type="checkbox" checked={consent} onChange={e=>setConsent(e.target.checked)}/>
        {t('변경 목록과 해시를 확인했고 원본 스테이징에 동의합니다.',
           'I reviewed files and digest and authorize staging to source.')}</label>
       <div className="mt-3 flex gap-2">
        <Action variant="secondary" onClick={()=>{setPreview(null);setConsent(false)}}>{t('닫기','Close')}</Action>
        <Action disabled={!consent||busy} onClick={()=>void stage()}>
         <ShieldCheck size={13}/>{t('승인 후 스테이징','Approve and stage')}</Action>
       </div>
      </div>}
     </>}
    </Panel>
   </div>
   <RemoteWorkersPanel onAvailability={setRemoteAvailability}/>
  </div>}
 </div>
}
