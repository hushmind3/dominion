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
 createdAt?:string;
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
 const active=job.status==='queued'||job.status==='running'
 const isPrompt=job.kind==='prompt'
 const session=job.input?.sessionId
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
 return <Panel className="min-w-0 rounded-xl bg-white p-4">
  <div className="flex min-w-0 items-start gap-3">
   <span className="grid size-9 shrink-0 place-items-center rounded-xl bg-[#F7E8E1] text-[#C96442]"><Workflow size={18}/></span>
   <div className="min-w-0 flex-1">
    <div className="flex flex-wrap items-center justify-between gap-2">
     <strong className="text-[12px] text-[#3A342D]">{job.kind==='meeting'?t('ops.meeting'):t('ops.agentJob')}</strong>
     <StatusPill status={job.status||'unknown'}/>
    </div>
    <p className="mt-1 whitespace-pre-wrap break-words text-[12px] leading-5 text-[#5C544A]">{title}</p>
    {job.createdAt&&<time className="mt-1 block text-[10px] text-[#8A8176]">{formatDateTime(job.createdAt)}</time>}
    {job.input?.externalRuntime&&<span className="mt-1 inline-block rounded-md bg-[#F3EEE5] px-2 py-0.5 text-[10px] text-[#5C544A]">{job.input.externalRuntime}</span>}
    {job.error&&<p role="alert" className="mt-2 break-words text-[11px] text-rose-700">{translateError(job.error)}</p>}
    <div className="mt-2 flex flex-wrap items-center gap-2">
     {session&&<a href={'#/chat?session='+encodeURIComponent(session)}
      className="inline-flex items-center gap-1 rounded-lg border border-[#E7E0D5] px-2.5 py-1.5 text-[11px] font-medium text-[#5C544A] hover:bg-[#FAF7F1]">
      <MessageSquare size={13}/>{en?'Open session':'관련 대화 열기'}
     </a>}
     {active&&isPrompt&&<button type="button" disabled={busy} onClick={()=>void run('cancel')}
      className="inline-flex items-center gap-1 rounded-lg border border-[#E7E0D5] px-2.5 py-1.5 text-[11px] text-rose-700 hover:bg-rose-50 disabled:opacity-50">
      <Square size={13}/>{en?'Stop task':'작업 중단'}
     </button>}
    </div>
    {active&&isPrompt&&<form className="mt-3 flex flex-wrap gap-2" onSubmit={event=>{event.preventDefault();void run('steer')}}>
     <input type="text" value={steering} maxLength={4000} disabled={busy}
      onChange={event=>setSteering(event.target.value)}
      aria-label={en?'Additional instruction':'작업 추가 지시'}
      placeholder={en?'Add a follow-up instruction…':'진행 중인 작업에 추가 지시…'}
      className="min-w-40 flex-1 rounded-lg border border-[#E7E0D5] bg-white px-3 py-2 text-[11px] outline-none focus:border-[#A36A4A]"/>
     <button type="submit" disabled={!steering.trim()||busy}
      className="inline-flex items-center gap-1 rounded-lg border border-[#D9BAA6] px-3 py-2 text-[11px] font-semibold text-[#9D5039] hover:bg-[#F7E8E1] disabled:opacity-50">
      <Send size={13}/>{en?'Send':'추가 지시'}
     </button>
    </form>}
    {result&&<details className="mt-3 rounded-lg border border-[#EFE9DF] bg-[#FCFAF6] px-3 py-2 text-[11px]">
     <summary className="cursor-pointer font-semibold text-[#5C544A]">{en?'View completed result':'작업 결과 펼치기'}</summary>
     <pre className="mt-2 max-h-64 overflow-auto whitespace-pre-wrap break-words text-[11px] leading-5 text-[#5C544A]">{result.slice(0,12000)}{result.length>12000?'\n…':''}</pre>
    </details>}
    {info&&<p role="status" className="mt-2 flex items-center gap-1 text-[11px] text-emerald-700"><Check size={13}/>{info}</p>}
    {error&&<p role="alert" className="mt-2 break-words text-[11px] text-rose-700">{error}</p>}
   </div>
  </div>
 </Panel>
}
