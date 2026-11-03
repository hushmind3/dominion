import {useCallback,useEffect,useMemo,useState} from 'react'
import {ArrowDownToLine,ArrowRight,Check,ChevronDown,CircleAlert,CloudDownload,Cpu,ExternalLink,FolderOpen,HardDrive,LoaderCircle,Pause,Play,RefreshCw,Search,Square,Trash2,X} from 'lucide-react'
import {Button,SelectInput,TextInput} from './Primitives'
import {get,post,del} from '../api'
declare global {interface Window {dominionDesktop?:{pickModel:(format:'gguf'|'mlx')=>Promise<string>}}}
type Catalog={id:string;author:string;name:string;library:string;createdAt?:string;downloads:number;likes:number;gated:boolean;updatedAt:string;description:string;task:string;tags:string[]}
type Option={id:string;format:'gguf'|'mlx';label:string;quantization:string;bytes:number;files:string[];shards:number}
type Detail=Catalog&{revision:string;license:string;architecture:string;options:Option[];readme:string}
type Installed={id:string;repo:string;optionId?:string;source?:string;externalPath?:string;format:'gguf'|'mlx';label:string;bytes:number;installedAt:string}
type Job={id:string;repo:string;optionId:string;status:string;downloaded:number;total:number;current:string;error:string}
type Runtime={status:string;id?:string;repo?:string;engine?:string;name?:string;error?:string;setup?:{status:string;backend:string;downloaded:number;total:number;tag:string;file?:string;error:string};capabilities?:{gguf:{available:boolean;backend?:string;installable?:boolean};mlx:{available:boolean};memory:{total:number;free:number;platform?:string;arch?:string};gpu?:{kind:string;free:number}|null}}
const fmt=(v=0)=>v>=1073741824?(v/1073741824).toFixed(1)+' GB':v>=1048576?(v/1048576).toFixed(0)+' MB':v>=1024?(v/1024).toFixed(0)+' KB':String(v)+' B'
const shorten=(v:string,n=45)=>v.length>n?v.slice(0,n-3)+'…':v
const compactNumber=(v=0)=>v>=1e6?(v/1e6).toFixed(1)+'M':v>=1e3?(v/1e3).toFixed(1)+'K':String(v)
const updatedLabel=(value:string)=>value?new Date(value).toLocaleDateString():''
const picks=[
 {id:'bartowski/Qwen2.5-7B-Instruct-GGUF',label:'Qwen 2.5 7B',note:'범용 대화·도구 호출에 무난한 7B'},
 {id:'bartowski/Qwen2.5-Coder-7B-Instruct-GGUF',label:'Qwen 2.5 Coder 7B',note:'코딩·에이전트 작업용 7B'},
 {id:'bartowski/Mistral-Nemo-Instruct-2407-GGUF',label:'Mistral Nemo 12B',note:'긴 문맥과 다국어에 강한 12B'},
 {id:'bartowski/DeepSeek-R1-Distill-Qwen-7B-GGUF',label:'DeepSeek R1 7B',note:'추론 특화蒸馏 7B'},
 {id:'bartowski/Phi-3.5-mini-instruct-GGUF',label:'Phi 3.5 mini',note:'가볍고 빠른 3.8B'},
 {id:'bartowski/gemma-2-9b-it-GGUF',label:'Gemma 2 9B',note:'Google 범용 instruction 9B'}
] as const
const viewTabs=[{id:'installed',label:'내 모델',icon:HardDrive},{id:'search',label:'모델 찾기',icon:Search},{id:'import',label:'로컬 모델 가져오기',icon:FolderOpen}] as const
export function ModelExplorer(){
 const [view,setView]=useState<'installed'|'search'|'import'>('installed')
 const [installed,setInstalled]=useState<Installed[]>([])
 const [jobs,setJobs]=useState<Job[]>([])
 const [runtime,setRuntime]=useState<Runtime>({status:'idle'})
 const [q,setQ]=useState('')
 const [format,setFormat]=useState('gguf')
 const [sort,setSort]=useState('downloads')
 const [catalog,setCatalog]=useState<Catalog[]>([])
 const [limit,setLimit]=useState(80)
 const [selected,setSelected]=useState('')
 const [detail,setDetail]=useState<Detail|null>(null)
 const [variant,setVariant]=useState('')
 const [loading,setLoading]=useState(false)
 const [working,setWorking]=useState('')
 const [error,setError]=useState('')
 const [localPath,setLocalPath]=useState('')
 const [localFormat,setLocalFormat]=useState<'gguf'|'mlx'>('gguf')
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
  if(view!=='search')return
  let valid=true
  const timer=setTimeout(async()=>{
   setLoading(true)
   try{
    const data=await get<Catalog[]>('/api/model-library/catalog?'+new URLSearchParams({q,format,sort,limit:String(limit)}))
    if(valid){setCatalog(data);setSelected(s=>data.some(m=>m.id===s)?s:'');setError('')}
   }catch(e){if(valid)setError((e as Error).message)}
   finally{if(valid)setLoading(false)}
  },300)
  return()=>{valid=false;clearTimeout(timer)}
 },[view,q,format,sort,limit])
 useEffect(()=>{setLimit(80)},[q,format,sort])
 useEffect(()=>{
  if(view!=='search'||!selected){setDetail(null);setVariant('');return}
  let active=true
  setDetail(null);setVariant('')
  get<Detail>('/api/model-library/details?id='+encodeURIComponent(selected)).then(d=>{
   if(active){setDetail(d);setVariant(d.options.find(o=>format==='all'||o.format===format)?.id||d.options[0]?.id||'')}
  }).catch(e=>{if(active)setError((e as Error).message)})
  return()=>{active=false}
 },[selected,view,format])
 const option=detail?.options.find(o=>o.id===variant)
 const selectedJob=jobs.find(j=>j.repo===detail?.id&&j.optionId===option?.id&&['resolving','downloading','paused','failed','cancelled'].includes(j.status))
 const existing=installed.find(m=>m.repo===detail?.id&&m.optionId===option?.id)
 const activeModel=installed.find(m=>m.id===runtime.id)
 const needEngine=runtime.capabilities?.gguf.available===false;
 const installing=['checking','downloading','extracting','verifying'].includes(runtime.setup?.status||'');
 const recommendedBackend=runtime.capabilities?.memory.platform==='win32'&&runtime.capabilities.memory.arch==='x64'?'cuda':'cpu';
 const installerPercent=Math.round((runtime.setup?.downloaded||0)/Math.max(1,runtime.setup?.total||0)*100);
 const safe=async(key:string,action:()=>Promise<unknown>)=>{
  setWorking(key);setError('')
  try{await action();await refresh()}
  catch(e){setError((e as Error).message)}
  finally{setWorking('')}
 }
 const installEngine=(backend:'cuda'|'cpu')=>safe('setup',()=>post('/api/model-library/runtime/install',{backend}))
 const importLocal=()=>safe('import',async()=>{
  const result=await post<Installed>('/api/model-library/import',{filePath:localPath.trim()})
  setLocalPath('');setView('installed');return result
 })
 const chooseLocal=async()=>{
  if(!window.dominionDesktop){setError('브라우저에서는 게이트웨이가 실행 중인 컴퓨터의 모델 경로를 직접 입력해 주세요.');return}
  try{const target=await window.dominionDesktop.pickModel(localFormat);if(target){setLocalPath(target);setError('')}}catch(e){setError((e as Error).message)}
 }
 const load=(id:string)=>safe('load'+id,()=>post('/api/model-library/runtime/load',{id,context,gpu}))
 const unload=()=>safe('unload',()=>post('/api/model-library/runtime/unload',{}))
 const remove=async(m:Installed)=>{
  if(!window.confirm(m.source==='external'?'등록을 해제하시겠습니까? 원본 모델 파일은 유지됩니다.':'다운로드한 모델 파일을 삭제하시겠습니까?'))return
  await safe('remove'+m.id,()=>del('/api/model-library/installed/'+m.id))
 }
 const btn='inline-flex h-8 items-center justify-center gap-2 rounded-lg border border-[#dfe4ed] bg-white px-3 text-[11px] font-semibold text-[#3A342D] hover:bg-[#f5f7fa] disabled:opacity-40'
 return <section className="overflow-hidden rounded-xl border border-[#dfe4eb] bg-white">
  <div className="flex flex-wrap items-center justify-between gap-3 border-b border-[#e7eaf0] px-4 py-3">
   <div className="flex flex-wrap items-center gap-1">{viewTabs.map(tab=>{const Icon=tab.icon;return <button key={tab.id} type="button" onClick={()=>{setView(tab.id);setError('')}} className={'flex h-8 items-center gap-1.5 rounded-lg px-3 text-[12px] font-semibold '+(view===tab.id?'bg-[#F7E8E1] text-[#A94E30]':'text-[#758295] hover:bg-[#f3f5f9]')}><Icon size={14}/>{tab.label}{tab.id==='installed'&&<span className="ml-1 rounded bg-white/75 px-1.5 text-[10px]">{installed.length}</span>}</button>})}</div>
   <div className="flex items-center gap-2 text-[11px] text-[#697789]"><span className={'size-1.5 rounded-full '+(runtime.status==='ready'?'bg-emerald-500':runtime.status==='loading'?'bg-amber-500':'bg-slate-300')}/>{runtime.status==='ready'?'모델 실행 중':runtime.status==='loading'?'로딩 중':runtime.status==='installing'?'실행기 설치 중':'모델 대기'}<button type="button" aria-label="새로고침" onClick={()=>void refresh()} className="ml-2 text-[#9C9388] hover:text-[#4A443C]"><RefreshCw size={14}/></button></div>
  </div>
  {error&&<div role="alert" className="mx-4 mt-3 flex items-start gap-2 rounded-lg border border-rose-200 bg-rose-50 px-3 py-2 text-[12px] text-rose-700"><CircleAlert size={15}/><span className="flex-1">{error}</span><button aria-label="닫기" onClick={()=>setError('')}><X size={14}/></button></div>}
  {runtime.status==='ready'&&<div className="flex flex-wrap items-center justify-between gap-3 border-b border-emerald-100 bg-[#f2fbf7] px-4 py-3">
   <span className="flex min-w-0 items-center gap-2 text-[12px]"><Check size={16} className="text-emerald-600"/><strong className="truncate">{runtime.name||runtime.repo}</strong><span className="text-emerald-700">실행 중</span></span>
   <div className="flex gap-2"><button type="button" className={btn} onClick={()=>{window.location.hash='#/chat'}}><ArrowRight size={14}/> 채팅으로 이동</button><button type="button" className={btn} onClick={()=>void unload()} disabled={Boolean(working)}><Square size={13}/> 중지</button></div>
  </div>}
  {view==='installed'&&<div className="p-4">
   <div className="mb-3 flex flex-wrap items-center justify-between gap-2"><h3 className="text-[13px] font-bold">등록된 모델</h3><button type="button" className={btn} onClick={()=>setView('import')}><FolderOpen size={14}/> 기존 파일 가져오기</button></div>
   {installed.length?<div className="divide-y divide-[#edf0f4] rounded-lg border border-[#e7eaf0]">{installed.map(m=><div key={m.id} className="flex flex-wrap items-center gap-3 p-3">
    <div className="grid size-9 shrink-0 place-items-center rounded-lg bg-[#edf1fb] text-[#5870b7]"><Cpu size={17}/></div>
    <div className="min-w-0 flex-1"><div className="break-all text-[12px] font-semibold text-[#263247]">{m.repo}</div><div className="mt-1 text-[11px] text-[#8591a0]">{m.format.toUpperCase()} · {fmt(m.bytes)} · {m.source==='external'?'로컬 파일 연결':'다운로드됨'}{m.externalPath?' · '+shorten(m.externalPath,65):''}</div></div>
    <div className="flex items-center gap-1.5">{runtime.status==='ready'&&runtime.id===m.id?<span className="mr-1 rounded bg-emerald-50 px-2 py-1 text-[10px] font-semibold text-emerald-700">사용 중</span>:<button type="button" className={btn+' !border-[#C96442] !bg-[#C96442] !text-white hover:!bg-[#A94E30]'} disabled={Boolean(working)||runtime.status==='loading'} onClick={()=>void load(m.id)}><Play size={13}/>{working==='load'+m.id?'로딩 중':'실행'}</button>}
     <button type="button" className="rounded-md p-2 text-[#9ca5b3] hover:bg-rose-50 hover:text-rose-600" aria-label={'모델 삭제 '+m.repo} disabled={Boolean(working)} onClick={()=>void remove(m)}><Trash2 size={14}/></button>
    </div>
   </div>)}</div>:<div className="flex min-h-48 flex-col items-center justify-center gap-3 rounded-lg border border-dashed border-[#dfe4ed] bg-[#fafbfc] px-4 py-7 text-center"><HardDrive size={25} className="text-[#a1adbc]"/><strong className="text-[13px]">등록된 모델이 없습니다</strong><p className="text-[11px] text-[#8290a1]">이미 가지고 있는 GGUF·MLX 파일을 불러오거나 새로 다운로드하세요.</p><div className="flex gap-2"><button type="button" className={btn} onClick={()=>setView('import')}>로컬 파일 등록</button><button type="button" className={btn} onClick={()=>setView('search')}>모델 다운로드</button></div></div>}
  </div>}
  {view==='import'&&<div className="max-w-3xl p-4">
   <h3 className="text-[13px] font-bold">다운로드하지 않고 기존 모델 사용</h3>
   <p className="mt-1 text-[11px] text-[#7b8898]">파일을 복사하지 않고 원본 경로를 등록합니다. GGUF 파일 또는 MLX 모델 폴더를 선택하세요.</p>
   <div className="mt-4 flex flex-wrap items-center gap-2"><button type="button" onClick={()=>{setLocalFormat('gguf');setLocalPath('')}} className={btn+(localFormat==='gguf'?' !border-[#8da2e7] !bg-[#edf1ff] !text-[#3857bb]':'')}>GGUF</button><button type="button" onClick={()=>{setLocalFormat('mlx');setLocalPath('')}} className={btn+(localFormat==='mlx'?' !border-[#8da2e7] !bg-[#edf1ff] !text-[#3857bb]':'')}>MLX</button></div>
   <label className="mt-4 block text-[11px] font-medium text-[#526074]">게이트웨이가 실행 중인 컴퓨터의 {localFormat==='gguf'?'GGUF 파일':'MLX 모델 폴더'} 경로</label>
   <div className="mt-1.5 flex flex-col gap-2 sm:flex-row"><TextInput className="!flex-1" value={localPath} onChange={e=>setLocalPath(e.target.value)} placeholder={localFormat==='gguf'?'예: C:\\Models\\model.gguf 또는 /models/model.gguf':'예: /Users/me/models/Model-MLX'}/><button type="button" className={btn} onClick={()=>void chooseLocal()}><FolderOpen size={14}/> 파일 선택</button></div>
   <p className="mt-2 text-[11px] text-[#8b97a7]">데스크톱 앱에서는 파일 선택창이 열립니다. 브라우저 접속은 게이트웨이 컴퓨터의 경로를 입력하세요.</p>
   <button type="button" disabled={!localPath.trim()||Boolean(working)} onClick={()=>void importLocal()} className="mt-5 flex h-9 items-center gap-2 rounded-lg bg-[#C96442] px-4 text-[12px] font-semibold text-white hover:bg-[#A94E30] disabled:opacity-40">{working==='import'?<LoaderCircle className="animate-spin" size={15}/>:<Check size={15}/>}모델 등록</button>
  </div>}
  {view==='search'&&<div className="grid min-h-[560px] grid-cols-1 lg:grid-cols-[400px_minmax(0,1fr)]">
   <div className="min-w-0 border-b border-[#E7E0D5] lg:border-b-0 lg:border-r">
    <div className="border-b border-[#EFE9DF] p-4">
     <div className="flex h-11 items-center gap-2 rounded-xl border border-[#E7E0D5] bg-[#FAF7F1] px-3 focus-within:border-[#C96442]"><Search size={17} className="text-[#A79C8E]"/><input value={q} onChange={e=>setQ(e.target.value)} placeholder="Hugging Face 모델 검색 (gemma, qwen, uncensored…)" className="h-full w-full min-w-0 bg-transparent text-[14px] outline-none placeholder:text-[#A79C8E]"/>{q&&<button type="button" aria-label="검색어 지우기" onClick={()=>setQ('')} className="rounded p-1 text-[#A79C8E] hover:bg-[#F3EEE5]"><X size={14}/></button>}</div>
     <div className="mt-3 flex flex-wrap items-center gap-1.5">{[{id:'all',label:'전체'},{id:'gguf',label:'GGUF'},{id:'mlx',label:'MLX'}].map(item=><Button key={item.id} variant={format===item.id?'primary':'ghost'} onClick={()=>setFormat(item.id)} className="!px-3 !py-1.5 !text-[12px]">{item.label}</Button>)}<SelectInput aria-label="정렬" value={sort} onChange={e=>setSort(e.target.value)} className="!ml-auto !w-auto !py-1.5 !text-[12px]"><option value="downloads">다운로드순</option><option value="likes">인기순</option><option value="lastModified">최신순</option></SelectInput></div>
     <div className="mt-2 flex items-center justify-between text-[11px] text-[#8A8176]"><span>{loading?'검색 중…':catalog.length+'개 결과'}</span><span>Hugging Face 실시간</span></div>
    </div>
    <div className="max-h-[610px] overflow-y-auto">
     {!q&&<div className="border-b border-[#EFE9DF] p-3"><div className="mb-2 px-1 text-[11px] font-semibold text-[#8A8176]">에디터 추천</div><div className="grid gap-1.5">{picks.map(pick=><Button key={pick.id} variant="ghost" onClick={()=>setSelected(pick.id)} className="!h-auto w-full !justify-start gap-2.5 !border-0 rounded-xl px-2.5 py-2 text-left hover:bg-[#FAF7F1]"><span className="grid size-8 shrink-0 place-items-center rounded-lg bg-[#F7E8E1] text-[12px] font-bold text-[#C96442]">{pick.label.slice(0,1)}</span><span className="min-w-0 flex-1"><strong className="block truncate text-[12px] text-[#3A342D]">{pick.label}</strong><small className="block truncate text-[10px] text-[#8A8176]">{pick.note}</small></span><ArrowRight size={13} className="shrink-0 text-[#A79C8E]"/></Button>)}</div></div>}
     {loading&&<div className="space-y-2 p-3">{[0,1,2,3].map(i=><div key={i} className="h-20 animate-pulse rounded-xl bg-[#FAF7F1]"/>)}</div>}
     {!loading&&catalog.map(m=><button type="button" key={m.id} onClick={()=>setSelected(m.id)} className={'w-full border-b border-[#EFE9DF] px-4 py-3.5 text-left transition-colors hover:bg-[#FAF7F1] '+(selected===m.id?'bg-[#F7E8E1]':'')}><div className="flex items-start gap-3"><span className="grid size-9 shrink-0 place-items-center rounded-xl bg-[#F3EEE5] text-[13px] font-bold text-[#C96442]">{m.name.slice(0,1).toUpperCase()}</span><div className="min-w-0 flex-1"><div className="flex items-center gap-2"><strong className="truncate text-[13px] text-[#24211D]">{m.name}</strong>{m.gated&&<span className="rounded bg-amber-50 px-1.5 py-0.5 text-[9px] font-semibold text-amber-700">gated</span>}</div><div className="mt-0.5 truncate text-[11px] text-[#8A8176]">{m.author} · {m.task||m.library||'model'}</div><p className="mt-1 line-clamp-2 text-[11px] leading-5 text-[#5C544A]">{m.description||'설명을 불러오는 중입니다.'}</p><div className="mt-1.5 flex flex-wrap items-center gap-2 text-[10px] text-[#8A8176]"><span>↓ {compactNumber(m.downloads)}</span><span>♡ {compactNumber(m.likes)}</span><span>{updatedLabel(m.updatedAt)}</span><span className="rounded bg-[#F3EEE5] px-1.5 py-0.5 font-semibold">{/gguf/i.test(m.tags.join(' '))?'GGUF':'MLX'}</span></div></div></div></button>)}
     {!loading&&!catalog.length&&<div className="p-8 text-center text-[12px] text-[#7C7469]">검색 결과가 없습니다.</div>}
     {!loading&&catalog.length>0&&catalog.length>=limit&&<div className="p-3"><Button variant="secondary" className="w-full" onClick={()=>setLimit(limit+80)}>더 보기 · {catalog.length}개 표시 중</Button></div>}
    </div>
   </div>
   <div className="min-w-0 p-4">
    {!selected?<div className="flex min-h-52 flex-col items-center justify-center gap-3 text-[12px] text-[#8d99a8]"><CloudDownload size={23}/>왼쪽에서 모델을 선택하세요.</div>:!detail?<div className="flex h-44 items-center justify-center text-[12px] text-[#8290a1]"><LoaderCircle size={17} className="mr-2 animate-spin"/>모델 상세 정보 불러오는 중</div>:<>
     <div className="flex items-start justify-between gap-3"><div className="min-w-0"><h3 className="break-all text-[14px] font-bold text-[#273448]">{detail.id}</h3><div className="mt-1 text-[11px] text-[#8b97a6]">{detail.architecture||'언어 모델'}{detail.license?' · '+detail.license:''} · {detail.options.length}개 배포 형식</div></div><a href={'https://huggingface.co/'+detail.id} target="_blank" rel="noreferrer" aria-label="Hugging Face에서 보기" className="rounded-md p-2 text-[#7C7469] hover:bg-[#F3EEE5]"><ExternalLink size={16}/></a></div>
     <p className="mt-3 line-clamp-3 text-[11px] text-[#708094]">{detail.description||detail.readme.replace(/^---[\s\S]*?---/,'').replace(/[#*\[\]>]/g,'').slice(0,250)||'모델 파일 크기와 형식을 확인한 후 다운로드하세요.'}</p>
     <div className="mt-5 text-[12px] font-semibold">다운로드할 파일 선택</div>
     <div className="mt-2 max-h-52 overflow-auto rounded-lg border border-[#e8ecf2]">{detail.options.filter(o=>format==='all'||o.format===format).map(o=><button type="button" key={o.id} onClick={()=>setVariant(o.id)} className={'flex w-full items-center gap-3 border-b border-[#edf0f4] px-3 py-3 text-left last:border-0 '+(variant===o.id?'bg-[#F7E8E1]':'hover:bg-[#fafbfc]')}><span className="rounded bg-[#F3EEE5] px-1.5 py-1 text-[10px] font-semibold">{o.format.toUpperCase()}</span><span className="min-w-0 flex-1 text-[12px] font-semibold">{o.label}<span className="ml-2 text-[10px] font-normal text-[#8a98a6]">{o.shards>1?o.shards+'개 파일':''}</span></span><span className="text-[11px] text-[#748196]">{fmt(o.bytes)}</span>{variant===o.id&&<Check size={14} className="text-[#C96442]"/>}</button>)}</div>
     {!detail.options.length&&<p className="mt-3 text-[12px] text-amber-700">이 저장소에는 지원되는 GGUF·MLX 모델 파일이 없습니다.</p>}
     {option&&<div className="mt-4 flex flex-wrap items-center gap-3 rounded-lg bg-[#f8f9fb] p-3">
      <span className="text-[11px] text-[#68778a]">다운로드 용량 <strong>{fmt(option.bytes)}</strong></span>
      {existing?<button type="button" className={btn} onClick={()=>{setView('installed')}}><Check size={14}/> 설치됨 · 내 모델 보기</button>:
       <button type="button" className="flex h-8 items-center gap-2 rounded-lg bg-[#C96442] px-3 text-[11px] font-semibold text-white disabled:opacity-40" disabled={Boolean(working)||['resolving','downloading'].includes(selectedJob?.status||'')||detail.gated} onClick={()=>void safe('download',()=>post('/api/model-library/download',{repo:detail.id,optionId:option.id}))}><ArrowDownToLine size={14}/>{working==='download'?'요청 중':selectedJob?.status==='downloading'?'다운로드 중':'다운로드 시작'}</button>}
      {detail.gated&&<span className="text-[11px] text-amber-700">접근 승인 필요</span>}
     </div>}
     {selectedJob&&<div className="mt-3 rounded-lg border border-[#e8ebf1] p-3 text-[11px]"><div className="flex justify-between gap-3"><span>{selectedJob.status} {selectedJob.current&&'· '+shorten(selectedJob.current,30)}</span><span>{fmt(selectedJob.downloaded)} / {fmt(selectedJob.total)}</span></div><div className="mt-2 h-1.5 overflow-hidden rounded-full bg-[#F3EEE5]"><div className="h-full bg-[#C96442]" style={{width:Math.min(100,selectedJob.downloaded/Math.max(1,selectedJob.total)*100)+'%'}}/></div>{selectedJob.error&&<p className="mt-2 text-rose-600">{selectedJob.error}</p>}<div className="mt-2 flex gap-2">{selectedJob.status==='downloading'&&<button type="button" className={btn} onClick={()=>void safe('pause',()=>post('/api/model-library/jobs/'+selectedJob.id+'/pause',{}))}><Pause size={13}/> 일시정지</button>}{['paused','failed','cancelled'].includes(selectedJob.status)&&<button type="button" className={btn} onClick={()=>void safe('resume',()=>post('/api/model-library/jobs/'+selectedJob.id+'/resume',{}))}><Play size={13}/> 재개</button>}{['resolving','downloading','paused'].includes(selectedJob.status)&&<button type="button" className={btn} onClick={()=>void safe('cancel',()=>post('/api/model-library/jobs/'+selectedJob.id+'/cancel',{}))}><Square size={13}/> 취소</button>}</div></div>}
    </>}
   </div>
  </div>}
  {(installing||runtime.setup?.status==='failed'||(runtime.status==='installing'))&&<div role="status" className="border-t border-[#e4e9f1] bg-[#f3f6fc] px-4 py-3 text-[11px] text-[#53637d]">
    <div className="flex flex-wrap items-center justify-between gap-2"><strong>{runtime.setup?.status==='failed'?'실행기 설치 실패':runtime.setup?.status==='verifying'?'실행기 검증 중':runtime.setup?.status==='extracting'?'압축 해제 중':'공식 llama.cpp 실행기 설치 중'}</strong><span>{runtime.setup?.tag||''} {runtime.setup?.total?installerPercent+'%':''}</span></div>
    {installing&&<div className="mt-2 h-1.5 overflow-hidden rounded-full bg-[#dce4f2]"><div className="h-full bg-[#C96442]" style={{width:installerPercent+'%'}}/></div>}
    {runtime.setup?.error&&<p className="mt-2 text-rose-700">{runtime.setup.error}</p>}
    {runtime.setup?.file&&<p className="mt-1 truncate text-[#8d9bad]">{runtime.setup.file}</p>}
   </div>}
  <div className="flex flex-wrap items-center justify-between gap-3 border-t border-[#e8ecf1] bg-[#fafbfc] px-4 py-3">
   <span className="flex flex-wrap gap-3 text-[11px] text-[#788598]"><span>GGUF: {runtime.capabilities?.gguf.available?'실행 가능'+(runtime.capabilities.gguf.backend?' · '+runtime.capabilities.gguf.backend.toUpperCase():''):'실행기 미설치'}</span>
    {needEngine&&runtime.capabilities?.gguf.installable&&<span className="inline-flex items-center gap-2">
     <button type="button" className="inline-flex h-7 items-center gap-1.5 rounded-md bg-[#C96442] px-2.5 text-[11px] font-semibold text-white disabled:opacity-50" disabled={installing||Boolean(working)} onClick={()=>void installEngine(recommendedBackend)}><ArrowDownToLine size={13}/>{installing?'설치 중':recommendedBackend==='cuda'?'NVIDIA CUDA 실행기 설치':'CPU 실행기 설치'}</button>
     {recommendedBackend==='cuda'&&<button type="button" className="inline-flex h-7 items-center rounded-md border border-[#dce3ed] px-2 text-[11px] text-[#65768a] disabled:opacity-40" disabled={installing||Boolean(working)} onClick={()=>void installEngine('cpu')}>CPU 버전</button>}
    </span>}
    {runtime.capabilities?.gguf.available&&recommendedBackend==='cuda'&&runtime.capabilities.gguf.backend==='cpu'&&<button type="button" disabled={installing||Boolean(working)} onClick={()=>void installEngine('cuda')} className="inline-flex h-7 items-center rounded-md border border-[#dce3ed] px-2 text-[11px] text-[#556bb4] disabled:opacity-40">CUDA 실행기로 전환</button>}
    <span>MLX: {runtime.capabilities?.mlx.available?'실행기 사용 가능':'Apple Silicon + mlx-lm 필요'}</span></span>
   <div className="flex items-center gap-3"><label className="flex items-center gap-1.5 text-[11px] text-[#6d7b8d]">컨텍스트 <SelectInput value={context} onChange={e=>setContext(Number(e.target.value))} className="!w-20 !py-1"><option value={2048}>2K</option><option value={4096}>4K</option><option value={8192}>8K</option><option value={16384}>16K</option><option value={32768}>32K</option></SelectInput></label><label className="flex items-center gap-1.5 text-[11px] text-[#6d7b8d]"><input type="checkbox" checked={gpu} onChange={e=>setGpu(e.target.checked)}/> GPU</label></div>
  </div>
  {runtime.status==='error'&&runtime.error&&<div className="border-t border-rose-100 bg-rose-50 px-4 py-2 text-[11px] text-rose-700">{runtime.error}</div>}
 </section>
}
