import {useState} from 'react'
import {post} from '../api'
import {t} from '../i18n'
import {Button,TextArea} from './Primitives'

type SteerableJob={
 id:string;kind:string;status:string;
 input?:{autonomous?:boolean;rsiSourceDir?:string};
}
export function JobSteering({job,onUpdated}:{job:SteerableJob;onUpdated:()=>void}){
 const [open,setOpen]=useState(false)
 const [mode,setMode]=useState<'append'|'replace'>('append')
 const [instruction,setInstruction]=useState('')
 const [sending,setSending]=useState(false)
 const [notice,setNotice]=useState('')
 const [error,setError]=useState('')
 const supported=job.kind==='prompt'&&['queued','running'].includes(job.status)
   &&job.input?.autonomous!==false&&!job.input?.rsiSourceDir
 if(!supported)return null
 const submit=async()=>{
  if(sending||!instruction.trim())return
  setSending(true);setError('');setNotice('')
  try{
   const result=await post<{steeringRevision:number}>(
    '/api/jobs/'+encodeURIComponent(job.id)+'/steer',{instruction:instruction.trim(),mode})
   setNotice(t('ops.steer.queued',{revision:result.steeringRevision}))
   setInstruction('');setOpen(false);onUpdated()
  }catch(e){setError((e as Error).message)}finally{setSending(false)}
 }
 return <div className="w-full min-w-0 border-t border-[#EFE9DF] pt-3">
  <Button size="sm" variant="secondary" aria-expanded={open}
   onClick={()=>{setOpen(v=>!v);setError('')}}>{t('ops.steer.open')}</Button>
  {notice&&<p role="status" className="mt-2 text-xs text-emerald-700">{notice}</p>}
  {error&&<p role="alert" className="mt-2 text-xs text-rose-700">{error}</p>}
  {open&&<div className="mt-3 grid gap-3 rounded-xl bg-[#FAF7F1] p-4">
   <div role="group" aria-label={t('ops.steer.mode')} className="flex flex-wrap gap-2">
    <Button size="sm" variant={mode==='append'?'primary':'secondary'}
     onClick={()=>setMode('append')}>{t('ops.steer.append')}</Button>
    <Button size="sm" variant={mode==='replace'?'primary':'secondary'}
     onClick={()=>setMode('replace')}>{t('ops.steer.replace')}</Button>
   </div>
   <p className="text-xs text-[#746B61]">{mode==='append'?t('ops.steer.appendHint'):t('ops.steer.replaceHint')}</p>
   <label htmlFor={'owner-steer-'+job.id} className="text-xs font-semibold">{t('ops.steer.instruction')}</label>
   <TextArea id={'owner-steer-'+job.id} rows={3} maxLength={4000}
    value={instruction} placeholder={t('ops.steer.placeholder')}
    onChange={e=>setInstruction(e.target.value)}/>
   <div className="flex justify-end gap-2">
    <Button size="sm" variant="secondary" disabled={sending}
     onClick={()=>setOpen(false)}>{t('ops.cancel')}</Button>
    <Button size="sm" variant="primary" disabled={sending||!instruction.trim()}
     onClick={()=>void submit()}>{sending?t('ops.steer.sending'):t('ops.steer.submit')}</Button>
   </div>
  </div>}
 </div>
}
