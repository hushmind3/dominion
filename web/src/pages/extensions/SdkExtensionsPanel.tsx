import {useEffect,useRef,useState} from 'react'
import {del,post} from '../../api'
import {useProviderModels} from '../../hooks/useProviderModels'
import {useLocale} from '../../i18n'
import {Action,FormField,Notice,Panel,StateTag,Subheading,useRemote,refresh} from '../../components/UI'
import {Button,TextInput,TextArea,SelectInput} from '../../components/Primitives'
type Manifest={schemaVersion:1;id:string;name:string;version:string;description?:string}
type App={id:string;manifest:Manifest;enabled:boolean}
const endpoint='/api/extensions/apps'
const empty:Manifest={schemaVersion:1,id:'',name:'',version:'1.0.0',description:''}
export function SdkExtensionsPanel(){
 const en=useLocale()==='en'
 const apps=useRemote<App[]>(endpoint,[])
 const [draft,setDraft]=useState<Manifest>(empty),[open,setOpen]=useState(false)
 const [busy,setBusy]=useState(false),[error,setError]=useState('')
 const [selectedRuns,setSelectedRuns]=useState<Record<string,string>>({})
 const inFlight=useRef(false)
 const change=async(fn:()=>Promise<unknown>)=>{
  if(inFlight.current)return false
  inFlight.current=true;setBusy(true);setError('')
  try{await fn();await apps.reload();refresh(endpoint);return true}
  catch(e){setError((e as Error).message);return false}
  finally{inFlight.current=false;setBusy(false)}
 }
 const save=async()=>{
  if(await change(()=>post(endpoint,draft))){setOpen(false);setDraft(empty)}
 }
 const remove=(app:App)=>{
  if(app.enabled||!confirm(en?'Unregister this app? Its prior runs are retained.':'등록을 해제할까요? 기존 작업 기록은 유지됩니다.'))return
  void change(()=>del(endpoint+'/'+encodeURIComponent(app.id)))
 }
 return <section className="mb-6" aria-label={en?'External SDK apps':'외부 SDK 앱'}>
  <Subheading title={en?'External SDK apps':'외부 SDK 앱'} extra={<Action onClick={()=>setOpen(v=>!v)} disabled={busy}>{en?'Register app':'앱 등록'}</Action>}/>
  <p className="mb-3 text-xs leading-5 text-muted">{en?'Register trusted external applications that use the Dominion SDK. Registration never downloads, launches or grants access to code. Disabling blocks new managed requests; existing work stays recoverable.':'별도로 설치한 외부 프로그램을 SDK에 연결합니다. 등록만으로 코드를 다운로드하거나 실행하지 않습니다. 비활성화하면 새 SDK 작업 요청을 차단하며 기존 기록은 유지합니다.'}</p>
  {!apps.loading&&!apps.error&&<div className="mb-3 flex flex-wrap gap-2 text-xs"><StateTag online={apps.value.some(app=>app.enabled)}>{en?'Enabled':'활성 앱'} {apps.value.filter(app=>app.enabled).length}</StateTag><StateTag>{en?'Registered':'등록 앱'} {apps.value.length}</StateTag></div>}
  <Notice message={error||apps.error} onRetry={apps.error?()=>void apps.reload():undefined}/>
  {open&&<Panel className="mb-3 grid gap-3 p-4">
   <FormField label={en?'App ID':'앱 ID'} hint="example.assistant"><TextInput aria-label={en?'App ID':'앱 ID'} value={draft.id} onChange={e=>setDraft({...draft,id:e.target.value})} disabled={busy}/></FormField>
   <FormField label={en?'Name':'앱 이름'}><TextInput aria-label={en?'App name':'앱 이름'} value={draft.name} onChange={e=>setDraft({...draft,name:e.target.value})} disabled={busy}/></FormField>
   <FormField label={en?'Version':'버전'}><TextInput value={draft.version} onChange={e=>setDraft({...draft,version:e.target.value})} disabled={busy}/></FormField>
   <FormField label={en?'Description':'설명'}><TextArea value={draft.description} onChange={e=>setDraft({...draft,description:e.target.value})} disabled={busy}/></FormField>
   <div className="flex gap-2"><Action disabled={busy||!draft.id||!draft.name||!draft.version} onClick={()=>void save()}>{en?'Save registration':'등록 저장'}</Action><Button variant="ghost" onClick={()=>setOpen(false)} disabled={busy}>{en?'Cancel':'취소'}</Button></div>
  </Panel>}
  {apps.loading?<p role="status" className="text-xs text-muted">{en?'Loading…':'불러오는 중…'}</p>:!apps.error&&!apps.value.length?<p className="text-xs text-muted">{en?'No registered apps.':'등록된 외부 SDK 앱이 없습니다.'}</p>:null}
  <div className="grid gap-2">{apps.value.map(app=><Panel key={app.id} className="flex flex-wrap items-center gap-3 p-4">
    <div className="min-w-0 flex-1"><strong>{app.manifest.name}</strong><p className="break-all text-xs text-muted">{app.id} · {app.manifest.version}</p><p className="text-xs text-muted">{app.manifest.description}</p><p className="text-xs">{app.enabled?(en?'Enabled for new SDK requests':'SDK 요청 허용'):(en?'Disabled':'비활성')}</p></div>
    <Button disabled={busy||Boolean(apps.error)} onClick={()=>void change(()=>post(endpoint+'/'+encodeURIComponent(app.id)+'/state',{enabled:!app.enabled}))}>{app.enabled?(en?'Disable':'비활성화'):(en?'Enable':'활성화')}</Button>
    <Button variant="ghost" disabled={busy||app.enabled||Boolean(apps.error)} onClick={()=>remove(app)}>{en?'Unregister':'등록 해제'}</Button>
    <GrantPanel appId={app.id} enabled={app.enabled} onRun={id=>setSelectedRuns(previous=>({...previous,[app.id]:id}))}/>
    <AppRunHistory appId={app.id} selected={selectedRuns[app.id]||''} onSelect={id=>setSelectedRuns(previous=>({...previous,[app.id]:id}))}/>
  </Panel>)}</div>
 </section>
}

