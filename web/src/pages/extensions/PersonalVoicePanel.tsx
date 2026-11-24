import {useEffect,useState} from 'react'
import {get,post,del} from '../../api'
import {Button,TextInput} from '../../components/Primitives'
import {Smartphone} from 'lucide-react'
type Connection={enabled:boolean;paired:boolean;address?:string;connectionCode?:string;remaining?:number;pairSeconds?:number;sessionSeconds?:number}
export function PersonalVoicePanel(){
 const [session,setSession]=useState<Connection|null>(null),[busy,setBusy]=useState(false),[err,setErr]=useState(''),[address,setAddress]=useState(''),[advanced,setAdvanced]=useState(false)
 useEffect(()=>{let active=true;const load=()=>void get<Connection>('/api/plugins/voice/personal').then(r=>{if(active)setSession(r)}).catch(()=>{});load();const timer=setInterval(load,3000);return()=>{active=false;clearInterval(timer)}},[])
 const toggle=async()=>{if(busy)return;setBusy(true);setErr('');try{
  if(session?.enabled){await del('/api/plugins/voice/personal');setSession({enabled:false,paired:false})}
  else setSession(await post<Connection>('/api/plugins/voice/personal',{confirmed:true,address:address.trim()||null}))
 }catch(e){setErr((e as Error).message)}finally{setBusy(false)}}
 return <div className="mb-5 space-y-3 rounded-xl border border-divider bg-canvas p-4">
  <div className="flex flex-wrap items-center justify-between gap-2"><div><h3 className="flex items-center gap-2 font-bold"><Smartphone size={20}/> DOMINION Voice · 개인용 간편 연결</h3><p className="mt-1 text-xs text-muted">같은 Wi-Fi에서 휴대폰 자체 음성 인식·읽어주기와 기존 도미니온 모델을 사용합니다. Docker 및 별도 음성 API 키 불필요.</p></div><Button disabled={busy||session===null} onClick={()=>void toggle()}>{session?.enabled?'연결 종료':'휴대폰 연결 켜기'}</Button></div>
  {err&&<p role="alert" className="text-xs text-danger">{err}</p>}
  {session?.enabled&&<div className="space-y-2 text-sm">
   {session.paired?<p className="text-success">휴대폰 연결됨 · 남은 대화 {session.remaining}회</p>:session.connectionCode?<><p>휴대폰 앱의 간편 연결에 아래 코드 입력</p><div className="flex flex-wrap items-center gap-2"><code className="select-all break-all rounded bg-surface p-2 font-mono text-base">{session.connectionCode}</code><Button size="sm" variant="secondary" onClick={()=>{void navigator.clipboard.writeText(session.connectionCode||'').catch(()=>setErr('코드를 직접 선택해서 복사하세요.'))}}>복사</Button></div><p className="text-xs text-muted">5분 안에 연결하세요. 연결 후 최대 2시간·100회 대화 가능.</p></>:<p className="text-warning">코드가 만료되었거나 연결이 잠겼습니다. 종료 후 다시 켜세요.</p>}
  </div>}
  {!session?.enabled&&<><button className="text-xs text-muted underline" onClick={()=>setAdvanced(!advanced)}>PC 주소 수동 설정 (자동 감지 실패 시)</button>{advanced&&<TextInput value={address} onChange={e=>setAddress(e.target.value)} placeholder="192.168.0.10"/>}</>}
  <p className="text-xs text-muted">개인 Wi-Fi에서만 사용하세요. 로컬 통신은 HTTP이며 외부에 관리자 API를 공개하지 않습니다. 음성 인식은 휴대폰의 시스템 서비스 정책을 따릅니다.</p>
 </div>
}
