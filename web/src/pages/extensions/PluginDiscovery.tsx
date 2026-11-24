import {useEffect,useRef,useState} from 'react'
import {Search,ExternalLink,PackagePlus} from 'lucide-react'
import {get,post} from '../../api'
import {ManagedPlugins,type PackageSpec} from './ManagedPlugins'
import {PackageMarket} from './PackageMarket'
import {Button,TextInput} from '../../components/Primitives'
import {useLocale} from '../../i18n'

type Source='all'|'github'|'npm'|'pypi'|'crates'|'huggingface'|'docker'
type Result={id:string;source:Source;name:string;kind:'package'|'repository';version?:string|null;description:string;license:string;url:string;compatibility:'unverified'}
type SearchPage={items:Result[];errors:{source:Source;message:string}[];pypiMode:string;searchOnly:boolean}
const sources:{id:Source;label:string}[]=[{id:'all',label:'All'},{id:'github',label:'GitHub'},{id:'npm',label:'npm'},{id:'pypi',label:'PyPI'},{id:'crates',label:'crates.io'},{id:'huggingface',label:'Hugging Face'},{id:'docker',label:'Docker Hub'}]
export function PluginDiscovery(){
 const en=useLocale()==='en'
 const [view,setView]=useState<'market'|'installed'|'external'|'requests'>(()=>new URLSearchParams(window.location.hash.split('?')[1]).get('extensionView')==='installed'?'installed':'market')
 const [image,setImage]=useState(''),[selectedPackage,setSelectedPackage]=useState<PackageSpec|null>(null)
 const [query,setQuery]=useState(''),[source,setSource]=useState<Source>('all')
 const [page,setPage]=useState<SearchPage|null>(null),[busy,setBusy]=useState(false),[error,setError]=useState('')
 const [proposals,setProposals]=useState<{id:string;digest:string;payload:{proposal:Record<string,unknown>;binding:unknown}}[]>([]),[reviewBusy,setReviewBusy]=useState(''),[reviewError,setReviewError]=useState(''),[revision,setRevision]=useState(0)
 const refreshProposals=async()=>{try{const r=await get<{items:typeof proposals}>('/api/plugins/proposals');setProposals(r.items||[])}catch(e){setReviewError((e as Error).message)}}
 useEffect(()=>{void refreshProposals()},[])
 const review=async(id:string,digest:string,approve:boolean)=>{if(reviewBusy)return;if(approve&&!window.confirm(en?'Execute this exact plugin request?':'표시된 플러그인 요청을 그대로 실행하시겠습니까?'))return;setReviewBusy(id);setReviewError('');try{if(approve){await post('/api/plugins/proposals/'+id+'/approve',{confirmed:true,digest});setRevision(v=>v+1)}else{await post('/api/plugins/proposals/'+id+'/dismiss',{confirmed:true,digest})}await refreshProposals()}catch(e){setReviewError((e as Error).message)}finally{setReviewBusy('')}}
 const active=useRef<AbortController|null>(null)
 useEffect(()=>()=>active.current?.abort(),[])
 const clear=()=>{active.current?.abort();active.current=null;setPage(null);setBusy(false);setError('')}
 const search=async()=>{
  if(query.trim().length<2)return
  clear()
  const controller=new AbortController();active.current=controller;setBusy(true)
  try{
   const result=await get<SearchPage>('/api/plugins/search?q='+encodeURIComponent(query.trim())+'&source='+source,controller.signal)
   if(active.current===controller&&!controller.signal.aborted){
    if(!result.searchOnly||!Array.isArray(result.items)||!Array.isArray(result.errors))throw Error(en?'Invalid registry response':'검색 응답이 올바르지 않습니다.')
    setPage(result)
   }
  }catch(e){if(active.current===controller&&!controller.signal.aborted)setError((e as Error).message)}
  finally{if(active.current===controller){active.current=null;setBusy(false)}}
 }
 return <>
 <header className="mb-4"><h2 className="text-xl font-bold">{en?'Extensions':'확장 프로그램'}</h2><p className="mt-1 text-sm text-muted">{en?'Find independent apps, install them and manage their supported features.':'필요한 프로그램을 찾아 설치하고 지원하는 기능을 연결하세요.'}</p></header>
 <div role="navigation" aria-label={en?'Extension views':'확장 프로그램 보기'} className="mb-4 flex flex-wrap gap-2 rounded-xl border border-divider bg-surface p-2">{(['market','installed'] as const).map(v=><Button key={v} aria-pressed={view===v} variant={view===v?'primary':'ghost'} onClick={()=>setView(v)}>{({market:en?'Discover':'플러그인 마켓',installed:en?'Installed':'설치됨',external:en?'Developer search':'개발자 검색',requests:en?'AI requests':'AI 요청'})[v]}</Button>)}<details className="self-center"><summary className="cursor-pointer px-3 py-2 text-sm text-muted">{en?'Advanced':'고급'}</summary><div className="flex gap-1"><Button size="sm" variant={view==='external'?'primary':'ghost'} onClick={()=>setView('external')}>{en?'Developer search':'개발자 검색'}</Button><Button size="sm" variant={view==='requests'?'primary':'ghost'} onClick={()=>setView('requests')}>{en?'AI requests':'AI 요청'}</Button></div></details></div>
 {view==='market'&&<PackageMarket initialQuery={new URLSearchParams(window.location.hash.split('?')[1]).get('q')||''} onInstalled={()=>{setRevision(v=>v+1);setView('installed')}}/>}
 <div hidden={view!=='requests'} className={view==='requests'?'':'!hidden'}><section className="mb-6 rounded-xl border border-divider bg-surface p-4"><div className="flex flex-wrap items-center justify-between gap-2"><h3 className="font-semibold">{en?'AI plugin requests':'AI 플러그인 요청'} · {proposals.length}</h3><Button size="sm" variant="secondary" onClick={()=>void refreshProposals()}>{en?'Refresh':'새로고침'}</Button></div><p className="my-2 text-xs text-muted">{en?'Review exact settings before installation or execution.':'AI가 요청한 설치·실행 설정을 확인하고 승인합니다.'}</p>{reviewError&&<p role="alert" className="text-xs text-danger">{reviewError}</p>}{proposals.map(p=><div key={p.id} className="border-t border-divider py-3"><pre className="whitespace-pre-wrap break-all text-xs">{JSON.stringify(p.payload,null,2)}</pre><div className="mt-2 flex gap-2"><Button size="sm" disabled={!!reviewBusy} onClick={()=>void review(p.id,p.digest,true)}>{reviewBusy===p.id?(en?'Processing…':'처리 중…'):(en?'Approve and execute':'승인하고 실행')}</Button><Button size="sm" variant="secondary" disabled={!!reviewBusy} onClick={()=>void review(p.id,p.digest,false)}>{en?'Dismiss':'거절'}</Button></div></div>)}</section></div>
 {view==='installed'&&<ManagedPlugins key={revision} selectedImage={image} onImageConsumed={()=>setImage('')} selectedPackage={selectedPackage} onPackageConsumed={()=>setSelectedPackage(null)}/>}
 <div hidden={view!=='external'} className={view==='external'?'':'!hidden'}><section className="mb-6 rounded-xl border border-divider bg-surface p-4">
  <h3 className="flex items-center gap-2 text-[15px] font-bold text-ink"><PackagePlus size={17}/>{en?'Open-source discovery':'외부 확장 프로그램 검색'}</h3>
  <p className="my-2 text-xs text-muted">{en?'Search public registries. Results need compatibility review before installation.':'공개 저장소를 검색합니다. 검색 결과는 설치 전에 DOMINION 호환성을 확인해야 합니다.'}</p>
  <div className="mb-3 flex flex-wrap gap-1.5" aria-label={en?'Package registries':'패키지 저장소'}>
   {sources.map(s=><Button key={s.id} variant={source===s.id?'primary':'ghost'} onClick={()=>{clear();setSource(s.id)}}>{s.id==='all'&&!en?'전체':s.label}</Button>)}
  </div>
  <form className="flex gap-2" onSubmit={e=>{e.preventDefault();void search()}}>
   <TextInput aria-label={en?'Search packages':'확장 프로그램 검색어'} placeholder="voice assistant / pipecat-ai" maxLength={160} value={query} onChange={e=>{clear();setQuery(e.target.value)}}/>
   <Button type="submit" disabled={busy||query.trim().length<2}><Search size={16}/>{busy?(en?'Searching…':'검색 중…'):(en?'Search':'검색')}</Button>
  </form>
  {(source==='all'||source==='pypi')&&<p className="mt-2 text-[11px] text-muted">{en?'PyPI supports exact package names here, not full-text search.':'PyPI는 현재 정확한 패키지 이름으로 조회합니다. 일반 키워드 검색은 지원하지 않습니다.'}</p>}
  {error&&<p role="alert" className="mt-3 text-xs text-danger">{error}</p>}
  {page&&<div className="mt-3">
   {page.errors.map(e=><p role="alert" key={e.source} className="text-xs text-warning">{e.source}: {e.message}</p>)}
   {!page.items.length&&<p className="py-3 text-xs text-muted">{en?'No matching results from responding registries.':'응답한 저장소에 일치하는 결과가 없습니다.'}</p>}
   <div className="max-h-[440px] divide-y divide-divider overflow-auto">{page.items.map(r=><div key={r.id} className="flex flex-wrap items-start justify-between gap-2 py-3">
    <div className="min-w-0 flex-1">
     <strong className="break-all text-sm text-ink">{r.name}</strong>
     <p className="text-[11px] text-muted">{r.source} · {r.kind==='repository'?(en?'Repository':'저장소'):(en?'Package':'패키지')}{r.version?' · '+r.version:''}{r.license?' · '+r.license:''}</p>
     <p className="mt-1 break-words text-xs text-muted">{r.description}</p>
     <p className="text-[11px] text-warning">{en?'Compatibility not verified':'호환성 미검증'}</p>
    </div>
    {r.source==='docker'&&<Button size="sm" variant="secondary" onClick={()=>{setImage(r.name+':');setView('installed')}}>{en?'Configure install':'설치 설정'}</Button>}
    {(r.source==='npm'||r.source==='pypi'||r.source==='crates')&&r.version&&<Button size="sm" variant="secondary" onClick={()=>{setSelectedPackage({runtime:r.source as PackageSpec['runtime'],package:r.name,version:r.version||'',entry:'',args:[]});setView('installed')}}>{en?'Configure package':'패키지 설치 설정'}</Button>}
    <a className="inline-flex items-center gap-1 text-xs text-brand" href={r.url} target="_blank" rel="noopener noreferrer">{en?'Source':'원본'} <ExternalLink size={12}/></a>
   </div>)}</div>
  </div>}
 </section></div>
 </>
}