type Grant={providerId:string;model:string;tokenBudget:number;remainingRuns:number;capability:string;generation:string}
type Provider={id:string;name:string;kind?:string;enabled?:boolean;model?:string}
function GrantPanel({appId,enabled,onRun}:{appId:string;enabled:boolean;onRun:(id:string)=>void}){
 const en=useLocale()==='en',path=endpoint+'/'+encodeURIComponent(appId)+'/grant'
 const [opened,setOpened]=useState(false)
 const grant=useRemote<Grant|null>(opened?path:null,null)
 const providers=useRemote<Provider[]>(opened?'/api/providers':null,[])
 const [providerId,setProviderId]=useState(''),[model,setModel]=useState('')
 const [budget,setBudget]=useState('8192'),[maxRuns,setMaxRuns]=useState('10')
 const [token,setToken]=useState(''),[busy,setBusy]=useState(false),[error,setError]=useState('')
 const inFlight=useRef(false)
 const catalogue=useProviderModels(opened?providerId:'')
 const available=providers.value.filter(p=>p.enabled!==false&&!['codex','cli'].includes(p.kind||''))
 const provider=available.find(p=>p.id===providerId)
 const models=[...new Set([...(provider?.model?[provider.model]:[]),...catalogue.value.map(m=>m.id)])]
 const valid=Boolean(provider&&model&&models.includes(model)&&Number.isInteger(Number(budget))&&Number(budget)>=1024&&Number(budget)<=32768&&Number.isInteger(Number(maxRuns))&&Number(maxRuns)>=1&&Number(maxRuns)<=100)
 const run=async(fn:()=>Promise<void>)=>{
  if(inFlight.current)return
  inFlight.current=true;setBusy(true);setError('');setToken('')
  try{await fn();await grant.reload()}
  catch(e){setError((e as Error).message)}
  finally{inFlight.current=false;setBusy(false)}
 }
 return <div className="w-full border-t border-divider pt-2">
  <Button variant="ghost" disabled={busy} onClick={()=>{setOpened(v=>!v);setToken('')}}>{opened?(en?'Close credential settings':'인증 설정 닫기'):(en?'Scoped credentials':'앱별 인증키 관리')}</Button>
  {opened&&<div className="mt-2 grid gap-2">
   <p className="text-xs text-muted">{en?'Isolated chat only: no tools, owner memory, other app history or owner API access. Existing owner SDK tasks remain separate.':'제한형 인증키는 지정 모델의 독립 대화만 허용합니다. 도구·사용자 기억·다른 앱 기록·본체 관리 접근은 허용하지 않습니다.'}</p>
   <Notice message={error||grant.error||providers.error}/>
   {grant.loading?<p className="text-xs text-muted">{en?'Loading…':'불러오는 중…'}</p>:grant.value?<p className="text-xs">{grant.value.model} · {en?'Remaining requests':'남은 요청'} {grant.value.remainingRuns} · {grant.value.tokenBudget} tokens</p>:<p className="text-xs text-muted">{en?'No credential issued':'발급된 인증키 없음'}</p>}
   <FormField label={en?'Provider':'허용 제공자'}><SelectInput aria-label={en?'Allowed provider':'허용 제공자'} value={providerId} disabled={busy} onChange={e=>{setProviderId(e.target.value);setModel('')}}><option value="">{en?'Select provider':'제공자 선택'}</option>{available.map(p=><option key={p.id} value={p.id}>{p.name}</option>)}</SelectInput></FormField>
   <FormField label={en?'Model':'허용 모델'}><SelectInput aria-label={en?'Allowed model':'허용 모델'} value={model} disabled={busy||!provider||catalogue.loading} onChange={e=>setModel(e.target.value)}><option value="">{en?'Select model':'모델 선택'}</option>{models.map(m=><option key={m} value={m}>{m}</option>)}</SelectInput></FormField>
   <Notice message={catalogue.error}/>
   <div className="grid grid-cols-2 gap-2"><FormField label={en?'Tokens per request':'요청당 토큰 예산'}><TextInput type="number" min={1024} max={32768} value={budget} disabled={busy} onChange={e=>setBudget(e.target.value)}/></FormField>
   <FormField label={en?'Request allowance':'허용 요청 횟수'}><TextInput aria-label={en?'Request allowance':'허용 요청 횟수'} type="number" min={1} max={100} value={maxRuns} disabled={busy} onChange={e=>setMaxRuns(e.target.value)}/></FormField></div>
   <div className="flex gap-2"><Action disabled={busy||!valid||grant.loading||Boolean(grant.error)} onClick={()=>{if(grant.value&&!confirm(en?'Rotate credential and reset request allowance?':'기존 인증키를 폐기하고 새 요청 한도를 발급할까요?'))return;void run(async()=>{const response=await post<{grant:Grant;token:string}>(path,{providerId,model,tokenBudget:Number(budget),maxRuns:Number(maxRuns)});setToken(response.token)})}}>{grant.value?(en?'Rotate key':'키 재발급'):(en?'Issue key':'키 발급')}</Action>
   <Button variant="ghost" disabled={busy||!grant.value} onClick={()=>{if(confirm(en?'Revoke this credential?':'이 인증키를 폐기할까요?'))void run(async()=>{await del(path)})}}>{en?'Revoke key':'키 폐기'}</Button></div>
   {!enabled&&<p className="text-xs text-muted">{en?'Enable the app before testing its connection.':'연결 시험은 앱을 활성화한 뒤 사용할 수 있습니다.'}</p>}
   {token&&<div className="rounded-lg border border-divider p-3"><p className="text-xs">{en?'Copy this key now. It will not be shown again.':'한 번만 표시되는 인증키입니다. 지금 복사해 외부 앱에 설정하세요.'}</p><TextInput aria-label={en?'One-time credential':'일회 표시 인증키'} readOnly value={token}/><Button variant="ghost" onClick={()=>setToken('')}>{en?'Hide key':'키 숨기기'}</Button><ScopedConnectionTest appId={appId} token={token} enabled={enabled&&!busy&&!grant.error&&Boolean(grant.value&&grant.value.remainingRuns>0)} onRun={onRun}/></div>}
  </div>}
 </div>
}

