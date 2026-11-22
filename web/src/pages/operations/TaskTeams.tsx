import {useCallback,useEffect,useRef,useState} from 'react'
import {GitBranch,Plus,RefreshCw,ShieldCheck,Square,Workflow} from 'lucide-react'
import {get,post,type Provider} from '../../api'
import {formatDateTime,useLocale} from '../../i18n'
import {Action,Notice,Panel,Subheading} from '../../components/UI'
import {StatusPill} from '../../components/StatusPill'
import {RemoteWorkersPanel,type RemoteAvailability} from './RemoteWorkers'
import {matchesRefreshTarget,type RefreshScope} from '../../refreshEvents'

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

const draftStorageKey='dominion.sub3.team.unsent.v1'
type SavedTeamDraft={goal:string;steps:Draft[];parallel:number;retries:number;restored:boolean}
function readTeamDraft():SavedTeamDraft {
 const blank:SavedTeamDraft={goal:'',steps:[draft('inspect')],parallel:2,retries:0,restored:false}
 try {
  const data=JSON.parse(sessionStorage.getItem(draftStorageKey)||'null') as Record<string,unknown>|null
  if(!data||typeof data.goal!=='string'||!Array.isArray(data.steps))return blank
  const seen=new Set<string>(),list:Draft[]=[]
  const text=(value:unknown,limit:number)=>typeof value==='string'?value.slice(0,limit):''
  for(const raw of data.steps.slice(0,12)){
   if(!raw||typeof raw!=='object'||Array.isArray(raw))continue
   const value=raw as Record<string,unknown>
   const id=text(value.id,128)
   if(!id||seen.has(id)||!/^[A-Za-z0-9_-]+$/.test(id))continue
   const mode:Mode=value.mode==='write'?'write':'read'
   const available:Runtime[]=['native','codex','claude','qwen','remote']
   const runtime:Runtime=mode==='write'?'native':
    available.includes(value.runtime as Runtime)?value.runtime as Runtime:'native'
   const isolation:Draft['isolation']=mode==='write'?'worktree':
    value.isolation==='worktree'?'worktree':'shared'
   const dependsOn=Array.isArray(value.dependsOn)?value.dependsOn
    .filter((dep:unknown):dep is string=>typeof dep==='string'&&seen.has(dep)).slice(0,11):[]
   list.push({id,prompt:text(value.prompt,32000),runtime,mode,isolation,
    providerId:text(value.providerId,256),model:text(value.model,256),
    dependsOn,resumeId:text(value.resumeId,128),
    checkPath:text(value.checkPath,1024),checkText:text(value.checkText,8192)})
   seen.add(id)
  }
  const steps=list.length?list:blank.steps
  const goal=data.goal.slice(0,16000)
  const parallel=Math.max(1,Math.min(3,Number(data.parallel)||2))
  const retries=Math.max(0,Math.min(2,Number(data.retries)||0))
  return {goal,steps,parallel,retries,restored:Boolean(goal||
   steps.length>1||steps[0].prompt||steps[0].mode!=='read'||
   steps[0].id!=='inspect'||parallel!==2||retries!==0)}
 }catch{return blank}
}

const field='w-full min-w-0 rounded-lg border border-[#E7E0D5] bg-white px-3 py-2 text-[12px]'
const btn='rounded-lg border border-[#E7E0D5] px-3 py-2 text-[11px] font-semibold hover:bg-[#FAF7F1] disabled:opacity-40'
const active=new Set(['queued','running'])
const recoverable=new Set(['failed','interrupted','needs_review'])
const phaseNames:Record<string,[string,string]>={
 scheduling:['작업 준비 중','Scheduling'],
 retrying_safe_reads:['안전한 읽기 작업 재시도 중','Retrying safe read tasks'],
 dispatching:['Worker 실행 중','Dispatching workers'],
 preparing_worktree:['작업 공간 준비 중','Preparing worktree'],
 verified:['검증 완료','Verified'],
}
const apiPath=(id:string,task?:string)=>'/api/task-teams/'+encodeURIComponent(id)+
 (task?'/worktrees/'+encodeURIComponent(task):'')
