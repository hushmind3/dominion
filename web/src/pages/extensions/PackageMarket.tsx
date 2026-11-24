import {useEffect,useRef,useState} from 'react'
import {Search,Package,ExternalLink,Download,Star} from 'lucide-react'
import {get,post} from '../../api'
import {Button,TextInput} from '../../components/Primitives'
import {useLocale} from '../../i18n'
type Manifest={name:string;description:string;version:string;image:string;network:boolean;port:number|null}
type Item={repository:string;name?:string;indexed?:boolean;description:string;stars:number;url:string;detailsLoaded:boolean;sourceRef?:string;commit?:string;manifestDigest?:string;manifest?:Manifest|null;sourceDownload?:string;releaseTag?:string;assets?:{name:string;url:string;size:number|null}[];releaseError?:string}
type Page={items:Item[];errors:{repository:string;message:string}[];indexCached?:boolean}
const keyOf=(item:Item)=>item.repository+'@'+(item.sourceRef||'')
export function PackageMarket({onInstalled}:{onInstalled:()=>void}){
 const en=useLocale()==='en',tr=(ko:string,eng:string)=>en?eng:ko
 const [q,setQ]=useState(''),[page,setPage]=useState<Page|null>(null),[busy,setBusy]=useState(false),[error,setError]=useState(''),[selected,setSelected]=useState<Item|null>(null),[loading,setLoading]=useState(''),[review,setReview]=useState(false),[installing,setInstalling]=useState(false)
 const request=useRef<AbortController|null>(null),detailRequest=useRef<AbortController|null>(null)
 const search=async(value:string)=>{
  request.current?.abort();detailRequest.current?.abort();const controller=new AbortController();request.current=controller;setBusy(true);setError('');setPage(null);setSelected(null);setReview(false);setLoading('')
  try{const r=await get<Page>('/api/plugins/market?q='+encodeURIComponent(value.trim()),controller.signal);if(request.current===controller&&!controller.signal.aborted){if(!Array.isArray(r?.items)||!Array.isArray(r?.errors))throw Error(tr('검색 응답이 올바르지 않습니다.','Invalid marketplace response'));setPage(r)}}
  catch(e){if(request.current===controller&&!controller.signal.aborted)setError((e as Error).message)}
  finally{if(request.current===controller)setBusy(false)}
 }
 useEffect(()=>{void search('');return()=>{request.current?.abort();detailRequest.current?.abort()}},[])
 const open=async(p:Item)=>{
  detailRequest.current?.abort();setError('');setReview(false);setSelected(null);setLoading(keyOf(p))
  if(p.detailsLoaded){setSelected(p);setLoading('');return}
  const controller=new AbortController();detailRequest.current=controller
  try{const r=await get<Item>('/api/plugins/market/details?repository='+encodeURIComponent(p.repository)+(p.sourceRef?'&sourceRef='+encodeURIComponent(p.sourceRef):''),controller.signal)
   if(detailRequest.current===controller&&!controller.signal.aborted){if(r.repository?.toLowerCase()!==p.repository.toLowerCase()||(p.sourceRef&&r.sourceRef!==p.sourceRef)||!r.detailsLoaded)throw Error(tr('배포 정보가 올바르지 않습니다.','Invalid release details'));setSelected(r)}}
  catch(e){if(detailRequest.current===controller&&!controller.signal.aborted)setError((e as Error).message)}
  finally{if(detailRequest.current===controller)setLoading('')}
 }
 const install=async()=>{if(!selected?.manifest||!selected.commit||!selected.manifestDigest||installing)return;setInstalling(true);setError('')
  try{await post('/api/plugins/market/install',{repository:selected.repository,commit:selected.commit,manifestDigest:selected.manifestDigest,confirmed:true});setSelected(null);setReview(false);onInstalled()}
  catch(e){setError((e as Error).message)}finally{setInstalling(false)}
 }
 return <section className="space-y-4 rounded-xl border border-divider bg-surface p-4">
  <div><h2 className="text-xl font-bold text-ink">{tr('플러그인 마켓','Plugin marketplace')}</h2><p className="mt-1 text-sm text-muted">{tr('프로그램 이름으로 검색하고 공개 배포 파일을 받거나 호환 확장 프로그램을 설치하세요.','Search by name, download releases, or install compatible extensions.')}</p></div>
  <form className="flex gap-2" onSubmit={e=>{e.preventDefault();void search(q)}}><TextInput aria-label={tr('플러그인 검색','Search plugins')} value={q} onChange={e=>setQ(e.target.value)} placeholder={tr('프로그램 이름 · owner/repo · owner/repo@branch','App name · owner/repo · owner/repo@branch')}/><Button type="submit" disabled={busy||installing}><Search size={16}/>{tr('검색','Search')}</Button></form>
  {busy&&<p role="status" className="text-sm text-muted">{tr('공개 저장소 검색 중…','Searching public repositories…')}</p>}
  {error&&<p role="alert" className="break-words text-sm text-danger">{error}</p>}
  {selected&&<article aria-label={tr('배포 상세','Release details')} className="space-y-3 rounded-xl border border-divider bg-canvas p-4">
   <div className="flex flex-wrap items-center justify-between gap-2"><h3 className="break-all font-bold">{selected.manifest?.name||selected.repository}</h3><Button size="sm" variant="secondary" disabled={installing} onClick={()=>{setSelected(null);setReview(false)}}>{tr('닫기','Close')}</Button></div>
   <p className="break-words text-sm">{selected.manifest?.description||selected.description}</p>{selected.sourceRef&&<p className="break-all text-xs text-muted">{selected.repository}@{selected.sourceRef}</p>}
   {selected.manifest?<div className="space-y-2"><p className="text-sm text-muted">{tr('설치 가능 패키지','Installable package')} · {selected.manifest.version}</p>
    {!review?<Button disabled={installing} onClick={()=>setReview(true)}><Download size={15}/>{tr('설치','Install')}</Button>:<div role="dialog" aria-label={tr('설치 확인','Review installation')} className="space-y-2 rounded-lg border border-divider p-3"><p className="break-all font-mono text-xs">{selected.manifest.image}</p><p className="text-xs">{tr('네트워크','Network')}: {selected.manifest.network?tr('허용','Allowed'):tr('차단','Blocked')} · {tr('서비스 포트','Service port')}: {selected.manifest.port??'–'}</p><div className="flex flex-wrap gap-2"><Button disabled={installing} onClick={()=>void install()}>{installing?tr('설치 중…','Installing…'):tr('확인 및 설치','Confirm install')}</Button><Button variant="secondary" disabled={installing} onClick={()=>setReview(false)}>{tr('취소','Cancel')}</Button></div></div>}
   </div>:<p className="text-xs text-muted">{tr('자동 설치 규격이 없는 프로그램입니다. 배포 파일은 직접 받을 수 있습니다.','No compatible install manifest. Published files remain downloadable.')}</p>}
   <div className="border-t border-divider pt-3 space-y-2"><h4 className="text-sm font-semibold">{tr('배포 파일','Release downloads')} {selected.releaseTag||''}</h4>
    {selected.assets?.map(a=><a key={a.url} href={a.url} target="_blank" rel="noopener noreferrer" className="flex items-center gap-2 break-all text-sm text-brand"><Download size={15} className="shrink-0"/>{a.name}{a.size!=null&&<span className="shrink-0 text-xs text-muted">{(a.size/1048576).toFixed(1)} MB</span>}</a>)}
    {!selected.assets?.length&&<p className="text-xs text-muted">{selected.releaseError||tr('등록된 최신 릴리스 파일이 없습니다.','No latest release assets published.')}</p>}
    {selected.sourceDownload&&<a className="inline-flex gap-2 items-center text-sm text-brand" href={selected.sourceDownload} target="_blank" rel="noopener noreferrer"><Download size={15}/>{tr('소스 ZIP 받기','Download source ZIP')}</a>}
   </div>
  </article>}
  {page&&<><p className="text-xs text-muted">{tr('검색 결과','Results')} · {page.items.length}{page.indexCached?' · '+tr('저장된 카탈로그','Cached catalog'):''}</p><div className="grid gap-3 lg:grid-cols-2">{page.items.map(p=><article key={keyOf(p)} className="flex min-w-0 flex-col gap-3 rounded-xl border border-divider bg-canvas p-4"><div className="flex items-start gap-3"><div className="shrink-0 rounded-xl border border-divider bg-surface p-3"><Package size={24}/></div><div className="min-w-0 flex-1"><h3 className="break-all font-bold">{p.manifest?.name||p.name||p.repository}</h3><p className="flex items-center gap-1 text-xs text-muted">{p.indexed?tr('확장 카탈로그','Extension catalog'):<><Star size={12}/>{p.stars??0}</>}{p.manifest?' · '+p.manifest.version:''}</p></div></div><p className="flex-1 break-words text-sm">{p.manifest?.description||p.description}</p><div className="flex flex-wrap items-center justify-between gap-2"><a href={p.url} target="_blank" rel="noopener noreferrer" className="inline-flex items-center gap-1 text-xs text-brand">{tr('소스','Source')}<ExternalLink size={12}/></a><Button disabled={installing||loading===keyOf(p)} onClick={()=>void open(p)}>{loading===keyOf(p)?tr('불러오는 중…','Loading…'):tr('설치 · 다운로드','Install / download')}</Button></div></article>)}</div>
   {!page.items.length&&<p className="rounded-xl border border-divider p-4 text-sm text-muted">{tr('검색 결과가 없습니다. 다른 이름을 입력하세요.','No results. Try another name.')}</p>}
   {page.errors.map(e=><p key={e.repository} className="break-all text-xs text-warning">{e.repository}: {e.message}</p>)}
  </>}
 </section>
}
