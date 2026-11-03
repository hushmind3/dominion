import {useState} from 'react'
import {ArrowRight,Check,Cloud,HardDrive,Plus,RefreshCw,Settings2,X} from 'lucide-react'
import {Provider,get,post} from '../api'
import {Button,TextInput,SelectInput} from '../components/Primitives'
import {FormField,Notice,refresh,useRemote} from '../components/UI'
import {DialogBackdrop,DialogPanel} from '../components/Dialog'
import {ModelExplorer} from '../components/ModelExplorer'

export function ModelHub(){
 const providers=useRemote<Provider[]>('/api/providers',[])
 const [counts,setCounts]=useState<Record<string,{list:string[];error?:string;busy?:boolean}>>({})
 const [error,setError]=useState('')
 const [modal,setModal]=useState(false)
 const [edit,setEdit]=useState<Provider|null>(null)
 const [name,setName]=useState(''),[baseUrl,setBaseUrl]=useState(''),[kind,setKind]=useState('openai'),[apiKey,setApiKey]=useState(''),[local,setLocal]=useState(false)
 const probe=async(p:Provider)=>{
  setCounts(old=>({...old,[p.id]:{list:[],busy:true}}))
  try{const list=await get<{id:string}[]>('/api/providers/'+p.id+'/models');setCounts(old=>({...old,[p.id]:{list:list.map(m=>m.id)}}))}
  catch(e){setCounts(old=>({...old,[p.id]:{list:[],error:(e as Error).message}}))}
 }
 const open=(p?:Provider)=>{setEdit(p||null);setName(p?.name||'');setBaseUrl(p?.baseUrl||'');setKind(p?.kind||'openai');setApiKey('');setLocal(Boolean(p?.local));setError('');setModal(true)}
 const save=async()=>{
  try{
   await post('/api/providers',{id:edit?.id,name,baseUrl,kind,apiKey:apiKey||undefined,local,enabled:true})
   setModal(false);await providers.reload();refresh()
  }catch(e){setError((e as Error).message)}
 }
 return <div className="mx-auto w-full max-w-[1200px] px-4 py-5 sm:px-6 lg:py-6">
  <header className="mb-5 flex flex-wrap items-end justify-between gap-3">
   <div><h1 className="m-0 text-[19px] font-bold tracking-tight text-[#24211D]">모델 관리</h1><p className="mt-1 text-[12px] text-[#7a8798]">로컬 모델을 등록하고 실행하거나 새 모델을 다운로드합니다.</p></div>
   <Button type="button" onClick={()=>open()} className="inline-flex h-8 items-center gap-2 rounded-lg border border-[#E1D9CC] bg-white px-3 text-[11px] font-semibold text-[#40506a] hover:bg-[#FAF7F1]"><Plus size={14}/> API 연결 추가</Button>
  </header>
  <ModelExplorer/>
  <section className="mt-5 rounded-xl border border-[#E7E0D5] bg-white">
   <div className="flex flex-wrap items-center justify-between gap-3 border-b border-[#EFE9DF] px-4 py-3"><div><h2 className="text-[13px] font-bold">모델 서버 · API 연결</h2><p className="mt-0.5 text-[11px] text-[#8b97a7]">Ollama, LM Studio, OpenAI 호환 제공자를 연결할 수 있습니다.</p></div><Button className="inline-flex h-8 items-center gap-2 rounded-lg border border-[#E1D9CC] px-3 text-[11px]" type="button" onClick={()=>providers.value.forEach(p=>void probe(p))}><RefreshCw size={13}/> 연결 확인</Button></div>
   {providers.value.length?<div className="divide-y divide-[#F0EAE1]">{providers.value.map(p=><div key={p.id} className="flex flex-wrap items-center gap-3 px-4 py-3">
    <span className="grid size-8 shrink-0 place-items-center rounded-lg bg-[#edf1fa] text-[#6179b6]">{p.local?<HardDrive size={15}/>:<Cloud size={15}/>}</span>
    <div className="min-w-0 flex-1"><div className="text-[12px] font-semibold">{p.name}{!p.enabled&&<span className="ml-2 text-[10px] text-[#a0aab8]">비활성화</span>}</div><div className="mt-0.5 break-all text-[11px] text-[#8b98a7]">{p.baseUrl}</div>
    {counts[p.id]?.error&&<div className="mt-1 text-[11px] text-rose-600">{counts[p.id].error}</div>}
    {counts[p.id]?.list.length>0&&<div className="mt-1 text-[10px] text-emerald-700">{counts[p.id].list.length}개 모델 인식됨 · {counts[p.id].list.slice(0,2).join(', ')}</div>}</div>
    <Button className="rounded-md px-2 py-1 text-[11px] text-[#C96442] hover:bg-[#FBF1EC]" type="button" onClick={()=>void probe(p)}>{counts[p.id]?.busy?'확인 중':'테스트'}</Button>
    <Button className="rounded-md p-2 text-[#8290a1] hover:bg-[#F3EEE5]" type="button" onClick={()=>open(p)} aria-label={p.name+' 설정'}><Settings2 size={15}/></Button>
   </div>)}</div>:<div className="px-4 py-6 text-[12px] text-[#8693a3]">연결된 제공자가 없습니다. API 연결 추가로 등록하세요.</div>}
  </section>
  {!modal&&error&&<Notice message={error}/>}
  {modal&&<DialogBackdrop onDismiss={()=>setModal(false)}><DialogPanel size="default">
   <div className="flex items-center justify-between border-b border-[#EFE9DF] p-4"><h2 className="text-[15px] font-bold">{edit?'연결 편집':'모델 서버 연결'}</h2><Button size="icon" variant="ghost" aria-label="닫기" onClick={()=>setModal(false)}><X size={16}/></Button></div>
   <div className="grid gap-3 p-4"><FormField label="연결 이름"><TextInput value={name} onChange={e=>setName(e.target.value)} placeholder="예: 내 LM Studio"/></FormField><FormField label="API URL"><TextInput value={baseUrl} onChange={e=>setBaseUrl(e.target.value)} placeholder="http://127.0.0.1:1234/v1"/></FormField><FormField label="형식"><SelectInput value={kind} onChange={e=>setKind(e.target.value)}><option value="openai">OpenAI 호환</option><option value="ollama">Ollama</option></SelectInput></FormField><FormField label="API 키"><TextInput type="password" value={apiKey} onChange={e=>setApiKey(e.target.value)} placeholder="기존 키 유지 시 비워두기"/></FormField><label className="flex items-center gap-2 text-[12px]"><TextInput type="checkbox" checked={local} onChange={e=>setLocal(e.target.checked)}/> 로컬 서버</label>{error&&<Notice message={error}/>}</div>
   <div className="flex justify-end gap-2 border-t border-[#EFE9DF] p-4"><Button onClick={()=>setModal(false)}>취소</Button><Button variant="primary" onClick={()=>void save()}><Check size={14}/> 저장</Button></div>
  </DialogPanel></DialogBackdrop>}
 </div>
}
