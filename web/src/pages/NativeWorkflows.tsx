import {useCallback,useEffect,useState,type FormEvent} from 'react'
import {ListOrdered,Plus,RefreshCw,Play,ArrowRight,ShieldCheck} from 'lucide-react'
import {get,post} from '../api'
import {Button,PageContainer,TextArea,TextInput} from '../components/Primitives'
import {Headline,FormField,Notice,Panel,StateTag} from '../components/UI'

type Workflow={id:string;name:string;steps:string[];createdAt:string}
type FlowRun={id:string;workflowId:string;status:string;currentStep:number;tasks:string[];
 error:string|null;createdAt:string}
const stop=new Set(['completed','failed','needs_review'])
const text='text-[12px] text-[#7C7469]'
export function NativeWorkflows({openSession}:{openSession:(id:string)=>void}){
 const [workflows,setWorkflows]=useState<Workflow[]>([])
 const [current,setCurrent]=useState(''),[runs,setRuns]=useState<FlowRun[]>([])
 const [name,setName]=useState(''),[draft,setDraft]=useState(['','']),[busy,setBusy]=useState(false)
 const [error,setError]=useState(''),[allowWrite,setAllowWrite]=useState(false),[allowShell,setAllowShell]=useState(false)
 const [allowMcp,setAllowMcp]=useState(false),[tokenBudget,setTokenBudget]=useState(8000),[costBudget,setCostBudget]=useState('')
 const reload=useCallback(async()=>{
  try{setWorkflows(await get<Workflow[]>('/api/workflows'));setError('')}
  catch(e){setError((e as Error).message)}
 },[])
 const reloadRuns=useCallback(async()=>{
  if(!current)return
  try{setRuns(await get<FlowRun[]>('/api/workflows/'+encodeURIComponent(current)+'/runs'))}
  catch(e){setError((e as Error).message)}
 },[current])
 useEffect(()=>{void reload()},[reload])
 useEffect(()=>{void reloadRuns()},[reloadRuns])
 useEffect(()=>{if(!runs.some(run=>!stop.has(run.status)))return
  const timer=setInterval(()=>{void reloadRuns()},3000)
  return()=>clearInterval(timer)
 },[runs,reloadRuns])
 const create=async(e:FormEvent)=>{e.preventDefault();setBusy(true);setError('')
  try{
   const value=await post<Workflow>('/api/workflows',{name,steps:draft.map(s=>s.trim()).filter(Boolean)})
   setWorkflows(previous=>[value,...previous]);setCurrent(value.id);setName('');setDraft(['',''])
  }catch(e){setError((e as Error).message)}finally{setBusy(false)}
 }
 const execute=async()=>{
  if(!current)return
  setBusy(true);setError('')
  try{
   const value=await post<FlowRun>('/api/workflows/'+encodeURIComponent(current)+'/runs',{
    requestId:crypto.randomUUID(),allowWrite,allowShell,allowMcp,allowSubagents:false,
    requireEvidence:allowWrite,tokenBudget,maxSteps:12,planningMode:'auto',modelTier:'auto',
    costBudgetKrw:costBudget.trim()?Number(costBudget):undefined
   })
   setRuns(previous=>[value,...previous.filter(run=>run.id!==value.id)])
  }catch(e){setError((e as Error).message)}finally{setBusy(false)}
 }
 const chosen=workflows.find(w=>w.id===current)
 return <PageContainer><Headline eyebrow="NATIVE WORKFLOW ENGINE" title="작업 워크플로"
  description="반복 작업을 단계별로 저장하고, 각 단계를 공통 Rust 에이전트 엔진으로 실행합니다. 실패한 단계의 작업 기록은 그대로 유지합니다."/>
  <div className="grid gap-4 xl:grid-cols-[minmax(0,1.15fr)_minmax(0,1fr)]">
   <div className="grid content-start gap-4">
    <Panel className="p-5"><h2 className="mb-3 text-[14px] font-bold">새 워크플로 만들기</h2>
     <form className="grid gap-3" onSubmit={e=>void create(e)}>
      <FormField label="작업 이름"><TextInput required maxLength={100} value={name} onChange={e=>setName(e.target.value)}/></FormField>
      <div className="space-y-3">{draft.map((step,index)=><FormField key={index} label={index+1+'단계 작업'}>
       <TextArea rows={2} maxLength={8000} value={step} onChange={e=>setDraft(items=>items.map((item,i)=>i===index?e.target.value:item))} placeholder="실제 수행할 작업 지시"/></FormField>)}</div>
      <div className="flex gap-2"><Button onClick={()=>setDraft(items=>items.length>=12?items:[...items,''])} disabled={draft.length>=12}><Plus size={13}/> 단계 추가</Button><Button variant="ghost" disabled={draft.length<=1} onClick={()=>setDraft(items=>items.slice(0,-1))}>마지막 단계 삭제</Button></div>
      <div><Button variant="primary" type="submit" disabled={busy||!name.trim()||!draft.some(x=>x.trim())}><ListOrdered size={14}/> 워크플로 저장</Button></div>
     </form>
    </Panel>
    {chosen&&<Panel className="p-5"><div className="mb-3 flex items-center justify-between"><h2 className="text-[14px] font-bold">{chosen.name}</h2><span className={text}>{chosen.steps.length}단계</span></div>
     <ol className="mb-4 space-y-2">{chosen.steps.map((prompt,index)=><li key={index} className="flex gap-2 rounded-lg border border-[#ECE5DB] p-3 text-[12px]"><strong>{index+1}.</strong><span className="whitespace-pre-wrap">{prompt}</span></li>)}</ol>
     <div className="mb-4 grid gap-2 text-[12px] sm:grid-cols-2">
      {([['파일 수정 허용',allowWrite,setAllowWrite],['검사 명령 허용',allowShell,setAllowShell],['MCP 허용',allowMcp,setAllowMcp]] as const)
       .map(([label,checked,set])=><label key={label} className="flex items-center gap-2"><input type="checkbox" checked={checked} onChange={e=>set(e.target.checked)}/>{label}</label>)}
      <FormField label="단계별 토큰 한도"><TextInput type="number" min={256} max={500000} value={tokenBudget} onChange={e=>setTokenBudget(Number(e.target.value))}/></FormField>
      <FormField label="단계별 원화 예산 (선택)"><TextInput type="number" min={0} max={10000000} value={costBudget} onChange={e=>setCostBudget(e.target.value)}/></FormField>
     </div>
     <p className={'mb-3 '+text}><ShieldCheck size={13} className="mr-1 inline"/> 각 단계는 별도로 허용한 작업 권한과 예산 범위 안에서 실행됩니다.</p>
     <Button variant="primary" disabled={busy} onClick={()=>void execute()}><Play size={14}/> 워크플로 실행</Button>
    </Panel>}
   </div>
   <div className="grid content-start gap-4">
    <Panel className="p-5"><div className="mb-3 flex justify-between"><h2 className="font-bold">저장된 워크플로</h2><Button size="sm" variant="ghost" onClick={()=>void reload()}><RefreshCw size={13}/></Button></div>
     <div className="grid max-h-80 gap-2 overflow-auto">{workflows.map(flow=><Button key={flow.id} variant="bare"
      onClick={()=>{setCurrent(flow.id);setRuns([])}} className={'rounded-lg border px-3 py-3 text-left '+(current===flow.id?'border-[#D7A78B] bg-[#F9EFE8]':'border-[#ECE5DB]')}>
      <strong className="block">{flow.name}</strong><span className={text}>{flow.steps.length}단계 · {new Date(flow.createdAt).toLocaleString('ko-KR')}</span>
     </Button>)}</div>
    </Panel>
    {chosen&&<Panel className="p-5"><div className="mb-3 flex items-center justify-between"><h2 className="font-bold">실행 기록</h2><Button variant="ghost" size="sm" onClick={()=>void reloadRuns()}><RefreshCw size={13}/> 새로고침</Button></div>
     <div className="grid gap-3">{runs.map(run=><div key={run.id} className="rounded-xl border border-[#ECE5DB] p-3 text-[12px]">
      <div className="mb-2 flex items-center justify-between"><strong>{run.currentStep}/{chosen.steps.length}단계 진행</strong><StateTag online={run.status==='completed'}>{run.status}</StateTag></div>
      {run.error&&<p className="mb-2 text-rose-700">{run.error}</p>}
      <div className="grid gap-1">{run.tasks.map((taskId,index)=><Button key={taskId} variant="ghost" className="justify-start text-[11px]" onClick={()=>openSession(taskId)}><ArrowRight size={12}/> {index+1}단계 실행 기록 · {taskId.slice(0,8)}</Button>)}</div>
     </div>)}</div>
    </Panel>}
   </div>
  </div><Notice message={error}/>
 </PageContainer>
}
