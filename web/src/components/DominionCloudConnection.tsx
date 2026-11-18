import {useEffect,useRef,useState} from 'react'
import {post} from '../api'

type GoogleResponse={credential:string}
type GoogleAccounts={id:{initialize:(options:{client_id:string;callback:(response:GoogleResponse)=>void})=>void;renderButton:(element:HTMLElement,options:Record<string,string>)=>void}}
declare global {interface Window {google?:{accounts:GoogleAccounts}}}
const gateway=(import.meta.env.VITE_DOMINION_CLOUD_URL as string|undefined)?.replace(/\/$/,'')||''
const clientId=(import.meta.env.VITE_DOMINION_GOOGLE_CLIENT_ID as string|undefined)||''
const model=(import.meta.env.VITE_DOMINION_CLOUD_MODEL as string|undefined)||''
type Login={token:string;account_id:string;expires_in:number}
type Me={account_id:string;credits_micro_krw:number}
export function DominionCloudConnection({onConnected}:{onConnected:()=>Promise<unknown>}){
 const button=useRef<HTMLDivElement>(null)
 const [session,setSession]=useState<Login|null>(null)
 const [balance,setBalance]=useState<number|null>(null)
 const [busy,setBusy]=useState(false)
 const [error,setError]=useState('')
 const ready=Boolean(gateway&&clientId&&model)
 async function cloudFetch<T>(path:string,init:RequestInit={}):Promise<T>{
  const response=await fetch(gateway+path,{...init,headers:{'content-type':'application/json',...(session?{authorization:'Bearer '+session.token}:{}),...init.headers},cache:'no-store'})
  const data=await response.json()
  if(!response.ok)throw new Error(data.error||'DOMINION Cloud 요청 실패')
  return data as T
 }
 useEffect(()=>{
  if(!ready||!button.current||session)return
  let active=true
  const script=document.createElement('script')
  script.src='https://accounts.google.com/gsi/client'
  script.async=true
  script.onload=()=>{
   if(!active||!button.current||!window.google)return
   window.google.accounts.id.initialize({client_id:clientId,callback:async ({credential})=>{
    setBusy(true);setError('')
    try{
     const response=await fetch(gateway+'/v1/auth/google',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({credential})})
     const data=await response.json()
     if(!response.ok)throw new Error(data.error||'로그인 실패')
     setSession(data as Login)
    }catch(e){setError((e as Error).message)}finally{setBusy(false)}
   }})
   window.google.accounts.id.renderButton(button.current,{theme:'outline',size:'large',text:'signin_with'})
  }
  script.onerror=()=>setError('구글 로그인 라이브러리를 불러오지 못했습니다.')
  document.head.appendChild(script)
  return()=>{active=false;script.remove()}
 },[ready,session])
 useEffect(()=>{
  if(!session)return
  let active=true
  fetch(gateway+'/v1/me',{headers:{authorization:'Bearer '+session.token},cache:'no-store'})
   .then(async response=>{if(!response.ok)throw new Error('계정 조회 실패');return response.json() as Promise<Me>})
   .then(data=>{if(active)setBalance(data.credits_micro_krw/1000)})
   .catch(e=>{if(active)setError(String(e))})
  return()=>{active=false}
 },[session])
 const connect=async()=>{
  if(!session||!ready||busy)return
  setBusy(true);setError('')
  try{
   // Existing Rust vault persists provider credentials; do not expose the upstream supplier key.
   await post('/api/providers',{name:'DOMINION Cloud',baseUrl:gateway+'/v1',kind:'openai',apiKey:session.token,local:false,enabled:true,model})
   await onConnected()
  }catch(e){setError((e as Error).message)}finally{setBusy(false)}
 }
 return <section className="mb-4 rounded-2xl border border-[#E7E0D5] bg-[#FFFDF9] p-4">
  <h2 className="text-[14px] font-bold text-[#24211D]">DOMINION Cloud · 공식 계정</h2>
  <p className="mt-1 text-[12px] text-[#7C7469]">구글 로그인으로 도미니언 계정을 연결합니다. 모델 공급자 API 키는 필요하지 않습니다.</p>
  {!ready?<p className="mt-2 text-[11px] text-[#A05B48]">클라우드 배포 설정이 아직 없습니다. VITE_DOMINION_CLOUD_URL, VITE_DOMINION_GOOGLE_CLIENT_ID, VITE_DOMINION_CLOUD_MODEL을 설정하세요.</p>
   :session?<div className="mt-3 flex flex-wrap items-center gap-3"><span className="text-[12px]">연결 계정: {session.account_id} · 잔액: {balance===null?'조회 중':balance.toLocaleString('ko-KR')+'원'}</span><button type="button" disabled={busy} onClick={()=>void connect()} className="rounded-lg bg-[#C96442] px-3 py-2 text-[12px] font-semibold text-white disabled:opacity-50">모델 공급자로 연결</button><button type="button" onClick={()=>{setSession(null);setBalance(null)}} className="text-[12px] text-[#7C7469]">로그아웃</button></div>
   :<div className="mt-3" ref={button}/>}
  {busy&&<p className="mt-2 text-[12px]">연결 중…</p>}
  {error&&<p role="alert" className="mt-2 text-[12px] text-red-600">{error}</p>}
  <p className="mt-2 text-[11px] text-[#9C9388]">현재 게이트웨이는 비스트리밍 텍스트 호출만 지원합니다. 도구 실행·실시간 채팅은 서버 확장 후 활성화됩니다.</p>
 </section>
}
