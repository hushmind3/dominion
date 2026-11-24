import {useEffect,useRef,useState} from 'react'
import {get,post} from '../../api'
import {Button,TextInput} from '../../components/Primitives'
import {useLocale} from '../../i18n'
import {registryDraft,type RegistryMcp} from '../../features/mcp/quickCommands'

type Plugin={id:string;name:string;image:string;running:boolean;state:string}
type Manifest={name:string;image:string;network:boolean;port:number|null}
type Detail={repository:string;sourceRef:string;commit:string;manifestDigest:string;installId:string;manifest:Manifest}
type Configuration={values:{servers:Record<string,unknown>[]}}
type Grant={enabled:boolean;digest?:string;tools?:unknown[]}
const connectorRepo='hushmind3/dominion-rust',connectorRef='dominion-sub3'
const connectorImage='ghcr.io/hushmind3/dominion-rust/mcp-connector@sha256:'
const pause=(ms:number)=>new Promise(resolve=>setTimeout(resolve,ms))
const same=(a:Record<string,unknown>,b:Record<string,unknown>)=>a.transport===b.transport&&(a.transport==='http'?a.url===b.url:a.command===b.command&&JSON.stringify(a.args||[])===JSON.stringify(b.args||[]))
async function ready(path:string){
 for(let n=0;n<20;n++){try{return await get<Configuration>(path)}catch(e){if(n===19)throw e;await pause(750)}}
 throw Error('Connector did not become ready')
}

