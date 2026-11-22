import {AppUpdatePanel} from '../components/AppUpdatePanel'
import {StatusPill} from '../components/StatusPill'
import {Button,TextInput,SelectInput} from '../components/Primitives'
import {useEffect,useRef,useState} from 'react'
import {Activity,Check,KeyRound,Lock,RefreshCw,Shield,Sparkles,TerminalSquare,Workflow} from 'lucide-react'
import {Agent,Provider,get,post,setAuthToken} from '../api'
import {t,useLocale,setLocale,formatDateTime,auditEventLabel} from '../i18n'
import {translateError} from '../i18n/errors'
import {statusLabel} from '../components/StatusPill'
import {Action,Empty,FormField,Headline,Notice,Panel,Subheading,refresh,useRemote} from '../components/UI'
import {BackupPanel} from './operations/Backup'
import {ExecutionInsights} from '../components/ExecutionInsights'
import {ExtensionRuntimeControls} from '../components/ExtensionRuntimeControls'
import {JobWorkflowCard} from '../components/JobWorkflowCard'
import {RemoteWorkPanel} from '../components/RemoteWorkPanel'
import {RunEventTimeline} from '../components/RunEventTimeline'
import {TeamWorkflowPanel} from '../components/TeamWorkflowPanel'
export function OperationsPage({section}:{section:'activity'|'settings'}){
 const loc=useLocale()
 const [advancedSettingsOpen,setAdvancedSettingsOpen]=useState(false)
 const [activityDetails,setActivityDetails]=useState(false)
 const jobs=useRemote<any[]>(section==='activity'?'/api/jobs':null,[]),audit=useRemote<any[]>(section==='activity'&&activityDetails?'/api/audit':null,[]);
 const executions=useRemote<any[]>(section==='activity'&&activityDetails?'/api/executions?limit=100':null,[])
 const browserStatus=useRemote<any>(section==='activity'&&activityDetails?'/api/browser/status':null,{status:{available:false,running:false},screenshot:null})
 const [shotTick,setShotTick]=useState(0)
 const [selectedExecution,setSelectedExecution]=useState(''),[operations,setOperations]=useState<any[]>([])
 useEffect(()=>{if(!selectedExecution){setOperations([]);return}let live=true;get<any[]>('/api/executions/'+selectedExecution+'/operations').then(rows=>{if(live)setOperations(rows)}).catch(()=>{if(live)setOperations([])});return()=>{live=false}},[selectedExecution])
 const tools=useRemote<any[]>(section==='settings'&&advancedSettingsOpen?'/api/tools':null,[]);
 const policy=useRemote<any>(section==='settings'&&advancedSettingsOpen?'/api/policy':null,{id:'default',autoApprove:[],autoApproveMcp:false,autoApproveAll:false});
 const [error,setError]=useState(''),[message,setMessage]=useState(''),[token,setToken]=useState('');
 const [policySaving,setPolicySaving]=useState(false);
 const policyWriteInFlight=useRef(false);
 // The policy endpoint replaces the whole object, so parallel checkbox
 // requests built from the same snapshot can silently undo earlier grants
 // or denials. Serialize owner edits and show only server-confirmed values.
 const savePolicy=async(value:any)=>{
  if(policyWriteInFlight.current||policy.loading||policy.error)return;
  policyWriteInFlight.current=true;setPolicySaving(true);setError('');setMessage('');
  try{
   const saved=await post<any>('/api/policy',value);
   policy.setValue(saved);
   refresh('/api/policy');
   setMessage(t('ops.policyChanged'));
  }catch(e){
   setError((e as Error).message);
   await policy.reload();
  }finally{policyWriteInFlight.current=false;setPolicySaving(false)}
 };
 const setToolApproval=(name:string,on:boolean)=>{
  if(policyWriteInFlight.current)return;
  const current=new Set<string>(Array.isArray(policy.value.autoApprove)?policy.value.autoApprove:[]);
  if(on)current.add(name);else current.delete(name);
  void savePolicy({...policy.value,autoApprove:[...current]});
 };
 const iconFor=(e:string)=>e.includes('tool')?TerminalSquare:e.includes('job')?Workflow:e.includes('oauth')?KeyRound:e.includes('channel')?Activity:Sparkles;
 return <div className="mx-auto w-full max-w-[1320px] px-5 py-4 sm:px-7 lg:px-6 lg:pb-8">
 
 {section==='activity'&&<><Headline eyebrow={t('ui.eyebrow.activity')} title={t('ops.activity.title')} description={t('ops.activity.description')} actions={<Action variant="secondary" onClick={()=>{jobs.reload();audit.reload();executions.reload()}}><RefreshCw size={16}/> {t('ops.refresh')}</Action>}/><Subheading title={t('ops.agentJobs')}/><JobList jobs={jobs.value} onChanged={()=>void jobs.reload()}/>{jobs.value.some(job=>job.kind==='team')&&<TeamWorkflowPanel jobs={jobs.value}/>}
 <details className="mt-6 rounded-2xl border border-divider bg-surface p-5" onToggle={event=>setActivityDetails(event.currentTarget.open)}><summary className="cursor-pointer text-sm font-semibold text-muted">{t('workspace.taskDetails')}</summary>{activityDetails&&<>
<div className="mt-10"><Subheading title={t('ops.timeline')} extra={<span className="text-[10px] font-bold uppercase tracking-widest text-[#9C9388]">{executions.value.length} EXECUTIONS</span>}/></div><Panel className="min-w-0 p-5">{executions.value.length?<div className="grid gap-2">{executions.value.slice(0,20).map(execution=><Button variant="bare" type="button" key={execution.id} onClick={()=>setSelectedExecution(selectedExecution===execution.id?'':execution.id)} className={'flex min-w-0 flex-wrap items-center gap-2 rounded-lg border px-3 py-2 text-left text-[11px] '+(selectedExecution===execution.id?'border-[#C7B18F] bg-[#F2EADF]':'border-[#EFE9DF] bg-white hover:bg-[#FAF7F1]')}><StatusPill status={execution.status||execution.taskOutcome||'unknown'}>{execution.taskOutcome?statusLabel(execution.taskOutcome):execution.status?statusLabel(execution.status):t('ops.record')}</StatusPill><span className="text-muted">{execution.agentId||t('ops.noAgent')}</span><strong className="min-w-0 flex-1 truncate">{execution.request||t('ops.recordFallback')}</strong><span className="text-[#9C9388]">{t('ops.toolTokens',{tools:execution.toolCalls||0,tokens:execution.tokens||0})}</span><time className="text-[#B1A594]">{execution.startedAt?formatDateTime(execution.startedAt):''}</time></Button>)}</div>:<Empty icon={Activity} title={t('ops.noTimeline')} description={t('ops.noTimeline.desc')}/>}{selectedExecution&&<ExecutionInsights executionId={selectedExecution}/>} {selectedExecution&&<RunEventTimeline executionId={selectedExecution}/>}
{selectedExecution&&<div className="mt-4 border-t border-[#EFE9DF] pt-3">{operations.length?operations.map(operation=><details className="border-b border-[#F3EDE4] py-2 text-[11px]" key={operation.id}><summary className="flex cursor-pointer flex-wrap items-center gap-2"><TerminalSquare size={13}/><strong>{operation.tool||t('ops.tool')}</strong><span className="text-muted">{operation.status||t('ops.record')}</span>{operation.effectState&&<span className="rounded bg-[#F7E8E1] px-1.5 py-0.5 text-[10px] text-brand-hover">{operation.effectState}</span>}{operation.error&&<span className="text-rose-600">{operation.error}</span>}</summary><pre className="mt-2 max-h-56 overflow-auto whitespace-pre-wrap break-all rounded bg-[#FAF7F1] p-2 text-[10px]">{JSON.stringify({arguments:operation.arguments,result:operation.result,error:operation.error},null,2)}</pre></details>):<p className="text-sm text-muted">{t('ops.noOperations')}</p>}</div>}</Panel><div className="mt-10"><Subheading title={t('ops.browser')} extra={<span className="text-[10px] font-bold uppercase tracking-widest text-[#9C9388]">{browserStatus.value.status?.running?t('ops.browser.running'):t('ops.browser.idle')}</span>}/></div>
<Panel className="min-w-0 p-5">
 <div className="flex flex-wrap items-center gap-3 text-[11px] text-muted">
  <StatusPill status={browserStatus.value.status?.available?'available':'unavailable'}>{browserStatus.value.status?.available?t('ops.browser.available'):t('ops.browser.unavailable')}</StatusPill>
  <span className="min-w-0 flex-1">{t('ops.browser.desc')}</span>
  <Action variant="secondary" onClick={()=>{browserStatus.reload();setShotTick(value=>value+1)}}><RefreshCw size={14}/> {t('ops.refresh')}</Action>
 </div>
 {browserStatus.value.screenshot?<figure className="mt-3"><figcaption className="text-[10px] font-bold uppercase tracking-widest text-[#9C9388]">{t('ops.browser.latest')}</figcaption><img key={shotTick} src={'/api/browser/screenshot?v='+shotTick} alt={t('ops.browser.latest')} className="mt-2 max-h-[360px] w-full rounded-lg border border-[#EFE9DF] bg-[#FAF7F1] object-contain"/></figure>:<p className="mt-3 text-[11px] text-[#9C9388]">{t('ops.browser.empty')}</p>}
</Panel>
<RemoteWorkPanel/><div className="mt-10"><Subheading title={t('ops.events')} extra={<span className="text-[10px] font-bold uppercase tracking-widest text-[#9C9388]">{t('ui.label.recentActivity')}</span>}/></div><Panel className="min-w-0 p-5">{audit.value.length?audit.value.map(a=>{const Icon=iconFor(a.event);return <div className="flex min-w-0 items-start gap-3 border-b border-[#EFE9DF] py-3 last:border-0" key={a.id}><span className="grid size-9 shrink-0 place-items-center rounded-lg bg-[#F3EEE5] text-muted"><Icon size={16}/></span><div><strong>{auditEventLabel(a.event)}</strong><p>{JSON.stringify(a.detail).slice(0,320)}</p></div><time>{formatDateTime(a.created_at||a.createdAt)}</time></div>}):<Empty icon={Activity} title={t('ops.noEvents')} description={t('ops.noEvents.desc')}/>}</Panel></>}</details></>}
 {section==='settings'&&<><div className="mb-4 flex flex-wrap items-center justify-between gap-3"><div><h2 className="text-[15px] font-semibold text-ink">{loc==='ko'?'일반 설정':'General settings'}</h2><p className="mt-1 text-[12px] text-[#8A8176]">{loc==='ko'?'자주 쓰는 설정만 먼저 표시합니다.':'Frequently used controls appear first.'}</p></div><Button variant="secondary" type="button" aria-expanded={advancedSettingsOpen} onClick={()=>setAdvancedSettingsOpen(v=>!v)}>{advancedSettingsOpen?(loc==='ko'?'고급 설정 접기':'Hide advanced'):(loc==='ko'?'고급 설정 보기':'Show advanced')}</Button></div><div className="grid grid-cols-1 items-start gap-4 xl:grid-cols-2">
 <Panel className="min-w-0 p-5"><Subheading title={t('ops.language')} extra={<span className="text-[11px] text-[#8A8176]">{loc==='ko'?'한국어':'English'}</span>}/><p>{t('ops.language.desc')}</p><SelectInput value={loc} onChange={e=>setLocale(e.target.value as 'ko'|'en')}><option value="ko">한국어</option><option value="en">English</option></SelectInput></Panel>
 <AppUpdatePanel/><BackupPanel/>{advancedSettingsOpen&&<><ExtensionRuntimeControls/><Panel className="min-w-0 p-5"><Subheading title={t('ops.permissions')} extra={<Shield size={19}/>}/><p>{t('ops.permissions.desc')}</p><label className="flex flex-wrap items-center justify-between gap-4"><div><strong>{t('ops.approveAll')}</strong><small>{t('ops.approveAll.desc')}</small></div><TextInput type="checkbox" checked={Boolean(policy.value.autoApproveAll)} disabled={policySaving||policy.loading||Boolean(policy.error)} onChange={e=>void savePolicy({...policy.value,autoApproveAll:e.target.checked})}/></label><label className="flex flex-wrap items-center justify-between gap-4"><div><strong>{t('ops.approveMcp')}</strong><small>{t('ops.approveMcp.desc')}</small></div><TextInput type="checkbox" checked={Boolean(policy.value.autoApproveMcp)} disabled={policySaving||policy.loading||Boolean(policy.error)} onChange={e=>void savePolicy({...policy.value,autoApproveMcp:e.target.checked})}/></label><div className="mt-3 border-t border-[#EFE9DF] pt-3"><div className="flex items-center justify-between"><strong className="text-sm">{t('ops.perTool')}</strong><small className="text-[10px] text-[#8A8176]">{t('ops.perTool.count',{count:Array.isArray(policy.value.autoApprove)?policy.value.autoApprove.length:0})}</small></div><p className="mt-1 text-[11px] text-[#8A8176]">{policy.value.autoApproveAll?t('ops.perTool.allOn'):t('ops.perTool.hint')}</p><div className="mt-2 grid max-h-56 gap-1 overflow-auto">{tools.value.filter((tool:any)=>tool.approval).slice(0,24).map((tool:any)=>{const checked=Array.isArray(policy.value.autoApprove)&&policy.value.autoApprove.includes(tool.name);return <label className="flex items-center gap-2 rounded-lg border border-[#EFE9DF] px-2.5 py-2 text-[11px] text-[#5C544A]" key={tool.name}><TextInput type="checkbox" checked={checked} disabled={policySaving||policy.loading||Boolean(policy.error)||Boolean(policy.value.autoApproveAll)} onChange={e=>setToolApproval(tool.name,e.target.checked)}/><span className="min-w-0 flex-1"><strong className="block text-[11px]">{tool.name}</strong><small className="block truncate text-[10px] text-[#9C9388]">{tool.description||t('ops.toolFallback')}</small></span><span className="rounded bg-[#F7E8E1] px-1.5 py-0.5 text-[9px] text-brand-hover">{tool.source||t('ops.toolFallback')}</span></label>})}{!tools.value.some((tool:any)=>tool.approval)&&<p className="text-[11px] text-[#9C9388]">{t('ops.perTool.none')}</p>}</div></div></Panel><Panel className="min-w-0 p-5"><Subheading title={t('ops.gateway')} extra={<Lock size={19}/>}/><p>{t('ops.gateway.desc')}</p><FormField label={t('ops.adminToken')}><TextInput type="password"  value={token} onChange={e=>setToken(e.target.value)} placeholder="DOMINION_ADMIN_TOKEN"/></FormField><Action variant="secondary" onClick={()=>{setAuthToken(token);setMessage(t('ops.tokenApplied'))}}><KeyRound size={16}/> {t('ops.applyToken')}</Action></Panel></>}</div></>}
 {(error||policy.error)&&<Notice message={error||policy.error}/>}
 {message&&<div className="my-3 flex items-center gap-2 rounded-xl border border-emerald-200 bg-emerald-50 p-3 text-sm text-emerald-700"><Check size={16}/>{message}</div>}
 
 </div>
}
function JobList({jobs,onChanged}:{jobs:any[];onChanged:()=>void}){
 return <div className="flex min-w-0 flex-col gap-3">
  {jobs.slice(0,30).map(job=><JobWorkflowCard key={job.id} job={job} onChanged={onChanged}/>)}
  {!jobs.length&&<Panel><Empty icon={Workflow} title={t('ops.noJobs')} description={t('ops.noJobs.desc')}/></Panel>}
 </div>
}
