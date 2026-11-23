import {useEffect,useMemo,useState} from 'react'
import {Check,Copy,RotateCcw,Save,Search} from 'lucide-react'
import {get,post,ApiError} from '../../api'
import {Button,TextArea} from '../../components/Primitives'
import {Notice,Panel} from '../../components/UI'

type Template={id:string;title:string;category:string;location:string;default:string;text:string;revision:string;customized:boolean;required:string[]}
type Catalog={items:Template[];scope:string;runtimeGuardsEditable:boolean}
const api='/api/settings/harness-prompts'
const categories=['전체','대화','에이전트','실행','검증','복구','로컬 모델']

export function HarnessPromptsPanel(){
 const [items,setItems]=useState<Template[]>([])
 const [loading,setLoading]=useState(true),[busy,setBusy]=useState(false)
 const [selected,setSelected]=useState(''),[baseline,setBaseline]=useState(''),[baseRevision,setBaseRevision]=useState(''),[draft,setDraft]=useState('')
 const [filter,setFilter]=useState('전체'),[query,setQuery]=useState('')
 const [error,setError]=useState(''),[notice,setNotice]=useState(''),[conflict,setConflict]=useState(false)
 const current=items.find(item=>item.id===selected)
 const dirty=draft!==baseline
 const missing=(current?.required||[]).filter(key=>!draft.includes('{'+key+'}'))
 const visible=useMemo(()=>items.filter(item=>(filter==='전체'||item.category===filter)
  &&(item.title+' '+item.id+' '+item.location).toLowerCase().includes(query.trim().toLowerCase())),[items,filter,query])
 const adopt=(item:Template)=>{setSelected(item.id);setDraft(item.text);setBaseline(item.text);setBaseRevision(item.revision);setError('');setConflict(false)}
 const reload=async(first=false)=>{
  try{
   const data=await get<Catalog>(api)
   setItems(data.items)
   if(first&&data.items[0])adopt(data.items[0])
   return data
  }catch(e){setError((e as Error).message);return undefined}
  finally{setLoading(false)}
 }
 useEffect(()=>{void reload(true)},[])
 const choose=(item:Template)=>{
  if(dirty&&!window.confirm('저장하지 않은 변경 내용이 있습니다. 이동하시겠습니까?'))return
  adopt(item);setNotice('')
 }
 const save=async(reset=false)=>{
  if(!current||busy||conflict)return
  if(reset&&(!current.customized||!window.confirm('서브2 기본 지침으로 복원하시겠습니까?')))return
  if(!reset&&(!dirty||missing.length>0||draft.length>12000))return
  setBusy(true);setError('');setNotice('')
  try{
   const response=await post<Catalog>(api,{id:selected,expectedRevision:baseRevision,...(reset?{reset:true}:{text:draft})})
   setItems(response.items)
   const saved=response.items.find(item=>item.id===selected)
   if(saved)adopt(saved)
   setNotice(reset?'서브2 기본값을 복원했습니다.':'저장했습니다. 다음 모델 호출부터 반영됩니다.')
  }catch(e){
   setError((e as Error).message)
   if(e instanceof ApiError&&e.status===409){setConflict(true);await reload()}
  }finally{setBusy(false)}
 }
 const discard=()=>{if(current)adopt(current)}
 const rebase=()=>{if(current){setBaseline(current.text);setBaseRevision(current.revision);setConflict(false);setError('')}}
 const copy=async()=>{try{await navigator.clipboard.writeText(draft);setNotice('수정 중인 내용을 복사했습니다.')}catch(e){setError((e as Error).message)}}
 if(loading)return <Panel className="p-6 text-sm text-[#81786D]">서브2 지침을 불러오는 중입니다.</Panel>
 return <div className="space-y-4">
  <div className="flex flex-wrap items-start justify-between gap-3">
   <div><h2 className="text-lg font-bold">서브2 하네스 실행 지침</h2>
    <p className="mt-1 max-w-2xl text-[12px] leading-6 text-[#81786D]">하네스의 실제 모델 지침을 단계별로 수정합니다. 기본 문구는 보존됩니다. 도구 권한과 실행 승인·검증 판정은 Rust에서 그대로 관리합니다.</p></div>
   <Button variant="secondary" disabled={busy} onClick={()=>void reload()}>새로고침</Button>
  </div>
  <Notice message={error}/>
  {notice&&<p role="status" className="flex items-center gap-2 rounded-xl bg-emerald-50 px-4 py-3 text-[12px] text-emerald-800"><Check size={15}/>{notice}</p>}
  <div className="grid gap-4 lg:grid-cols-[minmax(240px,290px)_minmax(0,1fr)]">
   <Panel className="min-w-0 p-3">
    <label className="relative block"><Search size={15} className="absolute left-3 top-3 text-[#91877C]"/>
     <input aria-label="하네스 지침 검색" placeholder="지침 이름·적용 파일 검색" value={query} onChange={e=>setQuery(e.target.value)} className="w-full rounded-xl border border-[#E7E0D5] px-3 py-2.5 pl-9 text-[12px] outline-none focus:border-[#AFA59A]"/></label>
    <div className="mt-3 flex flex-wrap gap-1.5" role="group" aria-label="하네스 분류">
     {categories.map(category=><button type="button" key={category} aria-pressed={filter===category} onClick={()=>setFilter(category)}
      className={'rounded-lg px-2.5 py-1.5 text-[11px] '+(filter===category?'bg-[#34312D] text-white':'bg-[#F4F1EB] text-[#746B60]')}>{category}</button>)}
    </div>
    <div className="mt-3 max-h-[560px] space-y-1 overflow-y-auto" role="list" aria-label="실행 지침 목록">
     {visible.map(item=><button key={item.id} type="button" role="listitem" aria-current={selected===item.id?'true':undefined}
      onClick={()=>choose(item)} className={'flex w-full items-center justify-between gap-2 rounded-xl px-3 py-2.5 text-left '+(selected===item.id?'bg-[#EEEAE4]':'hover:bg-[#F5F2ED]')}>
      <span className="min-w-0"><span className="block text-[12px] font-semibold">{item.title}</span><small className="block truncate text-[10px] text-[#8B8176]">{item.id}</small></span>
      {item.customized&&<span className="rounded bg-[#E9DFD2] px-1.5 py-0.5 text-[9px] font-semibold">수정</span>}
     </button>)}
     {visible.length===0&&<p className="p-4 text-[12px] text-[#8A8177]">해당 지침이 없습니다.</p>}
    </div>
   </Panel>
   {current?<Panel className="min-w-0 space-y-4 p-5">
    <div className="flex flex-wrap items-start justify-between gap-2">
     <div><p className="text-[11px] text-[#8E8478]">{current.category} · {current.location}</p>
      <h3 className="mt-1 text-[16px] font-bold">{current.title}</h3><p className="mt-1 text-[10px] text-[#948B80]">{current.id} · {current.customized?'사용자 수정본':'기본값'}</p></div>
     <Button variant="ghost" size="sm" onClick={()=>void copy()}><Copy size={14}/> 복사</Button>
    </div>
    {conflict&&<div role="alert" className="rounded-xl border border-amber-300 bg-amber-50 p-3 text-[12px]">원본이 다른 곳에서 수정됐습니다. 작성한 내용은 유지했습니다.
     <div className="mt-2 flex gap-2"><Button size="sm" variant="secondary" onClick={rebase}>내 편집 계속</Button><Button size="sm" variant="ghost" onClick={discard}>새 원본 사용</Button></div>
    </div>}
    <label className="block space-y-2"><span className="block text-[12px] font-semibold">실제 모델에 전달되는 지침</span>
     <TextArea rows={14} spellCheck={false} aria-label="실제 모델에 전달되는 지침" value={draft} disabled={busy} onChange={e=>setDraft(e.target.value)} className="font-mono !text-[12px] !leading-6"/></label>
    {current.required.length>0&&<p className="rounded-lg bg-[#F5F1EA] p-3 text-[11px] leading-5 text-[#756B60]">
     실제 실행값: {current.required.map(key=>'{'+key+'}').join(', ')}
     {missing.length>0&&<span className="block text-rose-700">누락: {missing.join(', ')}</span>}</p>}
    <div className="flex flex-wrap items-center justify-between gap-2">
     <small className="text-[#92887B]">{draft.length.toLocaleString()} / 12,000자{dirty?' · 미저장':''}</small>
     <div className="flex flex-wrap gap-2">
      <Button variant="ghost" onClick={discard} disabled={busy||(!dirty&&!conflict)}>취소</Button>
      <Button variant="secondary" onClick={()=>void save(true)} disabled={busy||conflict||!current.customized}><RotateCcw size={14}/> 기본값</Button>
      <Button variant="primary" onClick={()=>void save()} disabled={busy||conflict||!dirty||draft.length>12000||missing.length>0}><Save size={14}/> 저장</Button>
     </div>
    </div>
    <details className="rounded-xl border border-[#E8E1D7] bg-[#FAF8F4] p-3">
     <summary className="cursor-pointer text-[12px] font-semibold">Rust 원본 지침 보기</summary>
     <pre className="mt-3 max-h-60 overflow-auto whitespace-pre-wrap break-words text-[11px] leading-6 text-[#6D645A]">{current.default}</pre>
    </details>
    <p className="text-[11px] text-[#8A8176]">저장한 지침은 다음 모델 호출부터 적용됩니다. 현재 진행 중인 작업에는 소급 적용하지 않습니다.</p>
   </Panel>:<Panel className="p-6 text-[12px] text-[#8A8176]">지침을 선택하세요.</Panel>}
  </div>
 </div>
}
