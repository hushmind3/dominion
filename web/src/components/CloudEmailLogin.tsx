import {useEffect,useRef,useState} from 'react'

type Flight={id:string;pollToken:string;expiresAt:number}
type StartReply={id?:string;poll_token?:string;expires_in?:number;error?:string}
type PollReply={status?:'waiting'|'done';token?:string;error?:string}
const storageKey='dominion_cloud_email_flight'
const safeToken=(text:unknown):text is string=>typeof text==='string'&&/^[A-Za-z0-9_-]{43}$/.test(text)
function savedFlight():Flight|null {
 try {
  const raw=sessionStorage.getItem(storageKey)
  if(!raw)return null
  const value=JSON.parse(raw) as Partial<Flight>
  if(!safeToken(value.id)||!safeToken(value.pollToken)||!Number.isSafeInteger(value.expiresAt)||Number(value.expiresAt)<=Date.now()){
   sessionStorage.removeItem(storageKey)
   return null
  }
  return value as Flight
 }catch{return null}
}
export function CloudEmailLogin({cloudUrl,onSuccess}:{cloudUrl:string;onSuccess:(token:string)=>Promise<void>}) {
 const [email,setEmail]=useState('')
 const [flight,setFlight]=useState<Flight|null>(savedFlight)
 const [busy,setBusy]=useState(false)
 const [message,setMessage]=useState(flight?'인증 메일에서 확인을 마치면 자동 로그인됩니다.':'')
 const claiming=useRef(false)
 async function requestEmail(){
  if(busy||flight)return
  setBusy(true);setMessage('')
  try{
   const response=await fetch(cloudUrl+'/v1/auth/email/start',{
    method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({email:email.trim()})
   })
   const data=await response.json().catch(()=>({})) as StartReply
   if(!response.ok)throw Error(data.error||'인증 메일을 발송하지 못했습니다.')
   if(!safeToken(data.id)||!safeToken(data.poll_token))throw Error('로그인 서버 응답이 올바르지 않습니다.')
   const next={id:data.id,pollToken:data.poll_token,expiresAt:Date.now()+Math.min(Math.max(Number(data.expires_in)||600,30),600)*1000}
   sessionStorage.setItem(storageKey,JSON.stringify(next))
   setFlight(next);setMessage('인증 메일을 보냈습니다. 받은편지함에서 확인 링크를 누르고 돌아오세요.')
  }catch(e){setMessage((e as Error).message)}finally{setBusy(false)}
 }
 function reset(){
  sessionStorage.removeItem(storageKey)
  setFlight(null);claiming.current=false;setMessage('')
 }
 useEffect(()=>{
  if(!flight)return
  let active=true
  let timer:ReturnType<typeof setTimeout>|undefined
  const poll=async()=>{
   if(!active||claiming.current)return
   if(Date.now()>flight.expiresAt){reset();setMessage('인증 시간이 만료됐습니다. 메일을 다시 요청하세요.');return}
   try{
    const response=await fetch(cloudUrl+'/v1/auth/email/status',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({id:flight.id,poll_token:flight.pollToken})})
    const data=await response.json().catch(()=>({})) as PollReply
    if(!active)return
    if(!response.ok)throw Error(data.error||'인증 진행 상태를 가져오지 못했습니다.')
    if(data.status==='done'&&data.token){
     claiming.current=true
     // The parent persists the session before clearing the login flight.
     await onSuccess(data.token)
     if(active)reset()
     return
    }
   }catch(error){
    if(!active)return
    // A temporary network error may occur while the user is switching tabs.
    // Retain the secret poll credential and retry until the expiry deadline.
    setMessage('인증 상태를 확인 중입니다. '+(error as Error).message)
   }
   if(active)timer=setTimeout(()=>void poll(),2500)
  }
  void poll()
  return()=>{active=false;if(timer)clearTimeout(timer)}
 },[flight,cloudUrl,onSuccess])
 return <div className="w-full space-y-2 border-t border-divider pt-3">
  <label htmlFor="dominion-email-login" className="block text-[12px] font-semibold">이메일로 로그인</label>
  <form onSubmit={event=>{event.preventDefault();void requestEmail()}} className="flex gap-2">
   <input id="dominion-email-login" type="email" autoComplete="email" required disabled={busy||Boolean(flight)}
    value={email} onChange={event=>setEmail(event.target.value)} placeholder="이메일 주소"
    className="min-w-0 flex-1 rounded-lg border border-divider px-3 py-2 text-[12px]"/>
   <button type="submit" disabled={busy||Boolean(flight)} className="rounded-lg bg-brand px-3 py-2 text-[12px] font-semibold text-white disabled:opacity-60">
    {busy?'발송 중…':flight?'확인 대기 중':'인증 메일 보내기'}
   </button>
  </form>
  {flight&&<button type="button" className="text-[12px] underline" onClick={reset}>다른 이메일로 다시 시도</button>}
  {message&&<p role="status" className="text-[12px] text-muted">{message}</p>}
  <p className="text-[11px] text-muted">이메일의 확인 버튼을 누르면 이 화면에서 자동 로그인됩니다. 링크는 10분간 유효합니다.</p>
 </div>
}
