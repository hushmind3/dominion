import {DialogBackdrop,DialogPanel} from '../components/Dialog'
import {Button,TextInput,SelectInput} from '../components/Primitives'
import {useState} from 'react'
import {ArrowRight,Check,Cloud,Cpu,HardDrive,Plus,RefreshCw,Server,Settings2,X} from 'lucide-react'
import {Provider,get,post} from '../api'
import {Action,FormField,Headline,Notice,Panel,StateTag,Subheading,refresh,useRemote} from '../components/UI'
import {ModelExplorer} from '../components/ModelExplorer'

export function ModelHub(){
 const providers=useRemote<Provider[]>('/api/providers',[])
 const [models,setModels]=useState<Record<string,{list:string[];error?:string}>>({})
 const [error,setError]=useState('')
 const [adding,setAdding]=useState(false)
 const [edit,setEdit]=useState<Provider|null>(null)
 const [name,setName]=useState(''),[baseUrl,setBaseUrl]=useState(''),[kind,setKind]=useState('openai'),[apiKey,setApiKey]=useState(''),[local,setLocal]=useState(false)
 const probe=async(p:Provider)=>{
  setModels(x=>({...x,[p.id]:{list:[]}}))
  try{const r=await get<{id:string}[]>('/api/providers/'+p.id+'/models');setModels(x=>({...x,[p.id]:{list:r.map(m=>m.id)}}))}
  catch(e){setModels(x=>({...x,[p.id]:{list:[],error:(e as Error).message}}))}
 }
 const openEdit=(p?:Provider)=>{
  setAdding(true);setEdit(p||null);setName(p?.name||'');setBaseUrl(p?.baseUrl||'');setKind(p?.kind||'openai');setApiKey('');setLocal(Boolean(p?.local))
 }
 const save=async()=>{
  setError('')
  try{await post('/api/providers',{id:edit?.id,name,baseUrl,kind,apiKey:apiKey||undefined,local,enabled:true});setAdding(false);await providers.reload();refresh()}
  catch(e){setError((e as Error).message)}
 }
 return <div className="mx-auto w-full max-w-[1320px] px-5 py-9 sm:px-8 lg:px-11 lg:pb-20">
  <Headline eyebrow="INTELLIGENCE · MODEL HUB" title="모든 모델을, 하나의 공간에." description="로컬 모델 검색·설치·실행부터 클라우드 API 연결까지 한곳에서 관리합니다." actions={<Action variant="secondary" onClick={()=>openEdit()}><Plus size={16}/> 제공자 추가</Action>}/>
  <div className="mb-8 grid grid-cols-1 gap-4 md:grid-cols-3"><div className="flex items-center gap-4 rounded-2xl border border-slate-200 bg-white p-5 shadow-sm"><span className="grid size-12 place-items-center rounded-xl bg-violet-50 text-violet-600"><Cpu size={20}/></span><div><small>연결된 제공자</small><strong>{providers.value.length}</strong></div></div><div className="flex items-center gap-4 rounded-2xl border border-slate-200 bg-white p-5 shadow-sm"><span className="grid size-12 place-items-center rounded-xl bg-blue-50 text-blue-600"><Server size={20}/></span><div><small>로컬 엔진</small><strong>{providers.value.filter(p=>p.local).length}</strong></div></div><div className="flex items-center gap-4 rounded-2xl border border-slate-200 bg-white p-5 shadow-sm"><span className="grid size-12 place-items-center rounded-xl bg-teal-50 text-teal-600"><Cloud size={20}/></span><div><small>API 제공자</small><strong>{providers.value.filter(p=>!p.local).length}</strong></div></div></div>
  <ModelExplorer/>
  <div className="mt-8"><Subheading title="모델 API · 외부 엔진 연결" extra={<Button className="inline-flex items-center gap-2 text-xs font-semibold text-slate-600 hover:text-blue-600" onClick={()=>providers.value.forEach(probe)}><RefreshCw size={15}/> 연결 확인</Button>}/><div className="grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3">{providers.value.map(p=><Panel className="flex min-w-0 flex-col gap-3 p-5" key={p.id}><div className="flex items-start justify-between gap-3"><span className={p.local?'grid size-11 place-items-center rounded-xl bg-emerald-50 text-emerald-700':'grid size-11 place-items-center rounded-xl bg-blue-50 text-blue-600'}>{p.local?<HardDrive size={21}/>:<Cloud size={21}/>}</span><Button size="icon" variant="ghost" title="설정" onClick={()=>openEdit(p)}><Settings2 size={17}/></Button></div><strong>{p.name}</strong><p>{p.baseUrl}</p><div className="flex flex-wrap items-center justify-between gap-3 border-t border-slate-100 pt-3"><StateTag online={Boolean(models[p.id]&&!models[p.id].error&&models[p.id].list.length)}>{models[p.id]?.error?'연결 실패':models[p.id]?.list.length?models[p.id].list.length+'개 모델':p.local?'로컬':'API / 클라우드'}</StateTag><Button onClick={()=>probe(p)}>모델 확인 <ArrowRight size={14}/></Button></div>{(models[p.id]?.list.length||0)>0&&<div className="flex flex-wrap gap-1.5 text-xs text-slate-500">{models[p.id].list.slice(0,4).map(m=><span key={m}>{m}</span>)}</div>}{models[p.id]?.error&&<small className="break-words text-xs text-rose-600">{models[p.id].error}</small>}</Panel>)}</div></div>
  {error&&<Notice message={error}/>}
  {adding&&<DialogBackdrop onDismiss={()=>setAdding(false)}><DialogPanel size="default"><div className="flex items-center justify-between border-b border-slate-100 p-5"><div><small>PROVIDER SETUP</small><h2>{edit?'모델 제공자 설정':'모델 제공자 연결'}</h2></div><Button size="icon" variant="ghost" onClick={()=>setAdding(false)}><X size={18}/></Button></div><div className="grid gap-4 p-5"><FormField label="이름"><TextInput  value={name} onChange={e=>setName(e.target.value)} placeholder="예: OpenRouter"/></FormField><FormField label="API Base URL"><TextInput  value={baseUrl} onChange={e=>setBaseUrl(e.target.value)} placeholder="https://api.example.com/v1"/></FormField><FormField label="프로토콜"><SelectInput  value={kind} onChange={e=>setKind(e.target.value)}><option value="openai">OpenAI 호환</option><option value="ollama">Ollama</option></SelectInput></FormField><FormField label="API Key" hint="서버의 암호화 저장소에 보관됩니다. 기존 키를 유지하려면 비워두세요."><TextInput type="password"  value={apiKey} onChange={e=>setApiKey(e.target.value)} placeholder="sk-..."/></FormField><label className="inline-flex items-center gap-2 text-sm text-slate-700"><TextInput type="checkbox" checked={local} onChange={e=>setLocal(e.target.checked)}/> 로컬 서버</label></div><div className="flex flex-wrap justify-end gap-2 border-t border-slate-100 p-5"><Action variant="secondary" onClick={()=>setAdding(false)}>취소</Action><Action onClick={save}><Check size={16}/> 연결 저장</Action></div></DialogPanel></DialogBackdrop>}
 </div>
}
