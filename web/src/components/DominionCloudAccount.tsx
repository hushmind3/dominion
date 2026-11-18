import {useEffect,useRef,useState} from 'react'
import {get,post} from '../api'

type GoogleResponse={credential:string}
type GoogleIdentity={accounts:{id:{initialize:(config:{client_id:string;callback:(response:GoogleResponse)=>void})=>void;renderButton:(element:HTMLElement,options:Record<string,unknown>)=>void}}}
declare global {interface Window {google?:GoogleIdentity}}
type Model={id:string;free:boolean}
const cloudUrl=(import.meta.env.VITE_DOMINION_CLOUD_URL||'').replace(/\/$/,'')
const clientId=import.meta.env.VITE_DOMINION_GOOGLE_CLIENT_ID||''
export function DominionCloudAccount({onProviderAdded}:{onProviderAdded:()=>Promise<unknown>}) {
 const button=useRef<HTMLDivElement>(null)
 const [token,setToken]=useState('')
 const [models,setModels]=useState<Model[]>([])
 const [selected,setSelected]=useState('')
 const [busy,setBusy]=useState(false)
 const [message,setMessage]=useState('')
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
 async function register(){
  if(!token||!selected)return
  setBusy(true);setMessage('')
  try{
   const existing=await get<Array<{id:string;baseUrl:string;model:string}>>('/api/providers')
   const match=existing.find(p=>p.baseUrl.replace(/\\/$/,'')===cloudUrl+'/v1'&&p.model===selected)
   if(match){
    setMessage('이미 등록된 모델입니다. 중복 등록하지 않았습니다.')
    return
   }
   await post('/api/providers',{name:'DOMINION Cloud · '+selected,baseUrl:cloudUrl+'/v1',kind:'openai',apiKey:token,model:selected,enabled:true,local:false,toolMode:'compat'})
   await onProviderAdded()
   setMessage('모델이 등록됐습니다. 채팅에서 선택해 사용하세요.')
  }catch(error){setMessage((error as Error).message)}finally{setBusy(false)}
 }
 if(!cloudUrl||!clientId)return <section className="rounded-xl border border-[#E7E0D5] bg-[#FFFDF9] p-4">
  <strong className="text-[13px]">DOMINION Cloud</strong>
  <p className="mt-1 text-[12px] text-[#7C7469]">구글 로그인 연결 대기 · 클라우드 주소와 구글 클라이언트 ID 설정 후 활성화됩니다.</p>
 </section>
 return <section className="rounded-xl border border-[#E7E0D5] bg-[#FFFDF9] p-4">
  <strong className="text-[13px]">DOMINION Cloud · 구글 로그인</strong>
  <p className="mt-1 text-[12px] text-[#7C7469]">개인 API 키 없이 도미니언 계정으로 무료 모델을 연결합니다.</p>
  <div className="mt-3" ref={button}/>
  {token&&<div className="mt-3 flex flex-wrap items-center gap-2">
   <select aria-label="도미니언 무료 모델" value={selected} onChange={e=>setSelected(e.target.value)} className="rounded-lg border px-3 py-2 text-[12px]">
    {models.map(model=><option key={model.id} value={model.id}>{model.id} · 무료</option>)}
   </select>
   <button type="button" disabled={busy||!selected} onClick={()=>void register()} className="rounded-lg bg-[#C96442] px-3 py-2 text-[12px] font-semibold text-white disabled:opacity-50">모델 등록</button>
  </div>}
  {message&&<p role="status" className="mt-2 text-[12px] text-[#7C7469]">{message}</p>}
 </section>
}
