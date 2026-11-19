import {useEffect,useRef,useState} from 'react'
import {get,post} from '../api'

type GoogleResponse={credential:string}
type GoogleIdentity={accounts:{id:{initialize:(config:{client_id:string;callback:(response:GoogleResponse)=>void})=>void;renderButton:(element:HTMLElement,options:Record<string,unknown>)=>void;disableAutoSelect?:()=>void}}}
declare global {interface Window {google?:GoogleIdentity}}
type Model={id:string;name?:string;free:boolean;provider?:string;input_micro_krw?:number;output_micro_krw?:number}
type CatalogProvider={id:string;name:string;api:string;models:number;doc?:string}
type RemoteModel={id:string;name?:string;context?:number|null;reasoning?:boolean|null;toolCall?:boolean|null}
const catalogProviderKind=(provider:CatalogProvider):'anthropic'|'openai'=>/anthropic/i.test(provider.id+' '+provider.api)?'anthropic':'openai'
const cloudUrl=(import.meta.env.VITE_DOMINION_CLOUD_URL||'').replace(/\/$/,'')
const clientId=import.meta.env.VITE_DOMINION_GOOGLE_CLIENT_ID||''
export function DominionCloudAccount({onProviderAdded,initialTab='cloud',initialProviderId,hideManagementNavigation=false}:{onProviderAdded:()=>Promise<unknown>;initialTab?:'cloud'|'providers';initialProviderId?:string;hideManagementNavigation?:boolean}) {
 const button=useRef<HTMLDivElement>(null)
 const [token,setToken]=useState('')
 const [models,setModels]=useState<Model[]>([])
 const [selected,setSelected]=useState('')
 const [modelSearch,setModelSearch]=useState('')
 const [modelFilter,setModelFilter]=useState<'all'|'free'|'paid'>('all')
 const [providerSort,setProviderSort]=useState<'name'|'models'>('name')
 const [managementTab,setManagementTab]=useState<'cloud'|'providers'>(initialTab)
 const [providerPage,setProviderPage]=useState(0)
 const [providerMinModels,setProviderMinModels]=useState(false)
 const [busy,setBusy]=useState(false)
 const [message,setMessage]=useState('')
 const [catalog,setCatalog]=useState<CatalogProvider[]>([])
 const [catalogSearch,setCatalogSearch]=useState('')
 const [catalogId,setCatalogId]=useState('')
 const [catalogKey,setCatalogKey]=useState('')
 const [catalogModel,setCatalogModel]=useState('')
 const [catalogModelSearch,setCatalogModelSearch]=useState('')
 const [catalogModels,setCatalogModels]=useState<RemoteModel[]>([])
 const [catalogProviderId,setCatalogProviderId]=useState('')
 const [catalogBusy,setCatalogBusy]=useState(false)
 const catalogBrowseSeq=useRef(0)
 const initialCatalogApplied=useRef('')
 const [registered,setRegistered]=useState<{providerId:string;model:string;source:'catalog'|'openrouter'|'cloud'}|null>(null)
 const [routerSelected,setRouterSelected]=useState('')
 const [catalogMessage,setCatalogMessage]=useState('')
 const [routerKey,setRouterKey]=useState('')
 const [routerBusy,setRouterBusy]=useState(false)
 const [routerMessage,setRouterMessage]=useState('')
 const [routerModels,setRouterModels]=useState<RemoteModel[]>([])
 const [routerSearch,setRouterSearch]=useState('')
 async function fetchModels(session:string){
  const response=await fetch(cloudUrl+'/v1/models',{headers:{Authorization:'Bearer '+session}})
  if(!response.ok)throw Error('모델 목록을 가져오지 못했습니다.')
  const data=await response.json() as {data:Model[]}
  const unique=[...new Map((data.data||[]).map(m=>[m.id.trim().toLowerCase(),m])).values()]
  setModels(unique)
  setSelected(previous=>unique.some(m=>m.id===previous)?previous:(unique[0]?.id||''))
 }
 useEffect(()=>{
  if(!cloudUrl||!clientId||!button.current)return
  let active=true
  const script=document.createElement('script')
  script.src='https://accounts.google.com/gsi/client'
  script.async=true
  script.onload=()=>{
   if(!active||!button.current||!window.google)return
   window.google.accounts.id.initialize({client_id:clientId,callback:async({credential})=>{
    setBusy(true);setMessage('')
    try{
     const response=await fetch(cloudUrl+'/v1/auth/google',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({credential})})
     if(!response.ok)throw Error('도미니언 로그인 실패')
     const data=await response.json() as {token:string}
     await fetchModels(data.token)
     setToken(data.token);setMessage('도미니언 계정으로 연결됐습니다.')
    }catch(error){setMessage((error as Error).message)}finally{setBusy(false)}
   }})
   window.google.accounts.id.renderButton(button.current,{theme:'outline',size:'large',text:'signin_with',width:240})
  }
  document.head.appendChild(script)
  return()=>{active=false;script.remove()}
 },[])
 function logout(){
  setToken('');setModels([]);setSelected('');setModelSearch('');setMessage('DOMINION Cloud에서 로그아웃했습니다.');
  window.google?.accounts.id.disableAutoSelect?.()
 }
 async function loadCatalog(){
  setCatalogBusy(true)
  try{
   const data=await get<{providers:CatalogProvider[];error?:string}>('/api/providers/catalog')
   setCatalog([...new Map((data.providers||[]).map(p=>[p.id.toLowerCase(),p])).values()])
   setCatalogMessage(data.error||'업체 '+(data.providers||[]).length+'개를 불러왔습니다.')
  }catch(e){setCatalogMessage((e as Error).message)}finally{setCatalogBusy(false)}
 }
 useEffect(()=>{if(managementTab==='providers'&&!catalog.length)void loadCatalog()},[managementTab])
 useEffect(()=>{
  if(!initialProviderId||managementTab!=='providers'||!catalog.length||initialCatalogApplied.current===initialProviderId)return
  const ordered=[...catalog].sort((a,b)=>a.name.localeCompare(b.name))
  const index=ordered.findIndex(p=>p.id===initialProviderId)
  if(index<0)return
  initialCatalogApplied.current=initialProviderId
  setCatalogSearch('');setProviderSort('name');setProviderMinModels(false)
  setProviderPage(Math.floor(index/40))
  setCatalogId(initialProviderId);setCatalogModel('');setCatalogModels([])
  setCatalogModelSearch('');setCatalogKey('');setRegistered(null)
  void loadCatalogModels(initialProviderId)
 },[initialProviderId,managementTab,catalog])
 async function loadCatalogModels(providerId=catalogId){
  const request=++catalogBrowseSeq.current
  if(!providerId)return
  setCatalogBusy(true);setCatalogMessage('공개 모델 목록 조회 중…')
  try{
   const response=await get<{models:RemoteModel[];cached?:boolean;error?:string}>('/api/providers/catalog/'+encodeURIComponent(providerId)+'/models')
   if(request!==catalogBrowseSeq.current)return
   const found=response.models||[]
   setCatalogModels(found)
   setCatalogModel(found[0]?.id||'')
   setCatalogMessage(response.error||(found.length+'개 모델 조회 완료 · API 키 없이 공개 목록 확인'))
  }catch(e){if(request===catalogBrowseSeq.current)setCatalogMessage((e as Error).message)}
  finally{if(request===catalogBrowseSeq.current)setCatalogBusy(false)}
 }
 async function ensureCatalogCredential(entry:CatalogProvider){
  const providers=await get<Array<{id:string;baseUrl:string;model?:string;hasKey?:boolean}>>('/api/providers')
  const baseUrl=entry.api.replace(/\/$/,'')
  const linked=providers.find(p=>p.hasKey&&p.baseUrl.replace(/\/$/,'')===baseUrl)
  const key=catalogKey.trim()
  if(!key&&!linked)throw Error('모델 목록은 API 키 없이 조회할 수 있습니다. 채팅에 등록하려면 API 키를 입력하거나 기존 인증을 연결하세요.')
  return {providers,linked,key,baseUrl}
 }
 async function saveCatalogModel(){
  const entry=catalog.find(p=>p.id===catalogId)
  if(!entry||!catalogModel)return
  setCatalogBusy(true)
  try{
   const {providers,linked,key,baseUrl}=await ensureCatalogCredential(entry)
   const duplicate=providers.find(p=>p.baseUrl.replace(/\/$/,'')===baseUrl&&p.model===catalogModel&&p.hasKey)
   if(duplicate){
    setRegistered({providerId:duplicate.id,model:catalogModel,source:'catalog'})
    setCatalogMessage('이미 채팅에 연결된 모델입니다.');return
   }
   const blank=!key?providers.find(p=>p.baseUrl.replace(/\/$/,'')===baseUrl&&!p.model&&p.hasKey):undefined
   const saved=await post<{id:string}>('/api/providers',{id:blank?.id,name:entry.name+' · '+catalogModel,baseUrl,kind:catalogProviderKind(entry),apiKey:key||undefined,credentialSourceId:key?undefined:linked?.id,model:catalogModel,enabled:true,local:false,toolMode:'auto'})
   setCatalogProviderId(saved.id)
   setRegistered({providerId:saved.id,model:catalogModel,source:'catalog'})
   await onProviderAdded()
   setCatalogMessage('모델이 채팅에 등록됐습니다.')
  }catch(e){setCatalogMessage((e as Error).message)}finally{setCatalogBusy(false)}
 }
 async function importCatalogModels(){
  const entry=catalog.find(p=>p.id===catalogId)
  if(!entry||!catalogModels.length)return
  setCatalogBusy(true);setCatalogMessage('모델 일괄 등록 중…')
  try{
   const {providers,linked,key,baseUrl}=await ensureCatalogCredential(entry)
   const known=new Set(providers.filter(p=>p.baseUrl.replace(/\/$/,'')===baseUrl&&p.hasKey).map(p=>p.model))
   const blank=providers.find(p=>p.baseUrl.replace(/\/$/,'')===baseUrl&&!p.model&&p.hasKey)
   let added=0
   for(const model of catalogModels){
    if(!model.id||known.has(model.id))continue
    await post('/api/providers',{id:blank&&added===0?blank.id:undefined,name:entry.name+' · '+model.id,baseUrl,kind:catalogProviderKind(entry),apiKey:key||undefined,credentialSourceId:key?undefined:linked?.id,model:model.id,enabled:true,local:false,toolMode:'auto'})
    known.add(model.id);added++
   }
   await onProviderAdded()
   setCatalogMessage('전체 '+catalogModels.length+'개 중 신규 '+added+'개 등록 · 중복 제외')
  }catch(e){setCatalogMessage((e as Error).message)}finally{setCatalogBusy(false)}
 }
 async function getOpenRouterFreeModels(){
  const response=await get<{models:RemoteModel[];cached?:boolean;error?:string}>('/api/providers/openrouter/free-models')
  if(response.error&&!response.models?.length)throw Error(response.error)
  const models=response.models||[]
  setRouterModels(models)
  return {models,message:response.error}
 }
 async function browseOpenRouterFree(){
  setRouterBusy(true);setRouterMessage('공개 무료 모델 조회 중…')
  try{
   const {models,message}=await getOpenRouterFreeModels()
   setRouterMessage(message||('OpenRouter 무료 텍스트 모델 '+models.length+'개 조회 · API 키 불필요'))
  }catch(error){setRouterMessage((error as Error).message)}finally{setRouterBusy(false)}
 }
 async function ensureOpenRouterCredential(){
  const baseUrl='https://openrouter.ai/api/v1'
  const existing=await get<Array<{id:string;baseUrl:string;model?:string;hasKey?:boolean}>>('/api/providers')
  const linked=existing.find(p=>p.hasKey&&p.baseUrl.replace(/\/$/,'')===baseUrl)
  const key=routerKey.trim()
  if(!key&&!linked)throw Error('공개 무료 모델은 키 없이 조회됩니다. 채팅 등록에는 OpenRouter API 키 또는 기존 연결이 필요합니다.')
  return {baseUrl,existing,linked,key}
 }
 async function saveOpenRouterModel(){
  const selectedModel=routerModels.find(model=>model.id===routerSelected)
  if(!selectedModel)return
  setRouterBusy(true);setRouterMessage('')
  try{
   const {baseUrl,existing,linked,key}=await ensureOpenRouterCredential()
   const duplicate=existing.find(p=>p.baseUrl.replace(/\/$/,'')===baseUrl&&p.model===selectedModel.id&&p.hasKey)
   if(duplicate){
    setRegistered({providerId:duplicate.id,model:selectedModel.id,source:'openrouter'})
    setRouterMessage('이미 연결된 모델입니다. 바로 채팅에서 이용할 수 있습니다.')
    return
   }
   const saved=await post<{id:string}>('/api/providers',{name:'OpenRouter 무료 · '+(selectedModel.name||selectedModel.id),baseUrl,kind:'openai',apiKey:key||undefined,credentialSourceId:key?undefined:linked?.id,model:selectedModel.id,enabled:true,local:false,toolMode:'auto'})
   setRegistered({providerId:saved.id,model:selectedModel.id,source:'openrouter'})
   await onProviderAdded()
   setRouterMessage('선택한 무료 모델을 채팅에 등록했습니다.')
  }catch(error){setRouterMessage((error as Error).message)}finally{setRouterBusy(false)}
 }
 async function importOpenRouterFree(){
  setRouterBusy(true);setRouterMessage('무료 모델 등록 중…')
  try{
   const {baseUrl,existing,linked,key}=await ensureOpenRouterCredential()
   const free=routerModels.length?routerModels:(await getOpenRouterFreeModels()).models
   const known=new Set(existing.filter(p=>p.baseUrl.replace(/\/$/,'')===baseUrl&&p.hasKey).map(p=>p.model))
   let added=0
   for(const model of free){
    if(!model.id||known.has(model.id))continue
    await post('/api/providers',{name:'OpenRouter 무료 · '+(model.name||model.id),baseUrl,kind:'openai',apiKey:key||undefined,credentialSourceId:key?undefined:linked?.id,model:model.id,enabled:true,local:false,toolMode:'auto'})
    known.add(model.id);added++
   }
   await onProviderAdded()
   setRouterMessage('무료 '+free.length+'개 확인, 신규 '+added+'개 등록, 중복 제외')
  }catch(error){setRouterMessage((error as Error).message)}finally{setRouterBusy(false)}
 }
 async function register(){
  if(!token||!selected)return
  setBusy(true);setMessage('')
  try{
   const existing=await get<Array<{id:string;baseUrl:string;model:string}>>('/api/providers')
   const match=existing.find(p=>p.baseUrl.replace(/\/$/,'')===cloudUrl+'/v1'&&p.model===selected)
   if(match){
    setRegistered({providerId:match.id,model:selected,source:'cloud'})
    setMessage('이미 등록된 모델입니다. 중복 등록하지 않았습니다.')
    return
   }
   const saved=await post<{id:string}>('/api/providers',{name:'DOMINION Cloud · '+selected,baseUrl:cloudUrl+'/v1',kind:'openai',apiKey:token,model:selected,enabled:true,local:false,toolMode:'compat'})
   setRegistered({providerId:saved.id,model:selected,source:'cloud'})
   await onProviderAdded()
   setMessage('모델이 등록됐습니다. 채팅에서 선택해 사용하세요.')
  }catch(error){setMessage((error as Error).message)}finally{setBusy(false)}
 }
 const filteredCatalog=catalog.filter(p=>(p.name+' '+p.id).toLowerCase().includes(catalogSearch.toLowerCase())&&(!providerMinModels||p.models>0)).sort((a,b)=>providerSort==='models'?b.models-a.models||a.name.localeCompare(b.name):a.name.localeCompare(b.name))
 const providerPageCount=Math.max(1,Math.ceil(filteredCatalog.length/40))
 const visibleCatalog=filteredCatalog.slice(providerPage*40,(providerPage+1)*40)
 const filteredModels=catalogModels.filter(model=>(model.id+' '+(model.name||'')).toLowerCase().includes(catalogModelSearch.toLowerCase()))
 const visibleModels=filteredModels.slice(0,100)
 const filteredRouterModels=routerModels.filter(model=>(model.id+' '+(model.name||'')).toLowerCase().includes(routerSearch.toLowerCase()))
 const goToRegisteredChat=()=>{if(registered)window.location.hash='#/chat?'+new URLSearchParams({provider:registered.providerId,model:registered.model})}
 return <section className="rounded-xl border border-[#E7E0D5] bg-[#FFFDF9] p-4">
  {!hideManagementNavigation&&<div className="flex flex-wrap gap-2 border-b border-[#E7E0D5] pb-3 mb-3" role="tablist" aria-label="모델 연결 관리">
   <button type="button" role="tab" aria-selected={managementTab==='cloud'} onClick={()=>setManagementTab('cloud')} className={`rounded-lg px-3 py-2 text-[12px] ${managementTab==='cloud'?'bg-[#C96442] text-white':'border'}`}>DOMINION Cloud</button>
   <button type="button" role="tab" aria-selected={managementTab==='providers'} onClick={()=>{setManagementTab('providers');if(!catalog.length)void loadCatalog()}} className={`rounded-lg px-3 py-2 text-[12px] ${managementTab==='providers'?'bg-[#C96442] text-white':'border'}`}>업체·모델 연결</button>
  </div>}
  {managementTab==='cloud'&&<><strong className="text-[13px]">DOMINION Cloud · 구글 로그인</strong>
  <p className="mt-1 text-[12px] text-[#7C7469]">DOMINION 계정 하나로 무료·유료 모델을 선택합니다. 업체 API 키는 서버에서 관리합니다.</p>
  {cloudUrl&&clientId?<div className="mt-3" ref={button}/>:<p className="mt-2 text-[12px] text-[#7C7469]">DOMINION Cloud 로그인 미설정 · 다른 AI 업체는 아래에서 연결할 수 있습니다.</p>}
  {token&&<div className="mt-3 flex flex-wrap items-center gap-2">
   <input aria-label="모델 검색" value={modelSearch} onChange={e=>setModelSearch(e.target.value)} placeholder="모델 이름 또는 업체 검색" className="min-w-0 flex-1 rounded-lg border px-3 py-2 text-[12px]"/>
   <select aria-label="모델 요금 필터" value={modelFilter} onChange={e=>setModelFilter(e.target.value as 'all'|'free'|'paid')} className="rounded-lg border px-3 py-2 text-[12px]">
    <option value="all">전체</option><option value="free">무료</option><option value="paid">유료</option>
   </select>
   <select aria-label="DOMINION 모델" value={models.some(model=>model.id===selected&&(modelFilter==='all'||(modelFilter==='free'?model.free:!model.free))&&(model.id+' '+(model.name||'')+' '+(model.provider||'')).toLowerCase().includes(modelSearch.toLowerCase()))?selected:''} onChange={e=>setSelected(e.target.value)} className="min-w-0 flex-1 rounded-lg border px-3 py-2 text-[12px]">
    <option value="" disabled>모델을 선택하세요</option>
    {models.filter(model=>(modelFilter==='all'||(modelFilter==='free'?model.free:!model.free))&&(model.id+' '+(model.name||'')+' '+(model.provider||'')).toLowerCase().includes(modelSearch.toLowerCase())).map(model=><option key={model.id} value={model.id}>{model.name||model.id} · {model.free?'무료':'유료'}{model.provider?' · '+model.provider:''}</option>)}
   </select>
   <button type="button" disabled={busy||!selected||!models.some(model=>model.id===selected&&(modelFilter==='all'||(modelFilter==='free'?model.free:!model.free))&&(model.id+' '+(model.name||'')+' '+(model.provider||'')).toLowerCase().includes(modelSearch.toLowerCase()))} onClick={()=>void register()} className="rounded-lg bg-[#C96442] px-3 py-2 text-[12px] font-semibold text-white disabled:opacity-50">모델 등록</button>
  </div>}
  {registered?.source==='cloud'&&<button type="button" onClick={goToRegisteredChat} className="mt-2 rounded-lg bg-[#C96442] px-3 py-2 text-[12px] font-semibold text-white">등록한 모델로 채팅하기</button>}
  {token&&<button type="button" className="mt-3 rounded-lg border px-3 py-2 text-[12px]" onClick={()=>{setToken('');setModels([]);setSelected('');setMessage('로그아웃되었습니다.');window.google?.accounts.id.disableAutoSelect?.()}}>로그아웃</button>}
  </>}
  {managementTab==='providers'&&<div className="mt-4 border-t border-[#E7E0D5] pt-4">

   <div className="mt-3"><strong className="text-[13px]">AI 업체 통합 연결 · models.dev</strong>
   <p className="mt-1 text-[12px] text-[#7C7469]">업체 선택 → 공개 모델 조회·검색 (API 키 불필요) → 채팅 등록 시 인증 연결 · 공개 모델 목록과 실제 사용 권한은 다를 수 있습니다.</p>
   <div className="mt-2 flex flex-wrap gap-2">
    <button type="button" disabled={catalogBusy} onClick={()=>void loadCatalog()} className="rounded-lg border px-3 py-2 text-[12px]">업체 목록 불러오기</button>
    <input aria-label="AI 업체 검색" value={catalogSearch} onChange={e=>{setCatalogSearch(e.target.value);setProviderPage(0)}} placeholder="업체 이름 검색" className="min-w-0 flex-1 rounded-lg border px-3 py-2 text-[12px]"/>
   </div>
   {catalog.length>0&&<div className="mt-2 flex flex-wrap items-center gap-2 text-[12px]">
    <span className="text-[#7C7469]">{filteredCatalog.length} / {catalog.length}개 업체</span>
    <label className="flex items-center gap-1"><input type="checkbox" checked={providerMinModels} onChange={e=>{setProviderMinModels(e.target.checked);setProviderPage(0)}}/>모델 있는 업체만</label>
   </div>}
   {catalog.length>0&&<div className="mt-2 flex flex-wrap gap-2"><select aria-label="업체 정렬" value={providerSort} onChange={e=>{setProviderSort(e.target.value as 'name'|'models');setProviderPage(0)}} className="rounded-lg border px-3 py-2 text-[12px]"><option value="name">이름순</option><option value="models">모델 많은 순</option></select>
    <select aria-label="AI 업체 선택" value={catalogId} onChange={e=>{const id=e.target.value;setRegistered(null);setCatalogId(id);setCatalogModels([]);setCatalogModel('');setCatalogModelSearch('');setCatalogKey('');setCatalogProviderId('');setCatalogMessage('');void loadCatalogModels(id)}} className="min-w-0 flex-1 rounded-lg border px-3 py-2 text-[12px]">
     <option value="">업체를 선택하세요</option>
     {visibleCatalog.map(p=><option key={p.id} value={p.id}>{p.name} ({p.models})</option>)}
    </select>
    <div className="flex items-center gap-2 text-[12px]"><button type="button" className="rounded border px-2 py-1" disabled={providerPage===0} onClick={()=>{catalogBrowseSeq.current++;setCatalogBusy(false);setProviderPage(p=>Math.max(0,p-1));setCatalogId('');setCatalogModels([]);setCatalogKey('');setCatalogModel('');setCatalogProviderId('')}}>이전</button><span>{providerPage+1} / {providerPageCount}</span><button type="button" className="rounded border px-2 py-1" disabled={providerPage+1>=providerPageCount} onClick={()=>{catalogBrowseSeq.current++;setCatalogBusy(false);setProviderPage(p=>p+1);setCatalogId('');setCatalogModels([]);setCatalogKey('');setCatalogModel('');setCatalogProviderId('')}}>다음</button></div>
    <button type="button" disabled={catalogBusy||!catalogId} onClick={()=>void loadCatalogModels()} className="rounded-lg bg-[#C96442] px-3 py-2 text-[12px] font-semibold text-white disabled:opacity-50">{catalogBusy?'조회 중…':'모델 조회 (키 불필요)'}</button>
   </div>}
   {catalogModels.length>0&&<div className="mt-3 rounded-xl border border-[#E7E0D5] bg-white p-3">
    <div className="flex flex-wrap items-center justify-between gap-2 text-[12px]"><strong>공개 모델 목록 · {filteredModels.length} / {catalogModels.length}개</strong><span className="text-[#7C7469]">API 키 없이 검색 가능</span></div>
    <input aria-label="업체 모델 검색" value={catalogModelSearch} onChange={e=>setCatalogModelSearch(e.target.value)} placeholder="모델 이름 또는 ID 검색" className="mt-2 w-full rounded-lg border px-3 py-2 text-[12px]"/>
    {visibleModels.length>0?<select aria-label="공개 업체 모델 선택" value={visibleModels.some(m=>m.id===catalogModel)?catalogModel:''} onChange={e=>setCatalogModel(e.target.value)} className="mt-2 w-full rounded-lg border px-3 py-2 text-[12px]">
     <option value="">모델을 선택하세요</option>
     {visibleModels.map(m=><option key={m.id} value={m.id}>{m.name||m.id} · {m.id}</option>)}
    </select>:<p className="mt-2 text-[11px] text-[#7C7469]">검색된 모델이 없습니다.</p>}
    {filteredModels.length>100&&<p className="mt-1 text-[11px] text-[#7C7469]">처음 100개만 표시합니다. 검색어로 모델을 좁혀 주세요.</p>}
    {catalogModels.find(m=>m.id===catalogModel)&&<p className="mt-2 break-all text-[11px] leading-5 text-[#5C544A]">모델 ID: {catalogModel}{catalogModels.find(m=>m.id===catalogModel)?.context?' · 컨텍스트 '+catalogModels.find(m=>m.id===catalogModel)?.context?.toLocaleString()+' 토큰':''}{catalogModels.find(m=>m.id===catalogModel)?.reasoning===true?' · 추론 지원':''}{catalogModels.find(m=>m.id===catalogModel)?.toolCall===true?' · 도구 호출 지원':''}</p>}
    <div className="mt-3 flex flex-wrap gap-2 border-t border-[#EFE9DF] pt-3">
     <input type="password" autoComplete="off" aria-label="업체 API 키 (등록 시에만 필요)" value={catalogKey} onChange={e=>setCatalogKey(e.target.value)} placeholder="채팅 등록용 API 키 (조회에는 불필요)" className="min-w-0 flex-1 rounded-lg border px-3 py-2 text-[12px]"/>
     <button type="button" disabled={catalogBusy||!visibleModels.some(m=>m.id===catalogModel)} onClick={()=>void saveCatalogModel()} className="rounded-lg bg-[#C96442] px-3 py-2 text-[12px] font-semibold text-white disabled:opacity-50">선택 모델 채팅 등록</button>
     <button type="button" disabled={catalogBusy} onClick={()=>void importCatalogModels()} className="rounded-lg border px-3 py-2 text-[12px] disabled:opacity-50">전체 등록</button>
    </div>
    <p className="mt-2 text-[11px] text-[#7C7469]">조회는 무료 공개 메타데이터입니다. 실제 모델 호출에는 해당 업체의 인증 및 사용 권한이 필요할 수 있습니다.</p>
   </div>}
   {catalog.length>0&&<p className="mt-2 text-[12px] text-[#7C7469]">검색 결과 {filteredCatalog.length}개 / 전체 {catalog.length}개 업체 · 중복 ID 제외</p>}
   <p className="mt-2 text-[12px] text-[#7C7469]">공개 모델 목록은 API 키 없이 조회할 수 있습니다. 채팅 등록·호출에는 API 키 또는 기존 인증이 필요할 수 있습니다.</p>
   {catalogMessage&&<p role="status" className="mt-2 text-[12px] text-[#7C7469]">{catalogMessage}</p>}
   {registered?.source==='catalog'&&<button type="button" onClick={goToRegisteredChat} className="mt-2 rounded-lg bg-[#C96442] px-3 py-2 text-[12px] font-semibold text-white">등록한 모델로 채팅하기</button>}
   </div>
  </div>}
  {managementTab==='providers'&&<div className="mt-4 border-t border-[#E7E0D5] pt-4">
   <strong className="text-[13px]">OpenRouter 무료 모델 일괄 등록</strong>
   <p className="mt-1 text-[12px] text-[#7C7469]">공개 무료 모델 이름·ID 검색은 API 키가 필요하지 않습니다. 실제 채팅 등록 시에만 키를 입력하세요.</p>
   <div className="mt-2 flex flex-wrap gap-2">
    <button type="button" disabled={routerBusy} onClick={()=>void browseOpenRouterFree()} className="rounded-lg border px-3 py-2 text-[12px] disabled:opacity-50">{routerBusy?'조회 중…':'무료 모델 조회 (키 불필요)'}</button>
    <input aria-label="OpenRouter 무료 모델 검색" value={routerSearch} onChange={e=>setRouterSearch(e.target.value)} placeholder="무료 모델 이름 검색" className="min-w-0 flex-1 rounded-lg border px-3 py-2 text-[12px]"/>
   </div>
   {routerModels.length>0&&<div className="mt-2 max-h-48 overflow-y-auto rounded-lg border bg-white px-3 py-2 text-[11px] text-[#5C544A]">
    <strong>공개 무료 모델 · {filteredRouterModels.length} / {routerModels.length}개</strong>
    {filteredRouterModels.slice(0,60).map(m=><button type="button" key={m.id} aria-pressed={routerSelected===m.id} onClick={()=>{setRouterSelected(m.id);setRegistered(null)}} className={'block w-full border-b border-[#EFE9DF] px-1 py-1.5 text-left hover:bg-[#FAF7F1] '+(routerSelected===m.id?'bg-[#F7E8E1]':'')}><span className="font-semibold">{m.name||m.id}</span><span className="ml-2 break-all text-[#8A8176]">{m.id}</span>{m.context?' · '+m.context.toLocaleString()+' 토큰':''}</button>)}
    {filteredRouterModels.length>60&&<p className="mt-2 text-[#8A8176]">처음 60개만 표시합니다. 검색어로 좁혀 주세요.</p>}
   </div>}
   <div className="mt-2 flex flex-wrap gap-2">
    <input type="password" autoComplete="off" aria-label="OpenRouter API 키 (등록 시에만 필요)" value={routerKey} onChange={e=>setRouterKey(e.target.value)} placeholder="채팅 등록용 API 키" className="min-w-0 flex-1 rounded-lg border px-3 py-2 text-[12px]"/>
    <button type="button" disabled={routerBusy||!routerSelected} onClick={()=>void saveOpenRouterModel()} className="rounded-lg bg-[#C96442] px-3 py-2 text-[12px] font-semibold text-white disabled:opacity-50">{routerBusy?'등록 중…':'선택 모델 등록'}</button>
    <button type="button" disabled={routerBusy||!routerModels.length} onClick={()=>void importOpenRouterFree()} className="rounded-lg border px-3 py-2 text-[12px] disabled:opacity-50">전체 등록</button>
   </div>
   {routerMessage&&<p role="status" className="mt-2 text-[12px] text-[#7C7469]">{routerMessage}</p>}
   {registered?.source==='openrouter'&&<button type="button" onClick={goToRegisteredChat} className="mt-2 rounded-lg bg-[#C96442] px-3 py-2 text-[12px] font-semibold text-white">등록한 모델로 채팅하기</button>
  </div>}
 </section>
}
