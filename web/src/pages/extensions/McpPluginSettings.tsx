import {useState} from 'react'
import {del,get,post} from '../../api'
import {Button,TextInput,TextArea,SelectInput} from '../../components/Primitives'
import {FormField,Notice} from '../../components/UI'
import {useLocale} from '../../i18n'
import type {Connection} from './catalog/usePluginDirectory'
export function McpPluginSettings({connection,onChanged,onRemoved}:{connection:Connection;onChanged:()=>void;onRemoved?:()=>void}){
 const en=useLocale()==='en'
 const [record,setRecord]=useState(connection),[args,setArgs]=useState(connection.args?.join('\n')||''),[token,setToken]=useState(''),[busy,setBusy]=useState(false),[error,setError]=useState(''),[notice,setNotice]=useState('')
 const act=async(fn:()=>Promise<void>)=>{if(busy)return;setBusy(true);setError('');try{await fn();onChanged()}catch(e){setError((e as Error).message)}finally{setBusy(false)}}
 const save=async(enabled=record.enabled)=>{const saved=await post<Connection>('/api/mcp',{id:record.id,name:record.name,transport:record.transport||(record.packageName?'stdio':'http'),url:record.url,command:record.command||(record.packageName?'npx':undefined),args:args?args.split('\n').filter(Boolean):record.packageName?['-y',record.packageName]:[],enabled,...(token?{token}:{})});setRecord(saved);setToken('');setNotice(en?'Saved':'저장했어요.')}
 return <section aria-label={en?'Plugin connection settings':'플러그인 연결 설정'} className="grid gap-4">
  <Notice message={error}/>{notice&&<p role="status" className="text-sm text-emerald-800">{notice}</p>}
  {record.id&&<div className="flex gap-2"><Button disabled={busy} onClick={()=>void act(()=>save(!record.enabled))}>{record.enabled?(en?'Disable':'비활성화'):(en?'Enable':'활성화')}</Button><Button variant="ghost" disabled={busy} onClick={()=>void act(async()=>{await del('/api/mcp/'+encodeURIComponent(record.id!));onRemoved?.()})}>{en?'Remove':'제거'}</Button><Button variant="ghost" disabled={busy||!record.enabled} onClick={()=>void act(async()=>{const tools=await get<{server:string}[]>('/api/mcp/discover');setNotice((en?'Connected tools: ':'연결된 도구: ')+tools.filter(tool=>tool.server===record.name).length)})}>{en?'Check connection':'연결 확인'}</Button></div>}
  <FormField label={en?'Name':'이름'}><TextInput value={record.name||''} onChange={e=>setRecord({...record,name:e.target.value})}/></FormField>
  <FormField label={en?'Connection method':'연결 방식'}><SelectInput value={record.transport||'http'} onChange={e=>setRecord({...record,transport:e.target.value as 'http'|'stdio'})}><option value="http">HTTP</option><option value="stdio">STDIO</option></SelectInput></FormField>
  {record.transport==='stdio'?<><FormField label={en?'Executable':'실행 명령'}><TextInput value={record.command||''} onChange={e=>setRecord({...record,command:e.target.value})}/></FormField><FormField label={en?'Arguments (one per line)':'인수 (한 줄에 하나)'}><TextArea value={args} onChange={e=>setArgs(e.target.value)}/></FormField></>:<><FormField label="URL"><TextInput value={record.url||''} onChange={e=>setRecord({...record,url:e.target.value})}/></FormField><FormField label={en?'Access token':'인증 토큰'}><TextInput type="password" autoComplete="off" value={token} onChange={e=>setToken(e.target.value)} placeholder={en?'Leave blank to retain existing token':'비워두면 기존 토큰 유지'}/></FormField></>}
  <div><Button disabled={busy||!record.name?.trim()} onClick={()=>void act(()=>save())}>{en?'Save settings':'설정 저장'}</Button></div>
 </section>
}
