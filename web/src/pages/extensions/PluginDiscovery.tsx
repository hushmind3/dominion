import {useEffect,useRef,useState} from 'react'
import {Search,ExternalLink,PackagePlus} from 'lucide-react'
import {get} from '../../api'
import {ManagedPlugins,type PackageSpec} from './ManagedPlugins'
import {Button,TextInput} from '../../components/Primitives'
import {useLocale} from '../../i18n'

type Source='all'|'github'|'npm'|'pypi'|'crates'|'huggingface'|'docker'
type Result={id:string;source:Source;name:string;kind:'package'|'repository';version?:string|null;description:string;license:string;url:string;compatibility:'unverified'}
type SearchPage={items:Result[];errors:{source:Source;message:string}[];pypiMode:string;searchOnly:boolean}
const sources:{id:Source;label:string}[]=[{id:'all',label:'All'},{id:'github',label:'GitHub'},{id:'npm',label:'npm'},{id:'pypi',label:'PyPI'},{id:'crates',label:'crates.io'},{id:'huggingface',label:'Hugging Face'},{id:'docker',label:'Docker Hub'}]
export function PluginDiscovery(){
 const en=useLocale()==='en'
 const [image,setImage]=useState(''),[selectedPackage,setSelectedPackage]=useState<PackageSpec|null>(null)
 const [query,setQuery]=useState(''),[source,setSource]=useState<Source>('all')
 const [page,setPage]=useState<SearchPage|null>(null),[busy,setBusy]=useState(false),[error,setError]=useState('')
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
 <ManagedPlugins selectedImage={image} onImageConsumed={()=>setImage('')} selectedPackage={selectedPackage} onPackageConsumed={()=>setSelectedPackage(null)}/>
 <section className="mb-6 rounded-xl border border-divider bg-surface p-4">
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
    {r.source==='docker'&&<Button size="sm" variant="secondary" onClick={()=>setImage(r.name+':')}>{en?'Configure install':'설치 설정'}</Button>}
    {(r.source==='npm'||r.source==='pypi'||r.source==='crates')&&r.version&&<Button size="sm" variant="secondary" onClick={()=>setSelectedPackage({runtime:r.source as PackageSpec['runtime'],package:r.name,version:r.version||'',entry:'',args:[]})}>{en?'Configure package':'패키지 설치 설정'}</Button>}
    <a className="inline-flex items-center gap-1 text-xs text-brand" href={r.url} target="_blank" rel="noopener noreferrer">{en?'Source':'원본'} <ExternalLink size={12}/></a>
   </div>)}</div>
  </div>}
 </section>
 </>
}