/** Owner-initiated workflow over existing SDK APIs; no embedded MCP runtime. */
export function McpRegistryPreview(){
 const en=useLocale()==='en',tr=(ko:string,eng:string)=>en?eng:ko
 const initial=new URLSearchParams(window.location.hash.split('?')[1]).get('q')||''
 const [q,setQ]=useState(initial),[rows,setRows]=useState<RegistryMcp[]>([]),[busy,setBusy]=useState(false),[error,setError]=useState(''),[notice,setNotice]=useState(''),[working,setWorking]=useState(''),[saved,setSaved]=useState<string[]>([]),[keys,setKeys]=useState<Record<string,string>>({}),[updated,setUpdated]=useState('')
 const active=useRef<AbortController|null>(null),locked=useRef(false)
 const search=async(term:string)=>{
  if(term.trim().length>160)return
  active.current?.abort();const controller=new AbortController();active.current=controller;setBusy(true);setError('')
  try{const result=await get<RegistryMcp[]>('/api/mcp/catalog?q='+encodeURIComponent(term.trim()),controller.signal)
   if(active.current===controller&&!controller.signal.aborted){if(!Array.isArray(result))throw Error('Invalid registry response');setRows(result);setUpdated(new Date().toLocaleTimeString())}
  }catch(e){if(active.current===controller&&!controller.signal.aborted)setError((e as Error).message)}
  finally{if(active.current===controller)setBusy(false)}
 }
 useEffect(()=>{const timer=setTimeout(()=>void search(q),q.trim()?350:0);return()=>{clearTimeout(timer);active.current?.abort()}},[q])
 useEffect(()=>{if(q.trim()||busy||working)return;const timer=setInterval(()=>{if(document.visibilityState==='visible'&&!locked.current)void search('')},300000);return()=>clearInterval(timer)},[q,busy,working])
 const connect=async(row:RegistryMcp)=>{
  const draft=registryDraft(row);if(!draft||locked.current)return
  locked.current=true;setWorking(row.name);setError('');setNotice(tr('연결 준비 중…','Preparing connection…'))
  let toolPath='',configPath='',revoked=false,changed=false;let previous:Configuration|null=null
  try{
   const inventory=await get<{available:boolean;items:Plugin[]|null;error?:string}>('/api/plugins/managed')
   if(!inventory.available||!inventory.items)throw Error(inventory.error||tr('Docker 실행 환경을 사용할 수 없습니다.','Docker is unavailable.'))
   let plugin=inventory.items.find(p=>p.image.startsWith(connectorImage)&&p.state!=='retained')
   if(!plugin){
    setNotice(tr('MCP 커넥터 자동 설치 중…','Installing MCP connector…'))
    const d=await get<Detail>('/api/plugins/market/details?repository='+encodeURIComponent(connectorRepo)+'&sourceRef='+encodeURIComponent(connectorRef))
    if(d.repository!==connectorRepo||d.sourceRef!==connectorRef||!d.manifest?.image.startsWith(connectorImage)||!d.commit||!d.manifestDigest)throw Error('Invalid connector package')
    plugin=await post<Plugin>('/api/plugins/market/install',{repository:d.repository,commit:d.commit,manifestDigest:d.manifestDigest,confirmed:true})
   }
   const root='/api/plugins/managed/'+encodeURIComponent(plugin.id)
   if(!plugin.running)await post(root+'/start',{})
   configPath=root+'/configuration';toolPath=root+'/tools'
   const config=await ready(configPath),servers=config.values?.servers;previous=config
   if(!Array.isArray(servers))throw Error('Invalid connector configuration')
   const existing=servers.find(s=>same(s,draft))
   const collision=servers.find(s=>String(s.name).toLowerCase()===row.name.toLowerCase()&&!same(s,draft))
   if(collision)throw Error(tr('동일한 이름의 다른 연결이 있습니다. 기존 연결은 변경하지 않았습니다.','A different connection has this name; existing settings were not changed.'))
   if(!existing&&servers.length>=8)throw Error(tr('이 커넥터의 연결 한도(8개)에 도달했습니다.','This connector has reached its eight-server limit.'))
   const grant=await get<Grant>(toolPath),key=keys[row.name]?.trim()||''
   if(existing&&existing.enabled!==false&&grant.enabled&&key&&typeof existing.apiKey==='string'&&existing.apiKey){throw Error(tr('이미 연결된 인증키는 고급 설정에서 변경할 수 있습니다. 기존 연결을 보호하기 위해 현재 키는 변경하지 않았습니다.','Change an existing credential in advanced settings; the current credential was not changed.'))}
   if(existing&&existing.enabled!==false&&grant.enabled&&!key){const live=await get<Grant>(toolPath+'/discover');if(live.digest&&live.digest===grant.digest){setSaved(s=>[...new Set([...s,row.name])]);setNotice(tr('이미 연결되어 있습니다.','Already connected.'));return}}
   if(!existing||existing.enabled===false||key){
    if(grant.enabled){await post(toolPath,{enabled:false,confirmed:true,digest:null});revoked=true}
    const auth=key&&draft.transport==='http'?{apiKey:key}:{};
    const next=existing?servers.map(s=>s===existing?{...s,enabled:true,...auth}:s):[...servers,{id:Array.from(crypto.getRandomValues(new Uint8Array(16)),n=>n.toString(16).padStart(2,'0')).join(''),...draft,...auth}]
    setNotice(tr('서버 설정 자동 저장 중…','Saving server configuration…'))
    await post(configPath,{confirmed:true,values:{...config.values,servers:next}});changed=true
   }else if(grant.enabled){await post(toolPath,{enabled:false,confirmed:true,digest:null});revoked=true}
   setNotice(tr('실제 도구를 확인하고 연결 중…','Discovering and connecting tools…'))
   const catalog=await get<Grant>(toolPath+'/discover')
   if(!catalog.digest||!catalog.tools?.length)throw Error(tr('사용 가능한 도구가 없습니다.','No available tools.'))
   await post(toolPath,{enabled:true,confirmed:true,digest:catalog.digest})
   setSaved(s=>[...new Set([...s,row.name])]);setKeys(k=>({...k,[row.name]:''}));setNotice(tr('연결 완료 · 사용 가능한 도구 ','Connected · available tools ')+catalog.tools.length)
  }catch(e){let recovery='';if(revoked&&previous){try{if(changed)await post(configPath,{confirmed:true,values:previous.values});const prior=await get<Grant>(toolPath+'/discover');if(!prior.digest||!prior.tools?.length)throw Error('No prior tools');await post(toolPath,{enabled:true,confirmed:true,digest:prior.digest});recovery=tr(' · 기존 연결을 복구했습니다.',' · Previous connections restored.')}catch{recovery=tr(' · 기존 도구 등록 복구 실패. 설치 관리에서 확인하세요.',' · Previous registration could not be restored; check Installed.')}}setError((e as Error).message+recovery);setNotice('')}
  finally{locked.current=false;setWorking('')}
 }
 const featured:RegistryMcp={name:'Hugging Face',url:'https://huggingface.co/mcp',description:tr('모델과 데이터셋 탐색','Find models and datasets')}
 const items=[...rows,...(!q.trim()?[featured]:[])].filter((r,i,a)=>a.findIndex(x=>x.name.toLowerCase()===r.name.toLowerCase()||(x.url&&x.url===r.url))===i)
 return <section className="space-y-3 rounded-xl border border-divider p-4">
  <h3 className="font-semibold">{tr('MCP 찾아보기','Discover MCP')}</h3>
  <p className="text-xs text-muted">{tr('연결 버튼으로 커넥터 설치·실행·설정·도구 등록을 자동 처리합니다. 실제 도구 실행 승인은 유지됩니다.','Connect installs and starts the connector, saves configuration and registers tools. Tool execution still requires approval.')}</p>
  <form className="flex gap-2" onSubmit={e=>{e.preventDefault();void search(q)}}><TextInput aria-label={tr('MCP 서버 검색','Search MCP servers')} maxLength={160} value={q} onChange={e=>setQ(e.target.value)} placeholder={tr('선택 사항: 이름으로 검색','Optional: search by name')}/><Button type="submit" disabled={busy||!!working}>{tr('검색','Search')}</Button><Button variant="secondary" disabled={busy||!!working} onClick={()=>{setQ('');void search('')}}>{tr('새로고침','Refresh')}</Button></form>
  {updated&&<p className="text-xs text-muted">{tr('최근 조회','Last fetched')} · {updated} · {tr('GitHub 인기 서버 우선','Popular GitHub servers first')}</p>}{busy&&<p role="status" className="text-sm text-muted">{tr('공식 카탈로그 갱신 중…','Refreshing official catalog…')}</p>}
  {notice&&<p role="status" className="text-sm text-muted">{notice}</p>}{error&&<p role="alert" className="text-danger text-sm break-words">{error}</p>}
  <div className="grid gap-2 sm:grid-cols-2">{items.map(row=>{const draft=registryDraft(row);return <article key={row.name} className="min-w-0 rounded-xl border border-divider p-3 space-y-2"><strong className="break-all text-sm">{row.name}</strong>{typeof row.stars==='number'&&<p className="text-xs text-muted">★ {row.stars.toLocaleString()}</p>}<p className="break-words text-xs text-muted">{row.description}</p>{draft&&<p className="text-xs text-muted break-all">{draft.transport==='http'?String(draft.url):[draft.command,...(Array.isArray(draft.args)?draft.args:[])].join(' ')}</p>}{draft?.transport==='http'&&<details><summary className="cursor-pointer text-xs text-muted">{tr('인증키가 필요한 경우','If credentials are required')}</summary><input type="password" autoComplete="off" aria-label={tr('API 키','API key')} className="mt-1 w-full rounded border border-divider bg-canvas p-2 text-sm" value={keys[row.name]||''} onChange={e=>setKeys(k=>({...k,[row.name]:e.target.value}))}/></details>}<Button size="sm" disabled={!draft||!!working||(saved.includes(row.name)&&!keys[row.name]?.trim())} onClick={()=>void connect(row)}>{saved.includes(row.name)&&!keys[row.name]?.trim()?tr('연결됨','Connected'):working===row.name?tr('연결 중…','Connecting…'):draft?tr('연결 · 도구 등록','Connect & register'):tr('자동 연결 미지원','Manual setup required')}</Button></article>})}</div>
  {!busy&&!items.length&&<p className="text-sm text-muted">{tr('일치하는 서버가 없습니다.','No matching servers.')}</p>}
  <details><summary className="cursor-pointer text-xs text-muted">{tr('고급 설정 · 인증정보 · 설치 관리','Advanced settings · credentials · installation')}</summary><a href="#/connections?view=plugins&extensionView=installed" className="text-sm text-brand underline">{tr('설치된 확장 관리','Manage installed extensions')}</a></details>
 </section>
}
