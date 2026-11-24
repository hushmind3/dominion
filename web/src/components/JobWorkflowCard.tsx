import {useState} from 'react'
import {Check,MessageSquare,Square,Send,Workflow} from 'lucide-react'
import {post} from '../api'
import {useLocale,t,formatDateTime} from '../i18n'
import {translateError} from '../i18n/errors'
import {StatusPill} from './StatusPill'
import {Panel} from './UI'

export type WorkflowJob={
 id:string;
 kind?:string;
 status?:string;
 createdAt?:string;queuedAt?:string;sessionId?:string;cancelRequested?:boolean;
 error?:string;
 result?:unknown;
 input?:{
  prompt?:string;
  goal?:string;
  sessionId?:string;
  externalRuntime?:string;
  dependsOnJobIds?:string[];
 };
}
export function JobWorkflowCard({job,onChanged}:{job:WorkflowJob;onChanged:()=>void}){
 const locale=useLocale()
 const en=locale==='en'
 const [steering,setSteering]=useState('')
 const [busy,setBusy]=useState(false)
 const [error,setError]=useState('')
 const [info,setInfo]=useState('')
 const title=job.input?.goal||job.input?.prompt||(en?'Agent task':'에이전트 작업')
 const active=(job.status==='queued'||job.status==='running')&&job.cancelRequested!==true
 const isPrompt=job.kind==='prompt'
 const session=job.sessionId||job.input?.sessionId
 const result=typeof job.result==='string'?job.result:
  job.result==null?'':JSON.stringify(job.result,null,2)
 const run=async(action:'cancel'|'steer')=>{
  if(busy)return
  if(action==='steer'&&!steering.trim())return
  if(action==='cancel'&&!window.confirm(en?'Stop this task?':'이 작업을 중단하시겠습니까?'))return
  setBusy(true);setError('');setInfo('')
  try{
   await post('/api/jobs/'+encodeURIComponent(job.id)+'/'+action,
    action==='steer'?{instruction:steering.trim(),mode:'append'}:{})
   if(action==='steer')setSteering('')
   setInfo(action==='steer'?(en?'Instruction submitted.':'추가 지시를 전달했습니다.'):(en?'Stop requested.':'중단을 요청했습니다.'))
   onChanged()
  }catch(err){setError(translateError((err as Error).message))}
  finally{setBusy(false)}
 }
 return <Panel className="min-w-0 rounded-xl bg-surface p-4 sm:p-5">
  <div className="flex min-w-0 items-start gap-3">
   <span className="grid size-9 shrink-0 place-items-center rounded-xl bg-brand-soft text-brand"><Workflow size={18}/></span>
   <div className="min-w-0 flex-1">
    <div className="flex flex-wrap items-center justify-between gap-2">
     <strong className="text-xs font-medium text-muted">{job.kind==='meeting'?t('ops.meeting'):t('ops.agentJob')}</strong>
     <StatusPill status={job.status||'unknown'}/>
    </div>
    <p className="mt-2 whitespace-pre-wrap break-words text-sm font-medium leading-6 text-ink">{title}</p>
    {(job.queuedAt||job.createdAt)&&<time className="mt-1 block text-xs text-muted">{formatDateTime(job.queuedAt||job.createdAt)}</time>}
    {job.input?.externalRuntime&&<span className="mt-1 inline-block rounded-md bg-canvas px-2 py-0.5 text-[10px] text-muted">{job.input.externalRuntime}</span>}
    {job.error&&<p role="alert" className="mt-2 break-words text-[11px] text-danger">{translateError(job.error)}</p>}
    <div className="mt-2 flex flex-wrap items-center gap-2">
     {session&&<a href={'#/chat?session='+encodeURIComponent(session)}
      className="inline-flex items-center gap-1 rounded-lg border border-divider px-2.5 py-1.5 text-[11px] font-medium text-muted hover:bg-canvas">
      <MessageSquare size={13}/>{en?'Open session':'관련 대화 열기'}
     </a>}
     {active&&isPrompt&&<button type="button" disabled={busy} onClick={()=>void run('cancel')}
      className="inline-flex items-center gap-1 rounded-lg border border-divider px-2.5 py-1.5 text-[11px] text-danger hover:bg-danger-soft disabled:opacity-50">
      <Square size={13}/>{en?'Stop task':'작업 중단'}
     </button>}
    </div>
    {active&&isPrompt&&<form className="mt-3 flex flex-wrap gap-2" onSubmit={event=>{event.preventDefault();void run('steer')}}>
     <input type="text" value={steering} maxLength={4000} disabled={busy}
      onChange={event=>setSteering(event.target.value)}
      aria-label={en?'Additional instruction':'작업 추가 지시'}
      placeholder={en?'Add a follow-up instruction…':'진행 중인 작업에 추가 지시…'}
      className="min-w-40 flex-1 rounded-lg border border-divider bg-surface px-3 py-2 text-[11px] outline-none focus:border-brand"/>
     <button type="submit" disabled={!steering.trim()||busy}
      className="inline-flex items-center gap-1 rounded-lg border border-brand px-3 py-2 text-[11px] font-semibold text-brand-hover hover:bg-brand-soft disabled:opacity-50">
      <Send size={13}/>{en?'Send':'추가 지시'}
     </button>
    </form>}
    {result&&<details className="mt-3 rounded-lg border border-divider bg-surface px-3 py-2 text-[11px]">
     <summary className="cursor-pointer text-xs font-semibold text-ink">{en?'View completed result':'작업 결과 펼치기'}</summary>
     <pre className="mt-2 max-h-80 overflow-auto whitespace-pre-wrap break-words text-sm leading-6 text-ink">{result.slice(0,12000)}{result.length>12000?'\n…':''}</pre>
    </details>}
    {info&&<p role="status" className="mt-2 flex items-center gap-1 text-[11px] text-success"><Check size={13}/>{info}</p>}
    {error&&<p role="alert" className="mt-2 break-words text-[11px] text-danger">{error}</p>}
   </div>
  </div>
 </Panel>
}
