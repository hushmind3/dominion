import {useState} from 'react'
import {post} from '../api'
import {t} from '../i18n'
import {Button,SelectInput,TextArea,TextInput} from './Primitives'

type Option={id:string;name:string}
export function AutonomousJobComposer({agents,providers,onCreated}:{
 agents:Option[];providers:Option[];onCreated:()=>void
}){
 const [goal,setGoal]=useState('')
 const [agentId,setAgentId]=useState('')
 const [providerId,setProviderId]=useState('')
 const [model,setModel]=useState('')
 const [verifyCommand,setVerifyCommand]=useState('')
 const [attempts,setAttempts]=useState(3)
 const [busy,setBusy]=useState(false)
 const [error,setError]=useState('')
 const [notice,setNotice]=useState('')
 const submit=async(event:React.FormEvent<HTMLFormElement>)=>{
  event.preventDefault()
  if(!goal.trim()||busy)return
  setBusy(true);setError('');setNotice('')
  try{
   await post('/api/jobs',{kind:'prompt',input:{
    prompt:goal.trim(),autonomous:true,toolsEnabled:true,
    maxAutonomousAttempts:attempts,executionStrategy:'focused',
    ...(agentId?{agentId}:{}),
    ...(providerId?{providerId}:{}),
    ...(model.trim()?{model:model.trim()}:{}),
    ...(verifyCommand.trim()?{verifyCommand:verifyCommand.trim()}:{}),
   }})
   setGoal('');setNotice(t('ops.compose.created'));onCreated()
  }catch(e){setError((e as Error).message||t('ops.compose.error'))}
  finally{setBusy(false)}
 }
 return <section className="mb-5 rounded-xl border border-[#E7E0D5] bg-white p-5">
  <h2 className="text-base font-bold text-[#3A342D]">{t('ops.compose.title')}</h2>
  <p className="mb-4 mt-1 text-xs text-[#7C7469]">{t('ops.compose.desc')}</p>
  <form onSubmit={e=>void submit(e)} className="grid gap-3">
   <label htmlFor="autonomous-task-goal" className="text-xs font-semibold">{t('ops.compose.goal')}</label>
   <TextArea id="autonomous-task-goal" required maxLength={64000} rows={4}
    placeholder={t('ops.compose.placeholder')} value={goal}
    onChange={e=>setGoal(e.target.value)}/>
   {agents.length>0&&<label className="grid gap-1 text-xs font-semibold">
    {t('ops.compose.agent')}
    <SelectInput value={agentId} onChange={e=>setAgentId(e.target.value)}>
     <option value="">{t('ops.compose.agentDefault')}</option>
     {agents.map(agent=><option key={agent.id} value={agent.id}>{agent.name}</option>)}
    </SelectInput>
   </label>}
   <details className="rounded-lg border border-[#EFE9DF] p-3">
    <summary className="cursor-pointer text-xs font-semibold">{t('ops.compose.advanced')}</summary>
    <div className="mt-3 grid gap-3 sm:grid-cols-2">
     <label className="grid gap-1 text-xs font-semibold">{t('ops.compose.provider')}
      <SelectInput value={providerId} onChange={e=>setProviderId(e.target.value)}>
       <option value="">{t('ops.compose.providerDefault')}</option>
       {providers.map(provider=><option key={provider.id} value={provider.id}>{provider.name}</option>)}
      </SelectInput>
     </label>
     <label className="grid gap-1 text-xs font-semibold">{t('ops.compose.model')}
      <TextInput value={model} onChange={e=>setModel(e.target.value)}/>
     </label>
     <label className="grid gap-1 text-xs font-semibold">{t('ops.compose.verify')}
      <TextInput maxLength={2048} value={verifyCommand} onChange={e=>setVerifyCommand(e.target.value)}/>
     </label>
     <label className="grid gap-1 text-xs font-semibold">{t('ops.compose.attempts')}
      <SelectInput value={attempts} onChange={e=>setAttempts(Number(e.target.value))}>
       {[1,2,3,4,5,6].map(n=><option value={n} key={n}>{n}</option>)}
      </SelectInput>
     </label>
    </div>
   </details>
   {notice&&<p role="status" className="text-xs text-emerald-700">{notice}</p>}
   {error&&<p role="alert" className="text-xs text-rose-700">{error}</p>}
   <div className="flex justify-end">
    <Button variant="primary" type="submit" disabled={busy||!goal.trim()}>
     {busy?t('ops.compose.creating'):t('ops.compose.create')}
    </Button>
   </div>
  </form>
 </section>
}
