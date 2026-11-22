import {useEffect,useState} from 'react'
import {post} from '../api'
import {useLocale} from '../i18n'
import {Action,FormField,Notice,Panel,Subheading,useRemote} from './UI'
import {TextArea,TextInput} from './Primitives'

type Instructions={id:string;instructions:string;enabled:boolean;updatedAt?:string}
const empty:Instructions={id:'global',instructions:'',enabled:true}

export function AssistantInstructions(){
 const en=useLocale()==='en'
 const remote=useRemote<Instructions>('/api/settings/assistant',empty)
 const [draft,setDraft]=useState('')
 const [enabled,setEnabled]=useState(true)
 const [dirty,setDirty]=useState(false)
 const [saving,setSaving]=useState(false)
 const [error,setError]=useState('')
 const [saved,setSaved]=useState(false)
 useEffect(()=>{
  if(dirty)return
  setDraft(remote.value.instructions||'')
  setEnabled(remote.value.enabled!==false)
 },[remote.value,dirty])
 const save=async()=>{
  if(saving||Array.from(draft).length>16000)return
  setSaving(true);setError('');setSaved(false)
  try{
   const value=await post<Instructions>('/api/settings/assistant',{instructions:draft,enabled})
   remote.setValue(value)
   setDirty(false);setSaved(true)
  }catch(cause){setError((cause as Error).message)}
  finally{setSaving(false)}
 }
 const changed=draft!==(remote.value.instructions||'')||enabled!==(remote.value.enabled!==false)
 return <Panel className="min-w-0 p-5">
  <Subheading title={en?'Shared assistant instructions':'공통 어시스턴트 지침'}/>
  <p className="mb-3 text-[11px] leading-5 text-muted">{en
   ?'Instructions saved here are included in future chats and room turns. They do not override tool approvals or safety controls.'
   :'여기에 저장한 지침은 이후 대화와 방 실행에 반영됩니다. 도구 승인이나 안전 설정을 바꾸지는 않습니다.'}</p>
  <FormField label={en?'Instruction text':'지침 내용'}>
   <TextArea rows={6} value={draft} disabled={saving} onChange={event=>{setDraft(event.target.value);setDirty(true);setSaved(false)}}
    placeholder={en?'Describe tone, preferences, and recurring context.':'말투, 선호 방식, 반복해서 필요한 배경 정보를 적어 주세요.'}/>
  </FormField>
  <div className="mt-2 flex flex-wrap items-center justify-between gap-3">
   <div className="flex items-center gap-2 text-[11px] text-muted">
    <TextInput type="checkbox" checked={enabled} disabled={saving} onChange={event=>{setEnabled(event.target.checked);setDirty(true);setSaved(false)}}/>
    <span>{en?'Use these instructions':'공통 지침 사용'}</span>
    <span className={Array.from(draft).length>16000?'text-rose-700':''}>{Array.from(draft).length.toLocaleString()} / 16,000</span>
   </div>
   <Action disabled={saving||remote.loading||!changed||Array.from(draft).length>16000} onClick={()=>void save()}>
    {saving?(en?'Saving…':'저장 중…'):(en?'Save instructions':'지침 저장')}
   </Action>
  </div>
  {remote.loading&&<p role="status" className="mt-2 text-[11px] text-muted">{en?'Loading saved instructions…':'저장된 지침 불러오는 중…'}</p>}
  {remote.error&&<Notice message={remote.error}/>}
  {Array.from(draft).length>16000&&<Notice message={en?'Instructions must be 16,000 characters or fewer.':'지침은 16,000자 이내로 입력해 주세요.'}/>}
  {error&&<Notice message={error}/>}
  {saved&&<p role="status" className="mt-2 text-[11px] text-emerald-700">{en?'Instructions saved.':'공통 지침을 저장했습니다.'}</p>}
 </Panel>
}
