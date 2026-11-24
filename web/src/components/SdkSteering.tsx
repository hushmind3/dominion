import {useState} from 'react'
import {post} from '../api'
import {useLocale} from '../i18n'
import {Button,SelectInput,TextArea} from './Primitives'
import {Notice} from './UI'
import {type SdkRun} from '../utils/sdkIntegration'
import {sdkSteeringAllowed,steeringBytes} from '../utils/intelligenceView'
export function SdkSteering({run,disabled,onChanged}:{run:SdkRun;disabled:boolean;onChanged:()=>Promise<unknown>}){
 const en=useLocale()==='en',tr=(ko:string,eng:string)=>en?eng:ko
 const [instruction,setInstruction]=useState(''),[mode,setMode]=useState('append'),[busy,setBusy]=useState(false),[error,setError]=useState(''),[notice,setNotice]=useState('')
 const eligible=sdkSteeringAllowed(run),bytes=steeringBytes(instruction)
 const submit=async()=>{
  if(!eligible||disabled||busy||!bytes||bytes>4000)return
  if(mode==='replace'&&!window.confirm(tr('기존 목표를 바꿉니다. 이미 실행된 작업은 되돌리지 않습니다.','Replace the existing goal? Completed effects are not undone.')))return
  setBusy(true);setError('');setNotice('')
  try{const result=await post<{revision:number;appliesAt:string}>('/api/sdk/runs/'+encodeURIComponent(run.id)+'/steer',{instruction:instruction.trim(),mode});setNotice(tr('다음 실행 경계에서 반영 예정','Accepted for the next native boundary')+' · '+result.revision);setInstruction('')}
  catch(e){setError((e as Error).message)}
  finally{await onChanged();setBusy(false)}
 }
 return <div className="mt-3 grid gap-2 rounded-lg border border-[#E7E0D5] p-3 text-xs">
  <strong>{tr('작업 지시 변경','Steer this run')}</strong>
  {(run.steering||[]).map(d=><div key={d.revision} className="rounded bg-[#F3EEE5] p-2"><strong>#{d.revision} · {d.mode==='replace'?tr('목표 변경','Replace'):tr('추가 지시','Append')}</strong><p className="whitespace-pre-wrap break-words">{d.instruction}</p></div>)}
  {eligible?<form className="grid gap-2" onSubmit={e=>{e.preventDefault();void submit()}}>
   <SelectInput aria-label={tr('지시 방식','Instruction mode')} value={mode} onChange={e=>setMode(e.target.value)}><option value="append">{tr('추가 지시','Append')}</option><option value="replace">{tr('기존 목표 변경','Replace')}</option></SelectInput>
   <TextArea aria-label={tr('새 지시','New instruction')} value={instruction} onChange={e=>setInstruction(e.target.value)} maxLength={4000}/>
   <span>{bytes}/4000 bytes · {(run.steering||[]).length}/8</span><Button type="submit" size="sm" disabled={disabled||busy||!bytes||bytes>4000}>{tr('지시 전달','Submit instruction')}</Button>
  </form>:<p>{tr('현재 상태에서는 지시 변경을 할 수 없습니다.','Steering is unavailable in this state.')}</p>}
  <Notice message={error}/>{notice&&<p role="status">{notice}</p>}
 </div>
}
