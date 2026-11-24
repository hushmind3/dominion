import {useEffect,useState} from 'react'
import {Link2,Plus,Copy,RefreshCw,KeyRound} from 'lucide-react'
import {Button,SelectInput,TextInput} from '../../components/Primitives'
import {Notice,StateTag,useRemote} from '../../components/UI'
import {post} from '../../api'
import {useLocale} from '../../i18n'
import {sdkPluginPresence} from '../../utils/sdkPluginPresence'

type Plugin={id:string;name:string;agentId:string;status:'enabled'|'disabled';lastSeenAt?:string|null;registeredAt:string;maxRounds:number;tokenBudget:number;dailyLimit:number}
type Receipt={plugin:Plugin;token:string}
const endpoint='/api/sdk/plugins'
export function SdkPluginRegistry({onSelect}:{onSelect:(id:string)=>void}){
 const en=useLocale()==='en'
 const tr=(ko:string,eng:string)=>en?eng:ko
 const plugins=useRemote<Plugin[]>(endpoint,[])
 const agents=useRemote<{id:string;name:string}[]>('/api/agents',[])
 const [now,setNow]=useState(Date.now())
 const [show,setShow]=useState(false)
 const [id,setId]=useState('')
 const [name,setName]=useState('')
 const [agent,setAgent]=useState('')
 const [secret,setSecret]=useState<Receipt|null>(null)
 const [busy,setBusy]=useState(false)
 const [error,setError]=useState('')
 const [message,setMessage]=useState('')
 useEffect(()=>{const timer=window.setInterval(()=>setNow(Date.now()),15000);return()=>window.clearInterval(timer)},[])
 const act=async(action:()=>Promise<unknown>)=>{
  if(busy)return
  setBusy(true);setError('');setMessage('');setSecret(null)
  try{await action();await plugins.reload();setNow(Date.now())}
  catch(e){setError((e as Error).message);await plugins.reload()}
  finally{setBusy(false)}
 }
 const enroll=()=>void act(async()=>{
  const receipt=await post<Receipt>(endpoint,{pluginId:id.trim(),name:name.trim(),agentId:agent})
  setSecret(receipt);setId('');setName('');setShow(false)
 })
 const copy=async(value:string)=>{
  try{await navigator.clipboard.writeText(value);setMessage(tr('복사했습니다.','Copied.'));setError('')}
  catch(e){setError((e as Error).message)}
 }
 const apiBase=window.location.origin
 const valid=/^[A-Za-z0-9_.-]{1,96}$/.test(id.trim())&&id.trim()!=='.'&&id.trim()!=='..'
 return <section className="mt-3 rounded-xl border border-[#E7E0D5] bg-white p-3" aria-label={tr('등록된 SDK 플러그인','Registered SDK plugins')}>
  <div className="flex flex-wrap items-center justify-between gap-2">
   <strong className="flex items-center gap-1.5 text-sm"><Link2 size={15}/>{tr('등록된 외부 SDK 플러그인','Registered external SDK plugins')}</strong>
   <div className="flex gap-1.5">
    <Button size="sm" onClick={()=>void plugins.reload()}><RefreshCw size={13}/>{tr('새로고침','Refresh')}</Button>
    <Button size="sm" variant="primary" disabled={busy} onClick={()=>{setShow(!show);setSecret(null)}}><Plus size={13}/>{tr('플러그인 등록','Register plugin')}</Button>
   </div>
  </div>
  <p className="mt-1 text-[11px] leading-5 text-[#7C7469]">{tr('전용 키로 인증한 프로그램만 최근 연결로 표시됩니다. 등록은 외부 프로그램 설치가 아니며, 선택한 에이전트로 작업을 제출할 권한을 부여합니다.','Only authenticated programs show recent activity. Registration does not install software; it authorizes task submission to the selected agent.')}</p>
  {plugins.error&&<Notice message={plugins.error}/>}
  {error&&<Notice message={error}/>}
  {message&&<p role="status" className="mt-2 text-xs">{message}</p>}
  {show&&<form className="my-3 grid gap-2 rounded-lg border border-[#E7E0D5] p-3" onSubmit={e=>{e.preventDefault();if(valid&&name.trim()&&agent&&!busy)enroll()}}>
   <label className="text-xs">{tr('플러그인 ID','Plugin ID')}<TextInput value={id} maxLength={96} onChange={e=>setId(e.target.value)} required placeholder="my-plugin"/></label>
   <label className="text-xs">{tr('표시 이름','Display name')}<TextInput value={name} maxLength={200} onChange={e=>setName(e.target.value)} required/></label>
   <label className="text-xs">{tr('실행 에이전트','Execution agent')}<SelectInput value={agent} onChange={e=>setAgent(e.target.value)} required><option value="">{tr('에이전트 선택','Select agent')}</option>{agents.value.map(a=><option key={a.id} value={a.id}>{a.name||a.id}</option>)}</SelectInput></label>
   {agents.error&&<Notice message={agents.error}/>}
   <p className="text-[11px] leading-5 text-[#7C7469]">{tr('동시 작업 1개 · 최근 24시간 20개 · 작업당 최대 8라운드/20,000토큰. 기존 도구 권한과 승인이 적용됩니다.','One active task, 20 per rolling day, up to 8 rounds/20,000 tokens per task. Existing tool permissions and approvals apply.')}</p>
   <Button type="submit" size="sm" variant="primary" disabled={busy||!valid||!name.trim()||!agent}>{tr('등록 및 키 발급','Register and issue key')}</Button>
  </form>}
  {secret&&<div className="my-3 rounded-lg border border-amber-200 bg-amber-50 p-3">
   <strong className="text-xs">{tr('연결 키 (이번에만 표시)','Connection key (shown once)')}</strong>
   <p className="mt-1 break-all text-xs">ID: {secret.plugin.id}</p>
   <code className="my-2 block break-all text-xs">{secret.token}</code>
   <div className="flex flex-wrap gap-2">
    <Button size="sm" onClick={()=>void copy(secret.token)}><Copy size={13}/>{tr('키 복사','Copy key')}</Button>
    <Button size="sm" onClick={()=>void copy(JSON.stringify({baseUrl:apiBase,pluginId:secret.plugin.id,pluginToken:secret.token},null,2))}><Copy size={13}/>{tr('SDK 설정 복사','Copy SDK config')}</Button>
    <Button size="sm" onClick={()=>setSecret(null)}>{tr('닫기','Close')}</Button>
   </div>
  </div>}
  {!plugins.loading&&!plugins.error&&!plugins.value.length&&<p className="mt-2 rounded-lg border border-dashed border-[#E7E0D5] p-3 text-center text-xs text-[#7C7469]">{tr('등록된 플러그인이 없습니다. 외부 프로그램을 연결하려면 먼저 등록하세요.','No plugins registered. Register one to connect an external program.')}</p>}
  <div className="mt-2 grid gap-2">
   {plugins.value.map(p=>{
    const presence=plugins.error?'unknown':sdkPluginPresence(p,now)
    const label=presence==='recent'?tr('최근 인증됨','Recently authenticated'):presence==='disabled'?tr('비활성화','Disabled'):presence==='never'?tr('연결 대기','Not yet connected'):presence==='unknown'?tr('상태 조회 실패','Unknown'):tr('최근 연결 없음','No recent activity')
    return <div key={p.id} className="rounded-lg border border-[#E7E0D5] p-3">
     <div className="flex flex-wrap items-center justify-between gap-2"><strong className="break-all text-xs">{p.name}</strong><StateTag online={presence==='recent'}>{label}</StateTag></div>
     <p className="mt-1 break-all text-[11px] text-[#7C7469]">{p.id} · {agents.value.find(a=>a.id===p.agentId)?.name||p.agentId}</p>
     <p className="mt-1 text-[11px] text-[#7C7469]">{tr('마지막 인증','Last authenticated')}: {p.lastSeenAt?new Date(p.lastSeenAt).toLocaleString(en?'en-US':'ko-KR'):'—'}</p>
     <div className="mt-2 flex flex-wrap gap-1.5">
      <Button size="sm" onClick={()=>onSelect(p.id)}>{tr('작업 보기','View runs')}</Button>
      <Button size="sm" disabled={busy} onClick={()=>void copy(apiBase+endpoint+'/'+encodeURIComponent(p.id)+'/heartbeat')}><Copy size={13}/>{tr('연결 확인 주소 복사','Copy heartbeat URL')}</Button>
      <Button size="sm" disabled={busy} onClick={()=>{if(window.confirm(tr('기존 키를 폐기하고 새 키를 발급하시겠습니까?','Revoke the old key and issue a new one?')))void act(async()=>setSecret(await post<Receipt>(endpoint+'/'+encodeURIComponent(p.id)+'/rotate',{})))}}><KeyRound size={13}/>{tr('키 재발급','Rotate key')}</Button>
      <Button size="sm" variant={p.status==='enabled'?'danger':'secondary'} disabled={busy} onClick={()=>{const enabled=p.status!=='enabled';if(enabled||window.confirm(tr('신규 요청과 인증을 차단합니다. 이미 접수된 작업은 실행 기록에서 별도로 중단하세요.','Block new requests and authentication. Cancel already-admitted runs separately in run history.')))void act(()=>post(endpoint+'/'+encodeURIComponent(p.id)+'/state',{enabled}))}}>{p.status==='enabled'?tr('연결 해제','Disable'):tr('다시 활성화','Enable')}</Button>
     </div>
    </div>
   })}
  </div>
  <details className="mt-3 text-xs"><summary className="cursor-pointer font-semibold">{tr('외부 프로그램 연결 코드','External client code')}</summary>
   <p className="mt-2 text-[11px] leading-5">{tr('전용 키를 환경 변수 등 안전한 위치에 보관하세요. 60초마다 heartbeat()를 호출하면 최근 인증 상태를 갱신합니다. 작업 제출은 같은 요청에 동일한 중복 방지 키를 사용하세요.','Store the key securely. Call heartbeat() every 60 seconds to refresh presence. Reuse the same idempotency key for retries of the same task.')}</p>
   <pre className="mt-2 overflow-x-auto whitespace-pre-wrap rounded-lg bg-[#F3EEE5] p-2 text-[11px]">{'const client = new DominionPluginClient({\n  baseUrl: "'+apiBase+'", pluginId: "my-plugin",\n  pluginToken: process.env.DOMINION_PLUGIN_TOKEN!\n});\nawait client.heartbeat();\nawait client.startRun({instruction: "Check project",\n  idempotencyKey: "task-1"});'}</pre>
  </details>
 </section>
}
