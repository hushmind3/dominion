import {useEffect,useState} from 'react'
import {Button,TextInput} from './Primitives'
type Link={id:string;app_name:string;capability?:string;status?:string;expires_at:number}
export function CloudAppConnections({cloudUrl,token}:{cloudUrl:string;token:string}){
 const [code,setCode]=useState(''),[preview,setPreview]=useState<Link|null>(null),[items,setItems]=useState<Link[]>([])
 const [busy,setBusy]=useState(false),[message,setMessage]=useState(''),[supported,setSupported]=useState<boolean|null>(null)
 const call=async<T,>(path:string,body?:unknown):Promise<T>=>{
  const r=await fetch(cloudUrl+'/v1/apps'+path,{redirect:'error',headers:{Authorization:'Bearer '+token,...(body===undefined?{}:{'content-type':'application/json'})},...(body===undefined?{}:{method:'POST',body:JSON.stringify(body)})})
  if(r.status===404&&path===''){setSupported(false);throw Error('현재 Cloud 서버에 앱 연결 기능이 배포되지 않았습니다.')}
  const data=await r.json();if(!r.ok)throw Error(data.error||'Cloud HTTP '+r.status)
  setSupported(true);return data as T
 }
 const reload=async()=>{const data=await call<{items:Link[]}>('');setItems(data.items||[])}
 useEffect(()=>{void reload().catch(e=>setMessage(e.message))},[cloudUrl,token])
 const run=async(action:()=>Promise<void>)=>{if(busy)return;setBusy(true);setMessage('');try{await action()}catch(e){setMessage((e as Error).message)}finally{setBusy(false)}}
 return <section className="mt-3 grid gap-2 rounded-xl border border-[#E7E0D5] bg-white p-3 text-xs" aria-label="Cloud 외부 앱 연결">
  <strong>외부 앱 연결 · DOMINION Assist</strong>
  <p className="text-[#7C7469]">Assist 또는 다른 앱에서 받은 일회용 연결 코드를 입력하세요. 동일 계정의 무료 AI 이용 한도를 공유합니다. 결제 및 계정 관리 권한은 전달되지 않습니다.</p>
  <form className="flex flex-wrap items-center gap-2" onSubmit={e=>{e.preventDefault();void run(async()=>setPreview(await call<Link>('/inspect',{id:code.trim()})))}}>
   <TextInput aria-label="외부 앱 연결 코드" className="min-w-0 flex-1" value={code} maxLength={43} onChange={e=>{setCode(e.target.value);setPreview(null)}} placeholder="연결 코드"/>
   <Button type="submit" size="sm" disabled={busy||supported===false||!/^[A-Za-z0-9_-]{43}$/.test(code.trim())}>연결 확인</Button>
  </form>
  {preview&&<div className="rounded-lg border p-2"><p>연결 요청 앱: <strong>{preview.app_name}</strong></p><p>본인이 실행한 앱만 승인하세요. {preview.capability==='assistant_remote'?'승인 시 등록된 PC의 비서 대화·작업·전화 기능에 접근할 수 있습니다.':preview.capability==='assistant_pc'?'승인 시 이 PC를 비서 실행 기기로 등록할 수 있습니다.':'승인된 앱은 7일 동안 무료 AI 모델을 호출할 수 있습니다.'}</p><div className="mt-2 flex gap-2"><Button size="sm" variant="primary" disabled={busy} onClick={()=>void run(async()=>{await call('/approve',{id:preview.id});setPreview(null);setCode('');await reload();setMessage('앱 연결을 승인했습니다.')})}>연결 승인</Button><Button size="sm" disabled={busy} onClick={()=>setPreview(null)}>취소</Button></div></div>}
  <div className="flex justify-between"><strong>연결된 앱</strong><Button size="sm" disabled={busy} onClick={()=>void run(reload)}>새로고침</Button></div>
  {items.map(app=><div key={app.id} className="flex flex-wrap items-center justify-between gap-2 border-t pt-2"><div><strong className="break-all">{app.app_name}</strong><p>{app.status==='revoked'?'연결 해제':app.expires_at<=Date.now()/1000?'만료':'연결됨'} · {new Date(app.expires_at*1000).toLocaleString('ko-KR')}</p></div>{app.status==='approved'&&<Button size="sm" variant="danger" disabled={busy} onClick={()=>{if(window.confirm('이 앱의 Cloud 연결을 해제하시겠습니까?'))void run(async()=>{await call('/revoke',{id:app.id});await reload()})}}>연결 해제</Button>}</div>)}
  {supported===true&&!items.length&&<p>연결된 앱 없음</p>}{message&&<p role="status" className="break-words">{message}</p>}
 </section>
}