export function TaskTeamsPanel({providers}:{providers:Provider[]}){
 const en=useLocale()==='en'
 const t=(ko:string,english:string)=>en?english:ko
 const [storedDraft]=useState(readTeamDraft)
 const [open,setOpen]=useState(true),[editing,setEditing]=useState(storedDraft.restored)
 const [remoteAvailability,setRemoteAvailability]=useState<RemoteAvailability>({
  enabled:false,online:0,loaded:false
 })
 const [goal,setGoal]=useState(storedDraft.goal),[steps,setSteps]=useState<Draft[]>(storedDraft.steps)
 const [parallel,setParallel]=useState(storedDraft.parallel),[retries,setRetries]=useState(storedDraft.retries)
 // Keep only unsent owner input in this tab, not model receipts or settled runs.
 useEffect(()=>{
  const pristine=!goal.trim()&&steps.length===1&&steps[0].id==='inspect'&&
   !steps[0].prompt&&steps[0].mode==='read'&&steps[0].runtime==='native'&&
   !steps[0].model&&!steps[0].providerId&&!steps[0].resumeId&&
   steps[0].dependsOn.length===0&&parallel===2&&retries===0
  try{
   if(pristine)sessionStorage.removeItem(draftStorageKey)
   else sessionStorage.setItem(draftStorageKey,JSON.stringify({goal,steps,parallel,retries}))
  }catch{}
 },[goal,steps,parallel,retries])
 const [teams,setTeams]=useState<Team[]>([]),[selected,setSelected]=useState('')
 const [teamQuery,setTeamQuery]=useState('')
 const [teamView,setTeamView]=useState<'all'|'active'|'attention'|'finished'>('all')
 const [details,setDetails]=useState<Team|null>(null)
 const [bridges,setBridges]=useState<Record<string,Bridge>>({})
 const [preview,setPreview]=useState<Preview|null>(null)
 const [consent,setConsent]=useState(false),[busy,setBusy]=useState(false)
 const [error,setError]=useState(''),[notice,setNotice]=useState('')
 const goalBytes=new TextEncoder().encode(goal.trim()).length
 // Polls from before an owner create/cancel/retry cannot rewind the selected
 // Team, especially while the server is saving a new DAG.
 const refreshSequence=useRef(0)
 const refresh=useCallback(async(targetId?:string)=>{
  const version=++refreshSequence.current
  const list=await get<Team[]>('/api/task-teams')
  if(version!==refreshSequence.current)return
  setTeams(list)
  const activeId=targetId||selected
  if(activeId){
   const team=list.find(v=>v.id===activeId)
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
  const onDataChange=(event:Event)=>{
   const scope=(event as CustomEvent<RefreshScope>).detail
   const endpoint=selected?'/api/task-teams/'+encodeURIComponent(selected):'/api/task-teams'
   if(matchesRefreshTarget('/api/task-teams',scope?.targets??[])||
    matchesRefreshTarget(endpoint,scope?.targets??[]))void refresh().catch(()=>{})
  }
  window.addEventListener('dominion-data-change',onDataChange)
  return()=>{alive=false;clearInterval(timer);window.removeEventListener('dominion-data-change',onDataChange);refreshSequence.current++}
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
  if(goalBytes>16000)return t('팀 목표는 UTF-8 16,000바이트 이하여야 합니다.','Team goal must be at most 16,000 UTF-8 bytes.')
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
  refreshSequence.current++
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
    setGoal('');setSteps([draft('inspect')]);setParallel(2);setRetries(0)
    try{sessionStorage.removeItem(draftStorageKey)}catch{}
   setNotice(t('팀이 실행 대기열에 등록됐습니다.','Team queued for execution.'))
   await refresh(created.id)
  }catch(err){setError((err as Error).message)}
  finally{setBusy(false)}
 }
 const act=async(kind:'cancel'|'retry')=>{
  if(!details||busy)return
  if(!window.confirm(kind==='retry'?
   t('새 팀으로 처음부터 실행합니다. 추가 모델 비용이 발생할 수 있습니다. 진행하시겠습니까?',
    'Replay from scratch as a new team? Additional model costs may occur.'):
   t('이 팀과 하위 Worker를 취소하시겠습니까?','Cancel this team and all workers?')))return
  refreshSequence.current++
  setBusy(true);setError('');setNotice('')
  try{
   const result=await post<any>(apiPath(details.id)+'/'+kind,kind==='retry'?{confirm:true}:{})
   if(kind==='retry'&&result.newTeamId){
    setSelected(result.newTeamId)
    setNotice(t('기존 기록은 유지되고 새 팀이 생성됐습니다.','Old records preserved; new team created.'))
   }else setDetails(result)
   setPreview(null);setConsent(false);await refresh(
    kind==='retry'?result.newTeamId:details.id)
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
  refreshSequence.current++
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
 const activeCount=teams.filter(team=>active.has(team.status)).length
 const attentionCount=teams.filter(team=>recoverable.has(team.status)).length
 const finishedCount=teams.filter(team=>team.status==='completed'||team.status==='cancelled').length
 const filteredTeams=teams.filter(team=>{
  if(teamView==='active'&&!active.has(team.status))return false
  if(teamView==='attention'&&!recoverable.has(team.status))return false
  if(teamView==='finished'&&team.status!=='completed'&&team.status!=='cancelled')return false
  const query=teamQuery.trim().toLocaleLowerCase()
  return !query||[team.goal,team.id,...team.tasks.map(task=>task.taskId)].some(value=>value.toLocaleLowerCase().includes(query))
 })
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
   <div className="grid grid-cols-2 gap-2 md:grid-cols-4" role="group" aria-label={t('팀 실행 현황','Team run overview')}>
    {([
     {id:'all',ko:'전체 작업',english:'All teams',count:teams.length},
     {id:'active',ko:'실행 중',english:'Active',count:activeCount},
     {id:'attention',ko:'확인 필요',english:'Needs attention',count:attentionCount},
     {id:'finished',ko:'종료',english:'Finished',count:finishedCount},
    ] as const).map(filter=><button key={filter.id} type="button"
     aria-pressed={teamView===filter.id} onClick={()=>setTeamView(filter.id)}
     className={'min-w-0 rounded-xl border px-3 py-3 text-left transition-colors focus-visible:outline-2 focus-visible:outline-offset-2 '+
      (teamView===filter.id?'border-[#AA7355] bg-[#F8EEE8] shadow-sm':'border-[#E7E0D5] bg-white hover:bg-[#FCF7F1]')}>
     <strong className="block text-[22px] leading-none tabular-nums text-[#342A24]">{filter.count}</strong>
     <span className="mt-2 block text-[11px] text-[#756B61]">{t(filter.ko,filter.english)}</span>
    </button>)}
   </div>
   {error&&<Notice message={error}/>} 
   {notice&&<p className="text-[12px] text-emerald-700" role="status">{notice}</p>}
   {editing&&<Panel className="p-4 sm:p-5">
    <p className="mb-4 text-[12px] text-[#746A5F]">{t(
     '목표를 입력하고 바로 실행하거나 조사·수정 Worker를 추가하세요. 독립 조사 작업은 병렬 실행되고 파일 수정은 Git Worktree로 격리할 수 있습니다.',
     'Enter a goal to run one worker, or add investigation and editing workers. Independent research runs in parallel; edits can use isolated Git worktrees.')}</p>
    <label className="grid gap-1 text-[12px] font-semibold">{t('팀 목표','Team goal')}
     <textarea className={field} rows={2} maxLength={16000} value={goal}
      onChange={e=>setGoal(e.target.value)}/></label>
    <p className={'-mt-3 text-right text-[10px] '+(goalBytes>16000?'text-rose-600':'text-[#8A8176]')}>
     UTF-8 {goalBytes.toLocaleString()} / 16,000 bytes</p>
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
   <div className="grid gap-4 xl:grid-cols-[minmax(280px,1fr)_minmax(0,1.8fr)] xl:items-start">
    <Panel className="min-w-0 p-3 xl:sticky xl:top-4">
     <div className="flex items-center justify-between gap-2">
      <strong className="text-[12px]">{t('작업 탐색','Team explorer')}</strong>
      <small className="text-[10px] text-[#82796D]">{filteredTeams.length} / {teams.length}</small>
     </div>
     <label className="mt-3 block">
      <span className="sr-only">{t('팀 검색','Search teams')}</span>
      <input type="search" value={teamQuery} onChange={event=>setTeamQuery(event.target.value)}
       placeholder={t('목표 · 팀 ID · Worker 검색','Search goal, team ID or worker')}
       className="min-h-11 w-full min-w-0 rounded-lg border border-[#E7E0D5] bg-[#FCFBF9] px-3 text-[12px] focus-visible:outline-2 focus-visible:outline-offset-2"/>
     </label>
     {filteredTeams.length===0?<p className="my-3 text-[11px] text-[#8A8176]">{teams.length?t('조건에 맞는 팀이 없습니다.','No teams match this view.'):t('아직 팀이 없습니다.','No teams yet.')}</p>:
     <div className="mt-3 grid max-h-[430px] gap-2 overflow-y-auto">
      {filteredTeams.slice(0,50).map(team=><button key={team.id} type="button"
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
        {t('진행 단계','Phase')}: {details.teamState?.phase?(phaseNames[details.teamState.phase]?.[en?1:0]||details.teamState.phase):'—'} ·
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
         <GitBranch size={13}/><StatusPill status={task.worktree.status||'preparing'}/>
         <StatusPill status={task.worktree.mergeStatus||'pending'}/>
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
