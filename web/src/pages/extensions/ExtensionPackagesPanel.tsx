import {useEffect,useRef,useState} from 'react'
import {PackagePlus,Search,Download,Trash2,RefreshCw} from 'lucide-react'
import {get,post,del} from '../../api'
import {useLocale} from '../../i18n'
import {Button,TextInput,TextArea} from '../../components/Primitives'
import {Notice,Panel,useRemote} from '../../components/UI'
type Manifest={id:string;name:string;description:string;version:string;sdkVersion:number;runtime:'node'|'python'|'native';targets?:string[];artifact:{url:string;sha256:string};tools:{name:string;description:string;parameters:unknown}[]}
type Entry={manifest:Manifest;source:string;revision:string;compatible:boolean}
type Installed={id:string;manifest:Manifest;source:string;revision:string;enabled:boolean}
type Results={items:Entry[];errors:{source:string;error:string}[]}
export function ExtensionPackagesPanel(){
 const en=useLocale()==='en'
 const sources=useRemote<{urls:string[];githubDiscovery:boolean}>('/api/extensions/registries',{urls:[],githubDiscovery:true},false)
 const installed=useRemote<Installed[]>('/api/extensions',[])
 const [sourceText,setSourceText]=useState('')
 const [githubDiscovery,setGithubDiscovery]=useState(true)
 const [query,setQuery]=useState('')
 const [results,setResults]=useState<Results>({items:[],errors:[]})
 const [busy,setBusy]=useState('')
 const fileRef=useRef<HTMLInputElement>(null)
 const [error,setError]=useState('')
 const [notice,setNotice]=useState('')
 const [searched,setSearched]=useState(false)
 useEffect(()=>{if(!sources.loading&&!sources.error){setSourceText(sources.value.urls.join('\n'));setGithubDiscovery(sources.value.githubDiscovery!==false)}},[sources.value,sources.loading,sources.error])
 const run=async(key:string,fn:()=>Promise<void>)=>{
  if(busy)return
  setBusy(key);setError('');setNotice('')
  try{await fn()}catch(e){setError((e as Error).message)}finally{setBusy('')}
 }
 const search=()=>run('search',async()=>{
  const next=await get<Results>('/api/extensions/catalog?q='+encodeURIComponent(query.trim()))
  setResults(next);setSearched(true)
 })
 const save=()=>run('sources',async()=>{
  await post('/api/extensions/registries',{urls:sourceText.split('\n').map(s=>s.trim()).filter(Boolean),githubDiscovery})
  await sources.reload();setResults({items:[],errors:[]});setSearched(false)
  setNotice(en?'Registry sources saved.':'레지스트리 주소를 저장했습니다.')
 })
 const install=(row:Entry)=>run(row.manifest.id,async()=>{
  const m=row.manifest
  if(!window.confirm((en?'Review extension download and declared tools. Installed code is initially disabled.':'설치할 파일과 도구를 확인해 주세요. 설치 직후에는 비활성화됩니다.')+'\n\n'+m.name+' '+m.version+'\n'+row.source+'\n'+m.artifact.url+'\nSHA-256: '+m.artifact.sha256+'\n'+m.tools.map(t=>t.name+': '+t.description).join('\n')))return
  await post('/api/extensions',{source:row.source,manifest:m,confirmed:true})
  await installed.reload()
  setNotice(en?'Downloaded and verified. Enable to use.':'다운로드 및 무결성 검사 완료. 활성화하면 사용할 수 있습니다.')
 })
 const importFile=(file:File)=>run('import',async()=>{
  if(file.size>12*1024*1024)throw Error(en?'Package exceeds 12 MiB file limit.':'패키지 파일이 12 MiB를 초과합니다.')
  const bundle=JSON.parse(await file.text())
  const m=bundle?.manifest as Manifest|undefined
  if(bundle?.bundleVersion!==1||!m||typeof m.name!=='string'||typeof m.version!=='string'||typeof bundle.artifactBase64!=='string'||!Array.isArray(m.tools))throw Error(en?'Invalid DOMINION package file.':'도미니온 패키지 형식이 올바르지 않습니다.')
  if(!window.confirm((en?'Install this local package? It will be disabled until you activate it.':'이 로컬 패키지를 설치할까요? 설치 후 활성화해야 사용할 수 있습니다.')+'\n\n'+m.name+' '+m.version+'\n'+m.runtime+'\nSHA-256: '+m.artifact?.sha256+'\n'+m.tools.map(t=>t.name+': '+t.description).join('\n')))return
  await post('/api/extensions/import',{bundleVersion:1,manifest:m,artifactBase64:bundle.artifactBase64,confirmed:true})
  await installed.reload()
  setNotice(en?'Local package installed and verified.':'로컬 패키지 설치 및 무결성 검사 완료.')
 })
 const enable=(row:Installed)=>run(row.id,async()=>{
  const enabled=!row.enabled
  if(enabled&&!window.confirm((en?'This executable runs with your OS account permissions. Only enable trusted code. Each agent tool call requires approval.':'이 코드는 현재 OS 계정 권한으로 실행됩니다. 신뢰하는 코드만 활성화하세요. 에이전트 도구 호출마다 승인이 필요합니다.')+'\n\n'+row.manifest.name+' '+row.manifest.version))return
  await post('/api/extensions/'+encodeURIComponent(row.id)+'/enable',{enabled,confirmed:enabled})
  await installed.reload()
 })
 const remove=(row:Installed)=>run(row.id,async()=>{
  if(!window.confirm((en?'Remove installed extension?':'설치된 확장 프로그램을 삭제할까요?')+'\n'+row.manifest.name))return
  await del('/api/extensions/'+encodeURIComponent(row.id));await installed.reload()
 })
 const byId=new Map(installed.value.map(row=>[row.id,row]))
 return <div className="grid gap-4">
  <div className="flex flex-wrap items-start justify-between gap-3"><div><h2 className="flex items-center gap-2 text-lg font-semibold"><PackagePlus size={20}/>{en?'Extension programs':'확장 프로그램'}</h2>
   <p className="mt-1 text-xs text-[#7C7469]">{en?'Discover and install SDK-compatible packages. MCP servers and model downloads are managed separately.':'SDK 호환 패키지를 검색하고 설치합니다. MCP 서버와 모델 다운로드는 별도 메뉴에서 관리합니다.'}</p></div>
   <input ref={fileRef} type="file" accept=".json,.dominion" hidden onChange={e=>{const file=e.currentTarget.files?.[0];e.currentTarget.value='';if(file)void importFile(file)}}/>
   <Button variant="secondary" disabled={Boolean(busy)} onClick={()=>fileRef.current?.click()}><PackagePlus size={14}/>{en?'Install from file':'파일에서 설치'}</Button>
  </div>
  <Notice message={error||sources.error||installed.error}/>
  {notice&&<p role="status" className="text-xs text-emerald-800">{notice}</p>}
  <Panel className="grid gap-3 p-4">
   <details>
    <summary className="cursor-pointer text-sm font-semibold">{en?'Search sources':'검색 출처'} · {sources.value.urls.length+(sources.value.githubDiscovery?1:0)}</summary>
    <label className="mt-3 flex items-center gap-2 text-xs"><input type="checkbox" checked={githubDiscovery} onChange={e=>setGithubDiscovery(e.target.checked)}/>{en?'Public GitHub repositories (dominion-extension topic)':'GitHub 공개 확장 검색 (dominion-extension 태그)'}</label>
    <p className="my-2 text-xs text-[#7C7469]">{en?'Add optional trusted HTTPS registry JSON URLs, one per line.':'필요한 경우 신뢰하는 확장 레지스트리 JSON의 HTTPS 주소를 한 줄씩 입력하세요.'}</p>
    <TextArea rows={3} aria-label={en?'Registry URLs':'확장 레지스트리 주소'} value={sourceText} onChange={e=>setSourceText(e.target.value)} placeholder="https://example.com/dominion-registry.json"/>
    <Button disabled={Boolean(busy)||sources.loading} onClick={()=>void save()}>{en?'Save sources':'주소 저장'}</Button>
   </details>
   <form className="flex items-center gap-2" onSubmit={e=>{e.preventDefault();void search()}}>
    <Search size={17}/><TextInput aria-label={en?'Search extensions':'확장 프로그램 검색'} value={query} maxLength={256} onChange={e=>setQuery(e.target.value)} placeholder={en?'Search by name or capability':'이름 또는 기능 검색'}/>
    <Button type="submit" disabled={Boolean(busy)||sources.loading||Boolean(sources.error)||(!sources.value.githubDiscovery&&!sources.value.urls.length)}>{busy==='search'?(en?'Searching…':'검색 중…'):(en?'Search':'검색')}</Button>
   </form>
   {results.errors.map((e,i)=><p key={i} role="alert" className="break-all text-xs text-rose-700">{e.source}: {e.error}</p>)}
   {searched&&!results.items.length&&<p className="text-xs text-[#7C7469]">{results.errors.length?(en?'No results from successful sources. Review source errors.':'정상 응답 출처의 검색 결과가 없습니다. 출처 오류를 확인하세요.'):(en?'No published extensions matched.':'검색 조건에 맞는 공개 확장이 없습니다.')}</p>}
   <div className="grid gap-2">{results.items.map(row=>{
    const m=row.manifest,existing=byId.get(m.id)
    const same=existing?.revision===row.revision
    const conflict=existing&&existing.source!==row.source
    return <div key={row.source+':'+row.revision} className="flex flex-wrap items-start justify-between gap-3 rounded-xl border border-[#E7E0D5] p-3">
     <div className="min-w-0 flex-1"><strong className="text-sm">{m.name}</strong><span className="ml-2 text-[10px] text-[#8A8176]">{m.version} · {m.runtime}</span>
      <p className="mt-1 break-words text-xs">{m.description}</p><p className="break-all text-[10px] text-[#8A8176]">{m.id} · {row.source}</p>
      <details className="mt-1 text-xs"><summary>{en?'Tools':'제공 도구'} {m.tools.length}</summary>{m.tools.map(t=><p key={t.name}>{t.name}: {t.description}</p>)}</details>
     </div>
     <Button disabled={Boolean(busy)||!row.compatible||same||Boolean(conflict)} onClick={()=>void install(row)}><Download size={14}/>{conflict?(en?'Different source':'출처 충돌'):!row.compatible?(en?'Unsupported platform':'호환되지 않음'):same?(en?'Installed':'설치됨'):existing?(en?'Change version':'버전 변경'):(en?'Install':'설치')}</Button>
    </div>
   })}</div>
  </Panel>
  <Panel className="grid gap-3 p-4">
   <div className="flex items-center justify-between"><h3 className="text-sm font-semibold">{en?'Installed extensions':'설치된 확장'} · {installed.value.length}</h3><Button variant="ghost" onClick={()=>void installed.reload()} aria-label={en?'Refresh':'새로고침'}><RefreshCw size={15}/></Button></div>
   {!installed.loading&&!installed.value.length&&<p className="text-xs text-[#7C7469]">{en?'Nothing installed. The core works independently.':'설치된 확장이 없습니다. 본체는 확장 없이 정상 작동합니다.'}</p>}
   {installed.value.map(row=><div key={row.id} className="flex flex-wrap items-center gap-3 border-t border-[#E7E0D5] py-3">
    <div className="min-w-0 flex-1"><strong className="text-sm">{row.manifest.name}</strong><p className="text-xs text-[#7C7469]">{row.manifest.version} · {row.manifest.runtime} · {row.enabled?(en?'Enabled':'활성'):(en?'Disabled':'비활성')}</p></div>
    <Button variant="secondary" disabled={Boolean(busy)} onClick={()=>void enable(row)}>{row.enabled?(en?'Disable':'비활성화'):(en?'Enable':'활성화')}</Button>
    <Button variant="ghost" disabled={Boolean(busy)} onClick={()=>void remove(row)}><Trash2 size={15}/>{en?'Remove':'삭제'}</Button>
   </div>)}
  </Panel>
 </div>
}
