import {useState} from 'react'
import {get,post} from '../../api'
import {Button,TextArea} from '../../components/Primitives'
import {useLocale} from '../../i18n'
type Config={values:Record<string,unknown>;description?:string}
export function ExtensionConfiguration({pluginId}:{pluginId:string}){
 const en=useLocale()==='en',tr=(ko:string,eng:string)=>en?eng:ko
 const path='/api/plugins/managed/'+encodeURIComponent(pluginId)+'/configuration'
 const [open,setOpen]=useState(false),[busy,setBusy]=useState(false),[value,setValue]=useState(''),[description,setDescription]=useState(''),[error,setError]=useState('')
 const load=async()=>{setBusy(true);setError('');try{const r=await get<Config>(path);if(!r.values||typeof r.values!=='object')throw Error('Invalid configuration');setValue(JSON.stringify(r.values,null,2));setDescription(r.description||'');setOpen(true)}catch(e){setError((e as Error).message)}finally{setBusy(false)}}
 const save=async()=>{if(!window.confirm(tr('이 확장 프로그램의 설정을 변경합니다. 기존 도구 등록은 먼저 해제해야 합니다.','Change this extension configuration? Disable registered tools first.')))return;setBusy(true);setError('');try{const values=JSON.parse(value);const r=await post<Config>(path,{confirmed:true,values});setValue(JSON.stringify(r.values,null,2));setDescription(r.description||'')}catch(e){setError((e as Error).message)}finally{setBusy(false)}}
 return <div className="space-y-2 rounded-lg border border-divider p-3"><Button size="sm" variant="secondary" disabled={busy} onClick={()=>open?setOpen(false):void load()}>{open?tr('설정 닫기','Close settings'):tr('확장 프로그램 설정','Extension settings')}</Button>
 {open&&<><p className="whitespace-pre-wrap break-words text-muted">{description}</p><TextArea aria-label={tr('확장 프로그램 설정 JSON','Extension configuration JSON')} value={value} onChange={e=>setValue(e.target.value)} rows={8}/><Button size="sm" disabled={busy} onClick={()=>void save()}>{tr('설정 저장','Save settings')}</Button></>}{error&&<p role="alert" className="break-words text-warning">{error}</p>}</div>
}
