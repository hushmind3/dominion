import {useCallback,useEffect,useState,type ComponentProps,type FormEvent,type ReactNode} from 'react'
import {Activity,ArrowRight,BrainCircuit,Clapperboard,Database,Download,MessageCircle,PauseCircle,Play,Plus,RefreshCw,UsersRound} from 'lucide-react'
import {get,post} from '../api'
import type {AgentPreset} from './NativeAgents'
import {Button,PageContainer,TextArea,TextInput,SelectInput} from '../components/Primitives'
import {Headline,Panel,FormField,Notice,StateTag} from '../components/UI'

type Run = {id:string;status:string;input:{prompt:string;sessionId?:string;maxSteps?:number;tokenBudget?:number};result?:{answer?:string;[key:string]:unknown}|null;error?:string|null;steps:number;tokens:number;costKrw:number;createdAt:string;cancelRequested?:boolean}
type RunEvent = {seq:number;taskId:string;type:string;detail?:Record<string,unknown>}
type Member = {name:string;role:string;economy?:boolean}
type Group = {id:string;topic:string;participants:Member[];messages:{speaker:string;content:string;at_ms:number}[];status:string;rounds:number;max_rounds:number;cost_krw:number;budget_krw:number}
type Scene = {heading:string;narration:string;seconds:number}
type Video = {id:string;prompt:string;status:string;scenes:Scene[];backend:string;error?:string|null;output?:string|null;cost_krw:number;budget_krw:number;images?:Record<string,string>}
type Trial = {id:string;repository:string;target_file:string;instruction:string;status:string;accepted:boolean;cost_krw:number;error?:string|null;baseline?:{score?:number;tests_passed:boolean};candidate?:{score?:number;tests_passed:boolean}}
type EvolutionCandidate = {trial_id:string;generation:number;proposal_index:number;training_score:number|null;validation_baseline_score:number|null;validation_score:number|null;eligible:boolean;reason?:string|null}
type Evolution = {id:string;starting_trial:string;status:string;max_generations:number;target_files:string[];candidates_per_generation?:number;validation_command?:string|null;candidate_groups?:string[][];candidates?:EvolutionCandidate[];descendants:string[];completed_generations:number;cost_budget_krw:number;spent_krw:number;stop_requested?:boolean;stop_reason?:string|null}
type Memory = {id:string;content:string;kind:string;status:string;sessionId?:string|null}
type Skill = {id:string;name:string;trigger:string;instructions:string;enabled:boolean}
type Health = {ok:boolean;engine:string;model:string;workspace:string}

