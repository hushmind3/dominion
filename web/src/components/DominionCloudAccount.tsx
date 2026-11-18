import {useEffect,useRef,useState} from 'react'
import {get,post} from '../api'

type GoogleResponse={credential:string}
type GoogleIdentity={accounts:{id:{initialize:(config:{client_id:string;callback:(response:GoogleResponse)=>void})=>void;renderButton:(element:HTMLElement,options:Record<string,unknown>)=>void}}}
declare global {interface Window {google?:GoogleIdentity}}
type Model={id:string;free:boolean}
type CatalogProvider={id:string;name:string;api:string;models:number;doc?:string}
type RemoteModel={id:string;name?:string}
const cloudUrl=(import.meta.env.VITE_DOMINION_CLOUD_URL||'').replace(/\/$/,'')
const clientId=import.meta.env.VITE_DOMINION_GOOGLE_CLIENT_ID||''
export function DominionCloudAccount({onProviderAdded}:{onProviderAdded:()=>Promise<unknown>}) {
 const button=useRef<HTMLDivElement>(null)
 const [token,setToken]=useState('')
 const [models,setModels]=useState<Model[]>([])
 const [selected,setSelected]=useState('')
 const [busy,setBusy]=useState(false)
 const [message,setMessage]=useState('')
 const [catalog,setCatalog]=useState<CatalogProvider[]>([])
 const [catalogSearch,setCatalogSearch]=useState('')
 const [catalogId,setCatalogId]=useState('')
 const [catalogKey,setCatalogKey]=useState('')
 const [catalogModel,setCatalogModel]=useState('')
 const [catalogModels,setCatalogModels]=useState<RemoteModel[]>([])
 const [catalogProviderId,setCatalogProviderId]=useState('')
 const [catalogBusy,setCatalogBusy]=useState(false)
 const [catalogMessage,setCatalogMessage]=useState('')
 const [routerKey,setRouterKey]=useState('')
 const [routerBusy,setRouterBusy]=useState(false)
 const [routerMessage,setRouterMessage]=useState('')
 async function fetchModels(session:string){
  const response=await fetch(cloudUrl+'/v1/models',{headers:{Authorization:'Bearer '+session}})
  if(!response.ok)throw Error('모델 목록을 가져오지 못했습니다.')
  const data=await response.json() as {data:Model[]}
  setModels(data.data)
  setSelected(data.data[0]?.id||'')
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
 async function loadCatalog(){
  setCatalogBusy(true)
  try{
   const data=await get<{providers:CatalogProvider[];error?:string}>('/api/providers/catalog')
   setCatalog(data.providers||[])
   setCatalogMessage(data.error||'업체 '+(data.providers||[]).length+'개를 불러왔습니다.')
  }catch(e){setCatalogMessage((e as Error).message)}finally{setCatalogBusy(false)}
 }
 async function connectCatalog(){
  const entry=catalog.find(p=>p.id===catalogId)
  if(!entry)return
  setCatalogBusy(true);setCatalogMessage('')
  try{
   const baseUrl=entry.api.replace(/\/$/,'')
   const existing=await get<Array<{id:string;baseUrl:string;model?:string}>>('/api/providers')
   const same=existing.find(p=>p.baseUrl.replace(/\/$/,'')===baseUrl)
   const provider=same||await post<{id:string}>('/api/providers',{name:entry.name,baseUrl,kind:'openai',apiKey:catalogKey.trim()||undefined,enabled:true,local:false,toolMode:'auto'})
   setCatalogProviderId(provider.id)
   const models=await get<RemoteModel[]>('/api/providers/'+encodeURIComponent(provider.id)+'/models')
   setCatalogModels(models)
   setCatalogModel(models[0]?.id||'')
   setCatalogMessage(models.length+'개 모델 조회 완료. 원하는 모델을 선택해 등록하세요.')
   await onProviderAdded()
  }catch(e){setCatalogMessage((e as Error).message)}finally{setCatalogBusy(false)}
 }
 async function saveCatalogModel(){
  const entry=catalog.find(p=>p.id===catalogId)
  if(!entry||!catalogModel||!catalogProviderId)return
  setCatalogBusy(true)
  try{
   const baseUrl=entry.api.replace(/\/$/,'')
   const providers=await get<Array<{id:string;baseUrl:string;model?:string}>>('/api/providers')
   const same=providers.find(p=>p.baseUrl.replace(/\/$/,'')===baseUrl&&p.model===catalogModel)
   if(same){setCatalogMessage('이미 등록된 모델입니다.');return}
   const reusable=providers.find(p=>p.id===catalogProviderId&&!p.model)
   await post('/api/providers',{id:reusable?.id,name:entry.name+' · '+catalogModel,baseUrl,kind:'openai',apiKey:catalogKey.trim()||undefined,credentialSourceId:catalogKey.trim()?undefined:catalogProviderId,model:catalogModel,enabled:true,local:false,toolMode:'auto'})
   await onProviderAdded()
   setCatalogMessage('모델이 채팅에 등록됐습니다.')
  }catch(e){setCatalogMessage((e as Error).message)}finally{setCatalogBusy(false)}
 }
 async function importOpenRouterFree(){
  if(!routerKey.trim())return
  setRouterBusy(true);setRouterMessage('무료 모델 조회 중…')
  try{
   const res=await fetch('https://openrouter.ai/api/v1/models')
   if(!res.ok)throw Error('OpenRouter 모델 목록 조회 실패')
   const data=await res.json() as {data:Array<{id:string;name?:string;pricing?:{prompt?:string;completion?:string};architecture?:{output_modalities?:string[]}}>}
   const free=data.data.filter(m=>m.id.endsWith(':free')&&Number(m.pricing?.prompt)===0&&Number(m.pricing?.completion)===0&&(!m.architecture?.output_modalities||m.architecture.output_modalities.includes('text')))
   const existing=await get<Array<{baseUrl:string;model:string}>>('/api/providers')
   const known=new Set(existing.filter(p=>p.baseUrl==='https://openrouter.ai/api/v1').map(p=>p.model))
   let added=0
   for(const m of free){
    if(known.has(m.id))continue
    await post('/api/providers',{name:'OpenRouter 무료 · '+(m.name||m.id),baseUrl:'https://openrouter.ai/api/v1',kind:'openai',apiKey:routerKey.trim(),model:m.id,enabled:true,local:false,toolMode:'auto'})
    known.add(m.id);added++
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
    setMessage('이미 등록된 모델입니다. 중복 등록하지 않았습니다.')
    return
   }
   await post('/api/providers',{name:'DOMINION Cloud · '+selected,baseUrl:cloudUrl+'/v1',kind:'openai',apiKey:token,model:selected,enabled:true,local:false,toolMode:'compat'})
   await onProviderAdded()
   setMessage('모델이 등록됐습니다. 채팅에서 선택해 사용하세요.')
  }catch(error){setMessage((error as Error).message)}finally{setBusy(false)}
 }
 return <section className="rounded-xl border border-[#E7E0D5] bg-[#FFFDF9] p-4">
  <strong className="text-[13px]">DOMINION Cloud · 구글 로그인</strong>
  <p className="mt-1 text-[12px] text-[#7C7469]">개인 API 키 없이 도미니언 계정으로 무료 모델을 연결합니다.</p>
  {cloudUrl&&clientId?<div className="mt-3" ref={button}/>:<p className="mt-2 text-[12px] text-[#7C7469]">DOMINION Cloud 로그인 미설정 · 다른 AI 업체는 아래에서 연결할 수 있습니다.</p>}
  {token&&<div className="mt-3 flex flex-wrap items-center gap-2">
   <select aria-label="도미니언 무료 모델" value={selected} onChange={e=>setSelected(e.target.value)} className="rounded-lg border px-3 py-2 text-[12px]">
    {models.map(model=><option key={model.id} value={model.id}>{model.id} · 무료</option>)}
   </select>
   <button type="button" disabled={busy||!selected} onClick={()=>void register()} className="rounded-lg bg-[#C96442] px-3 py-2 text-[12px] font-semibold text-white disabled:opacity-50">모델 등록</button>
  </div>}
  {message&&<p role="status" className="mt-2 text-[12px] text-[#7C7469]">{message}</p>}
  <div className="mt-4 border-t border-[#E7E0D5] pt-4">
   <strong className="text-[13px]">AI 업체 통합 연결 · models.dev</strong>
   <p className="mt-1 text-[12px] text-[#7C7469]">업체 검색 → API 키 연결 → 실제 모델 조회 → 채팅 등록</p>
   <div className="mt-2 flex flex-wrap gap-2">
    <button type="button" disabled={catalogBusy} onClick={()=>void loadCatalog()} className="rounded-lg border px-3 py-2 text-[12px]">업체 목록 불러오기</button>
    <input aria-label="AI 업체 검색" value={catalogSearch} onChange={e=>setCatalogSearch(e.target.value)} placeholder="업체 검색" className="min-w-0 flex-1 rounded-lg border px-3 py-2 text-[12px]"/>
   </div>
   {catalog.length>0&&<div className="mt-2 flex flex-wrap gap-2">
    <select aria-label="AI 업체 선택" value={catalogId} onChange={e=>{setCatalogId(e.target.value);setCatalogModels([]);setCatalogProviderId('')}} className="min-w-0 flex-1 rounded-lg border px-3 py-2 text-[12px]">
     <option value="">업체를 선택하세요</option>
     {catalog.filter(p=>(p.name+' '+p.id).toLowerCase().includes(catalogSearch.toLowerCase())).map(p=><option key={p.id} value={p.id}>{p.name} ({p.models})</option>)}
    </select>
    <input type="password" autoComplete="off" aria-label="업체 API 키" value={catalogKey} onChange={e=>setCatalogKey(e.target.value)} placeholder="API 키" className="min-w-0 flex-1 rounded-lg border px-3 py-2 text-[12px]"/>
    <button type="button" disabled={catalogBusy||!catalogId} onClick={()=>void connectCatalog()} className="rounded-lg bg-[#C96442] px-3 py-2 text-[12px] font-semibold text-white disabled:opacity-50">연결 및 모델 조회</button>
   </div>}
   {catalogModels.length>0&&<div className="mt-2 flex flex-wrap gap-2">
    <select aria-label="연결된 업체 모델" value={catalogModel} onChange={e=>setCatalogModel(e.target.value)} className="min-w-0 flex-1 rounded-lg border px-3 py-2 text-[12px]">{catalogModels.map(m=><option key={m.id} value={m.id}>{m.name||m.id}</option>)}</select>
    <button type="button" disabled={catalogBusy||!catalogModel} onClick={()=>void saveCatalogModel()} className="rounded-lg bg-[#C96442] px-3 py-2 text-[12px] font-semibold text-white disabled:opacity-50">채팅에 등록</button>
   </div>}
   {catalogMessage&&<p role="status" className="mt-2 text-[12px] text-[#7C7469]">{catalogMessage}</p>}
  </div>
  <div className="mt-4 border-t border-[#E7E0D5] pt-4">
   <strong className="text-[13px]">OpenRouter 무료 모델 일괄 등록</strong>
   <p className="mt-1 text-[12px] text-[#7C7469]">OpenRouter API 키로 최신 무료 텍스트 모델을 가져옵니다.</p>
   <div className="mt-2 flex flex-wrap gap-2">
    <input type="password" autoComplete="off" aria-label="OpenRouter API 키" value={routerKey} onChange={e=>setRouterKey(e.target.value)} placeholder="OpenRouter API 키" className="min-w-0 flex-1 rounded-lg border px-3 py-2 text-[12px]"/>
    <button type="button" disabled={routerBusy||!routerKey.trim()} onClick={()=>void importOpenRouterFree()} className="rounded-lg bg-[#C96442] px-3 py-2 text-[12px] font-semibold text-white disabled:opacity-50">{routerBusy?'등록 중…':'무료 모델 전체 등록'}</button>
   </div>
   {routerMessage&&<p role="status" className="mt-2 text-[12px] text-[#7C7469]">{routerMessage}</p>}
  </div>
 </section>
}
