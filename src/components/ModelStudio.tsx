import {useCallback,useEffect,useMemo,useState} from 'react'
import {Activity,ArrowDownToLine,ArrowRight,BookOpenText,BrainCircuit,Check,ChevronDown,CircleAlert,CloudDownload,Code2,Cpu,Download,ExternalLink,Eye,HardDrive,Info,Layers3,LoaderCircle,Pause,Play,RefreshCw,Search,SlidersHorizontal,Square,Trash2,Wrench,X} from 'lucide-react'
import {del,get,post} from '../api'

type Model={id:string;downloads:number;likes:number;updatedAt:string;task:string;tags:string[];gated:boolean;description:string;capabilities:{vision:boolean;tools:boolean;reasoning:boolean}}
type Option={id:string;format:'gguf'|'mlx';label:string;quantization:string;bytes:number;files:string[];shards:number}
type Detail=Model&{revision:string;architecture:string;license:string;readme:string;options:Option[]}
type Installed={id:string;repo:string;revision:string;optionId:string;format:'gguf'|'mlx';label:string;files:string[];bytes:number;installedAt:string}
type Job={id:string;repo:string;optionId:string;status:string;downloaded:number;total:number;current:string;error?:string}
type Capability={available:boolean;command:string}
type Runtime={status:string;id?:string;repo?:string;name?:string;engine?:string;error?:string;capabilities?:{gguf:Capability;mlx:Capability;memory:{total:number;free:number;platform:string;arch:string};gpu?:{kind:string;total:number;free:number;source:string}|null}}
const size=(n=0)=>n>=1e9?(n/1e9).toFixed(2)+' GB':n>=1e6?(n/1e6).toFixed(0)+' MB':n>=1e3?(n/1e3).toFixed(0)+' KB':n+' B'
const short=(n=0)=>n>=1e6?(n/1e6).toFixed(1)+'M':n>=1e3?(n/1e3).toFixed(0)+'K':String(n)
const date=(s?:string)=>s?new Date(s).toLocaleDateString('ko-KR'):'—'
const activeJob=(s:string)=>s==='resolving'||s==='downloading'
const formatName=(f:string)=>f==='gguf'?'GGUF':'MLX'
const modelTitle=(repo:string)=>repo.split('/').at(-1)||repo
const infoText=(detail:Detail)=>detail.description||detail.readme.replace(/^---[\s\S]*?---/,'').replace(/[#*\[\]>]/g,'').trim().slice(0,460)||'모델의 배포 파일과 메타데이터를 확인해 주세요.'

export function ModelStudio({onOpenChat}:{onOpenChat:()=>void}){
 const [view,setView]=useState<'discover'|'installed'|'downloads'>('discover')
 const [query,setQuery]=useState('')
 const [format,setFormat]=useState<'all'|'gguf'|'mlx'>('all')
 const [sort,setSort]=useState<'downloads'|'likes'|'lastModified'>('downloads')
 const [models,setModels]=useState<Model[]>([])
 const [selected,setSelected]=useState('')
 const [detail,setDetail]=useState<Detail|null>(null)
 const [variant,setVariant]=useState('')
 const [readmeOpen,setReadmeOpen]=useState(false)
 const [searching,setSearching]=useState(false)
 const [detailLoading,setDetailLoading]=useState(false)
 const [error,setError]=useState('')
 const [installed,setInstalled]=useState<Installed[]>([])
 const [jobs,setJobs]=useState<Job[]>([])
 const [runtime,setRuntime]=useState<Runtime>({status:'idle'})
 const [working,setWorking]=useState('')
 const [gpu,setGpu]=useState(true)
 const [context,setContext]=useState(4096)
 const [manual,setManual]=useState(0)
 const refresh=useCallback(async()=>{
  const out=await Promise.allSettled([get<Installed[]>('/api/model-library/installed'),get<Job[]>('/api/model-library/jobs'),get<Runtime>('/api/model-library/runtime')])
  if(out[0].status==='fulfilled')setInstalled(out[0].value)
  if(out[1].status==='fulfilled')setJobs(out[1].value)
  if(out[2].status==='fulfilled')setRuntime(out[2].value)
 },[])
 useEffect(()=>{void refresh();const timer=setInterval(()=>void refresh(),1800);return()=>clearInterval(timer)},[refresh])
 useEffect(()=>{
  let alive=true
  const timer=setTimeout(async()=>{
   setSearching(true)
   try{
    const params=new URLSearchParams({q:query,format,sort})
    const data=await get<Model[]>('/api/model-library/catalog?'+params.toString())
    if(!alive)return
    setModels(data)
    setSelected(s=>data.some(x=>x.id===s)?s:data[0]?.id||'')
    setError('')
   }catch(e){if(alive){setModels([]);setSelected('');setError((e as Error).message)}}
   finally{if(alive)setSearching(false)}
  },query?320:40)
  return()=>{alive=false;clearTimeout(timer)}
 },[query,format,sort,manual])
 useEffect(()=>{
  let alive=true
  setDetail(null);setVariant('');setReadmeOpen(false)
  if(!selected){setDetailLoading(false);return}
  setDetailLoading(true)
  get<Detail>('/api/model-library/details?id='+encodeURIComponent(selected)).then(data=>{
   if(!alive)return
   setDetail(data)
   setVariant(data.options.find(x=>format==='all'||x.format===format)?.id||data.options[0]?.id||'')
  }).catch(e=>{if(alive)setError((e as Error).message)})
  .finally(()=>{if(alive)setDetailLoading(false)})
  return()=>{alive=false}
 },[selected])
 const options=useMemo(()=>detail?.options.filter(x=>format==='all'||x.format===format)||[],[detail,format])
 const choice=options.find(x=>x.id===variant)||options[0]
 const owned=detail&&choice?installed.find(x=>x.repo===detail.id&&x.optionId===choice.id&&x.revision===detail.revision):undefined
 const job=detail&&choice?jobs.find(x=>x.repo===detail.id&&x.optionId===choice.id&&['resolving','downloading','paused','failed','cancelled'].includes(x.status)):undefined
 const canRun=(m:Installed)=>Boolean(runtime.capabilities?.[m.format]?.available)
 const trackedJobs=jobs.filter(j=>j.status!=='completed')
 const gpuInfo=runtime.capabilities?.gpu
 const estimated=choice?choice.bytes*1.12+650_000_000+(context/4096)*1_100_000_000:0
 const mayFit=Boolean(gpuInfo&&gpuInfo.free>=estimated)
 const doAction=async(name:string,fn:()=>Promise<unknown>)=>{
  setWorking(name);setError('')
  try{await fn();await refresh()}
  catch(e){setError((e as Error).message)}
  finally{setWorking('')}
 }
 const download=()=>detail&&choice?void doAction('download',()=>post('/api/model-library/download',{repo:detail.id,optionId:choice.id})):undefined
 const manage=(id:string,action:'pause'|'resume'|'cancel')=>void doAction(action+id,()=>post('/api/model-library/jobs/'+id+'/'+action,{}))
 const load=(id:string)=>void doAction('load'+id,()=>post('/api/model-library/runtime/load',{id,gpu,context}))
 const stop=()=>void doAction('stop',()=>post('/api/model-library/runtime/unload',{}))
 const remove=(id:string)=>{
  if(!window.confirm('이 모델의 다운로드 파일을 로컬 디스크에서 삭제하시겠습니까?'))return
  void doAction('remove'+id,()=>del('/api/model-library/installed/'+id))
 }
 const loading=runtime.status==='loading'
 const ready=runtime.status==='ready'
 return <section className="ms-studio" aria-label="로컬 모델 스튜디오">
  <header className="ms-hero">
   <div className="ms-hero-main"><div className="ms-overline"><Layers3 size={15}/> MODEL STUDIO <span>·</span> LOCAL FIRST</div><h2>모델 라이브러리</h2><p>원하는 모델을 찾고, 다운로드하고, 이곳에서 바로 실행하세요.</p></div>
   <div className="ms-engine"><span className={'ms-engine-bullet '+(ready?'ready':loading?'loading':'')}/><div><strong>{ready?'모델 실행 중':loading?'모델 불러오는 중':runtime.status==='error'?'실행 오류':'실행 대기'}</strong><small>{runtime.repo?modelTitle(runtime.repo):'DOMINION Runtime'}</small></div>{ready?<button onClick={onOpenChat}><ArrowRight size={16}/></button>:<Cpu size={19}/>}</div>
  </header>
  <div className="ms-toolbar"><div className="ms-tabs" role="tablist" aria-label="모델 관리 화면">
   <button role="tab" aria-selected={view==='discover'} className={view==='discover'?'active':''} onClick={()=>setView('discover')}><Search size={17}/> 모델 탐색</button>
   <button role="tab" aria-selected={view==='installed'} className={view==='installed'?'active':''} onClick={()=>setView('installed')}><HardDrive size={17}/> 설치된 모델 <b>{installed.length}</b></button>
   <button role="tab" aria-selected={view==='downloads'} className={view==='downloads'?'active':''} onClick={()=>setView('downloads')}><Activity size={17}/> 다운로드 <b>{trackedJobs.length}</b></button>
  </div>
  <button className="ms-refresh" onClick={()=>{setManual(x=>x+1);void refresh()}} aria-label="목록 새로고침" title="새로고침"><RefreshCw size={17}/></button></div>
  {error&&<div className="ms-error" role="alert"><CircleAlert size={17}/><span>{error}</span><button aria-label="오류 닫기" onClick={()=>setError('')}><X size={16}/></button></div>}
  {view==='discover'&&<div className="ms-discover">
   <div className="ms-left"><div className="ms-search-wrap">
    <div className="ms-search-box"><Search size={19}/><input value={query} onChange={e=>setQuery(e.target.value)} aria-label="모델 검색" placeholder="모델명, 제작사 검색"/>{query&&<button onClick={()=>setQuery('')} title="검색어 지우기"><X size={15}/></button>}</div>
    <div className="ms-filters">
     <div className="ms-format" aria-label="파일 형식 선택">{(['all','gguf','mlx'] as const).map(f=><button key={f} className={format===f?'active':''} aria-pressed={format===f} onClick={()=>setFormat(f)}>{f==='all'?'전체':f.toUpperCase()}</button>)}</div>
     <label className="ms-sort"><SlidersHorizontal size={14}/><select aria-label="정렬" value={sort} onChange={e=>setSort(e.target.value as typeof sort)}><option value="downloads">다운로드순</option><option value="likes">추천순</option><option value="lastModified">최근 업데이트</option></select><ChevronDown size={13}/></label>
    </div>
   </div>
   <div className="ms-list-head"><strong>{query?'검색 결과':'인기 공개 모델'}</strong><span>{searching?'검색 중…':models.length+'개'}</span></div>
   <div className="ms-model-list" aria-label="검색된 모델">
    {models.map(m=><button key={m.id} className={'ms-model-row '+(m.id===selected?'active':'')} onClick={()=>setSelected(m.id)} aria-pressed={m.id===selected}>
     <span className="ms-repo-logo"><Cpu size={20}/></span><span className="ms-repo-info"><strong>{modelTitle(m.id)}</strong><small>{m.id.split('/')[0]}</small><span className="ms-repo-meta"><b>↓ {short(m.downloads)}</b><b>♡ {short(m.likes)}</b>{m.tags.some(t=>/gguf/i.test(t))&&<em>GGUF</em>}{m.tags.some(t=>/mlx/i.test(t))&&<em>MLX</em>}</span></span>{m.gated&&<span className="ms-gated">승인 필요</span>}</button>)}
    {!models.length&&<div className="ms-list-empty">{searching?<><LoaderCircle className="ms-spin" size={22}/><span>모델을 검색하고 있습니다</span></>:<><Search size={26}/><strong>검색된 모델이 없습니다</strong><span>다른 모델명이나 형식을 선택해 보세요</span></>}</div>}
   </div>
   <footer className="ms-list-foot"><span><CloudDownload size={15}/> Hugging Face 공개 저장소</span><span>실제 검색 결과</span></footer>
   </div>
   <div className="ms-details">{detailLoading?<div className="ms-detail-empty"><LoaderCircle className="ms-spin" size={28}/><span>모델 정보를 불러오고 있습니다</span></div>:detail?<>
    <div className="ms-model-heading"><div><div className="ms-mini-label">MODEL DETAILS</div><h3>{modelTitle(detail.id)}</h3><p>{detail.id}</p></div><a href={'https://huggingface.co/'+detail.id} target="_blank" rel="noreferrer" title="원본 저장소" aria-label="원본 저장소 열기"><ExternalLink size={19}/></a></div>
    <div className="ms-facts"><span><ArrowDownToLine size={15}/>{short(detail.downloads)} 다운로드</span><span>♡ {short(detail.likes)} 추천</span><span>업데이트 {date(detail.updatedAt)}</span></div>
    <div className="ms-description"><p>{infoText(detail).slice(0,420)}</p><div className="ms-tags"><span>ARCH <strong>{String(detail.architecture||'—').slice(0,35)}</strong></span><span>LICENSE <strong>{detail.license||'미표기'}</strong></span></div><div className="ms-capabilities">{detail.capabilities.vision&&<span><Eye size={14}/> Vision</span>}{detail.capabilities.tools&&<span><Wrench size={14}/> Tools</span>}{detail.capabilities.reasoning&&<span><BrainCircuit size={14}/> Reasoning</span>}{!detail.capabilities.vision&&!detail.capabilities.tools&&!detail.capabilities.reasoning&&<span><Code2 size={14}/> {detail.task||'언어 모델'}</span>}</div></div>
    <div className="ms-section-heading"><div><h4>다운로드 버전</h4><small>실제 저장소 파일 · 용량 비교</small></div><span>{options.length}개 옵션</span></div>
    <div className="ms-variants">{options.map(o=><button key={o.id} onClick={()=>setVariant(o.id)} className={'ms-variant '+(choice?.id===o.id?'selected':'')} aria-pressed={choice?.id===o.id}>
     <span className={'ms-format-label '+o.format}>{formatName(o.format)}</span><span className="ms-variant-title"><strong>{o.label}</strong><small>{o.quantization||'양자화 정보 없음'}{o.shards>1?' · '+o.shards+'개 파일':''}</small></span><span className="ms-file-size">{size(o.bytes)}</span><span className="ms-variant-check">{choice?.id===o.id&&<Check size={14}/>}</span>
    </button>)}
    {!options.length&&<div className="ms-empty-options">이 저장소에는 선택한 형식의 지원 파일이 없습니다.</div>}</div>
    {choice&&<div className="ms-variant-action">
     <div className="ms-estimation"><div><HardDrive size={15}/> 다운로드 {size(choice.bytes)}</div><div><Cpu size={15}/> {gpuInfo?((gpuInfo.kind==='unified'?'통합 메모리':'VRAM')+' 여유 '+size(gpuInfo.free)):'메모리 정보 없음'}</div></div>
     {gpuInfo&&<div className={'ms-estimate-hint '+(mayFit?'positive':'')}><Info size={15}/> {mayFit?'메모리 적재 가능성 높음':'메모리 부족 또는 일부 오프로딩 가능성'} · 추정 사용량 {size(estimated)} (실제 적재 결과와 다를 수 있음)</div>}
     {job&&<Progress job={job} busy={Boolean(working)} onControl={manage}/>}
     <div className="ms-action-row">{owned?<><span className="ms-owned"><Check size={17}/> 설치됨</span><button className="ms-primary" onClick={()=>load(owned.id)} disabled={Boolean(working)||loading||!canRun(owned)}>{working?'실행 요청 중':!canRun(owned)?'실행 엔진 없음':runtime.id===owned.id&&ready?'다시 실행':'모델 실행'}<Play size={15}/></button></>:<button className="ms-primary" disabled={Boolean(working)||activeJob(job?.status||'')||job?.status==='paused'||detail.gated} onClick={download}><Download size={17}/>{detail.gated?'접근 승인 필요':job?.status==='paused'?'아래 재개 버튼 이용':'다운로드'}<span>{size(choice.bytes)}</span></button>}</div>
    </div>}
    <button className="ms-readme-toggle" onClick={()=>setReadmeOpen(x=>!x)} aria-expanded={readmeOpen}><span><BookOpenText size={16}/> 원본 README</span><ChevronDown size={17} className={readmeOpen?'flipped':''}/></button>
    {readmeOpen&&<pre className="ms-readme">{detail.readme.replace(/^---[\s\S]*?---/,'').trim().slice(0,7500)||'README 내용을 불러오지 못했습니다.'}</pre>}
   </>:<div className="ms-detail-empty"><Layers3 size={28}/><strong>모델을 선택하세요</strong><span>왼쪽 목록에서 검색 결과를 선택하면 파일과 실행 정보가 표시됩니다.</span></div>}</div>
  </div>}
  {view==='installed'&&<div className="ms-management"><div className="ms-management-head"><div><h3>설치된 모델</h3><p>로컬 디스크에 저장된 실제 모델만 표시합니다.</p></div><strong>{installed.length}개</strong></div>{installed.length?installed.map(m=><article className="ms-installed-card" key={m.id}><span className="ms-installed-logo"><HardDrive size={22}/></span><div className="ms-installed-text"><strong>{modelTitle(m.repo)}</strong><small>{m.repo}</small><div><span>{m.format.toUpperCase()}</span><span>{m.label}</span><span>{size(m.bytes)}</span><span>설치 {date(m.installedAt)}</span></div></div><div className="ms-installed-actions">{runtime.id===m.id&&ready?<button className="ms-outline" onClick={stop} disabled={Boolean(working)}><Square size={15}/> 실행 중지</button>:<button className="ms-primary" onClick={()=>load(m.id)} disabled={Boolean(working)||loading||!canRun(m)}><Play size={16}/>{canRun(m)?'실행':'엔진 필요'}</button>}<button className="ms-delete" title="모델 삭제" aria-label={modelTitle(m.repo)+' 삭제'} onClick={()=>remove(m.id)} disabled={Boolean(working)}><Trash2 size={17}/></button></div></article>):<div className="ms-empty-management"><HardDrive size={33}/><h4>다운로드한 모델이 없습니다</h4><p>모델 탐색에서 원하는 버전을 설치하세요.</p><button onClick={()=>setView('discover')}>모델 탐색 <ArrowRight size={15}/></button></div>}</div>}
  {view==='downloads'&&<div className="ms-management"><div className="ms-management-head"><div><h3>다운로드 작업</h3><p>진행률, 일시정지, 재개를 관리합니다.</p></div><strong>{jobs.length}개</strong></div>{jobs.length?jobs.map(j=><div className="ms-download-card" key={j.id}><div className="ms-download-title"><CloudDownload size={18}/><div><strong>{modelTitle(j.repo)}</strong><small>{j.optionId.replace(/^gguf:|^mlx:/,'')}</small></div></div><Progress job={j} busy={Boolean(working)} onControl={manage}/></div>):<div className="ms-empty-management"><CloudDownload size={33}/><h4>진행 중인 다운로드가 없습니다</h4><p>모델 탐색에서 다운로드를 시작하면 여기에 표시됩니다.</p></div>}</div>}
  <footer className="ms-runtime-footer"><div className="ms-runtime-meta"><span className={'ms-runtime-led '+(ready?'ready':loading?'loading':'')}/><div><strong>{ready?'런타임 활성':loading?'모델 불러오는 중':'로컬 런타임'}</strong><small>{ready?runtime.name||runtime.repo:runtime.status==='error'?'실행 오류 발생':'GGUF · '+(runtime.capabilities?.gguf.available?'사용 가능':'실행 파일 필요')+' / MLX · '+(runtime.capabilities?.mlx.available?'사용 가능':'실행 파일 필요')}</small></div></div><div className="ms-runtime-controls"><label>컨텍스트<select value={context} onChange={e=>setContext(Number(e.target.value))}><option value={2048}>2K</option><option value={4096}>4K</option><option value={8192}>8K</option><option value={16384}>16K</option><option value={32768}>32K</option></select></label><label className="ms-gpu"><input type="checkbox" checked={gpu} onChange={e=>setGpu(e.target.checked)}/> GGUF GPU 자동 배치</label>{ready&&<><button className="ms-outline" onClick={stop} disabled={Boolean(working)}><Square size={15}/> 언로드</button><button className="ms-primary" onClick={onOpenChat}><ArrowRight size={15}/> 대화 시작</button></>}</div></footer>
  {runtime.status==='error'&&runtime.error&&<div className="ms-error ms-runtime-error"><CircleAlert size={17}/><span>{runtime.error}</span></div>}
 </section>
}

function Progress({job,busy,onControl}:{job:Job;busy:boolean;onControl:(id:string,action:'pause'|'resume'|'cancel')=>void}){
 const percentage=Math.min(100,Math.max(0,100*job.downloaded/Math.max(1,job.total)))
 const stage:Record<string,string>={resolving:'준비 중',downloading:'다운로드 중',paused:'일시정지',failed:'실패',cancelled:'중단됨',completed:'완료',installed:'설치됨'}
 return <div className="ms-progress-block"><div className="ms-progress-header"><strong>{stage[job.status]||job.status}</strong><span>{size(job.downloaded)} / {size(job.total)} · {percentage.toFixed(0)}%</span></div>
  <div className="ms-progress-track" role="progressbar" aria-label={modelTitle(job.repo)+' 다운로드'} aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(percentage)}><i style={{width:percentage+'%'}}/></div>
  <div className="ms-progress-bottom"><small>{job.error||job.current||'파일 처리 중'}</small><div>{activeJob(job.status)&&<button disabled={busy} onClick={()=>onControl(job.id,'pause')}><Pause size={14}/> 일시정지</button>}{['paused','failed','cancelled'].includes(job.status)&&<button disabled={busy} onClick={()=>onControl(job.id,'resume')}><Play size={14}/> 재개</button>}{['downloading','resolving','paused'].includes(job.status)&&<button disabled={busy} onClick={()=>onControl(job.id,'cancel')}><X size={14}/> 중단</button>}</div></div>
 </div>
}
