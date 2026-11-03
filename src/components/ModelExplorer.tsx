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
 return <section className="ml-root">
  <div className="ml-heading"><div><span className="ml-eyebrow"><Sparkles size={14}/> LOCAL MODEL LIBRARY</span><h2>모델을 찾고, 설치하고, 실행하세요.</h2><p>공개 모델 저장소에서 검색하고 GGUF·MLX 배포 파일을 직접 관리합니다.</p></div><div className="ml-runtime"><span className={'ml-status-dot '+(runtime.status==='ready'?'green':'')}/><div><strong>{status}</strong><small>{runtime.repo||'DOMINION 로컬 실행 환경'}</small></div></div></div>
  <div className="ml-tabs"><Button className={tab==='explore'?'active':''} onClick={()=>setTab('explore')}><Search size={16}/> 모델 탐색</Button><Button className={tab==='installed'?'active':''} onClick={()=>setTab('installed')}><HardDrive size={16}/> 설치된 모델 <span>{installed.length}</span></Button><Button className="ml-refresh" onClick={()=>void refresh()} title="새로고침"><RefreshCw size={16}/></Button></div>
  {error&&<div className="ml-alert"><span>{error}</span><Button onClick={()=>setError('')} title="닫기"><X size={16}/></Button></div>}
  {tab==='explore'?<div className="ml-browser">
   <aside className="ml-list">
    <div className="ml-search"><Search size={18}/><TextInput aria-label="모델 검색" placeholder="모델명 또는 제작사 검색..." value={q} onChange={e=>setQ(e.target.value)}/>{busy&&<LoaderCircle size={17} className="ml-spin"/>}{q&&<Button title="검색어 지우기" onClick={()=>setQ('')}><X size={15}/></Button>}</div>
    <div className="ml-filters"><div className="ml-format">{['all','gguf','mlx'].map(x=><Button key={x} className={format===x?'on':''} onClick={()=>setFormat(x)}>{x==='all'?'전체':x.toUpperCase()}</Button>)}</div><label><SlidersHorizontal size={14}/><SelectInput aria-label="모델 정렬" value={sort} onChange={e=>setSort(e.target.value)}><option value="downloads">다운로드순</option><option value="likes">인기순</option><option value="lastModified">최신순</option></SelectInput><ChevronDown size={13}/></label></div>
    <div className="ml-list-caption">{q?'검색 결과':'Hugging Face 인기 모델'} <span>{items.length}개</span></div>
    <div className="ml-scroll-list">
     {items.map(m=><Button key={m.id} className={'ml-model '+(selected===m.id?'selected':'')} onClick={()=>setSelected(m.id)}><span className="ml-model-symbol"><Cpu size={21}/></span><span className="ml-model-content"><strong>{m.id}</strong><small>{m.task||m.tags.find(t=>t==='gguf'||t==='mlx')||'모델'} · ↓ {compact(m.downloads)} · ♥ {compact(m.likes)}</small></span>{m.gated&&<span className="ml-gated">승인</span>}</Button>)}
     {!items.length&&!busy&&<div className="ml-empty">모델을 찾지 못했습니다. 다른 이름이나 필터로 검색해 보세요.</div>}
    </div>
    <div className="ml-list-footer"><CloudDownload size={15}/> 검색·파일 정보 출처: Hugging Face</div>
   </aside>
   <div className="ml-detail">
    {loading?<div className="ml-detail-placeholder"><LoaderCircle size={26} className="ml-spin"/> 모델 정보 불러오는 중</div>:detail?<><div className="ml-detail-title"><div><small>MODEL REPOSITORY</small><h3>{detail.id}</h3></div><a href={'https://huggingface.co/'+detail.id} target="_blank" rel="noreferrer" aria-label="Hugging Face 원본"><ExternalLink size={18}/></a></div>
    <div className="ml-metrics"><span><Download size={15}/> {compact(detail.downloads)}</span><span>♡ {compact(detail.likes)}</span>{detail.updatedAt&&<span>업데이트 {new Date(detail.updatedAt).toLocaleDateString('ko-KR')}</span>}</div>
    <div className="ml-summary"><p>{detail.description||detail.readme.replace(/^---[\s\S]*?---/,'').replace(/[#*\[\]>]/g,'').trim().slice(0,290)||'모델 저장소의 파일과 형식을 아래에서 확인하세요.'}</p><div className="ml-meta">{detail.architecture&&<span>ARCH <b>{String(detail.architecture).slice(0,55)}</b></span>}{detail.license&&<span>LICENSE <b>{detail.license}</b></span>}<span>OPTIONS <b>{detail.options.length}</b></span></div><div className="ml-capabilities">{detail.capabilities.vision&&<span><Eye size={14}/> Vision</span>}{detail.capabilities.tools&&<span><Wrench size={14}/> Tool Use</span>}{detail.capabilities.reasoning&&<span><BrainCircuit size={14}/> Reasoning</span>}{!detail.capabilities.vision&&!detail.capabilities.tools&&!detail.capabilities.reasoning&&<span><Code2 size={14}/> {detail.task||'Text model'}</span>}</div></div>
    <h4 className="ml-subtitle">다운로드 옵션 <small>원본 저장소 배포 파일</small></h4>
    <div className="ml-option-list">{detail.options.filter(o=>format==='all'||o.format===format).map(o=><Button key={o.id} className={'ml-option '+(variant===o.id?'chosen':'')} onClick={()=>setVariant(o.id)}><span className={'ml-option-format '+o.format}>{o.format.toUpperCase()}</span><span className="ml-option-name"><strong>{o.label}</strong><small>{o.quantization}{o.shards>1?' · '+o.shards+'개 파일':''}</small></span><span className="ml-option-size">{fmt(o.bytes)}</span>{variant===o.id&&<Check size={17}/>}</Button>)}</div>
    {!detail.options.length&&<div className="ml-empty">지원되는 GGUF 또는 MLX 다운로드 파일이 없습니다.</div>}
    {selectedOption&&<div className="ml-install-panel"><div className="ml-estimate"><span><HardDrive size={15}/> 디스크 {fmt(selectedOption.bytes)}</span><span><Cpu size={15}/> 모델 파일 외에 KV 캐시·실행 메모리 필요</span>{gpu&&gpuInfo&&<span className={'ml-memory-fit '+(mayFit?'yes':'caution')}><Cpu size={15}/>{gpuInfo.kind==='unified'?'통합 메모리':'GPU VRAM'} {fmt(gpuInfo.free)} 여유 · 예상 {fmt(estimatedMemory)} · {mayFit?'적재 가능성 높음':'전체 적재 어려울 수 있음'} (추정)</span>}{gpu&&!gpuInfo&&<span>GPU 메모리 정보가 없어 실제 로딩 후 확인이 필요합니다.</span>}</div>
     {selectedJob&&<><div className="ml-progress-top"><strong>{selectedJob.status==='downloading'?'다운로드 중':selectedJob.status==='paused'?'일시정지':selectedJob.status==='failed'?'오류':selectedJob.status==='resolving'?'준비 중':'중단됨'}</strong><span>{fmt(selectedJob.downloaded)} / {fmt(selectedJob.total||selectedOption.bytes)}</span></div><div className="ml-progress"><span style={{width:Math.min(100,100*selectedJob.downloaded/Math.max(1,selectedJob.total||selectedOption.bytes))+'%'}}/></div><small className="ml-current-file">{selectedJob.error||selectedJob.current||'파일 준비 중'}</small><div className="ml-job-actions">{selectedJob.status==='downloading'&&<Button onClick={()=>void control(selectedJob.id,'pause')}><Pause size={14}/> 일시정지</Button>}{['paused','failed','cancelled'].includes(selectedJob.status)&&<Button onClick={()=>void control(selectedJob.id,'resume')}><Play size={14}/> 재개</Button>}{['downloading','paused','resolving'].includes(selectedJob.status)&&<Button onClick={()=>void control(selectedJob.id,'cancel')}><Square size={14}/> 중단</Button>}</div></>}
     {selectedInstalled?<div className="ml-installed-action"><span><Check size={17}/> 설치됨</span><Button className="ml-primary" disabled={Boolean(running)||runtime.status==='loading'} onClick={()=>void load(selectedInstalled.id)}><Play size={17}/> {runtime.id===selectedInstalled.id&&runtime.status==='ready'?'다시 실행':'모델 로드'}</Button></div>:<Button className="ml-primary ml-download-button" disabled={!selectedOption||Boolean(running)||selectedJob?.status==='downloading'||selectedJob?.status==='resolving'} onClick={()=>void install()}><ArrowDownToLine size={18}/>{running==='download'?'다운로드 준비 중':selectedJob?.status==='paused'?'다운로드 재개 버튼을 이용하세요':'다운로드'} <span>{fmt(selectedOption.bytes)}</span></Button>}
    </div>}
    <h4 className="ml-subtitle"><BookOpen size={16}/> README</h4><pre className="ml-readme">{detail.readme.replace(/^---[\s\S]*?---/,'').trim().slice(0,3500)||'저장소에 README가 없거나 읽을 수 없습니다.'}</pre></>:<div className="ml-detail-placeholder"><Search size={25}/> 모델을 선택하세요.</div>}
   </div>
  </div>:<div className="ml-installed-list">{installed.length?installed.map(m=><div className="ml-installed-card" key={m.id}><span className="ml-installed-icon"><HardDrive size={22}/></span><div className="ml-installed-info"><strong>{m.repo}</strong><span>{m.label} · {m.format.toUpperCase()} · {fmt(m.bytes)}</span><small>설치 {new Date(m.installedAt).toLocaleString('ko-KR')}</small></div><div className="ml-installed-buttons">{runtime.id===m.id&&runtime.status==='ready'?<Button className="ml-stop" disabled={Boolean(running)} onClick={()=>void unload()}><Square size={15}/> 언로드</Button>:<Button className="ml-primary" disabled={Boolean(running)||runtime.status==='loading'} onClick={()=>void load(m.id)}><Play size={15}/> 실행</Button>}<Button className="ml-icon-button" disabled={Boolean(running)} onClick={()=>void remove(m.id)} aria-label="모델 삭제"><Trash2 size={16}/></Button></div></div>):<div className="ml-empty ml-empty-installed"><HardDrive size={27}/> 설치된 모델이 없습니다. 모델 탐색에서 다운로드할 수 있습니다.</div>}</div>}
  <div className="ml-hardware"><div><Server size={18}/><strong>로컬 실행 환경</strong><span>GGUF · {runtime.capabilities?.gguf.available?'llama-server 사용 가능':'llama-server 필요'}</span><span>MLX · {runtime.capabilities?.mlx.available?'mlx-lm 사용 가능':'Apple Silicon + mlx-lm 필요'}</span>{runtime.capabilities?.memory&&<span>RAM {fmt(runtime.capabilities.memory.total)} · 여유 {fmt(runtime.capabilities.memory.free)}</span>}</div><div className="ml-hardware-controls"><label>컨텍스트<SelectInput value={context} onChange={e=>setContext(Number(e.target.value))}><option value={2048}>2K</option><option value={4096}>4K</option><option value={8192}>8K</option><option value={16384}>16K</option><option value={32768}>32K</option></SelectInput></label><label className="ml-checkbox"><TextInput type="checkbox" checked={gpu} onChange={e=>setGpu(e.target.checked)}/> GGUF GPU 자동 배치</label>{runtime.status==='loading'&&<span className="ml-load-state"><LoaderCircle size={14} className="ml-spin"/> 모델 로딩 중</span>}{runtime.status==='ready'&&<Button className="ml-stop" onClick={()=>void unload()}><Square size={14}/> 실행 중지</Button>}</div></div>
  {runtime.status==='error'&&runtime.error&&<div className="ml-alert">{runtime.error}</div>}
 </section>
}
