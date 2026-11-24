import {useState} from 'react'
import {importConfigItem} from './configurationImport'
import {get,post} from '../../api'
import {Button,TextArea,TextInput,SelectInput} from '../../components/Primitives'
import {DialogBackdrop,DialogPanel} from '../../components/Dialog'
import {useLocale} from '../../i18n'
type Schema={type?:string;title?:string;titleKo?:string;description?:string;format?:string;default?:unknown;enum?:string[];enumLabels?:string[];enumLabelsKo?:string[];properties?:Record<string,Schema>;items?:Schema;maxItems?:number;readOnly?:boolean;generated?:boolean;visibleWhen?:{field:string;equals:string}}
type Config={values:Record<string,unknown>;description?:string;schema?:Schema}
const object=(v:unknown):Record<string,unknown>=>v&&typeof v==='object'&&!Array.isArray(v)?v as Record<string,unknown>:{}
const fields=(s:Schema)=>Object.entries(object(s.properties)).filter(([k,v])=>!['__proto__','constructor','prototype'].includes(k)&&v&&typeof v==='object').slice(0,64) as [string,Schema][]
function initial(s:Schema,depth=0):unknown{
 if(depth>5)return null
 if(s.generated===true)return crypto.randomUUID()
 if(s.default!==undefined)return structuredClone(s.default)
 if(s.type==='object')return Object.fromEntries(fields(s).filter(([,v])=>v.generated||v.default!==undefined).map(([k,v])=>[k,initial(v,depth+1)]))
 if(s.type==='array')return []
 if(s.type==='boolean')return false
 if(s.type==='number'||s.type==='integer')return 0
 return ''
}
function Field({schema:s,value,onChange,label,en,depth=0,disabled=false}:{schema:Schema;value:unknown;onChange:(v:unknown)=>void;label:string;en:boolean;depth?:number;disabled?:boolean}){
 const tr=(ko:string,eng:string)=>en?eng:ko,title=(en?s.title:s.titleKo||s.title)||label
 if(depth>5||s.generated===true||s.readOnly===true)return null
 if(s.type==='object'&&s.properties&&Object.keys(s.properties).length){
  const row=object(value)
  return <div className="space-y-3">{fields(s).filter(([,v])=>!v.visibleWhen||row[v.visibleWhen.field]===v.visibleWhen.equals).map(([k,v])=><Field key={k} schema={v} value={row[k]} label={k} en={en} depth={depth+1} disabled={disabled} onChange={next=>onChange({...row,[k]:next})}/>)}</div>
 }
 if(s.type==='array'&&s.items){
  const rows=Array.isArray(value)?value:[],item=s.items,max=typeof s.maxItems==='number'?Math.min(64,Math.max(0,s.maxItems)):64
  if(item.type==='string')return <label className="block space-y-1 text-sm"><span>{title}</span><TextArea disabled={disabled} aria-label={title} value={rows.map(String).join('\n')} onChange={e=>onChange(e.target.value.split('\n'))} rows={3}/><span className="block text-xs text-muted">{tr('한 줄에 한 항목','One item per line')}</span></label>
  return <section className="space-y-3"><div className="flex flex-wrap items-center justify-between gap-2"><h4 className="font-semibold">{title} · {rows.length}</h4><Button size="sm" disabled={disabled||rows.length>=max} onClick={()=>onChange([...rows,initial(item)])}>{tr('추가','Add')}</Button></div>
   {item.type==='object'&&item.properties&&<ImportItem schema={item} disabled={disabled||rows.length>=max} en={en} onAdd={v=>onChange([...rows,v])}/>}
   {rows.length===0&&<p className="rounded-lg bg-canvas p-3 text-sm text-muted">{tr('아직 등록된 항목이 없습니다. 추가 버튼을 누르세요.','No entries yet. Add an entry to begin.')}</p>}
   {rows.map((row,i)=><div key={i} className="space-y-3 rounded-xl border border-divider p-3"><div className="flex items-center justify-between"><strong>{i+1}. {String(object(row).name||title)}</strong><Button variant="ghost" size="sm" disabled={disabled} onClick={()=>onChange(rows.filter((_,j)=>i!==j))}>{tr('삭제','Remove')}</Button></div><Field schema={item} value={row} label={title} en={en} depth={depth+1} disabled={disabled} onChange={next=>onChange(rows.map((v,j)=>j===i?next:v))}/></div>)}
  </section>
 }
 if(s.type==='boolean')return <label className="flex items-center gap-2 text-sm"><input type="checkbox" disabled={disabled} checked={value===true} onChange={e=>onChange(e.target.checked)}/>{title}</label>
 if(Array.isArray(s.enum)&&s.enum.every(v=>typeof v==='string'))return <label className="block space-y-1 text-sm"><span>{title}</span><SelectInput disabled={disabled} value={typeof value==='string'?value:''} onChange={e=>onChange(e.target.value)}>{!s.enum.includes(String(value))&&<option value="">{tr('선택','Select')}</option>}{s.enum.slice(0,64).map((v,i)=><option key={v} value={v}>{(en?s.enumLabels:s.enumLabelsKo||s.enumLabels)?.[i]||v}</option>)}</SelectInput></label>
 if(s.type==='object'||s.type==='array')return <details className="rounded-lg border border-divider p-2"><summary className="cursor-pointer text-sm">{title} · {tr('고급 설정','Advanced settings')}</summary><JsonField value={value??(s.type==='array'?[]:{})} onChange={onChange} label={title} disabled={disabled}/></details>
 return <label className="block space-y-1 text-sm"><span>{title}</span><TextInput disabled={disabled} type={s.format==='password'?'password':s.type==='integer'||s.type==='number'?'number':'text'} autoComplete={s.format==='password'?'new-password':undefined} value={typeof value==='string'||typeof value==='number'?value:''} onChange={e=>onChange(s.type==='integer'||s.type==='number'?Number(e.target.value):e.target.value)}/>{s.description&&<span className="block text-xs text-muted">{s.description}</span>}</label>
}
function ImportItem({schema,disabled,en,onAdd}:{schema:Schema;disabled:boolean;en:boolean;onAdd:(v:unknown)=>void}){
 const [open,setOpen]=useState(false),[draft,setDraft]=useState(''),[error,setError]=useState('')
 return <div className="space-y-2 rounded-lg border border-divider p-2"><Button variant="ghost" size="sm" disabled={disabled} aria-expanded={open} onClick={()=>setOpen(v=>!v)}>{en?'Import item':'항목 가져오기'}</Button>
 {open&&<><p className="text-xs text-muted">{en?'Paste one configuration object. Existing items are preserved; review and save separately.':'설정 객체 1개를 붙여넣으세요. 기존 항목은 유지되며 검토 후 별도로 저장합니다.'}</p>
 <TextArea disabled={disabled} aria-label={en?'Configuration item JSON':'가져올 설정 JSON'} value={draft} onChange={e=>{setDraft(e.target.value);setError('')}} rows={4}/>
 {error&&<p role="alert" className="text-xs text-danger">{error}</p>}
 <Button size="sm" disabled={disabled||!draft.trim()} onClick={()=>{try{onAdd(importConfigItem(schema,draft,()=>crypto.randomUUID()));setDraft('');setError('');setOpen(false)}catch(e){setError((e as Error).message)}}}>{en?'Add to draft':'초안에 추가'}</Button></>}
 </div>
}
function JsonField({value,onChange,label,disabled}:{value:unknown;onChange:(v:unknown)=>void;label:string;disabled:boolean}){
 const [draft,setDraft]=useState(JSON.stringify(value,null,2)),[error,setError]=useState(false)
 return <><TextArea aria-label={label} disabled={disabled} value={draft} onChange={e=>{setDraft(e.target.value);try{onChange(JSON.parse(e.target.value));setError(false)}catch{setError(true)}}} rows={4}/>{error&&<p role="alert" className="text-xs text-danger">Invalid JSON — not applied</p>}</>
}
export function ExtensionConfiguration({pluginId}:{pluginId:string}){
 const en=useLocale()==='en',tr=(ko:string,eng:string)=>en?eng:ko,path='/api/plugins/managed/'+encodeURIComponent(pluginId)+'/configuration'
 const [config,setConfig]=useState<Config|null>(null),[busy,setBusy]=useState(false),[raw,setRaw]=useState(false),[draft,setDraft]=useState(''),[error,setError]=useState(''),[notice,setNotice]=useState('')
 const accept=(r:Config)=>{if(!r.values||typeof r.values!=='object'||Array.isArray(r.values))throw Error('Invalid configuration');setConfig(r);setDraft(JSON.stringify(r.values,null,2))}
 const load=async()=>{setBusy(true);setError('');setNotice('');try{const r=await get<Config>(path);accept(r);setRaw(!(r.schema?.type==='object'&&r.schema.properties))}catch(e){setError((e as Error).message)}finally{setBusy(false)}}
 const save=async()=>{if(!config)return;setBusy(true);setError('');setNotice('');try{const values=raw?JSON.parse(draft):config.values;accept(await post<Config>(path,{confirmed:true,values}));setNotice(tr('설정을 저장했습니다. 제공 도구를 확인하고 등록하세요.','Settings saved. Discover and register the provided tools.'))}catch(e){setError((e as Error).message)}finally{setBusy(false)}}
 const switchMode=()=>{try{if(raw){setConfig(c=>c?{...c,values:JSON.parse(draft)}:c)}else setDraft(JSON.stringify(config?.values,null,2));setRaw(!raw);setError('')}catch{setError(tr('JSON 형식을 확인하세요.','Check the JSON syntax.'))}}
 const close=()=>{if(!busy){setConfig(null);setDraft('');setError('')}}
 return <div className="space-y-2 rounded-xl border border-divider p-3"><div className="flex flex-wrap items-center justify-between gap-2"><strong>{tr('프로그램 설정','App settings')}</strong><Button size="sm" disabled={busy} onClick={()=>void load()}>{tr('설정 열기','Open settings')}</Button></div>
 <p className="text-xs text-muted">{tr('프로그램 자체 설정을 관리합니다.','Manage this extension’s own settings.')}</p>
 {!config&&error&&<p role="alert" className="break-words text-warning">{error}</p>}
 {config&&<DialogBackdrop onDismiss={close}><DialogPanel aria-label={tr('확장 프로그램 설정','Extension settings')} className="space-y-4 p-5"><div className="flex items-center justify-between gap-2"><h3 className="text-lg font-bold">{tr('확장 프로그램 설정','Extension settings')}</h3><Button variant="ghost" size="sm" disabled={busy} onClick={close}>{tr('닫기','Close')}</Button></div>
 <p className="text-sm text-muted">{tr('기존에 등록한 도구가 있다면 먼저 등록을 해제한 후 설정을 변경하세요.','Disable registered tools before changing the configuration.')}</p>
 {raw?<TextArea aria-label={tr('확장 프로그램 설정 JSON','Extension configuration JSON')} disabled={busy} value={draft} onChange={e=>setDraft(e.target.value)} rows={12}/>:config.schema&&<Field schema={config.schema} value={config.values} label="" en={en} disabled={busy} onChange={v=>setConfig(c=>c?{...c,values:object(v)}:c)}/>}
 {config.schema?.type==='object'&&config.schema.properties&&<Button size="sm" variant="ghost" disabled={busy} onClick={switchMode}>{raw?tr('일반 설정','Form settings'):tr('고급 JSON 설정','Advanced JSON settings')}</Button>}
 {raw&&config.description&&<p className="whitespace-pre-wrap break-words text-xs text-muted">{config.description}</p>}
 {error&&<p role="alert" className="break-words text-danger">{error}</p>}{notice&&<p role="status" className="text-success">{notice}</p>}
 <div className="flex justify-end gap-2 border-t border-divider pt-3"><Button variant="secondary" disabled={busy} onClick={close}>{tr('닫기','Close')}</Button><Button disabled={busy} onClick={()=>void save()}>{busy?tr('저장 중…','Saving…'):tr('설정 저장','Save settings')}</Button></div>
 </DialogPanel></DialogBackdrop>}
 </div>
}
