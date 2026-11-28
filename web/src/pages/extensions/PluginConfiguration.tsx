import {useEffect,useState} from 'react'
import {Button,TextInput,TextArea,SelectInput} from '../../components/Primitives'
import {FormField,Notice,useRemote} from '../../components/UI'
import {post} from '../../api'
import {useLocale} from '../../i18n'
type Schema={type?:string;title?:string;description?:string;properties?:Record<string,Schema>;enum?:unknown[];writeOnly?:boolean;format?:string}
export function PluginConfiguration({id,onChanged}:{id:string;onChanged:()=>void}){
 const en=useLocale()==='en',path='/api/plugins/host/plugin/'+encodeURIComponent(id)+'/config'
 const source=useRemote<{config:Record<string,unknown>;schema?:Schema}>(path,{config:{}})
 const [draft,setDraft]=useState<Record<string,unknown>>({}),[busy,setBusy]=useState(false),[error,setError]=useState(''),[saved,setSaved]=useState(false)
 useEffect(()=>{setDraft(source.value.config)},[source.value])
 const fields=source.value.schema?.properties||Object.fromEntries(Object.keys(draft).map(key=>[key,{type:typeof draft[key]}]))
 return <section className="my-6 rounded-2xl border border-divider bg-white p-5" aria-label={en?'Plugin settings':'플러그인 설정'}><h2 className="mb-4 text-lg font-semibold">{en?'Plugin settings':'플러그인 설정'}</h2><Notice message={error||source.error} onRetry={()=>void source.reload()}/>
  <div className="grid gap-4">{Object.entries(fields).map(([key,schema])=><SchemaField key={key} name={schema.title||key} schema={schema} value={draft[key]} onChange={value=>setDraft(previous=>({...previous,[key]:value}))}/>)}</div>
  {!source.loading&&!Object.keys(fields).length&&!source.error&&<p className="text-sm text-muted">{en?'This plugin has no additional configuration fields.':'이 플러그인에는 추가 설정 항목이 없습니다.'}</p>}
  <div className="mt-5 flex items-center gap-3"><Button disabled={busy||source.loading||Boolean(source.error)||!Object.keys(fields).length} onClick={async()=>{setBusy(true);setError('');setSaved(false);try{await post(path,{config:draft});setSaved(true);onChanged()}catch(e){setError((e as Error).message)}finally{setBusy(false)}}}>{en?'Save settings':'설정 저장'}</Button>{saved&&<p role="status" className="text-sm text-emerald-800">{en?'Saved':'저장했어요.'}</p>}</div>
 </section>
}
function SchemaField({name,schema,value,onChange}:{name:string;schema:Schema;value:unknown;onChange:(value:unknown)=>void}){
 if(schema.properties)return <fieldset className="grid gap-3 rounded-xl border border-divider p-4"><legend className="px-2 text-sm font-semibold">{name}</legend>{Object.entries(schema.properties).map(([key,child])=><SchemaField key={key} name={child.title||key} schema={child} value={(value as Record<string,unknown>)?.[key]} onChange={next=>onChange({...((value||{}) as Record<string,unknown>),[key]:next})}/>)}</fieldset>
 return <FormField label={name} hint={schema.description}>{schema.enum?<SelectInput value={String(value??'')} onChange={e=>onChange(schema.enum?.find(item=>String(item)===e.target.value))}><option value=""/>{schema.enum.map(option=><option key={String(option)} value={String(option)}>{String(option)}</option>)}</SelectInput>:schema.type==='boolean'?<TextInput type="checkbox" checked={value===true} onChange={e=>onChange(e.target.checked)}/>:schema.type==='array'||schema.type==='object'?<JsonField value={value} onChange={onChange}/>:<TextInput type={schema.writeOnly||schema.format==='password'?'password':schema.type==='number'||schema.type==='integer'?'number':'text'} autoComplete="off" value={String(value??'')} onChange={e=>onChange(['number','integer'].includes(schema.type||'')?Number(e.target.value):e.target.value)}/>}</FormField>
}
function JsonField({value,onChange}:{value:unknown;onChange:(value:unknown)=>void}){
 const [text,setText]=useState(JSON.stringify(value??{},null,2)),[invalid,setInvalid]=useState(false)
 return <TextArea value={text} aria-invalid={invalid} rows={4} onChange={e=>{setText(e.target.value);try{onChange(JSON.parse(e.target.value));setInvalid(false)}catch{setInvalid(true)}}}/>
}
