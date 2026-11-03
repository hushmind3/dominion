import {Button,TextInput,SelectInput} from './Primitives'
import {useCallback,useEffect,useMemo,useState} from 'react'
import {ArrowDownToLine,BookOpen,BrainCircuit,Check,ChevronDown,CloudDownload,Code2,Cpu,Download,ExternalLink,Eye,HardDrive,LoaderCircle,Pause,Play,RefreshCw,Search,Server,SlidersHorizontal,Sparkles,Square,Trash2,Wrench,X} from 'lucide-react'
import {del,get,post} from '../api'

type Model={id:string;downloads:number;likes:number;updatedAt:string;task:string;tags:string[];gated:boolean;description:string;capabilities:{vision:boolean;tools:boolean;reasoning:boolean}}
type Option={id:string;format:'gguf'|'mlx';label:string;quantization:string;bytes:number;files:string[];shards:number}
type Detail=Model&{revision:string;architecture:string;license:string;readme:string;options:Option[]}
type Installed={id:string;repo:string;revision:string;optionId:string;format:'gguf'|'mlx';label:string;files:string[];bytes:number;installedAt:string}
type Job={id:string;repo:string;optionId:string;status:string;downloaded:number;total:number;current:string;error:string}
type Runtime={status:string;id?:string;repo?:string;name?:string;engine?:string;error?:string;capabilities?:{gguf:{available:boolean};mlx:{available:boolean};memory:{total:number;free:number;platform:string;arch:string};gpu?:{kind:string;total:number;free:number;source:string}|null}}
const fmt=(n=0)=>n>=1e9?(n/1e9).toFixed(2)+' GB':n>=1e6?(n/1e6).toFixed(0)+' MB':n>=1e3?(n/1e3).toFixed(0)+' KB':n+' B'
const compact=(n=0)=>n>=1e6?(n/1e6).toFixed(1)+'M':n>=1e3?(n/1e3).toFixed(0)+'K':String(n)
const same=(m:Installed,o:Option,detail:Detail)=>m.repo===detail.id&&m.optionId===o.id&&m.revision===detail.revision
export function ModelExplorer(){
 const [tab,setTab]=useState<'explore'|'installed'>('explore')
 const [q,setQ]=useState('')
 const [format,setFormat]=useState('all')
 const [sort,setSort]=useState('downloads')
 const [items,setItems]=useState<Model[]>([])
 const [selected,setSelected]=useState('')
 const [detail,setDetail]=useState<Detail|null>(null)
 const [variant,setVariant]=useState('')
 const [busy,setBusy]=useState(false)
 const [loading,setLoading]=useState(false)
 const [error,setError]=useState('')
 const [installed,setInstalled]=useState<Installed[]>([])
 const [jobs,setJobs]=useState<Job[]>([])
 const [runtime,setRuntime]=useState<Runtime>({status:'idle'})
 const [running,setRunning]=useState('')
 const [context,setContext]=useState(4096)
 const [gpu,setGpu]=useState(true)
 const refresh=useCallback(async()=>{
  const results=await Promise.allSettled([get<Installed[]>('/api/model-library/installed'),get<Job[]>('/api/model-library/jobs'),get<Runtime>('/api/model-library/runtime')])
  if(results[0].status==='fulfilled')setInstalled(results[0].value)
  if(results[1].status==='fulfilled')setJobs(results[1].value)
  if(results[2].status==='fulfilled')setRuntime(results[2].value)
 },[])
 useEffect(()=>{void refresh();const timer=setInterval(()=>void refresh(),1800);return()=>clearInterval(timer)},[refresh])
 useEffect(()=>{
  let live=true
  const timer=setTimeout(async()=>{
   setBusy(true)
   try{
    const params=new URLSearchParams({q,format,sort})
    const rows=await get<Model[]>('/api/model-library/catalog?'+params)
    if(!live)return
    setItems(rows)
    setSelected(s=>rows.some(r=>r.id===s)?s:rows[0]?.id||'')
    setError('')
   }catch(e){if(live)setError((e as Error).message)}
   finally{if(live)setBusy(false)}
  },250)
  return()=>{live=false;clearTimeout(timer)}
 },[q,format,sort])
 useEffect(()=>{
  if(!selected){setDetail(null);return}
  let live=true
  setLoading(true);setDetail(null);setVariant('')
  get<Detail>('/api/model-library/details?id='+encodeURIComponent(selected)).then(d=>{
   if(!live)return
   setDetail(d)
   setVariant(d.options.find(o=>format==='all'||o.format===format)?.id||d.options[0]?.id||'')
  }).catch(e=>{if(live)setError(e.message)}).finally(()=>{if(live)setLoading(false)})
  return()=>{live=false}
 },[selected])
 const selectedOption=detail?.options.find(x=>x.id===variant)
 const selectedInstalled=useMemo(()=>detail&&selectedOption?installed.find(x=>same(x,selectedOption,detail)):undefined,[detail,installed,selectedOption])
 const selectedJob=useMemo(()=>detail&&selectedOption?jobs.find(j=>j.repo===detail.id&&j.optionId===selectedOption.id&&['resolving','downloading','paused','failed','cancelled'].includes(j.status)):undefined,[jobs,detail,selectedOption])
 const gpuInfo=runtime.capabilities?.gpu
 const estimatedMemory=selectedOption?selectedOption.bytes*1.12+750_000_000+(context/4096)*1_100_000_000:0
 const mayFit=Boolean(gpuInfo&&estimatedMemory<=gpuInfo.free)
 const install=async()=>{
  if(!detail||!selectedOption)return
  setRunning('download');setError('')
  try{await post('/api/model-library/download',{repo:detail.id,optionId:selectedOption.id});await refresh()}
  catch(e){setError((e as Error).message)}
  finally{setRunning('')}
 }
 const control=async(id:string,action:'pause'|'resume'|'cancel')=>{
  setRunning(action+id);setError('')
  try{await post('/api/model-library/jobs/'+id+'/'+action,{});await refresh()}
  catch(e){setError((e as Error).message)}
  finally{setRunning('')}
 }
 const load=async(id:string)=>{
  setRunning('load'+id);setError('')
  try{await post('/api/model-library/runtime/load',{id,context,gpu});await refresh()}
  catch(e){setError((e as Error).message)}
  finally{setRunning('')}
 }
 const unload=async()=>{
  setRunning('unload')
  try{await post('/api/model-library/runtime/unload',{});await refresh()}
  catch(e){setError((e as Error).message)}
  finally{setRunning('')}
 }
 const remove=async(id:string)=>{
  if(!window.confirm('로컬에 다운로드한 모델 파일을 삭제하시겠습니까?'))return
  setRunning('remove'+id)
  try{await del('/api/model-library/installed/'+id);await refresh()}
  catch(e){setError((e as Error).message)}
  finally{setRunning('')}
 }
 const status=runtime.status==='ready'?'모델 실행 중':runtime.status==='loading'?'모델 로딩 중':runtime.status==='error'?'실행 오류':'대기 중'
 return <section className="my-9 min-w-0 overflow-hidden rounded-2xl border border-slate-200 bg-white shadow-sm">
  <div className="flex flex-wrap items-center justify-between gap-4 border-b border-slate-200 p-5"><div><span className="text-[10px] font-extrabold uppercase tracking-widest text-blue-600"><Sparkles size={14}/> LOCAL MODEL LIBRARY</span><h2>모델을 찾고, 설치하고, 실행하세요.</h2><p>공개 모델 저장소에서 검색하고 GGUF·MLX 배포 파일을 직접 관리합니다.</p></div><div className="flex flex-wrap items-center gap-2 text-xs text-slate-500"><span className={'ml-status-dot '+(runtime.status==='ready'?'green':'')}/><div><strong>{status}</strong><small>{runtime.repo||'DOMINION 로컬 실행 환경'}</small></div></div></div>
  <div className="flex flex-wrap items-center gap-2 border-b border-slate-200 px-5 pt-4 pb-3"><Button className={tab==='explore'?'active':''} onClick={()=>setTab('explore')}><Search size={16}/> 모델 탐색</Button><Button className={tab==='installed'?'active':''} onClick={()=>setTab('installed')}><HardDrive size={16}/> 설치된 모델 <span>{installed.length}</span></Button><Button className="inline-flex items-center gap-1.5 text-xs font-semibold text-blue-600 hover:text-blue-700" onClick={()=>void refresh()} title="새로고침"><RefreshCw size={16}/></Button></div>
  {error&&<div className="m-4 rounded-xl border border-rose-200 bg-rose-50 p-3 text-xs leading-6 text-rose-700"><span>{error}</span><Button onClick={()=>setError('')} title="닫기"><X size={16}/></Button></div>}
  {tab==='explore'?<div className="grid min-h-[490px] grid-cols-1 lg:grid-cols-[340px_minmax(0,1fr)]">
   <aside className="flex min-h-0 min-w-0 flex-col border-b border-slate-200 lg:border-b-0 lg:border-r">
    <div className="flex items-center gap-2 border-b border-slate-100 p-3"><Search size={18}/><TextInput aria-label="모델 검색" placeholder="모델명 또는 제작사 검색..." value={q} onChange={e=>setQ(e.target.value)}/>{busy&&<LoaderCircle size={17} className="animate-spin"/>}{q&&<Button title="검색어 지우기" onClick={()=>setQ('')}><X size={15}/></Button>}</div>
    <div className="flex flex-wrap items-center gap-2 border-b border-slate-100 p-3"><div className="inline-flex items-center gap-2 rounded-lg border border-slate-200 px-2.5 py-1.5 text-xs text-slate-600">{['all','gguf','mlx'].map(x=><Button key={x} className={format===x?'on':''} onClick={()=>setFormat(x)}>{x==='all'?'전체':x.toUpperCase()}</Button>)}</div><label><SlidersHorizontal size={14}/><SelectInput aria-label="모델 정렬" value={sort} onChange={e=>setSort(e.target.value)}><option value="downloads">다운로드순</option><option value="likes">인기순</option><option value="lastModified">최신순</option></SelectInput><ChevronDown size={13}/></label></div>
    <div className="px-4 py-3 text-[10px] font-bold uppercase tracking-widest text-slate-400">{q?'검색 결과':'Hugging Face 인기 모델'} <span>{items.length}개</span></div>
    <div className="max-h-[490px] min-h-0 overflow-y-auto px-2 pb-2">
     {items.map(m=><Button key={m.id} className={'ml-model '+(selected===m.id?'selected':'')} onClick={()=>setSelected(m.id)}><span className="grid size-10 shrink-0 place-items-center rounded-xl bg-blue-50 text-blue-600"><Cpu size={21}/></span><span className="min-w-0 flex-1 break-words text-xs text-slate-600"><strong>{m.id}</strong><small>{m.task||m.tags.find(t=>t==='gguf'||t==='mlx')||'모델'} · ↓ {compact(m.downloads)} · ♥ {compact(m.likes)}</small></span>{m.gated&&<span className="rounded-md bg-amber-50 px-2 py-0.5 text-[10px] font-semibold text-amber-700">승인</span>}</Button>)}
     {!items.length&&!busy&&<div className="p-8 text-center text-sm text-slate-400">모델을 찾지 못했습니다. 다른 이름이나 필터로 검색해 보세요.</div>}
    </div>
    <div className="border-t border-slate-100 px-4 py-3 text-xs text-slate-400"><CloudDownload size={15}/> 검색·파일 정보 출처: Hugging Face</div>
   </aside>
   <div className="min-w-0 p-5">
    {loading?<div className="flex min-h-80 flex-col items-center justify-center gap-3 text-center text-sm text-slate-500"><LoaderCircle size={26} className="animate-spin"/> 모델 정보 불러오는 중</div>:detail?<><div className="mb-4 flex min-w-0 flex-wrap items-start justify-between gap-3"><div><small>MODEL REPOSITORY</small><h3>{detail.id}</h3></div><a href={'https://huggingface.co/'+detail.id} target="_blank" rel="noreferrer" aria-label="Hugging Face 원본"><ExternalLink size={18}/></a></div>
    <div className="my-4 grid grid-cols-2 gap-3 sm:grid-cols-3"><span><Download size={15}/> {compact(detail.downloads)}</span><span>♡ {compact(detail.likes)}</span>{detail.updatedAt&&<span>업데이트 {new Date(detail.updatedAt).toLocaleDateString('ko-KR')}</span>}</div>
    <div className="rounded-xl bg-slate-50 p-4 text-sm leading-6 text-slate-600"><p>{detail.description||detail.readme.replace(/^---[\s\S]*?---/,'').replace(/[#*\[\]>]/g,'').trim().slice(0,290)||'모델 저장소의 파일과 형식을 아래에서 확인하세요.'}</p><div className="flex flex-wrap items-center gap-2 text-xs text-slate-500">{detail.architecture&&<span>ARCH <b>{String(detail.architecture).slice(0,55)}</b></span>}{detail.license&&<span>LICENSE <b>{detail.license}</b></span>}<span>OPTIONS <b>{detail.options.length}</b></span></div><div className="mt-4 flex flex-wrap items-center gap-2">{detail.capabilities.vision&&<span><Eye size={14}/> Vision</span>}{detail.capabilities.tools&&<span><Wrench size={14}/> Tool Use</span>}{detail.capabilities.reasoning&&<span><BrainCircuit size={14}/> Reasoning</span>}{!detail.capabilities.vision&&!detail.capabilities.tools&&!detail.capabilities.reasoning&&<span><Code2 size={14}/> {detail.task||'Text model'}</span>}</div></div>
    <h4 className="my-4 text-sm font-extrabold text-slate-800">다운로드 옵션 <small>원본 저장소 배포 파일</small></h4>
    <div className="max-h-56 space-y-2 overflow-y-auto">{detail.options.filter(o=>format==='all'||o.format===format).map(o=><Button key={o.id} className={'ml-option '+(variant===o.id?'chosen':'')} onClick={()=>setVariant(o.id)}><span className={'ml-option-format '+o.format}>{o.format.toUpperCase()}</span><span className="min-w-0 flex-1 break-words text-xs font-semibold text-slate-700"><strong>{o.label}</strong><small>{o.quantization}{o.shards>1?' · '+o.shards+'개 파일':''}</small></span><span className="shrink-0 text-xs text-slate-500">{fmt(o.bytes)}</span>{variant===o.id&&<Check size={17}/>}</Button>)}</div>
    {!detail.options.length&&<div className="p-8 text-center text-sm text-slate-400">지원되는 GGUF 또는 MLX 다운로드 파일이 없습니다.</div>}
    {selectedOption&&<div className="mt-5 rounded-xl border border-slate-200 bg-slate-50 p-4"><div className="mt-3 flex flex-wrap items-center gap-2 text-xs text-slate-600"><span><HardDrive size={15}/> 디스크 {fmt(selectedOption.bytes)}</span><span><Cpu size={15}/> 모델 파일 외에 KV 캐시·실행 메모리 필요</span>{gpu&&gpuInfo&&<span className={'ml-memory-fit '+(mayFit?'yes':'caution')}><Cpu size={15}/>{gpuInfo.kind==='unified'?'통합 메모리':'GPU VRAM'} {fmt(gpuInfo.free)} 여유 · 예상 {fmt(estimatedMemory)} · {mayFit?'적재 가능성 높음':'전체 적재 어려울 수 있음'} (추정)</span>}{gpu&&!gpuInfo&&<span>GPU 메모리 정보가 없어 실제 로딩 후 확인이 필요합니다.</span>}</div>
     {selectedJob&&<><div className="flex flex-wrap items-center justify-between gap-2 text-xs text-slate-600"><strong>{selectedJob.status==='downloading'?'다운로드 중':selectedJob.status==='paused'?'일시정지':selectedJob.status==='failed'?'오류':selectedJob.status==='resolving'?'준비 중':'중단됨'}</strong><span>{fmt(selectedJob.downloaded)} / {fmt(selectedJob.total||selectedOption.bytes)}</span></div><div className="mt-2 h-2 w-full overflow-hidden rounded-full bg-slate-200"><span style={{width:Math.min(100,100*selectedJob.downloaded/Math.max(1,selectedJob.total||selectedOption.bytes))+'%'}}/></div><small className="mt-2 truncate text-xs text-slate-400">{selectedJob.error||selectedJob.current||'파일 준비 중'}</small><div className="mt-3 flex flex-wrap items-center gap-2">{selectedJob.status==='downloading'&&<Button onClick={()=>void control(selectedJob.id,'pause')}><Pause size={14}/> 일시정지</Button>}{['paused','failed','cancelled'].includes(selectedJob.status)&&<Button onClick={()=>void control(selectedJob.id,'resume')}><Play size={14}/> 재개</Button>}{['downloading','paused','resolving'].includes(selectedJob.status)&&<Button onClick={()=>void control(selectedJob.id,'cancel')}><Square size={14}/> 중단</Button>}</div></>}
     {selectedInstalled?<div className="inline-flex items-center gap-2 rounded-xl border border-emerald-200 bg-emerald-50 px-3 py-2 text-xs font-semibold text-emerald-700"><span><Check size={17}/> 설치됨</span><Button className="inline-flex min-h-9 items-center justify-center gap-2 rounded-xl border border-blue-600 bg-blue-600 px-4 py-2 text-xs font-bold text-white hover:bg-blue-700" disabled={Boolean(running)||runtime.status==='loading'} onClick={()=>void load(selectedInstalled.id)}><Play size={17}/> {runtime.id===selectedInstalled.id&&runtime.status==='ready'?'다시 실행':'모델 로드'}</Button></div>:<Button className="inline-flex min-h-9 items-center justify-center gap-2 rounded-xl border border-blue-600 bg-blue-600 px-4 py-2 text-xs font-bold text-white hover:bg-blue-700 inline-flex min-h-9 items-center justify-center gap-2 rounded-xl border border-slate-200 bg-white px-4 py-2 text-xs font-bold text-blue-700 hover:bg-slate-50" disabled={!selectedOption||Boolean(running)||selectedJob?.status==='downloading'||selectedJob?.status==='resolving'} onClick={()=>void install()}><ArrowDownToLine size={18}/>{running==='download'?'다운로드 준비 중':selectedJob?.status==='paused'?'다운로드 재개 버튼을 이용하세요':'다운로드'} <span>{fmt(selectedOption.bytes)}</span></Button>}
    </div>}
    <h4 className="my-4 text-sm font-extrabold text-slate-800"><BookOpen size={16}/> README</h4><pre className="mt-4 max-h-72 overflow-y-auto rounded-xl bg-slate-50 p-4 text-xs leading-6 text-slate-600">{detail.readme.replace(/^---[\s\S]*?---/,'').trim().slice(0,3500)||'저장소에 README가 없거나 읽을 수 없습니다.'}</pre></>:<div className="flex min-h-80 flex-col items-center justify-center gap-3 text-center text-sm text-slate-500"><Search size={25}/> 모델을 선택하세요.</div>}
   </div>
  </div>:<div className="grid grid-cols-1 gap-3 p-4 lg:grid-cols-2">{installed.length?installed.map(m=><div className="flex min-w-0 flex-col gap-3 rounded-xl border border-slate-200 bg-white p-4" key={m.id}><span className="grid size-10 shrink-0 place-items-center rounded-xl bg-indigo-50 text-indigo-600"><HardDrive size={22}/></span><div className="min-w-0 flex-1 break-words text-xs text-slate-600"><strong>{m.repo}</strong><span>{m.label} · {m.format.toUpperCase()} · {fmt(m.bytes)}</span><small>설치 {new Date(m.installedAt).toLocaleString('ko-KR')}</small></div><div className="flex flex-wrap items-center gap-2">{runtime.id===m.id&&runtime.status==='ready'?<Button className="inline-flex items-center gap-2 rounded-xl border border-rose-200 bg-rose-50 px-3 py-2 text-xs font-semibold text-rose-700" disabled={Boolean(running)} onClick={()=>void unload()}><Square size={15}/> 언로드</Button>:<Button className="inline-flex min-h-9 items-center justify-center gap-2 rounded-xl border border-blue-600 bg-blue-600 px-4 py-2 text-xs font-bold text-white hover:bg-blue-700" disabled={Boolean(running)||runtime.status==='loading'} onClick={()=>void load(m.id)}><Play size={15}/> 실행</Button>}<Button className="inline-grid size-9 place-items-center rounded-lg text-slate-500 hover:bg-slate-100" disabled={Boolean(running)} onClick={()=>void remove(m.id)} aria-label="모델 삭제"><Trash2 size={16}/></Button></div></div>):<div className="p-8 text-center text-sm text-slate-400 p-10 text-center text-sm text-slate-400"><HardDrive size={27}/> 설치된 모델이 없습니다. 모델 탐색에서 다운로드할 수 있습니다.</div>}</div>}
  <div className="mt-5 rounded-xl border border-slate-200 bg-slate-50 p-4"><div><Server size={18}/><strong>로컬 실행 환경</strong><span>GGUF · {runtime.capabilities?.gguf.available?'llama-server 사용 가능':'llama-server 필요'}</span><span>MLX · {runtime.capabilities?.mlx.available?'mlx-lm 사용 가능':'Apple Silicon + mlx-lm 필요'}</span>{runtime.capabilities?.memory&&<span>RAM {fmt(runtime.capabilities.memory.total)} · 여유 {fmt(runtime.capabilities.memory.free)}</span>}</div><div className="mt-4 flex flex-wrap items-center gap-3"><label>컨텍스트<SelectInput value={context} onChange={e=>setContext(Number(e.target.value))}><option value={2048}>2K</option><option value={4096}>4K</option><option value={8192}>8K</option><option value={16384}>16K</option><option value={32768}>32K</option></SelectInput></label><label className="inline-flex items-center gap-2 text-xs text-slate-600"><TextInput type="checkbox" checked={gpu} onChange={e=>setGpu(e.target.checked)}/> GGUF GPU 자동 배치</label>{runtime.status==='loading'&&<span className="mt-3 text-xs text-slate-600"><LoaderCircle size={14} className="animate-spin"/> 모델 로딩 중</span>}{runtime.status==='ready'&&<Button className="inline-flex items-center gap-2 rounded-xl border border-rose-200 bg-rose-50 px-3 py-2 text-xs font-semibold text-rose-700" onClick={()=>void unload()}><Square size={14}/> 실행 중지</Button>}</div></div>
  {runtime.status==='error'&&runtime.error&&<div className="m-4 rounded-xl border border-rose-200 bg-rose-50 p-3 text-xs leading-6 text-rose-700">{runtime.error}</div>}
 </section>
}
