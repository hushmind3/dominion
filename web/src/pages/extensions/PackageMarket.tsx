import {useEffect,useRef,useState} from 'react'
import {Search,Package,ExternalLink,Download,ShieldCheck} from 'lucide-react'
import {get,post} from '../../api'
import {Button,TextInput} from '../../components/Primitives'
import {useLocale} from '../../i18n'
type Item={repository:string;commit:string;manifestDigest:string;installId:string;url:string;manifest:{name:string;description:string;version:string;image:string;network:boolean;port:number|null}}
type Page={items:Item[];errors:{repository:string;message:string}[]}
export function PackageMarket({onInstalled}:{onInstalled:()=>void}){
 const en=useLocale()==='en'
 const [q,setQ]=useState(''),[page,setPage]=useState<Page|null>(null),[busy,setBusy]=useState(false),[error,setError]=useState(''),[review,setReview]=useState<Item|null>(null),[installing,setInstalling]=useState(false)
 const request=useRef<AbortController|null>(null)
 const tr=(ko:string,eng:string)=>en?eng:ko
 const search=async(value:string)=>{
  request.current?.abort();const controller=new AbortController();request.current=controller;setBusy(true);setError('');setPage(null)
  try{const r=await get<Page>('/api/plugins/market?q='+encodeURIComponent(value.trim()),controller.signal);if(request.current===controller&&!controller.signal.aborted)setPage(r)}
  catch(e){if(request.current===controller&&!controller.signal.aborted)setError((e as Error).message)}
  finally{if(request.current===controller)setBusy(false)}
 }
 useEffect(()=>{void search('');return()=>request.current?.abort()},[])
 const install=async()=>{if(!review||installing)return;setInstalling(true);setError('')
  try{await post('/api/plugins/market/install',{repository:review.repository,commit:review.commit,manifestDigest:review.manifestDigest,confirmed:true});setReview(null);onInstalled()}
  catch(e){setError((e as Error).message)}finally{setInstalling(false)}
 }
 return <section className="space-y-5 rounded-xl border border-divider bg-surface p-5">
  <div><h2 className="text-xl font-bold text-ink">{tr('플러그인 마켓','Plugin marketplace')}</h2><p className="mt-1 text-sm text-muted">{tr('공개 배포 패키지를 찾아 직접 설치하세요. AI 모델 연결 없이도 사용 가능합니다.','Discover and install published packages without an AI model.')}</p></div>
  <form className="flex gap-2" onSubmit={e=>{e.preventDefault();void search(q)}}><TextInput aria-label={tr('플러그인 검색','Search plugins')} value={q} onChange={e=>setQ(e.target.value)} placeholder={tr('이름 또는 GitHub owner/repo','Name or GitHub owner/repo')}/><Button type="submit" disabled={busy}><Search size={16}/>{tr('검색','Search')}</Button></form>
  <div className="flex flex-wrap gap-2">{['assistant','automation','developer',''].map(v=><Button key={v} size="sm" variant="secondary" disabled={busy} onClick={()=>{setQ(v);void search(v)}}>{v?({assistant:tr('개인비서','Assistant'),automation:tr('자동화','Automation'),developer:tr('개발','Development')})[v]:tr('전체','All')}</Button>)}</div>
  {busy&&<p role="status" className="text-sm text-muted">{tr('배포 패키지 확인 중…','Checking published packages…')}</p>}
  {error&&<p role="alert" className="break-words text-sm text-danger">{error}</p>}
  {page&&<><p className="text-xs text-muted">{tr('설치 규격이 확인된 패키지','Packages with valid install manifests')} · {page.items.length}</p>
   <div className="grid gap-3 lg:grid-cols-2">{page.items.map(p=><article key={p.repository} className="flex flex-col gap-3 rounded-xl border border-divider bg-canvas p-4"><div className="flex items-start gap-3"><div className="rounded-xl border border-divider bg-surface p-3"><Package size={24}/></div><div className="min-w-0 flex-1"><h3 className="break-words font-bold">{p.manifest.name}</h3><p className="break-all text-xs text-muted">{p.repository} · {p.manifest.version}</p></div></div><p className="flex-1 break-words text-sm">{p.manifest.description}</p><p className="flex items-center gap-1 text-xs text-muted"><ShieldCheck size={14}/>{tr('배포 규격 확인 · 실행 호환성 별도 확인','Manifest validated · runtime compatibility not verified')}</p><div className="flex items-center justify-between gap-2"><a href={p.url} target="_blank" rel="noreferrer" className="inline-flex items-center gap-1 text-xs text-brand">{tr('소스','Source')}<ExternalLink size={12}/></a><Button disabled={installing} onClick={()=>{setError('');setReview(p)}}><Download size={15}/>{tr('설치','Install')}</Button></div></article>)}</div>
   {!page.items.length&&<div className="rounded-xl border border-divider p-6 text-center text-sm text-muted">{tr('현재 검색에서 설치 가능한 공개 패키지를 찾지 못했습니다. 일반 오픈소스는 개발자 검색에서 찾을 수 있습니다.','No published compatible packages found. Use developer search for general open source.')}</div>}
   {!!page.errors.length&&<details className="text-xs text-muted"><summary>{tr('배포 규격이 없거나 조회 실패한 저장소','Unavailable or invalid packages')} · {page.errors.length}</summary>{page.errors.map(e=><p key={e.repository} className="break-all">{e.repository}: {e.message}</p>)}</details>}
  </>}
  {review&&<div role="dialog" aria-label={tr('설치 확인','Review installation')} className="space-y-3 rounded-xl border border-divider bg-canvas p-4"><h3 className="font-bold">{review.manifest.name} · {review.manifest.version}</h3><p className="text-sm">{tr('외부 프로그램을 Docker에 설치합니다. 설치 후 실행 여부는 직접 결정합니다.','Install an external app into Docker. Start it separately.')}</p><p className="break-all font-mono text-xs">{review.manifest.image}</p><p className="text-xs">{tr('네트워크','Network')}: {review.manifest.network?tr('허용','Allowed'):tr('차단','Blocked')} · {tr('서비스 포트','Service port')}: {review.manifest.port??'–'}</p><div className="flex gap-2"><Button disabled={installing} onClick={()=>void install()}>{installing?tr('설치 중…','Installing…'):tr('확인 및 설치','Confirm install')}</Button><Button variant="secondary" disabled={installing} onClick={()=>setReview(null)}>{tr('취소','Cancel')}</Button></div></div>}
 </section>
}
