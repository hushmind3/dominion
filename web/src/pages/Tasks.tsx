import {useState} from 'react'
import {ArrowRight, CheckCircle2, CircleAlert, Clock3, RefreshCw, Workflow} from 'lucide-react'
import {Button, PageContainer} from '../components/Primitives'
import {Notice} from '../components/UI'
import {PageHeader, ViewTabs, SearchField, EmptyState} from '../components/ui/WorkspaceUI'
import {JobWorkflowCard, type WorkflowJob} from '../components/JobWorkflowCard'
import {TeamWorkflowPanel} from '../components/TeamWorkflowPanel'
import {useRemote} from '../hooks/useRemote'
import {useLocale} from '../i18n'

const ongoing=new Set(['queued','running','planning','waiting','pending'])
const attention=new Set(['needs_review','failed','interrupted','uncertain','waiting_for_approval','budget_exceeded','answered_unverified'])
const stateOf=(job:WorkflowJob)=>ongoing.has(job.status||'')?'ongoing':attention.has(job.status||'')?'attention':job.status==='completed'||job.status==='answered_verified'?'completed':job.status==='cancelled'?'cancelled':'other'

export function TasksPage() {
 const en=useLocale()==='en'
 const tr=(ko:string,english:string)=>en?english:ko
 const jobs=useRemote<WorkflowJob[]>('/api/jobs',[])
 const [filter,setFilter]=useState('all')
 const [query,setQuery]=useState('')
 const views=[{id:'all',label:tr('전체 작업','All tasks'),icon:Workflow},{id:'ongoing',label:tr('진행 중','In progress'),icon:Clock3},{id:'attention',label:tr('확인 필요','Needs attention'),icon:CircleAlert},{id:'completed',label:tr('완료','Completed'),icon:CheckCircle2},{id:'cancelled',label:tr('중단됨','Stopped'),icon:CircleAlert}]
 const filtered=jobs.value.filter(job=>(filter==='all'||stateOf(job)===filter)&&(job.input?.goal||job.input?.prompt||'').toLocaleLowerCase().includes(query.trim().toLocaleLowerCase()))
 return <PageContainer>
  <PageHeader title={tr('맡긴 작업','Your tasks')} description={tr('진행 중인 작업에 추가 지시를 보내고, 완료된 결과를 확인하세요.','Add instructions to ongoing tasks and review completed results.')} actions={<Button variant="secondary" onClick={()=>void jobs.reload()} disabled={jobs.refreshing}><RefreshCw size={16} className={jobs.refreshing?'animate-spin':''}/>{tr('새로고침','Refresh')}</Button>}/>
  <ViewTabs value={filter} onChange={setFilter} label={tr('작업 상태','Task status')} options={views.map(item=>({...item,count:item.id==='all'?jobs.value.length:jobs.value.filter(job=>stateOf(job)===item.id).length}))}/>
  <div className="mb-6 flex max-w-xl"><SearchField value={query} onChange={setQuery} label={tr('작업 검색','Search tasks')} placeholder={tr('맡긴 내용으로 검색','Search by task description')}/></div>
  <Notice message={jobs.error} onRetry={()=>void jobs.reload()}/>
  <div className="grid gap-4">{filtered.slice(0,60).map(job=><JobWorkflowCard key={job.id} job={job} onChanged={()=>void jobs.reload()}/>)}</div>
  {!jobs.loading&&!jobs.error&&!filtered.length&&<EmptyState icon={Workflow} title={tr(query?'검색 결과가 없습니다.':'아직 맡긴 작업이 없습니다.',query?'No matching tasks.':'No tasks yet.')} description={tr('대화나 그룹에서 할 일을 맡기면 이곳에서 진행 상태와 결과를 이어서 볼 수 있습니다.','Start a task from a conversation or group, then follow its progress and results here.')} action={<a href="#/rooms" className="inline-flex min-h-10 items-center gap-2 rounded-xl border border-divider bg-surface px-4 py-2 text-sm font-semibold text-ink hover:bg-canvas">{tr('그룹에서 작업 맡기기','Open a group')}<ArrowRight size={16}/></a>}/>}
  {jobs.value.some(job=>job.kind==='team')&&<TeamWorkflowPanel/>}
 </PageContainer>
}
