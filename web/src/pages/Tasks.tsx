import {useEffect,useMemo,useState} from 'react'
import {ArrowRight,CheckCircle2,CircleAlert,Clock3,Cloud,Play,RefreshCw,Workflow} from 'lucide-react'
import {Button,PageContainer} from '../components/Primitives'
import {Notice} from '../components/UI'
import {PageHeader,ViewTabs,SearchField,EmptyState} from '../components/ui/WorkspaceUI'
import {JobWorkflowCard,type WorkflowJob} from '../components/JobWorkflowCard'
import {TeamWorkflowPanel} from '../components/TeamWorkflowPanel'
import {NativeAgentPanel} from '../components/NativeAgentPanel'
import {RemoteWorkPanel} from '../components/RemoteWorkPanel'
import {useRemote} from '../hooks/useRemote'
import {useLocale} from '../i18n'
import {workspaceView} from '../utils/workspaceRouting'
import type {Provider} from '../api'

type TaskView='list'|'native'|'remote'
type JobState='all'|'ongoing'|'attention'|'completed'|'cancelled'|'other'
const ongoing=new Set(['queued','running','planning','waiting','pending','reviewing','repairing','executing'])
const attention=new Set(['needs_review','failed','interrupted','uncertain','waiting_for_approval','awaiting_approval','in_doubt','budget_exceeded','answered_unverified','exhausted','denied'])
const stateOf=(job:WorkflowJob):Exclude<JobState,'all'>=>ongoing.has(job.status||'')?'ongoing':attention.has(job.status||'')?'attention':job.status==='completed'||job.status==='answered_verified'?'completed':job.status==='cancelled'?'cancelled':'other'
const priority={attention:0,ongoing:1,other:2,completed:3,cancelled:4}
const readView=():TaskView=>{const view=workspaceView(window.location.hash,'tasks');return view==='native'||view==='remote'?view:'list'}