const complete = new Set(['completed','failed','cancelled','needs_review'])
const layout='grid gap-4 lg:grid-cols-[minmax(0,1.7fr)_minmax(260px,.8fr)]'
const panel='p-4 sm:p-5'
const subtle='text-[12px] text-[#7C7469]'
const row='flex flex-wrap items-center gap-2'
const fmtKrw=(n:number)=>Math.round(n||0).toLocaleString('ko-KR')+'원'
const errText=(e:unknown)=>e instanceof Error?e.message:String(e)
function Section({children,className=''}:{children:ReactNode;className?:string}){return <Panel className={panel+' '+className}>{children}</Panel>}
function InlineError({error}:{error:string}){return <Notice message={error}/>}
function LoadingButton({busy,children,...rest}:{busy:boolean;children:ReactNode}&ComponentProps<typeof Button>){return <Button disabled={busy||rest.disabled} {...rest}>{busy?'처리 중…':children}</Button>}
function useCollection<T>(path:string,empty:T){
 const [value,setValue]=useState<T>(empty),[error,setError]=useState(''),[loading,setLoading]=useState(true)
 const reload=useCallback(async()=>{try{const data=await get<T>(path);setValue(data);setError('')}catch(e){setError(errText(e))}finally{setLoading(false)}},[path])
 useEffect(()=>{void reload()},[reload])
 return {value,setValue,error,loading,reload}
}
function useOperation(){
 const [busy,setBusy]=useState(false),[error,setError]=useState('')
 const run=useCallback(async<T,>(action:()=>Promise<T>):Promise<T|undefined>=>{
   setBusy(true);setError('')
   try{return await action()}catch(e){setError(errText(e));return undefined}finally{setBusy(false)}
 },[])
 return {busy,error,run}
}
function Status({value}:{value:string}){return <StateTag online={['completed','open','evaluated','adopted','ready'].includes(value)}>{value}</StateTag>}
function ListButton({selected,children,onClick}:{selected:boolean;children:ReactNode;onClick:()=>void}){
 return <Button variant="bare" onClick={onClick} className={'block w-full rounded-lg border px-3 py-3 text-left text-[12px] '+(selected?'border-[#D7A78B] bg-[#F9EFE8]':'border-[#ECE5DB] hover:bg-[#F8F5EF]')}>{children}</Button>
}
function Page({title,description,eyebrow,children}:{title:string;description:string;eyebrow:string;children:ReactNode}){
 return <PageContainer><Headline title={title} description={description} eyebrow={eyebrow}/>{children}</PageContainer>
}
function RunList({items,selected,onSelect}:{items:Run[];selected:string;onSelect:(id:string)=>void}){
 return <div className="grid max-h-[470px] gap-2 overflow-auto">{items.length?items.map(item=><ListButton key={item.id} selected={selected===item.id} onClick={()=>onSelect(item.id)}>
  <div className="flex items-center justify-between gap-2"><strong className="line-clamp-2">{item.input.prompt}</strong><Status value={item.status}/></div>
  <p className={'mt-1 '+subtle}>{new Date(item.createdAt).toLocaleString('ko-KR')} · {item.steps}단계 · {item.tokens.toLocaleString()}토큰 · {fmtKrw(item.costKrw)}</p>
 </ListButton>):<p className={subtle}>아직 실행 기록이 없습니다.</p>}</div>
}
export function NativeTasks({home=false}:{home?:boolean}){
 const tasks=useCollection<Run[]>('/api/tasks',[])
 const presets=useCollection<AgentPreset[]>('/api/agents',[])
 const [active,setActive]=useState(()=>new URLSearchParams(window.location.hash.split('?')[1]||'').get('task')||''),[prompt,setPrompt]=useState(''),[sessionId,setSessionId]=useState('')
 const [allowWrite,setAllowWrite]=useState(false),[allowShell,setAllowShell]=useState(false),[allowMcp,setAllowMcp]=useState(false)
 const [allowSubagents,setAllowSubagents]=useState(false),[requireEvidence,setRequireEvidence]=useState(false)
 const [planningMode,setPlanningMode]=useState('auto'),[modelTier,setModelTier]=useState('auto'),[agentId,setAgentId]=useState(''),[checkKind,setCheckKind]=useState('auto'),[testFile,setTestFile]=useState('calc.test.js'),[maxSteps,setMaxSteps]=useState(12),[tokenBudget,setTokenBudget]=useState(40000),[costBudget,setCostBudget]=useState('')
 const [instruction,setInstruction]=useState(''),[resumeText,setResumeText]=useState(''),[events,setEvents]=useState<RunEvent[]>([]),[children,setChildren]=useState<Run[]>([])
 const op=useOperation()
 const selected=tasks.value.find(item=>item.id===active)
 const acceptanceChecks=checkKind==='cargo'?[{program:'cargo',args:['test','--quiet']}]:
  checkKind==='npm'?[{program:'npm',args:['test','--silent']}]:
  checkKind==='node'?[{program:'node',args:['--test',testFile.trim()]}]:[]
 const latestAcceptance=events.filter(event=>event.type==='acceptance_result').at(-1)
 const latestSchemaRoute=events.filter(event=>event.type==='tool_schema_routed').at(-1)
 useEffect(()=>{if(!active)return;let done=false;const load=()=>{void get<Run[]>('/api/tasks/'+encodeURIComponent(active)+'/children').then(v=>{if(!done)setChildren(v)}).catch(()=>{if(!done)setChildren([])})}
  load();return()=>{done=true}
 },[active])
 useEffect(()=>{
  if(!active){setEvents([]);return}
  let alive=true;const seen=new Set<number>();setEvents([])
  const stream=new EventSource('/api/tasks/'+encodeURIComponent(active)+'/events')
  stream.onmessage=event=>{try{const item=JSON.parse(event.data) as RunEvent
    if(!alive||seen.has(item.seq))return;seen.add(item.seq)
    setEvents(current=>[...current,item].slice(-350))
    if(['finished','completed','failed','cancelled','needs_review','step_completed','task_completed','subagent_completed'].includes(item.type)){void tasks.reload()}
    if(item.type==='finished')stream.close()
  }catch{}}
  stream.onerror=()=>{if(!alive)return;void tasks.reload()}
  return()=>{alive=false;stream.close()}
 },[active,tasks.reload])
 useEffect(()=>{const timer=setInterval(()=>{void tasks.reload()},5000);return()=>clearInterval(timer)},[tasks.reload])
 const submit=async(e:FormEvent)=>{e.preventDefault();const cleaned=prompt.trim();if(!cleaned)return
  const created=await op.run(()=>post<Run>('/api/tasks',{requestId:crypto.randomUUID(),prompt:cleaned,sessionId:sessionId.trim()||undefined,
   allowWrite,allowShell,allowMcp,allowSubagents,requireEvidence,planningMode,modelTier,agentId:agentId||undefined,acceptanceChecks,maxSteps,tokenBudget,
    costBudgetKrw:costBudget.trim()?Number(costBudget):undefined}))
  if(created){setActive(created.id);setPrompt('');void tasks.reload()}
 }
 const continueTask=async()=>{
  if(!active||!resumeText.trim())return
  const created=await op.run(()=>post<Run>('/api/tasks/'+encodeURIComponent(active)+'/resume',{
   requestId:crypto.randomUUID(),prompt:resumeText,
   allowWrite,allowShell,allowMcp,allowSubagents,requireEvidence,planningMode,modelTier,agentId:agentId||undefined,acceptanceChecks,maxSteps,tokenBudget,
   costBudgetKrw:costBudget.trim()?Number(costBudget):undefined
  }))
  if(created){setResumeText('');setActive(created.id);void tasks.reload()}
 }
 const intervene=async(route:'cancel'|'steer')=>{
  if(!active)return
  const updated=await op.run(()=>post<Run>('/api/tasks/'+encodeURIComponent(active)+'/'+route,route==='steer'?{text:instruction}:{}))
  if(updated){if(route==='steer')setInstruction('');void tasks.reload()}
 }
 return <Page title={home?'무엇을 맡기시겠습니까?':'자율 에이전트 작업'} eyebrow="NATIVE AUTONOMOUS WORKSPACE" description="Rust 실행 엔진이 작업을 계획하고 수행합니다. 중간 지시, 기록, 토큰 사용량을 한 화면에서 확인하세요.">
  <div className={layout}>
   <div className="grid min-w-0 content-start gap-4">
    <Section>
     <form onSubmit={e=>void submit(e)} className="grid gap-3">
      <FormField label="작업 지시"><TextArea value={prompt} onChange={e=>setPrompt(e.target.value)} required maxLength={32000} rows={5} placeholder="프로젝트를 분석하고 필요한 파일을 수정한 뒤 검증까지 진행하세요."/></FormField>
      <div className="grid gap-2 sm:grid-cols-2 lg:grid-cols-3 text-[12px]">
       {([['파일 수정 허용',allowWrite,setAllowWrite],['검사 명령 허용',allowShell,setAllowShell],['MCP 도구 허용',allowMcp,setAllowMcp],['보조 에이전트 허용',allowSubagents,setAllowSubagents],['실행 증거 필수',requireEvidence,setRequireEvidence]] as const).map(([label,checked,set])=><label key={label} className="flex items-center gap-2"><input type="checkbox" checked={checked} onChange={e=>set(e.target.checked)}/>{label}</label>)}
      </div>
      <details className="rounded-xl border border-[#ECE5DB] p-3"><summary className="cursor-pointer text-[12px] font-semibold">고급 실행 설정</summary>
       <div className="mt-3 grid gap-3 sm:grid-cols-2">
        <FormField label="세션 ID (선택)"><TextInput value={sessionId} maxLength={128} onChange={e=>setSessionId(e.target.value)}/></FormField>
        <FormField label="작업 계획"><SelectInput value={planningMode} onChange={e=>setPlanningMode(e.target.value)}><option value="auto">자동</option><option value="always">항상 계획</option><option value="never">계획 생략</option></SelectInput></FormField>
        <FormField label="최대 모델 단계"><TextInput type="number" min={1} max={24} value={maxSteps} onChange={e=>setMaxSteps(Number(e.target.value))}/></FormField>
        <FormField label="에이전트"><SelectInput value={agentId} onChange={e=>setAgentId(e.target.value)}><option value="">기본 에이전트</option>{presets.value.map(p=><option key={p.id} value={p.id}>{p.name}</option>)}</SelectInput></FormField>
        {allowShell&&<FormField label="코드 검증 명령"><SelectInput value={checkKind} onChange={e=>setCheckKind(e.target.value)}>
         <option value="auto">프로젝트에서 자동 발견</option>
         <option value="cargo">Rust · cargo test --quiet</option>
         <option value="npm">Node · npm test --silent</option>
         <option value="node">Node · 지정한 테스트 파일</option>
        </SelectInput></FormField>}
        {allowShell&&checkKind==='node'&&<FormField label="테스트 파일 (작업 폴더 상대 경로)">
         <TextInput value={testFile} onChange={e=>setTestFile(e.target.value)} placeholder="calc.test.js" maxLength={180}/>
        </FormField>}
        <FormField label="모델 선택 정책"><SelectInput value={modelTier} onChange={e=>setModelTier(e.target.value)}><option value="auto">자동 · 작업 난도에 따라</option><option value="economy">비용 절약 우선</option><option value="quality">품질 우선</option></SelectInput></FormField>
         <FormField label="토큰 예산"><TextInput type="number" min={256} max={500000} value={tokenBudget} onChange={e=>setTokenBudget(Number(e.target.value))}/></FormField>
         <FormField label="모델 비용 예산 (원, 선택)"><TextInput type="number" min={0} max={10000000} value={costBudget} onChange={e=>setCostBudget(e.target.value)} placeholder="비우면 별도 제한 없음"/></FormField>
       </div>
      </details>
      <div className="flex items-center justify-between gap-3"><span className={subtle}>파일 변경·셸·외부 도구는 개별적으로 허용합니다.</span><LoadingButton busy={op.busy} type="submit" variant="primary"><Play size={14}/> 실행 시작</LoadingButton></div>
     </form><InlineError error={op.error}/>
    </Section>
    {selected&&<Section>
     <div className="flex flex-wrap items-center justify-between gap-3"><h2 className="font-bold">실행 진행</h2><Status value={selected.status}/></div>
     <div className="my-4 grid grid-cols-2 gap-3 rounded-xl bg-[#F6F3EC] p-3 text-center text-[12px] sm:grid-cols-4"><div>단계<strong className="block text-lg">{selected.steps}</strong></div><div>토큰<strong className="block text-lg">{selected.tokens.toLocaleString()}</strong></div><div>추정 비용<strong className="block text-lg">{fmtKrw(selected.costKrw)}</strong></div><div>상태<strong className="block text-sm">{selected.cancelRequested?'중단 요청됨':selected.status}</strong></div></div>
     {selected.error&&<InlineError error={selected.error}/>}
     {latestSchemaRoute&&<div className="mb-3 rounded-xl border border-[#E7E0D5] bg-white p-3 text-[12px]">
      <strong className="block">🔎 도구 자동 선택 · 실제 모델 요청</strong>
      <p className="mt-1 text-[#6F675D]">
       등록 {Number(latestSchemaRoute.detail?.catalogCount??0)}개 중
       {' '}{Number(latestSchemaRoute.detail?.advertisedCount??0)}개 도구 스키마만 전달
      </p>
      <p className="mt-1 text-[#8B8175]">
       JSON 스키마 {Number(latestSchemaRoute.detail?.advertisedBytes??0).toLocaleString('ko-KR')}B
       {' / 전체 '}{Number(latestSchemaRoute.detail?.catalogBytes??0).toLocaleString('ko-KR')}B
       {' · '}선택 제외 {Number(latestSchemaRoute.detail?.omittedCount??0)}개
      </p>
      <p className="mt-1 text-[11px] text-[#8B8175]">
       MCP 도구는 필요할 때 검색해 불러옵니다. 이 수치는 JSON 크기이며
       실제 청구 토큰과 원가는 제공자 계측값으로 별도 확인합니다.
      </p>
     </div>}
     {latestAcceptance&&<div className="mb-3 rounded-xl border border-[#E7E0D5] bg-white p-3 text-[12px]">
      <strong className="block">🧪 실파일 코드 검증 {latestAcceptance.detail?.success===true?'· 통과':'· 실패 / 자동 수정 대상'}</strong>
      <p className="mt-1 text-[#6F675D]">{String(latestAcceptance.detail?.check||'검증 명령')}</p>
      <p className="mt-1 text-[#8B8175]">소스 수정 차수 {String(latestAcceptance.detail?.revision??'-')} · 종료 코드 {String(latestAcceptance.detail?.exitCode??'-')}</p>
      {latestAcceptance.detail?.success!==true&&<p className="mt-2 whitespace-pre-wrap break-words text-rose-700">{String(latestAcceptance.detail?.stderrTail||'실패한 실행 기록에서 진단 정보를 확인할 수 있습니다.').slice(-1600)}</p>}
     </div>}
     {selected.result&&<div className="mb-4 rounded-xl border border-[#E7E0D5] bg-white p-4"><h3 className="mb-2 text-[12px] font-bold">실행 결과</h3><pre className="whitespace-pre-wrap break-words text-[12px]">{typeof selected.result.answer==='string'?selected.result.answer:JSON.stringify(selected.result,null,2)}</pre></div>}
     {!complete.has(selected.status)&&<div className="mb-3 grid gap-2 sm:grid-cols-[minmax(0,1fr)_auto_auto]"><TextInput aria-label="실행 중 방향 변경" value={instruction} onChange={e=>setInstruction(e.target.value)} placeholder="다음 단계에 반영할 지시"/><LoadingButton busy={op.busy} disabled={!instruction.trim()} onClick={()=>void intervene('steer')}>지시 전달</LoadingButton><LoadingButton busy={op.busy} variant="danger" onClick={()=>void intervene('cancel')}><PauseCircle size={14}/> 중단</LoadingButton></div>}
     {complete.has(selected.status)&&<div className="mb-4 rounded-xl border border-[#E7E0D5] p-3">
       <h3 className="mb-2 text-[12px] font-bold">작업 이어서 실행</h3>
       <p className={'mb-2 '+subtle}>이전 작업의 저장된 기록을 참고해 새 실행을 만듭니다. 파일·셸·MCP 권한은 위 설정에서 다시 명시적으로 선택합니다.</p>
       <div className="grid gap-2 sm:grid-cols-[minmax(0,1fr)_auto]"><TextInput aria-label="작업 이어서 실행 지시" value={resumeText} onChange={e=>setResumeText(e.target.value)} placeholder="실패한 부분을 확인하고 수정해 주세요."/><LoadingButton busy={op.busy} disabled={!resumeText.trim()} onClick={()=>void continueTask()} variant="primary"><RefreshCw size={14}/> 이어서 실행</LoadingButton></div>
      </div>}
      <details open><summary className="cursor-pointer text-[12px] font-semibold">실행 이벤트 · {events.length}건</summary><ol className="mt-3 max-h-72 space-y-2 overflow-auto">{events.map(ev=><li key={ev.seq} className="rounded-lg border border-[#EEE8E0] p-3 text-[11px]"><div className="flex justify-between"><strong>{ev.type}</strong><span className="text-[#A79C8E]">#{ev.seq}</span></div>{ev.detail&&<pre className="mt-1 max-h-36 overflow-auto whitespace-pre-wrap break-all text-[#6F675D]">{JSON.stringify(ev.detail,null,2)}</pre>}</li>)}</ol></details>
     {children.length>0&&<details className="mt-3"><summary className="cursor-pointer text-[12px] font-semibold">보조 에이전트 {children.length}개</summary><div className="mt-2"><RunList items={children} selected="" onSelect={setActive}/></div></details>}
    </Section>}
   </div>
   <div className="grid min-w-0 content-start gap-4">
    <Section><div className="mb-4 flex justify-between"><h2 className="text-[13px] font-bold">실행 기록</h2><Button variant="ghost" size="sm" onClick={()=>void tasks.reload()}><RefreshCw size={13}/> 갱신</Button></div><InlineError error={tasks.error}/><RunList items={tasks.value} selected={active} onSelect={setActive}/></Section>
   </div>
  </div>
 </Page>
}
export function NativeDashboard({navigate}:{navigate:(page:'chat'|'rooms'|'video'|'rsi'|'memory')=>void}){
 const tasks=useCollection<Run[]>('/api/tasks',[])
 const groups=useCollection<Group[]>('/api/groups',[])
 const videos=useCollection<Video[]>('/api/videos',[])
 const trials=useCollection<Trial[]>('/api/improvements',[])
 const stats=[{title:'에이전트 실행',value:tasks.value.length,icon:Activity,to:'chat' as const},{title:'그룹 대화',value:groups.value.length,icon:UsersRound,to:'rooms' as const},{title:'영상 프로젝트',value:videos.value.length,icon:Clapperboard,to:'video' as const},{title:'자기개선 실험',value:trials.value.length,icon:BrainCircuit,to:'rsi' as const}]
 return <Page title="DOMINION 작업공간" description="메인 프런트엔드와 같은 화면에서 네이티브 Rust 실행 엔진의 작업과 결과를 관리합니다." eyebrow="RUST NATIVE WORKSPACE">
  <div className="mb-5 grid grid-cols-2 gap-3 lg:grid-cols-4">{stats.map(s=><Button key={s.title} onClick={()=>navigate(s.to)} variant="bare" className="min-h-28 rounded-xl border border-[#E7E0D5] bg-white p-4 text-left"><div className="flex justify-between text-[12px] text-[#7C7469]">{s.title}<s.icon size={18}/></div><strong className="mt-3 block text-2xl">{s.value}</strong></Button>)}</div>
  <div className="grid gap-3 md:grid-cols-2">
   <Section><h2 className="mb-3 text-[14px] font-bold">빠른 실행</h2><p className={'mb-4 '+subtle}>코드 수정부터 장기 작업까지 모델과 도구를 연결해 실행합니다.</p><Button variant="primary" onClick={()=>navigate('chat')}><ArrowRight size={14}/> 작업 시작</Button></Section>
   <Section><h2 className="mb-3 text-[14px] font-bold">최근 실행</h2><RunList items={tasks.value.slice(0,5)} selected="" onSelect={()=>navigate('chat')}/></Section>
  </div>
  {(tasks.error||groups.error||videos.error||trials.error)&&<InlineError error={[tasks.error,groups.error,videos.error,trials.error].filter(Boolean).join(' · ')}/>}
 </Page>
}
export function NativeGroups(){
 const data=useCollection<Group[]>('/api/groups',[])
 const [active,setActive]=useState(''),[topic,setTopic]=useState(''),[names,setNames]=useState(['설계자','검토자']),[roles,setRoles]=useState(['구조와 사용자 경험을 제안','오류와 개선점을 검토']),[economy,setEconomy]=useState([false,true]),[budget,setBudget]=useState(5000),[input,setInput]=useState('')
 const op=useOperation();const selected=data.value.find(x=>x.id===active)
 const mutate=async(id:string,segment:string,payload:unknown={})=>{const item=await op.run(()=>post<Group>('/api/groups/'+encodeURIComponent(id)+segment,payload));if(item){data.setValue(old=>old.map(x=>x.id===item.id?item:x));return item}}
 const create=async(e:FormEvent)=>{e.preventDefault();const item=await op.run(()=>post<Group>('/api/groups',{topic,participants:names.map((name,i)=>({name,role:roles[i],economy:economy[i]})),budget_krw:budget,max_rounds:12}));if(item){setActive(item.id);void data.reload()}}
 return <Page title="멀티모델 그룹 대화" description="참가자별 역할·공유 문맥·발언 순서를 Rust가 관리합니다. 다음 발언을 선택할 때 모델을 호출합니다." eyebrow="MULTI AGENT">
  <div className={layout}>
   <div className="grid content-start gap-4">
    <Section><h2 className="mb-3 text-[14px] font-bold">새 회의실</h2><form onSubmit={e=>void create(e)} className="grid gap-3">
     <FormField label="대화 주제"><TextInput required value={topic} onChange={e=>setTopic(e.target.value)}/></FormField>
     <div className="grid gap-3 sm:grid-cols-2">{[0,1].map(i=><div key={i} className="grid gap-2"><FormField label={'참가자 '+(i+1)+' 이름'}><TextInput required value={names[i]} onChange={e=>setNames(v=>v.map((s,n)=>n===i?e.target.value:s))}/></FormField><FormField label="역할"><TextArea required value={roles[i]} onChange={e=>setRoles(v=>v.map((s,n)=>n===i?e.target.value:s))}/></FormField><label className="flex items-center gap-2 text-[12px]"><input type="checkbox" checked={economy[i]} onChange={e=>setEconomy(v=>v.map((s,n)=>n===i?e.target.checked:s))}/>절약형 모델 사용</label></div>)}</div>
     <FormField label="예산 (원)"><TextInput type="number" min={0} max={1000000} value={budget} onChange={e=>setBudget(Number(e.target.value))}/></FormField>
     <div><LoadingButton busy={op.busy} type="submit" variant="primary"><Plus size={14}/> 회의실 만들기</LoadingButton></div></form>
    </Section>
    {selected&&<Section>
     <div className="mb-2 flex items-center justify-between"><h2 className="text-[14px] font-bold">{selected.topic}</h2><Status value={selected.status}/></div>
     <p className={'mb-4 '+subtle}>라운드 {selected.rounds}/{selected.max_rounds} · 사용액 {fmtKrw(selected.cost_krw)} / {fmtKrw(selected.budget_krw)}</p>
     <div className="max-h-[550px] space-y-3 overflow-auto rounded-xl bg-[#F7F3EC] p-3">{selected.messages.length?selected.messages.map((message,i)=><article key={i} className="rounded-xl bg-white p-3 text-[12px]"><strong className="mb-2 block">{message.speaker}</strong><p className="whitespace-pre-wrap">{message.content}</p></article>):<p className={subtle}>대화를 시작해 주세요.</p>}</div>
     <form onSubmit={e=>{e.preventDefault();void mutate(active,'/messages',{content:input}).then(x=>{if(x)setInput('')})}} className="mt-3 flex gap-2"><TextInput value={input} onChange={e=>setInput(e.target.value)} required placeholder="@참가자 이름으로 발언 요청"/><LoadingButton busy={op.busy} type="submit">메시지</LoadingButton></form>
     <LoadingButton busy={op.busy||selected.status!=='open'} onClick={()=>void mutate(active,'/next')} variant="primary" className="mt-3"><MessageCircle size={14}/> 다음 에이전트 발언</LoadingButton>
    </Section>}
    <InlineError error={op.error}/>
   </div>
   <Section><div className="mb-3 flex justify-between"><h2 className="text-[13px] font-bold">저장된 회의실</h2><Button size="sm" variant="ghost" onClick={()=>void data.reload()}><RefreshCw size={13}/></Button></div><InlineError error={data.error}/><div className="grid gap-2">{data.value.map(item=><ListButton selected={active===item.id} onClick={()=>setActive(item.id)} key={item.id}><strong>{item.topic}</strong><p className={subtle}>{item.participants.map(p=>p.name+(p.economy?' (절약형)':' (품질형)')).join(' · ')} · {item.rounds}라운드</p></ListButton>)}</div></Section>
  </div>
 </Page>
}
export function NativeVideos(){
 const data=useCollection<Video[]>('/api/videos',[])
 const [active,setActive]=useState(''),[prompt,setPrompt]=useState(''),[backend,setBackend]=useState('local'),[videoBudget,setVideoBudget]=useState(5000),[heading,setHeading]=useState(''),[narration,setNarration]=useState(''),[seconds,setSeconds]=useState(4)
 const [editing,setEditing]=useState<number|null>(null),[editHeading,setEditHeading]=useState(''),[editNarration,setEditNarration]=useState(''),[editSeconds,setEditSeconds]=useState(4)
 const op=useOperation();const selected=data.value.find(x=>x.id===active)
 const create=async(e:FormEvent)=>{e.preventDefault();const scenes=heading.trim()&&narration.trim()?[{heading,narration,seconds}]:undefined
  const item=await op.run(()=>post<Video>('/api/videos',{prompt,backend,scenes,budget_krw:videoBudget}));if(item){setActive(item.id);void data.reload()}}
 const uploadImage=async(index:number,file?:File)=>{
  if(!file)return
  if(!['image/png','image/jpeg'].includes(file.type)||file.size>5*1024*1024){
   window.alert('PNG/JPEG 이미지만 5MB까지 첨부할 수 있습니다.');return
  }
  const dataUrl=await new Promise<string>((resolve,reject)=>{
   const reader=new FileReader()
   reader.onload=()=>resolve(String(reader.result||''))
   reader.onerror=()=>reject(new Error('이미지를 읽을 수 없습니다.'))
   reader.readAsDataURL(file)
  }).catch(()=>null)
  if(!dataUrl||!active)return
  const item=await op.run(()=>post<Video>(
   '/api/videos/'+encodeURIComponent(active)+'/scenes/'+index+'/image',{dataUrl}))
  if(item){data.setValue(old=>old.map(x=>x.id===item.id?item:x))}
 }
 const beginEdit=(index:number,scene:Scene)=>{
  setEditing(index);setEditHeading(scene.heading);setEditNarration(scene.narration);setEditSeconds(scene.seconds)
 }
 const saveScene=async(e:FormEvent)=>{e.preventDefault();if(editing===null||!active)return
  const item=await op.run(()=>post<Video>('/api/videos/'+encodeURIComponent(active)+'/scenes/'+editing,{
   heading:editHeading,narration:editNarration,seconds:editSeconds
  }))
  if(item){setEditing(null);data.setValue(old=>old.map(x=>x.id===item.id?item:x))}
 }
 const render=async()=>{const item=await op.run(()=>post<Video>('/api/videos/'+encodeURIComponent(active)+'/render',{}));if(item){data.setValue(old=>old.map(x=>x.id===item.id?item:x));void data.reload()}}
 useEffect(()=>{if(!active||selected?.status!=='rendering')return;const timer=setInterval(()=>{void data.reload()},3000);return()=>clearInterval(timer)},[active,selected?.status,data.reload])
 return <Page title="영상 스튜디오" description="장면 구성, 로컬 MP4 렌더링 또는 사용자가 선택한 외부 영상 모델을 연결합니다." eyebrow="VIDEO STUDIO"><div className={layout}>
  <div className="grid content-start gap-4">
   <Section><form onSubmit={e=>void create(e)} className="grid gap-3">
    <FormField label="영상 주제"><TextArea required rows={3} value={prompt} onChange={e=>setPrompt(e.target.value)}/></FormField>
    <FormField label="제작 방식"><SelectInput value={backend} onChange={e=>setBackend(e.target.value)}><option value="local">로컬 장면 렌더링</option><option value="veo">Veo (별도 API 요금)</option></SelectInput></FormField>
    <FormField label="영상 예산 한도 (원)"><TextInput type="number" value={videoBudget} min={0} max={1000000} onChange={e=>setVideoBudget(Number(e.target.value))}/></FormField>
    <details><summary className="cursor-pointer text-[12px] font-semibold">장면 직접 설정</summary><div className="mt-3 grid gap-2"><FormField label="장면 제목"><TextInput value={heading} onChange={e=>setHeading(e.target.value)}/></FormField><FormField label="내레이션"><TextArea value={narration} onChange={e=>setNarration(e.target.value)}/></FormField><FormField label="길이 (초)"><TextInput type="number" min={2} max={30} value={seconds} onChange={e=>setSeconds(Number(e.target.value))}/></FormField></div></details>
    <div><LoadingButton busy={op.busy} type="submit" variant="primary"><Clapperboard size={14}/> 프로젝트 만들기</LoadingButton></div>
   </form></Section>
   {selected&&<Section><div className="flex items-center justify-between"><h2 className="font-bold">{selected.prompt}</h2><Status value={selected.status}/></div><p className={'my-2 '+subtle}>제작 비용: {fmtKrw(selected.cost_krw)} / 한도 {fmtKrw(selected.budget_krw)} · 방식: {selected.backend}</p>
    <div className="my-4 grid gap-2">{selected.scenes.map((scene,i)=><div key={i} className="rounded-lg border border-[#EFE9DF] p-3 text-[12px]"><div className="flex items-center justify-between gap-2"><strong>{i+1}. {scene.heading}</strong><Button size="sm" variant="ghost" onClick={()=>beginEdit(i,scene)}>수정</Button></div><p className="mt-1 whitespace-pre-wrap">{scene.narration}</p><small>{scene.seconds}초</small>
    {selected.backend==='local'&&<label className="mt-2 flex flex-wrap items-center gap-2 rounded-lg border border-dashed border-[#D8CFC2] p-2 text-[11px]">
     <span>{selected.images?.[i]?'🖼️ 이미지 등록됨 · 교체':'🖼️ 이 장면에 이미지 첨부'}</span>
     <input type="file" accept="image/png,image/jpeg" className="max-w-full text-[11px]" onChange={e=>void uploadImage(i,e.target.files?.[0])} disabled={op.busy}/>
    </label>}</div>)}</div>
    {editing!==null&&<form onSubmit={e=>void saveScene(e)} className="mb-4 grid gap-2 rounded-xl bg-[#F6F3EC] p-3">
     <h3 className="font-semibold">{editing+1}번 장면 수정</h3>
     <FormField label="장면 제목"><TextInput required value={editHeading} maxLength={180} onChange={e=>setEditHeading(e.target.value)}/></FormField>
     <FormField label="내레이션"><TextArea value={editNarration} maxLength={1000} onChange={e=>setEditNarration(e.target.value)}/></FormField>
     <FormField label="길이 (초)"><TextInput type="number" min={2} max={30} value={editSeconds} onChange={e=>setEditSeconds(Number(e.target.value))}/></FormField>
     <p className={subtle}>수정한 장면 클립만 무효화하고 나머지 클립은 재사용합니다. 외부 영상 모델을 선택한 경우 수정 장면에 새 API 요금이 발생할 수 있습니다.</p>
     <div className="flex gap-2"><LoadingButton busy={op.busy} type="submit" variant="primary">장면 저장</LoadingButton><Button onClick={()=>setEditing(null)}>취소</Button></div>
    </form>}
    {selected.error&&<InlineError error={selected.error}/>}
    <div className={row}><LoadingButton busy={op.busy||selected.status==='rendering'} variant="primary" onClick={()=>void render()}><Play size={14}/> 렌더링</LoadingButton><Button onClick={()=>void data.reload()}><RefreshCw size={14}/> 상태 확인</Button>
    {selected.status==='completed'&&<a className="inline-flex items-center gap-2 rounded-lg border border-[#E7E0D5] bg-white px-4 py-2 text-[12px] font-semibold" href={'/api/videos/'+encodeURIComponent(active)+'/content'} download><Download size={14}/> MP4 받기</a>}</div>
   </Section>}
   <InlineError error={op.error}/>
  </div>
  <Section><div className="mb-3 flex justify-between"><h2 className="font-bold">영상 프로젝트</h2><Button size="sm" variant="ghost" onClick={()=>void data.reload()}><RefreshCw size={14}/></Button></div><InlineError error={data.error}/><div className="grid gap-2">{data.value.map(item=><ListButton selected={active===item.id} onClick={()=>{setActive(item.id);setEditing(null)}} key={item.id}><strong>{item.prompt}</strong><p className={subtle}>{item.status} · {item.scenes.length}개 장면</p></ListButton>)}</div></Section>
 </div></Page>
}
export function NativeImprovements(){
 const data=useCollection<Trial[]>('/api/improvements',[])
 const evolutions=useCollection<Evolution[]>('/api/evolutions',[])
 const [active,setActive]=useState(''),[repository,setRepository]=useState(''),[targetFile,setTargetFile]=useState(''),[instruction,setInstruction]=useState(''),[testCommand,setTestCommand]=useState('cargo test'),[scoreCommand,setScoreCommand]=useState('')
 const [generations,setGenerations]=useState(2),[evolutionBudget,setEvolutionBudget]=useState('1000'),[targetSequence,setTargetSequence]=useState('')
 const [candidateCount,setCandidateCount]=useState(2),[validationCommand,setValidationCommand]=useState('')
 const op=useOperation();const selected=data.value.find(x=>x.id===active)
 const related=evolutions.value.filter(run=>run.starting_trial===active||run.descendants.includes(active)||(run.candidate_groups||[]).some(ids=>ids.includes(active)))
 useEffect(()=>{if(!evolutions.value.some(run=>run.status==='running'))return
   const timer=window.setInterval(()=>{void evolutions.reload();void data.reload()},2500)
   return()=>window.clearInterval(timer)
 },[evolutions.value,evolutions.reload,data.reload])
 const create=async(e:FormEvent)=>{e.preventDefault();const item=await op.run(()=>post<Trial>('/api/improvements',{repository,target_file:targetFile,instruction,test_command:testCommand,score_command:scoreCommand.trim()||null,parent:null}));if(item){setActive(item.id);void data.reload()}}
 const act=async(action:'evaluate'|'adopt'|'descendant')=>{const item=await op.run(()=>post<Trial>('/api/improvements/'+encodeURIComponent(active)+'/'+action,{}));if(item){if(action==='descendant')setActive(item.id);void data.reload()}}
 const evolve=async()=>{
   const files=targetSequence.split(',').map(v=>v.trim()).filter(Boolean)
   const budget=Number(evolutionBudget)
   const item=await op.run(async()=>{
     if(!Number.isFinite(budget)||budget<1||budget>1000000)throw Error('원화 예산은 1~1,000,000원으로 입력하세요.')
     if(files.length!==0&&files.length!==generations)throw Error('세대별 파일 수를 세대 수와 일치시켜 주세요.')
     return post<Evolution>('/api/improvements/'+encodeURIComponent(active)+'/evolution',{
       max_generations:generations,cost_budget_krw:budget,target_files:files,
       candidates_per_generation:candidateCount,
       validation_command:validationCommand.trim()||null
     })
   })
   if(item){void evolutions.reload();void data.reload()}
 }
 const stopEvolution=async(id:string)=>{
   const run=await op.run(()=>post<Evolution>('/api/evolutions/'+encodeURIComponent(id)+'/stop',{}))
   if(run)void evolutions.reload()
 }
 return <Page title="자기개선 연구실" eyebrow="RECURSIVE SELF IMPROVEMENT" description="격리된 Git 작업공간에서 개선 후보를 만들고 원본과 비교한 결과를 보고 채택합니다.">
  <div className={layout}><div className="grid content-start gap-4">
   <Section><form onSubmit={e=>void create(e)} className="grid gap-3">
    <div className="grid gap-3 sm:grid-cols-2"><FormField label="저장소 경로 (상대 경로)"><TextInput required value={repository} onChange={e=>setRepository(e.target.value)} placeholder="my-project"/></FormField><FormField label="개선할 파일"><TextInput required value={targetFile} onChange={e=>setTargetFile(e.target.value)} placeholder="src/lib.rs"/></FormField></div>
    <FormField label="개선 지시"><TextArea required value={instruction} onChange={e=>setInstruction(e.target.value)} placeholder="작동을 보존하면서 비용을 줄이세요"/></FormField>
    <div className="grid gap-3 sm:grid-cols-2"><FormField label="테스트 명령"><TextInput required value={testCommand} onChange={e=>setTestCommand(e.target.value)}/></FormField><FormField label="점수 명령 (선택)"><TextInput value={scoreCommand} onChange={e=>setScoreCommand(e.target.value)}/></FormField></div>
    <div><LoadingButton busy={op.busy} type="submit" variant="primary"><Plus size={14}/> 개선 후보 생성</LoadingButton></div>
   </form></Section>
   {selected&&<Section><div className="flex items-center justify-between"><h2 className="font-bold">{selected.target_file}</h2><Status value={selected.status}/></div><p className={'my-2 '+subtle}>{selected.instruction}</p>
    <div className="grid grid-cols-2 gap-2 rounded-xl bg-[#F7F3EC] p-3 text-[12px]"><div>기준 측정<pre>{selected.baseline?JSON.stringify(selected.baseline,null,2):'대기'}</pre></div><div>후보 측정<pre>{selected.candidate?JSON.stringify(selected.candidate,null,2):'대기'}</pre></div></div>
    {selected.error&&<InlineError error={selected.error}/>}
    <div className={'mt-3 '+row}><LoadingButton busy={op.busy||selected.status!=='awaiting_approval'} onClick={()=>void act('evaluate')}>독립 평가</LoadingButton><LoadingButton busy={op.busy||selected.status!=='evaluated'} variant="primary" onClick={()=>void act('adopt')}>검증 후보 채택</LoadingButton><LoadingButton busy={op.busy||selected.status!=='adopted'} onClick={()=>void act('descendant')}>후손 실험</LoadingButton></div>
   </Section>}
   {selected?.status==='adopted'&&<Section><h2 className="mb-2 font-bold">연속 진화 · A→B→A</h2>
    <p className={subtle}>같은 Git 기준본에서 후보를 여러 개 생성해 경쟁시킨 후, 검증된 1개만 다음 세대에 계승합니다. 별도 검증 명령을 설정하면 훈련 점수가 높아도 검증 기준을 통과하지 못한 후보는 채택하지 않습니다.</p>
    <div className="my-3 grid gap-3 sm:grid-cols-2">
     <FormField label="세대 수 (최대 4)"><SelectInput value={String(generations)} onChange={e=>setGenerations(Number(e.target.value))}><option value="1">1세대</option><option value="2">2세대</option><option value="3">3세대</option><option value="4">4세대</option></SelectInput></FormField>
     <FormField label="누적 모델 예산 (원)"><TextInput value={evolutionBudget} onChange={e=>setEvolutionBudget(e.target.value)} placeholder="1000"/></FormField>
    </div>
    <div className="grid gap-3 sm:grid-cols-2"><FormField label="세대당 후보 경쟁 수"><SelectInput value={String(candidateCount)} onChange={e=>setCandidateCount(Number(e.target.value))}><option value="1">1개</option><option value="2">2개</option><option value="3">3개</option><option value="4">4개</option></SelectInput></FormField><FormField label="검증용 별도 점수 명령 (선택)"><TextInput value={validationCommand} onChange={e=>setValidationCommand(e.target.value)} placeholder="node scripts/score-validation.mjs"/></FormField></div>
    <FormField label="개선 파일 순서 (선택, 쉼표로 구분)"><TextInput value={targetSequence} onChange={e=>setTargetSequence(e.target.value)} placeholder="src/engine.rs, src/strategy.rs, src/engine.rs"/></FormField>
    <p className={'mt-2 '+subtle}>비우면 같은 파일을 반복합니다. 파일을 지정하면 세대 수만큼 입력하세요. 비용은 제공자 사용량을 기준으로 추산하며 호출 1회의 초과 청구 가능성은 남습니다.</p>
    <div className="mt-3"><LoadingButton busy={op.busy||related.some(run=>run.status==='running')} variant="primary" onClick={()=>void evolve()}><Play size={14}/> 검증된 연속 진화 실행</LoadingButton></div>
   </Section>}
   {related.length>0&&<Section><div className="mb-3 flex items-center justify-between"><h2 className="font-bold">세대별 검증 기록</h2><Button size="sm" onClick={()=>{void evolutions.reload();void data.reload()}}><RefreshCw size={14}/> 갱신</Button></div>
    <div className="grid gap-3">{related.map(run=><div key={run.id} className="rounded-lg border border-[#E7E0D5] p-3 text-[12px]">
     <div className="flex items-center justify-between gap-2"><strong>{run.completed_generations}/{run.max_generations}세대 채택</strong><div className={row}><Status value={run.status}/>{run.status==='running'&&(run.stop_requested?<span className={subtle}>중단 요청됨</span>:<LoadingButton busy={op.busy} size="sm" onClick={()=>void stopEvolution(run.id)}><PauseCircle size={13}/> 중단</LoadingButton>)}</div></div>
     <p className={'mt-1 '+subtle}>{run.target_files.join(' → ')} · 세대당 {run.candidates_per_generation||1}개 후보 · 모델 비용 {fmtKrw(run.spent_krw)} / {fmtKrw(run.cost_budget_krw)}</p>
     <div className={'mt-2 '+row}>{run.descendants.map((id,i)=><Button key={id} size="sm" variant="ghost" onClick={()=>setActive(id)}>{i+1}세대 채택</Button>)}</div>
     {(run.candidates||[]).length>0&&<div className="mt-3 grid gap-2">{(run.candidates||[]).map(item=><div key={item.trial_id} className="rounded-lg border border-[#ECE5DB] bg-[#F8F6F1] p-2">
       <div className="flex flex-wrap items-center justify-between gap-2"><Button size="sm" variant="ghost" onClick={()=>setActive(item.trial_id)}>{item.generation+1}세대 · 후보 {item.proposal_index+1}</Button><span className={subtle}>{run.descendants.includes(item.trial_id)?'채택':item.eligible?(run.status==='running'?'검증 통과 · 경쟁 진행 중':'검증 통과 · 미채택'):'제외'}</span></div>
       <p className={subtle}>훈련 {item.training_score??'—'} · 독립 검증 {item.validation_score??'—'} (기준 {item.validation_baseline_score??'—'})</p>
       {item.reason&&<p className={subtle}>{item.reason}</p>}
     </div>)}</div>}
     {run.stop_reason&&<InlineError error={run.stop_reason}/>}
    </div>)}</div>
   </Section>}
   <InlineError error={op.error}/>
  </div>
  <Section><div className="mb-3 flex justify-between"><h2 className="font-bold">실험 기록</h2><Button variant="ghost" onClick={()=>void data.reload()}><RefreshCw size={14}/></Button></div><InlineError error={data.error}/><div className="grid gap-2">{data.value.map(item=><ListButton key={item.id} selected={active===item.id} onClick={()=>setActive(item.id)}><strong>{item.repository} / {item.target_file}</strong><p className={subtle}>{item.status} · {fmtKrw(item.cost_krw)}</p></ListButton>)}</div></Section>
 </div>
 </Page>
}
export function NativeKnowledge(){
 const memories=useCollection<Memory[]>('/api/memory',[]),skills=useCollection<Skill[]>('/api/skills',[])
 const [content,setContent]=useState(''),[kind,setKind]=useState('preference'),[sessionId,setSessionId]=useState('')
 const [name,setName]=useState(''),[trigger,setTrigger]=useState(''),[instructions,setInstructions]=useState('')
 const op=useOperation()
 const remember=async(e:FormEvent)=>{e.preventDefault();const m=await op.run(()=>post<Memory>('/api/memory',{content,kind,sessionId:sessionId.trim()||null}));if(m){setContent('');void memories.reload()}}
 const skill=async(e:FormEvent)=>{e.preventDefault();const v=await op.run(()=>post<Skill>('/api/skills',{name,trigger,instructions,enabled:false}));if(v){setName('');setTrigger('');setInstructions('');void skills.reload()}}
 return <Page title="기억 · 스킬" eyebrow="KNOWLEDGE & EXTENSIONS" description="지속적인 선호와 작업 방법을 Rust 저장소에 기록하고 필요한 스킬만 활성화합니다.">
  <div className="grid gap-4 lg:grid-cols-2">
   <div className="grid content-start gap-4"><Section><h2 className="mb-3 font-bold">새 기억</h2><form onSubmit={e=>void remember(e)} className="grid gap-3">
    <FormField label="기억 내용"><TextArea value={content} onChange={e=>setContent(e.target.value)} required maxLength={4096}/></FormField>
    <div className="grid gap-2 sm:grid-cols-2"><FormField label="종류"><SelectInput value={kind} onChange={e=>setKind(e.target.value)}><option value="preference">선호</option><option value="fact">사실</option><option value="correction">정정</option><option value="procedure">절차</option></SelectInput></FormField><FormField label="대화 ID (비우면 전체)"><TextInput value={sessionId} maxLength={128} onChange={e=>setSessionId(e.target.value)}/></FormField></div>
    <div><LoadingButton busy={op.busy} type="submit" variant="primary"><Database size={14}/> 기억 저장</LoadingButton></div></form></Section>
    <Section><h2 className="mb-3 font-bold">기억 목록</h2><InlineError error={memories.error}/><div className="grid gap-2">{memories.value.map(item=><div key={item.id} className="rounded-lg border border-[#ECE5DB] p-3 text-[12px]"><p className="whitespace-pre-wrap">{item.content}</p><div className="mt-2 flex items-center justify-between"><span className={subtle}>{item.kind} · {item.status}</span>{item.status==='active'&&<Button size="sm" variant="ghost" onClick={()=>void op.run(()=>post('/api/memory/'+encodeURIComponent(item.id)+'/disable',{})).then(()=>void memories.reload())}>비활성화</Button>}</div></div>)}</div></Section></div>
   <div className="grid content-start gap-4"><Section><h2 className="mb-3 font-bold">새 스킬</h2><form onSubmit={e=>void skill(e)} className="grid gap-3"><FormField label="이름"><TextInput value={name} onChange={e=>setName(e.target.value)} required maxLength={100}/></FormField><FormField label="호출 조건"><TextInput value={trigger} onChange={e=>setTrigger(e.target.value)} maxLength={200}/></FormField><FormField label="실행 지침"><TextArea value={instructions} onChange={e=>setInstructions(e.target.value)} required maxLength={12000}/></FormField><div><LoadingButton busy={op.busy} type="submit" variant="primary"><Plus size={14}/> 스킬 저장</LoadingButton></div></form></Section>
    <Section><h2 className="mb-3 font-bold">스킬 목록</h2><InlineError error={skills.error}/><div className="grid gap-2">{skills.value.map(item=><div key={item.id} className="rounded-lg border border-[#ECE5DB] p-3 text-[12px]"><strong>{item.name}</strong><p className={subtle}>{item.trigger}</p><div className="mt-2 flex items-center justify-between"><span>{item.enabled?'활성':'비활성'}</span><Button size="sm" onClick={()=>void op.run(()=>post('/api/skills/'+encodeURIComponent(item.id)+'/enable',{enabled:!item.enabled})).then(()=>void skills.reload())}>{item.enabled?'비활성화':'활성화'}</Button></div></div>)}</div></Section>
   </div><InlineError error={op.error}/>
  </div>
 </Page>
}
export function NativeConnections(){
 const data=useCollection<Health>('/api/health',{ok:false,engine:'',model:'',workspace:''})
 return <Page title="연결 · 실행 환경" eyebrow="NATIVE RUST CONNECTIONS" description="이 화면은 Rust 엔진의 실제 연결 상태를 표시합니다. 모델 제공자는 서버 환경 설정으로 연결합니다.">
  <Section><div className="mb-3 flex items-center justify-between"><h2 className="text-[14px] font-bold">엔진 연결</h2><Button onClick={()=>void data.reload()}><RefreshCw size={14}/> 새로고침</Button></div>
  <InlineError error={data.error}/><div className="grid gap-3 sm:grid-cols-2">{[['상태',data.value.ok?'연결됨':'연결되지 않음'],['실행 엔진',data.value.engine],['현재 모델',data.value.model],['작업공간',data.value.workspace]].map(([label,value])=><div className="rounded-lg bg-[#F6F3EC] p-4" key={label}><p className={subtle}>{label}</p><strong className="break-words text-[14px]">{value||'—'}</strong></div>)}</div>
  <div className="mt-4 rounded-lg border border-[#ECE5DB] p-4 text-[12px]"><h3 className="mb-2 font-bold">모델 연결 설정</h3><p className={subtle}>DOMINION_MODEL_URL, DOMINION_MODEL, DOMINION_API_KEY 환경변수로 모델 제공자를 설정합니다. 키 값은 이 화면에 표시하지 않습니다.</p></div>
  </Section>
 </Page>
}
export function NativeActivity(){return <NativeTasks/>}