type HistoryRow={id:string;status:string;model?:string;instruction?:string;createdAt?:string;scoped:boolean;taskOutcome?:string;verified?:boolean}
type RunDetail=HistoryRow&{answer?:string;error?:string;actions?:{cancel?:boolean}}
const labels:Record<string,string>={queued:'대기',running:'실행 중',completed:'응답 완료',failed:'실패',cancelled:'취소',interrupted:'중단',awaiting_approval:'승인 대기',needs_review:'검토 필요',in_doubt:'실행 확인 필요'}
function AppRunHistory({appId,selected,onSelect}:{appId:string;selected:string;onSelect:(id:string)=>void}){
 const en=useLocale()==='en',path=endpoint+'/'+encodeURIComponent(appId)+'/history'
 const [opened,setOpened]=useState(false),[error,setError]=useState(''),[busy,setBusy]=useState(false)
 useEffect(()=>{if(selected)setOpened(true)},[selected])
 const history=useRemote<HistoryRow[]>(opened?path:null,[])
 const detail=useRemote<RunDetail|null>(opened&&selected?'/api/sdk/runs/'+encodeURIComponent(selected):null,null)
 const status=(value:string)=>en?value:(labels[value]||value)
 const cancel=async()=>{
  if(!detail.value?.actions?.cancel||busy||!confirm(en?'Cancel this run?':'이 작업을 취소할까요?'))return
  setBusy(true);setError('')
  try{await post('/api/sdk/runs/'+encodeURIComponent(selected)+'/cancel',{});await Promise.all([history.reload(),detail.reload()])}
  catch(e){setError((e as Error).message)}finally{setBusy(false)}
 }
 return <div className="w-full border-t border-divider pt-2">
  <div className="flex flex-wrap items-center gap-2"><Button variant="ghost" onClick={()=>{if(opened)onSelect('');setOpened(!opened)}}>{opened?(en?'Close run history':'실행 기록 닫기'):(en?'Run history':'앱 실행 기록')}</Button>
  {opened&&<Button variant="ghost" disabled={history.refreshing||detail.refreshing} onClick={()=>void Promise.all([history.reload(),detail.reload()])}>{en?'Refresh':'기록 새로고침'}</Button>}</div>
  {opened&&<div className="mt-2 grid gap-3">
   <Notice message={error||history.error||detail.error}/>
   <p className="text-xs text-muted">{en?'Latest 40 requests for this app. Select a run for its live result.':'이 앱의 최근 40개 요청입니다. 작업을 선택하면 실제 결과와 상태를 확인할 수 있습니다.'}</p>
   {history.loading?<p role="status" className="text-xs text-muted">{en?'Loading…':'기록 불러오는 중…'}</p>:!history.error&&!history.value.length?<p className="text-xs text-muted">{en?'No runs yet.':'아직 실행 기록이 없습니다.'}</p>:null}
   <div className="grid max-h-72 gap-1 overflow-y-auto">{history.value.map(row=><button type="button" key={row.id} aria-pressed={selected===row.id} className={'min-w-0 rounded-lg border p-2 text-left '+(selected===row.id?'border-current bg-surface':'border-divider')} onClick={()=>{setError('');onSelect(row.id)}}>
    <div className="flex flex-wrap items-center gap-2"><StateTag online={row.status==='completed'}>{status(row.status)}</StateTag><span className="text-xs text-muted">{row.scoped?(en?'Restricted app':'제한형 앱'):(en?'Owner SDK':'관리자 SDK')} · {row.model||'—'}</span></div>
    <p className="mt-1 truncate text-sm">{row.instruction||row.id}</p><p className="text-xs text-muted">{row.createdAt||''}</p>
   </button>)}</div>
   {selected&&detail.loading&&<p role="status" className="text-xs text-muted">{en?'Loading result…':'결과 불러오는 중…'}</p>}
   {detail.value&&<div className="grid gap-2 rounded-lg border border-divider p-3">
    <div className="flex flex-wrap items-center gap-2"><strong>{en?'Run result':'실행 결과'}</strong><StateTag online={detail.value.status==='completed'}>{status(detail.value.status)}</StateTag><span className="text-xs text-muted">{detail.value.verified===true?(en?'Verified':'검증 완료'):(en?'Not verified':'검증되지 않음')}</span></div>
    <p className="break-all text-xs text-muted">{detail.value.id}</p>
    {detail.value.answer&&<pre className="max-h-64 overflow-auto whitespace-pre-wrap break-words text-sm">{detail.value.answer}</pre>}
    <Notice message={detail.value.error||''}/>
    {detail.value.actions?.cancel&&<Button disabled={busy} onClick={()=>void cancel()}>{en?'Cancel run':'실행 취소'}</Button>}
   </div>}
  </div>}
 </div>
}
function ScopedConnectionTest({appId,token,enabled,onRun}:{appId:string;token:string;enabled:boolean;onRun:(id:string)=>void}){
 const en=useLocale()==='en'
 const [instruction,setInstruction]=useState(en?'Reply with OK.':'연결되었습니다라고 답해주세요.')
 const [busy,setBusy]=useState(false),[error,setError]=useState(''),[submitted,setSubmitted]=useState('')
 const retry=useRef<{instruction:string;key:string}|null>(null),inFlight=useRef(false)
 const submit=async()=>{
  if(!enabled||!instruction.trim()||inFlight.current)return
  inFlight.current=true;setBusy(true);setError('')
  if(retry.current?.instruction!==instruction)retry.current={instruction,key:crypto.randomUUID()}
  try{
   // Intentionally do not use the owner transport; exercise exactly the external credential.
   const res=await fetch(endpoint+'/'+encodeURIComponent(appId)+'/runs',{method:'POST',redirect:'error',cache:'no-store',
    headers:{'Content-Type':'application/json','x-dominion-extension-token':token},
    body:JSON.stringify({instruction,idempotencyKey:retry.current.key})})
   const row=await res.json()
   if(!res.ok||typeof row.id!=='string')throw new Error(row.error||('HTTP '+res.status))
   setSubmitted(row.id);retry.current=null;onRun(row.id);refresh(endpoint+'/'+encodeURIComponent(appId)+'/history')
  }catch(e){setError((e as Error).message)}finally{inFlight.current=false;setBusy(false)}
 }
 return <div className="mt-3 grid gap-2 border-t border-divider pt-3">
  <strong className="text-xs">{en?'Test external connection':'외부 앱 연결 시험'}</strong>
  <p className="text-xs text-muted">{en?'Uses one request from this key and the selected model. No owner credential is sent.':'이 인증키로 실제 모델을 호출합니다. 요청 1회가 차감되며 관리자 인증키는 전송하지 않습니다.'}</p>
  <TextArea aria-label={en?'Test message':'연결 시험 메시지'} value={instruction} disabled={busy||!enabled} onChange={e=>{setInstruction(e.target.value);setSubmitted('')}}/>
  <Button disabled={!enabled||busy||!instruction.trim()||Boolean(submitted)} onClick={()=>void submit()}>{busy?(en?'Submitting…':'요청 중…'):(en?'Send test request':'연결 시험 보내기')}</Button>
  <Notice message={error}/>
  {submitted&&<p role="status" className="text-xs">{en?'Request accepted. See run history for the actual response.':'요청이 접수됐습니다. 아래 실행 기록에서 실제 응답을 확인하세요.'}</p>}
 </div>
}