export function TasksPage(){
 const en=useLocale()==='en',tr=(ko:string,english:string)=>en?english:ko
 const [view,setView]=useState<TaskView>(readView)
 const [filter,setFilter]=useState<JobState>('all')
 const [query,setQuery]=useState('')
 const [limit,setLimit]=useState(24)
 useEffect(()=>{
  const sync=()=>setView(readView())
  window.addEventListener('hashchange',sync);window.addEventListener('popstate',sync)
  return()=>{window.removeEventListener('hashchange',sync);window.removeEventListener('popstate',sync)}
 },[])
 const choose=(next:TaskView)=>{setView(next);window.location.hash=next==='list'?'#/tasks':'#/tasks?view='+next}
 const jobs=useRemote<WorkflowJob[]>(view==='list'?'/api/jobs':null,[])
 const providers=useRemote<Provider[]>(view==='native'?'/api/providers':null,[])
 const views:{id:JobState;label:string;icon:typeof Workflow}[]=[{id:'all',label:tr('전체','All'),icon:Workflow},{id:'ongoing',label:tr('진행 중','In progress'),icon:Clock3},{id:'attention',label:tr('확인 필요','Needs attention'),icon:CircleAlert},{id:'completed',label:tr('완료','Completed'),icon:CheckCircle2},{id:'cancelled',label:tr('중단','Stopped'),icon:CircleAlert},{id:'other',label:tr('기타','Other'),icon:Workflow}]
 const counts=useMemo(()=>{const result:Record<JobState,number>={all:jobs.value.length,ongoing:0,attention:0,completed:0,cancelled:0,other:0};for(const job of jobs.value)result[stateOf(job)]++;return result},[jobs.value])
 const filtered=useMemo(()=>{
  const needle=query.trim().toLocaleLowerCase()
  return jobs.value.filter(job=>(filter==='all'||stateOf(job)===filter)&&[job.input?.goal,job.input?.prompt,job.id,job.kind,job.status].some(value=>(value||'').toLocaleLowerCase().includes(needle))).sort((a,b)=>priority[stateOf(a)]-priority[stateOf(b)]||(b.queuedAt||b.createdAt||'').localeCompare(a.queuedAt||a.createdAt||''))
 },[jobs.value,filter,query])
 const modes:{id:TaskView;label:string;icon:typeof Workflow}[]=[{id:'list',label:tr('작업 목록','Task list'),icon:Workflow},{id:'native',label:tr('자동 작업','Autonomous work'),icon:Play},{id:'remote',label:tr('원격 작업','Remote work'),icon:Cloud}]
 return <PageContainer className="!max-w-[1200px]">
  <PageHeader title={tr('작업 관리','Work center')} description={tr('작업을 맡기고, 진행 상태와 실제 결과를 확인하세요.','Start work and review its progress and results.')} actions={view==='list'?<Button variant="secondary" onClick={()=>void jobs.reload()} disabled={jobs.refreshing}><RefreshCw size={16} className={jobs.refreshing?'animate-spin':''}/>{tr('새로고침','Refresh')}</Button>:undefined}/>
  <nav aria-label={tr('작업 방식','Work modes')} className="mb-5 grid grid-cols-1 gap-1 rounded-xl border border-divider bg-canvas p-1.5 sm:grid-cols-3">{modes.map(({id,label,icon:Icon})=><Button key={id} variant="bare" aria-current={view===id?'page':undefined} onClick={()=>choose(id)} className="flex min-h-11 items-center justify-center gap-2 rounded-lg px-3 text-sm font-medium text-muted hover:bg-surface aria-[current=page]:bg-surface aria-[current=page]:font-semibold aria-[current=page]:text-brand-hover aria-[current=page]:shadow-sm"><Icon size={17}/>{label}</Button>)}</nav>
  {view==='native'&&<><Notice message={providers.error} onRetry={()=>void providers.reload()}/><NativeAgentPanel providers={providers.value}/></>}
  {view==='remote'&&<RemoteWorkPanel initialOpen/>}
  {view==='list'&&<>
   <div className="mb-5 grid grid-cols-2 gap-2 sm:grid-cols-4">{views.filter(item=>['all','ongoing','attention','completed'].includes(item.id)).map(({id,label,icon:Icon})=><Button key={id} variant="bare" onClick={()=>{setFilter(id);setLimit(24)}} aria-pressed={filter===id} className="flex min-h-20 flex-col items-start justify-between gap-2 rounded-xl border border-divider bg-surface p-3 text-left hover:bg-canvas aria-pressed:border-brand/40 aria-pressed:bg-brand-soft"><span className="flex w-full items-center justify-between gap-2 text-xs font-medium text-muted">{label}<Icon size={17}/></span><strong className="text-2xl font-semibold tabular-nums text-ink">{jobs.loading?'—':counts[id]}</strong></Button>)}</div>
   <ViewTabs<JobState> value={filter} onChange={value=>{setFilter(value);setLimit(24)}} label={tr('작업 상태','Task status')} options={views.filter(item=>item.id!=='other'||counts.other>0).map(item=>({...item,count:counts[item.id]}))}/>
   <div className="mb-4 flex flex-wrap items-center justify-between gap-3"><div className="flex min-w-0 flex-1 sm:max-w-md"><SearchField value={query} onChange={value=>{setQuery(value);setLimit(24)}} label={tr('작업 검색','Search tasks')} placeholder={tr('작업 내용·ID 검색','Search instructions or ID')}/></div><span className="text-xs text-muted">{tr('확인 필요 → 진행 중 → 최신순','Attention → active → newest')}</span></div>
   <Notice message={jobs.error} onRetry={()=>void jobs.reload()}/>
   {jobs.loading&&!jobs.error&&<p role="status" className="py-10 text-center text-sm text-muted">{tr('작업 불러오는 중…','Loading tasks…')}</p>}
   <div className="grid gap-3">{filtered.slice(0,limit).map(job=><JobWorkflowCard key={job.id} job={job} onChanged={()=>void jobs.reload()}/>)}</div>
   {!jobs.loading&&!jobs.error&&!filtered.length&&<EmptyState icon={Workflow} title={tr(query||filter!=='all'?'일치하는 작업이 없습니다.':'아직 맡긴 작업이 없습니다.',query||filter!=='all'?'No matching tasks.':'No tasks yet.')} description={tr('자동 작업을 직접 시작하거나 그룹 대화에서 협업을 요청할 수 있습니다.','Start autonomous work directly or delegate in a group conversation.')} action={<div className="flex flex-wrap justify-center gap-2"><Button variant="primary" onClick={()=>choose('native')}><Play size={16}/>{tr('자동 작업 시작','Start autonomous work')}</Button><a href="#/rooms" className="inline-flex min-h-10 items-center gap-2 rounded-xl border border-divider bg-surface px-4 py-2 text-sm font-semibold text-ink hover:bg-canvas">{tr('그룹 대화 열기','Open groups')}<ArrowRight size={16}/></a></div>}/>}
   {filtered.length>limit&&<Button variant="secondary" className="mt-4" onClick={()=>setLimit(n=>n+24)}>{tr('더 보기','Load more')}</Button>}
   {filtered.some(job=>job.kind==='team'||job.kind==='external_agent')&&<section className="mt-8"><h3 className="mb-3 text-base font-semibold">{tr('팀 작업과 외부 실행 상세','Team and external execution details')}</h3><TeamWorkflowPanel visibleIds={filtered.map(job=>job.id)}/></section>}
  </>}
 </PageContainer>
}
