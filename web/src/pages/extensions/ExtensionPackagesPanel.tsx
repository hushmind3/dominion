import {useEffect,useRef,useState} from 'react'
import {PackagePlus,Package,Search,Download,Trash2,RefreshCw,Settings2,ChevronRight,CheckCircle2,AlertCircle,LoaderCircle,Upload,X,Power} from 'lucide-react'
import {get,post,del} from '../../api'
import {useLocale} from '../../i18n'
import {Button,TextInput,TextArea,SelectInput} from '../../components/Primitives'
import {Notice,Panel,useRemote,refresh} from '../../components/UI'
import {DialogBackdrop,DialogPanel,DialogHeader,DialogBody,DialogActions} from '../../components/Dialog'
import type {ExtensionManifest as Manifest,ExtensionCatalogEntry as Entry,InstalledExtension as Installed,ExtensionCatalog as Results,ExtensionStatus,ExtensionRegistries,ExtensionBundle,ExtensionVersions} from '../../../../sdk/typescript/src/index'
type Detail={kind:'catalog';entry:Entry}|{kind:'installed';id:string}|{kind:'local';bundle:ExtensionBundle}
type Review='install'|'enable'|'remove'|null
const sourceLabel=(source:string,en:boolean)=>source.startsWith('local:')?(en?'Local file':'로컬 파일'):source.startsWith('github:')?source.slice(7):source
const runtimeLabel=(runtime:Manifest['runtime'])=>runtime==='node'?'Node.js':runtime==='python'?'Python':'Native'
export function ExtensionPackagesPanel(){
 const en=useLocale()==='en'
 const tr=(ko:string,english:string)=>en?english:ko
 const sources=useRemote<ExtensionRegistries>('/api/extensions/registries',{urls:[],githubDiscovery:true},false)
 const installed=useRemote<Installed[]>('/api/extensions',[])
 const [view,setView]=useState<'browse'|'installed'>('browse')
 const [query,setQuery]=useState(''),[installedQuery,setInstalledQuery]=useState('')
 const [filter,setFilter]=useState('all'),[runtime,setRuntime]=useState('all')
 const [results,setResults]=useState<Results>({items:[],errors:[]})
 const [searched,setSearched]=useState<string|null>(null),[searching,setSearching]=useState(false),[searchError,setSearchError]=useState('')
 const searchAbort=useRef<AbortController|null>(null)
 const [settings,setSettings]=useState(false)
 const [sourceText,setSourceText]=useState(''),[githubDiscovery,setGithubDiscovery]=useState(true)
 const [detail,setDetail]=useState<Detail|null>(null),[review,setReview]=useState<Review>(null)
 const [busy,setBusy]=useState(''),[error,setError]=useState(''),[notice,setNotice]=useState('')
 const [diagnostics,setDiagnostics]=useState<Record<string,ExtensionStatus>>({})
 const [versions,setVersions]=useState<Record<string,ExtensionVersions>>({})
 const running=useRef(false),fileRef=useRef<HTMLInputElement>(null)
 const byId=new Map(installed.value.map(row=>[row.id,row]))
 const sourceCount=sources.value.urls.length+(sources.value.githubDiscovery?1:0)
 const canSearch=!sources.loading&&!sources.error&&sourceCount>0
 const invalidateSearch=()=>{searchAbort.current?.abort();searchAbort.current=null;setSearching(false);setSearched(null);setResults({items:[],errors:[]});setSearchError('')}
 useEffect(()=>()=>searchAbort.current?.abort(),[])
 const search=async(text=query)=>{
  if(!canSearch)return
  searchAbort.current?.abort()
  const controller=new AbortController()
  searchAbort.current=controller;setSearching(true);setSearchError('')
  const q=text.trim()
  try{const response=await get<Results>('/api/extensions/catalog?q='+encodeURIComponent(q),controller.signal)
   if(!controller.signal.aborted){setResults(response);setSearched(q)}
  }catch(e){if(!controller.signal.aborted)setSearchError((e as Error).message)}
  finally{if(searchAbort.current===controller){searchAbort.current=null;setSearching(false)}}
 }
 const run=async(key:string,fn:()=>Promise<void>)=>{
  if(running.current)return
  running.current=true;setBusy(key);setError('');setNotice('')
  try{await fn()}catch(e){setError((e as Error).message)}finally{running.current=false;setBusy('')}
 }
 const openSettings=()=>{setError('');setSourceText(sources.value.urls.join('\n'));setGithubDiscovery(sources.value.githubDiscovery);setSettings(true)}
 const save=()=>run('sources',async()=>{
  const saved=await post<ExtensionRegistries>('/api/extensions/registries',{urls:sourceText.split('\n').map(s=>s.trim()).filter(Boolean),githubDiscovery})
  sources.setValue(saved);invalidateSearch();setVersions({});setSettings(false);setNotice(tr('검색 출처를 저장했습니다.','Search sources saved.'))
 })
 const show=(next:Detail,step:Review=null)=>{setError('');setReview(step);setDetail(next)}
 const close=()=>{if(!running.current){setDetail(null);setReview(null);setError('')}}
 const updateInstalled=(row:Installed)=>{
  installed.setValue(old=>[...old.filter(item=>item.id!==row.id),row])
  setDiagnostics(old=>{const next={...old};delete next[row.id];return next})
  setVersions(old=>{const next={...old};delete next[row.id];return next})
  refresh('/api/tools')
 }
 const finishInstall=(saved:Installed)=>{
  updateInstalled(saved);setView('installed');setDetail({kind:'installed',id:saved.id});setReview(null)
  setNotice(tr('설치했습니다. 사용하려면 활성화하세요.','Installed. Enable the extension to use it.'))
 }
 const install=()=>run('install',async()=>{
  if(detail?.kind==='catalog'){const {entry}=detail;finishInstall(await post<Installed>('/api/extensions',{source:entry.source,manifest:entry.manifest,confirmed:true}))}
  else if(detail?.kind==='local'){finishInstall(await post<Installed>('/api/extensions/import',{...detail.bundle,confirmed:true}))}
 })
 const importFile=(file:File)=>run('import',async()=>{
  if(file.size>12*1024*1024)throw Error(tr('패키지 파일이 12 MiB를 초과합니다.','Package exceeds 12 MiB.'))
  const bundle=JSON.parse(await file.text()) as ExtensionBundle
  const m=bundle?.manifest
  if(bundle?.bundleVersion!==1||!m||typeof m.name!=='string'||typeof m.version!=='string'||typeof m.id!=='string'||typeof m.artifact?.sha256!=='string'||typeof bundle.artifactBase64!=='string'||!Array.isArray(m.tools))throw Error(tr('도미니온 패키지 형식이 올바르지 않습니다.','Invalid DOMINION package.'))
  show({kind:'local',bundle},'install')
 })
 const enable=(row:Installed,enabled:boolean)=>run('enable',async()=>{
  const saved=await post<Installed>('/api/extensions/'+encodeURIComponent(row.id)+'/enable',{enabled,confirmed:enabled})
  updateInstalled(saved);setReview(null);setNotice(enabled?tr('활성화했습니다. 에이전트에게 확장 이름으로 요청하세요.','Enabled. Ask an agent to use the extension by name.'):tr('비활성화했습니다.','Disabled.'))
 })
 const diagnose=(row:Installed)=>run('status',async()=>{
  const status=await get<ExtensionStatus>('/api/extensions/'+encodeURIComponent(row.id)+'/status')
  setDiagnostics(old=>({...old,[row.id]:status}))
 })
 const checkVersions=(row:Installed)=>run('versions',async()=>{
  const available=await get<ExtensionVersions>('/api/extensions/'+encodeURIComponent(row.id)+'/versions')
  setVersions(old=>({...old,[row.id]:available}))
 })
 const remove=(row:Installed)=>run('remove',async()=>{
  await del('/api/extensions/'+encodeURIComponent(row.id))
  installed.setValue(old=>old.filter(item=>item.id!==row.id));setDetail(null);setReview(null);refresh('/api/tools')
  setNotice(tr('확장 프로그램을 삭제했습니다.','Extension removed.'))
 })
 const selected=detail?.kind==='installed'?byId.get(detail.id):detail?.kind==='catalog'?byId.get(detail.entry.manifest.id):detail?.kind==='local'?byId.get(detail.bundle.manifest.id):undefined
 const manifest=detail?.kind==='catalog'?detail.entry.manifest:detail?.kind==='local'?detail.bundle.manifest:selected?.manifest
 const source=detail?.kind==='catalog'?detail.entry.source:detail?.kind==='local'?'local:'+detail.bundle.manifest.id:selected?.source||''
 const conflict=Boolean(selected&&selected.source!==source)
 const same=detail?.kind==='catalog'&&selected?.revision===detail.entry.revision
 const compatible=detail?.kind!=='catalog'||detail.entry.compatible
 const shownCatalog=results.items.filter(row=>runtime==='all'||row.manifest.runtime===runtime)
 const shownInstalled=installed.value.filter(row=>(filter==='all'||(filter==='enabled')===row.enabled)&&[row.manifest.name,row.id,row.manifest.description].join(' ').toLowerCase().includes(installedQuery.trim().toLowerCase())).sort((a,b)=>a.manifest.name.localeCompare(b.manifest.name))
 const active=installed.value.filter(row=>row.enabled).length
 const stateLabel=(enabled:boolean)=>enabled?tr('활성','Enabled'):tr('비활성','Disabled')
 const pill=(enabled:boolean)=><span className={'rounded-full px-2 py-0.5 text-[11px] '+(enabled?'bg-emerald-50 text-emerald-800':'bg-[#F3EEE5] text-[#6F675D]')}>{stateLabel(enabled)}</span>
 const busyIcon=<LoaderCircle size={15} className="animate-spin"/>
 const card=(m:Manifest,s:string,action:()=>void,extra:React.ReactNode)=><article className="flex min-w-0 items-start gap-3 rounded-xl border border-[#E7E0D5] bg-white p-4">
   <span className="grid size-11 shrink-0 place-items-center rounded-xl bg-[#F3EEE5] text-[#7C7469]"><Package size={22}/></span>
   <button type="button" onClick={action} className="min-w-0 flex-1 text-left focus-visible:outline focus-visible:outline-[#C96442]">
    <strong className="block break-words text-sm">{m.name}</strong>
    <span className="block truncate text-[11px] text-[#7C7469]">{sourceLabel(s,en)} · {m.version}</span>
    <span className="mt-2 block line-clamp-2 text-xs leading-5 text-[#625B52]">{m.description||tr('설명 없음','No description')}</span>
    <span className="mt-2 block text-[11px] text-[#7C7469]">{runtimeLabel(m.runtime)} · {m.tools.length} {tr('개 도구','tools')}</span>
   </button>
   <div className="flex shrink-0 flex-col items-end gap-2">{extra}</div>
  </article>
 return <div className="grid min-w-0 gap-4" data-extension-store>
  <header className="flex flex-wrap items-start justify-between gap-3">
   <div><h2 className="flex items-center gap-2 text-lg font-semibold"><PackagePlus size={20}/>{tr('확장 프로그램','Extensions')}</h2><p className="mt-1 text-xs text-[#7C7469]">{tr('필요한 기능만 찾아 설치하고 관리하세요.','Discover and manage only the capabilities you need.')}</p></div>
   <div className="flex flex-wrap gap-2">
    <input ref={fileRef} type="file" accept=".json,.dominion" hidden onChange={e=>{const file=e.currentTarget.files?.[0];e.currentTarget.value='';if(file)void importFile(file)}}/>
    <Button disabled={Boolean(busy)} onClick={()=>fileRef.current?.click()}>{busy==='import'?busyIcon:<Upload size={15}/>} {tr('파일 설치','Install from file')}</Button>
    <Button disabled={Boolean(busy)||sources.loading} onClick={openSettings}><Settings2 size={15}/> {tr('검색 출처','Search sources')}</Button>
   </div>
  </header>
  <div className="flex flex-wrap items-center gap-2 border-b border-[#E7E0D5] pb-3" role="tablist" aria-label={tr('확장 관리','Extension management')}>
   <Button role="tab" aria-selected={view==='browse'} variant={view==='browse'?'primary':'ghost'} onClick={()=>setView('browse')}><Search size={15}/>{tr('탐색','Browse')}</Button>
   <Button role="tab" aria-selected={view==='installed'} variant={view==='installed'?'primary':'ghost'} onClick={()=>setView('installed')}><Package size={15}/>{tr('설치됨','Installed')} {installed.value.length}</Button>
   <span className="ml-auto text-xs text-[#7C7469]">{installed.loading?tr('불러오는 중','Loading'):active+' '+tr('개 활성','active')}</span>
  </div>
  {!detail&&!settings&&<Notice message={error||installed.error||sources.error}/>}
  {notice&&!detail&&<p role="status" className="text-xs text-emerald-800">{notice}</p>}
  {view==='browse'?<section className="grid min-w-0 gap-3" role="tabpanel" aria-label={tr('탐색','Browse')}>
   <form className="flex flex-wrap items-center gap-2" onSubmit={e=>{e.preventDefault();void search()}}>
    <TextInput className="flex-1 min-w-[140px]" aria-label={tr('확장 프로그램 검색','Search extensions')} value={query} maxLength={256} onChange={e=>setQuery(e.target.value)} placeholder={tr('이름 또는 필요한 기능 검색','Search by name or capability')}/>
    <Button type="submit" variant="primary" disabled={!canSearch}>{searching?busyIcon:<Search size={15}/>} {searching?tr('다시 검색','Search again'):tr('검색','Search')}</Button>
    {searching&&<Button onClick={()=>{searchAbort.current?.abort();setSearching(false)}}>{tr('검색 취소','Cancel search')}</Button>}
   </form>
   <div className="flex flex-wrap items-center gap-2 text-xs text-[#7C7469]">
    <span>{searched===null?tr('공개 확장 탐색','Discover published extensions'):tr('검색 결과','Results')+' '+shownCatalog.length+(searched?' · '+searched:'')}</span>
    <SelectInput className="ml-auto w-auto" aria-label={tr('실행 환경 필터','Runtime filter')} value={runtime} onChange={e=>setRuntime(e.target.value)}><option value="all">{tr('모든 실행 환경','All runtimes')}</option><option value="python">Python</option><option value="node">Node.js</option><option value="native">Native</option></SelectInput>
   </div>
   <Notice message={searchError}/>
   {!!results.errors.length&&<details className="rounded-xl border border-[#E7E0D5] p-3 text-xs"><summary className="cursor-pointer text-amber-800">{tr('일부 출처 조회 실패','Some sources failed')} · {results.errors.length}</summary>{results.errors.map((e,i)=><p key={i} className="mt-2 break-all">{e.source}: {e.error}</p>)}</details>}
   {searching&&<p role="status" className="flex items-center gap-2 text-xs text-[#7C7469]">{busyIcon}{tr('실제 레지스트리에서 확장을 찾고 있습니다.','Searching configured registries.')}</p>}
   {!searching&&searched===null&&!searchError&&<Panel className="grid justify-items-center gap-3 p-8 text-center"><PackagePlus size={32} className="text-[#8A8176]"/><strong>{tr('필요한 확장을 찾아보세요','Find an extension')}</strong><p className="text-xs text-[#7C7469]">{canSearch?tr('검색어를 입력하거나 공개 목록을 탐색하세요.','Search or browse the public catalog.'):tr('검색 출처를 설정하거나 파일에서 설치하세요.','Configure a search source or install from file.')}</p><Button variant="primary" onClick={()=>canSearch?void search(''):openSettings()}>{canSearch?tr('전체 확장 탐색','Browse all'):tr('검색 출처 설정','Configure sources')}</Button></Panel>}
   {searched!==null&&!searching&&!shownCatalog.length&&<Panel className="p-6 text-center text-sm text-[#7C7469]">{results.errors.length?tr('정상 응답 출처에서 결과를 찾지 못했습니다.','No matches from successful sources.'):tr('조건에 맞는 확장이 없습니다.','No matching extensions.')}</Panel>}
   <div className="grid gap-3 lg:grid-cols-2">{shownCatalog.map(entry=>{
    const existing=byId.get(entry.manifest.id),sameRevision=existing?.revision===entry.revision
    const blocked=existing&&existing.source!==entry.source
    return <div key={entry.source+':'+entry.revision}>{card(entry.manifest,entry.source,()=>show({kind:'catalog',entry}),<>
     {sameRevision?pill(Boolean(existing?.enabled)):null}
     <Button size="sm" disabled={Boolean(busy)} onClick={()=>show(sameRevision?{kind:'installed',id:entry.manifest.id}:{kind:'catalog',entry},sameRevision||blocked||!entry.compatible?null:'install')}>{sameRevision?tr('관리','Manage'):blocked?tr('출처 충돌','Source conflict'):!entry.compatible?tr('호환 정보','Compatibility'):existing?tr('버전 변경','Change version'):tr('설치','Install')}</Button>
    </>)}</div>
   })}</div>
  </section>:<section role="tabpanel" aria-label={tr('설치됨','Installed')} className="grid min-w-0 gap-3">
   <div className="flex flex-wrap gap-2"><TextInput className="flex-1 min-w-[140px]" aria-label={tr('설치된 확장 검색','Search installed extensions')} placeholder={tr('설치된 확장 검색','Search installed extensions')} value={installedQuery} onChange={e=>setInstalledQuery(e.target.value)}/><SelectInput className="w-auto" aria-label={tr('활성 상태 필터','Activation filter')} value={filter} onChange={e=>setFilter(e.target.value)}><option value="all">{tr('전체','All')}</option><option value="enabled">{tr('활성','Enabled')}</option><option value="disabled">{tr('비활성','Disabled')}</option></SelectInput><Button disabled={Boolean(busy)} onClick={()=>void installed.reload()} aria-label={tr('목록 새로고침','Refresh list')}><RefreshCw size={15}/></Button></div>
   {installed.loading?<p role="status" className="text-xs">{tr('설치 목록을 불러오는 중','Loading installed extensions')}</p>:!shownInstalled.length&&<Panel className="grid justify-items-center gap-2 p-8 text-center"><Package size={30} className="text-[#8A8176]"/><strong>{installed.value.length?tr('조건에 맞는 확장이 없습니다','No matching installed extensions'):tr('설치된 확장이 없습니다','No extensions installed')}</strong><Button onClick={()=>{setView('browse');setFilter('all');setInstalledQuery('')}}>{tr('확장 탐색','Browse extensions')}</Button></Panel>}
   <div className="grid gap-3 lg:grid-cols-2">{shownInstalled.map(row=><div key={row.id}>{card(row.manifest,row.source,()=>show({kind:'installed',id:row.id}),<>{pill(row.enabled)}<Button size="sm" onClick={()=>show({kind:'installed',id:row.id})}>{tr('관리','Manage')}<ChevronRight size={14}/></Button></>)}</div>)}</div>
  </section>}
  {settings&&<DialogBackdrop onDismiss={()=>{if(!running.current)setSettings(false)}}><DialogPanel aria-label={tr('검색 출처 설정','Search source settings')}>
   <DialogHeader><h2 className="font-semibold">{tr('검색 출처 설정','Search source settings')}</h2><Button size="icon" disabled={Boolean(busy)} onClick={()=>setSettings(false)} aria-label={tr('닫기','Close')}><X size={18}/></Button></DialogHeader>
   <DialogBody><Notice message={error}/><label className="flex items-center gap-2 text-sm"><input type="checkbox" checked={githubDiscovery} onChange={e=>setGithubDiscovery(e.target.checked)}/>{tr('GitHub 공개 확장 검색','Public GitHub discovery')}</label><p className="text-xs text-[#7C7469]">{tr('선택 기능입니다. 도미니온 자체 확장 규격을 사용합니다.','Optional discovery provider; packages use the DOMINION contract.')}</p><label className="grid gap-2 text-sm">{tr('독립 레지스트리 주소','Independent registry URLs')}<TextArea rows={4} value={sourceText} onChange={e=>setSourceText(e.target.value)} placeholder="https://example.com/catalog.json"/></label><p className="text-xs text-[#7C7469]">{tr('신뢰하는 HTTPS 주소를 한 줄씩 입력하세요. 최대 5개입니다.','One trusted HTTPS URL per line, maximum 5.')}</p></DialogBody>
   <DialogActions><Button disabled={Boolean(busy)} onClick={()=>setSettings(false)}>{tr('취소','Cancel')}</Button><Button variant="primary" disabled={Boolean(busy)} onClick={()=>void save()}>{busy==='sources'?busyIcon:null}{tr('저장','Save')}</Button></DialogActions>
  </DialogPanel></DialogBackdrop>}
  {detail&&manifest&&<DialogBackdrop onDismiss={close}><DialogPanel size="wide" aria-label={manifest.name}>
   <DialogHeader><div className="min-w-0"><h2 className="break-words text-lg font-semibold">{manifest.name}</h2><p className="text-xs text-[#7C7469]">{manifest.id} · {manifest.version} · {runtimeLabel(manifest.runtime)}</p></div><Button size="icon" disabled={Boolean(busy)} onClick={close} aria-label={tr('닫기','Close')}><X size={18}/></Button></DialogHeader>
   <DialogBody>
    <Notice message={error}/>{notice&&<p role="status" className="text-xs text-emerald-800">{notice}</p>}
    <p className="whitespace-pre-wrap break-words text-sm leading-6">{manifest.description}</p>
    <div className="grid gap-2 rounded-xl bg-[#F7F2EA] p-3 text-xs"><p className="break-all">{tr('배포 출처','Publisher source')}: {sourceLabel(source,en)}</p><p>{tr('실행 환경','Runtime')}: {runtimeLabel(manifest.runtime)} · SDK {manifest.sdkVersion}</p><p>{tr('호환 플랫폼','Targets')}: {manifest.targets?.join(', ')||tr('플랫폼 제한 없음','No declared platform restriction')}</p>{!compatible&&<p className="text-rose-700">{tr('현재 플랫폼과 호환되지 않습니다.','Not compatible with this platform.')}</p>}</div>
    {detail.kind==='installed'&&selected&&<>
     <div className="flex flex-wrap items-center gap-2">{pill(selected.enabled)}<Button size="sm" disabled={Boolean(busy)} onClick={()=>void diagnose(selected)}>{busy==='status'?busyIcon:<RefreshCw size={14}/>} {tr('상태 확인','Check status')}</Button><Button size="sm" disabled={Boolean(busy)||selected.source.startsWith('local:')} onClick={()=>void checkVersions(selected)}>{busy==='versions'?busyIcon:<Download size={14}/>} {tr('버전 확인','Check versions')}</Button></div>
     {diagnostics[selected.id]&&<div className="grid gap-1 text-xs" role="status">{[ [diagnostics[selected.id].integrityOk,tr('파일 무결성 정상','Artifact verified'),diagnostics[selected.id].integrityError], [diagnostics[selected.id].runtimeOk,tr('실행 환경 확인됨','Runtime found'),diagnostics[selected.id].runtimeError] ].map(([ok,label,message],i)=><p key={i} className={'flex items-start gap-2 '+(ok?'text-emerald-800':'text-rose-700')}>{ok?<CheckCircle2 size={15}/>:<AlertCircle size={15}/>} {ok?label:message}</p>)}</div>}
     {selected.source.startsWith('local:')&&<p className="text-xs text-[#7C7469]">{tr('로컬 확장은 새 패키지 파일을 설치해 업데이트할 수 있습니다.','Update local extensions by importing a new package file.')}</p>}
     {versions[selected.id]?.installedRevision===selected.revision&&<div className="grid gap-2 text-xs"><strong>{tr('배포된 다른 버전','Other published revisions')}</strong>{versions[selected.id].reason==='source_disabled'?tr('검색 출처가 비활성화되어 있습니다.','Source is disabled.'):!versions[selected.id].items.length?tr('확인된 다른 버전이 없습니다.','No other published revisions found.'):versions[selected.id].items.map(entry=><div key={entry.revision} className="flex items-center justify-between gap-2 border-t border-[#E7E0D5] pt-2"><span>{entry.manifest.version} · {entry.revision}</span><Button size="sm" disabled={!entry.compatible||Boolean(busy)} onClick={()=>show({kind:'catalog',entry},'install')}>{tr('버전 검토','Review version')}</Button></div>)}</div>}
     {selected.enabled&&<p className="rounded-lg bg-[#F3EEE5] p-3 text-xs">{tr('에이전트 모드에서 “'+manifest.name+' 사용해”라고 요청하세요. 도구 실행은 승인 후 이루어집니다.','Ask in agent mode: “Use '+manifest.name+'”. Tool execution requires approval.')}</p>}
    </>}
    <section className="grid gap-2"><h3 className="font-semibold text-sm">{tr('제공 기능','Provided tools')} · {manifest.tools.length}</h3>{manifest.tools.map(tool=><details key={tool.name} className="rounded-lg border border-[#E7E0D5] p-3 text-xs"><summary className="cursor-pointer font-medium break-words">{tool.name}</summary><p className="mt-2 break-words">{tool.description}</p><pre className="mt-2 max-h-36 overflow-auto whitespace-pre-wrap break-all text-[10px]">{JSON.stringify(tool.parameters,null,2)}</pre></details>)}</section>
    <details className="text-xs"><summary className="cursor-pointer font-medium">{tr('파일 및 무결성 정보','Artifact and integrity')}</summary><p className="mt-2 break-all">{manifest.artifact.url}</p><p className="mt-1 break-all">SHA-256: {manifest.artifact.sha256}</p></details>
    {conflict&&<p role="alert" className="text-xs text-rose-700">{tr('같은 ID가 다른 출처에서 설치되어 있습니다. 덮어쓸 수 없습니다.','An installed package with this ID belongs to another source.')}</p>}
    {review&&<div className="rounded-xl border border-amber-200 bg-amber-50 p-3 text-xs leading-6"><strong>{review==='install'?tr('설치 전 확인','Review installation'):review==='enable'?tr('실행 권한 확인','Review execution permissions'):tr('삭제 확인','Confirm removal')}</strong><p>{review==='install'?tr('설치 또는 버전 변경 후에는 비활성화됩니다. 기존 확장 데이터는 버전 변경 시 유지됩니다.','Installation and version changes leave the package disabled. Data survives version changes.'):review==='enable'?tr('확장 코드는 현재 OS 계정의 권한으로 실행됩니다. 신뢰하는 코드만 활성화하세요. 개별 실행은 승인해야 합니다.','Code runs with your OS account permissions. Enable only trusted code. Individual invocations require approval.'):tr('확장 파일과 저장된 확장 데이터가 삭제됩니다.','This removes the extension and its stored data.')}</p></div>}
   </DialogBody>
   <DialogActions>
    {review?<><Button disabled={Boolean(busy)} onClick={()=>setReview(null)}>{tr('취소','Cancel')}</Button><Button variant={review==='remove'?'danger':'primary'} disabled={Boolean(busy)||(review==='install'&&(conflict||!compatible||same))} onClick={()=>{if(review==='install')void install();else if(selected&&review==='enable')void enable(selected,true);else if(selected&&review==='remove')void remove(selected)}}>{busy?busyIcon:null}{review==='install'?tr('확인하고 설치','Confirm install'):review==='enable'?tr('확인하고 활성화','Confirm enable'):tr('데이터 포함 삭제','Delete with data')}</Button></>:detail.kind==='installed'&&selected?<><Button variant="danger" disabled={Boolean(busy)} onClick={()=>setReview('remove')}><Trash2 size={15}/>{tr('삭제','Remove')}</Button><Button variant="primary" disabled={Boolean(busy)} onClick={()=>selected.enabled?void enable(selected,false):setReview('enable')}><Power size={15}/>{selected.enabled?tr('비활성화','Disable'):tr('활성화','Enable')}</Button></>:<Button variant="primary" disabled={Boolean(busy)||conflict||!compatible} onClick={()=>same&&selected?show({kind:'installed',id:selected.id}):setReview('install')}>{same?tr('관리','Manage'):tr('설치 검토','Review install')}</Button>}
   </DialogActions>
  </DialogPanel></DialogBackdrop>}
 </div>
}
