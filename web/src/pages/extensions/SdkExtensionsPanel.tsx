import {useEffect,useRef,useState} from 'react'
import {del,get,post} from '../../api'
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
    <ManagedPackage app={app}/>
    <GrantPanel appId={app.id} enabled={app.enabled} onRun={id=>setSelectedRuns(previous=>({...previous,[app.id]:id}))}/>
    <TaskRequests appId={app.id} enabled={app.enabled} onRun={id=>setSelectedRuns(previous=>({...previous,[app.id]:id}))}/>
    <AppRunHistory appId={app.id} selected={selectedRuns[app.id]||''} onSelect={id=>setSelectedRuns(previous=>({...previous,[app.id]:id}))}/>
  </Panel>)}</div>
 </section>
}

type Grant={providerId:string;model:string;tokenBudget:number;remainingRuns:number;capability:string;generation:string;allowTaskRequests?:boolean}
type Provider={id:string;name:string;kind?:string;enabled?:boolean;model?:string}
function GrantPanel({appId,enabled,onRun}:{appId:string;enabled:boolean;onRun:(id:string)=>void}){
 const en=useLocale()==='en',path=endpoint+'/'+encodeURIComponent(appId)+'/grant'
 const [opened,setOpened]=useState(false)
 const grant=useRemote<Grant|null>(opened?path:null,null)
 const providers=useRemote<Provider[]>(opened?'/api/providers':null,[])
 const [providerId,setProviderId]=useState(''),[model,setModel]=useState('')
 const [budget,setBudget]=useState('8192'),[maxRuns,setMaxRuns]=useState('10'),[allowTasks,setAllowTasks]=useState(false)
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
   <label className="flex items-center gap-2 text-xs"><input type="checkbox" checked={allowTasks} disabled={busy} onChange={e=>setAllowTasks(e.target.checked)}/>{en?'Allow task proposals (owner review required)':'에이전트 작업 제안 허용 (사용자 승인 필요)'}</label>
   {grant.value?.allowTaskRequests&&<p className="text-xs text-muted">{en?'Current key permits task proposals.':'현재 인증키는 작업 제안을 허용합니다.'}</p>}
   <div className="flex gap-2"><Action disabled={busy||!valid||grant.loading||Boolean(grant.error)} onClick={()=>{if(grant.value&&!confirm(en?'Rotate credential and reset request allowance?':'기존 인증키를 폐기하고 새 요청 한도를 발급할까요?'))return;void run(async()=>{const response=await post<{grant:Grant;token:string}>(path,{providerId,model,tokenBudget:Number(budget),maxRuns:Number(maxRuns),allowTaskRequests:allowTasks});setToken(response.token)})}}>{grant.value?(en?'Rotate key':'키 재발급'):(en?'Issue key':'키 발급')}</Action>
   <Button variant="ghost" disabled={busy||!grant.value} onClick={()=>{if(confirm(en?'Revoke this credential?':'이 인증키를 폐기할까요?'))void run(async()=>{await del(path)})}}>{en?'Revoke key':'키 폐기'}</Button></div>
   {!enabled&&<p className="text-xs text-muted">{en?'Enable the app before testing its connection.':'연결 시험은 앱을 활성화한 뒤 사용할 수 있습니다.'}</p>}
   {token&&<div className="rounded-lg border border-divider p-3"><p className="text-xs">{en?'Copy this key now. It will not be shown again.':'한 번만 표시되는 인증키입니다. 지금 복사해 외부 앱에 설정하세요.'}</p><TextInput aria-label={en?'One-time credential':'일회 표시 인증키'} readOnly value={token}/><Button variant="ghost" onClick={()=>setToken('')}>{en?'Hide key':'키 숨기기'}</Button><ScopedConnectionTest appId={appId} token={token} enabled={enabled&&!busy&&!grant.error&&Boolean(grant.value)} remaining={grant.value?.remainingRuns??0} onRun={onRun}/></div>}
  </div>}
 </div>
}

type HistoryRow={id:string;status:string;model?:string;instruction?:string;createdAt?:string;scoped:boolean;taskOutcome?:string;verified?:boolean}
type RunDetail=HistoryRow&{answer?:string;error?:string;actions?:{cancel?:boolean;approve?:boolean;deny?:boolean};checkpoint?:{pendingCall?:{id:string;name:string;arguments:unknown}|null}}
const labels:Record<string,string>={queued:'대기',running:'실행 중',completed:'응답 완료',failed:'실패',cancelled:'취소',interrupted:'중단',awaiting_approval:'승인 대기',needs_review:'검토 필요',in_doubt:'실행 확인 필요'}
function AppRunHistory({appId,selected,onSelect}:{appId:string;selected:string;onSelect:(id:string)=>void}){
 const en=useLocale()==='en',path=endpoint+'/'+encodeURIComponent(appId)+'/history'
 const [opened,setOpened]=useState(false),[error,setError]=useState(''),[busy,setBusy]=useState(false)
 useEffect(()=>{if(selected)setOpened(true)},[selected])
 const history=useRemote<HistoryRow[]>(opened?path:null,[])
 const detail=useRemote<RunDetail|null>(opened&&selected?'/api/sdk/runs/'+encodeURIComponent(selected):null,null)
 const status=(value:string)=>en?value:(labels[value]||value)
 const live=Boolean(detail.value&&['queued','running','awaiting_approval'].includes(detail.value.status))
 useEffect(()=>{if(!opened||!live)return;const timer=window.setInterval(()=>void detail.reload(),2000);return()=>window.clearInterval(timer)},[opened,live,detail.reload])
 const pending=detail.value?.checkpoint?.pendingCall
 const decide=async(allow:boolean)=>{
  if(busy||!pending||!(allow?detail.value?.actions?.approve:detail.value?.actions?.deny))return
  setBusy(true);setError('')
  try{
   if(allow)await post('/api/sdk/runs/'+encodeURIComponent(selected)+'/approve',{callId:pending.id})
   else await post('/api/approvals',{id:pending.id,allow:false})
   await Promise.all([history.reload(),detail.reload()])
  }catch(e){setError((e as Error).message)}finally{setBusy(false)}
 }
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
    {pending&&(detail.value.actions?.approve||detail.value.actions?.deny)&&<div className="grid gap-2 rounded-lg border border-divider p-3">
     <strong>{en?'Tool approval required':'도구 실행 승인 필요'}</strong><p className="text-xs">{pending.name}</p><pre className="max-h-48 overflow-auto whitespace-pre-wrap break-words text-xs">{JSON.stringify(pending.arguments,null,2)}</pre>
     <div className="flex gap-2"><Action disabled={busy||!detail.value.actions?.approve} onClick={()=>void decide(true)}>{en?'Approve tool once':'도구 1회 승인'}</Action><Button disabled={busy||!detail.value.actions?.deny} onClick={()=>void decide(false)}>{en?'Deny tool':'도구 거절'}</Button></div>
    </div>}
    {detail.value.actions?.cancel&&<Button disabled={busy} onClick={()=>void cancel()}>{en?'Cancel run':'실행 취소'}</Button>}
   </div>}
  </div>}
 </div>
}
function ScopedConnectionTest({appId,token,enabled,remaining,onRun}:{appId:string;token:string;enabled:boolean;remaining:number;onRun:(id:string)=>void}){
 const en=useLocale()==='en'
 const [instruction,setInstruction]=useState(en?'Reply with OK.':'연결되었습니다라고 답해주세요.')
 const [busy,setBusy]=useState(false),[error,setError]=useState(''),[submitted,setSubmitted]=useState('')
 const retry=useRef<{instruction:string;key:string}|null>(null),inFlight=useRef(false)
 // The server checks the durable retry receipt before charging quota. If the
 // response was lost after the last allowance was consumed, allow replay of
 // that exact message only, never a new submission at zero remaining.
 const replay=Boolean(retry.current&&retry.current.instruction===instruction)
 const canSubmit=enabled&&(remaining>0||replay)
 const submit=async()=>{
  if(!canSubmit||!instruction.trim()||inFlight.current)return
  inFlight.current=true;setBusy(true);setError('')
  try{
   const attempt=retry.current?.instruction===instruction?retry.current:{instruction,key:crypto.randomUUID()}
   retry.current=attempt
   // Intentionally do not use the owner transport; exercise exactly the external credential.
   const res=await fetch(endpoint+'/'+encodeURIComponent(appId)+'/runs',{method:'POST',redirect:'error',cache:'no-store',
    headers:{'Content-Type':'application/json','x-dominion-extension-token':token},
    body:JSON.stringify({instruction,idempotencyKey:attempt.key})})
   const row=await res.json()
   if(!res.ok||typeof row.id!=='string')throw new Error(row.error||('HTTP '+res.status))
   setSubmitted(row.id);retry.current=null;onRun(row.id);refresh(endpoint+'/'+encodeURIComponent(appId)+'/history')
  }catch(e){setError((e as Error).message)}finally{inFlight.current=false;setBusy(false)}
 }
 return <div className="mt-3 grid gap-2 border-t border-divider pt-3">
  <strong className="text-xs">{en?'Test external connection':'외부 앱 연결 시험'}</strong>
  <p className="text-xs text-muted">{en?'Uses one request from this key and the selected model. No owner credential is sent.':'이 인증키로 실제 모델을 호출합니다. 요청 1회가 차감되며 관리자 인증키는 전송하지 않습니다.'}</p>
  <TextArea aria-label={en?'Test message':'연결 시험 메시지'} value={instruction} disabled={busy||!enabled} onChange={e=>{setInstruction(e.target.value);setSubmitted('')}}/>
  <Button disabled={!canSubmit||busy||!instruction.trim()||Boolean(submitted)} onClick={()=>void submit()}>{busy?(en?'Submitting…':'요청 중…'):replay?(en?'Retry same request':'같은 요청 다시 확인'):(en?'Send test request':'연결 시험 보내기')}</Button>
  {enabled&&remaining===0&&!submitted&&<p className="text-xs text-muted">{replay?(en?'No allowance remains. Retrying the same receipt will not create a new request.':'남은 횟수는 0회입니다. 같은 요청의 접수 기록만 다시 확인할 수 있습니다.'):(en?'Request allowance exhausted.':'허용 요청 횟수를 모두 사용했습니다.')}</p>}
  <Notice message={error}/>
  {submitted&&<p role="status" className="text-xs">{en?'Request accepted. See run history for the actual response.':'요청이 접수됐습니다. 아래 실행 기록에서 실제 응답을 확인하세요.'}</p>}
 </div>
}

type ReleaseAsset={name:string;version:string;source:string;sha256:string;bytes:number}
type PackageState={installed:boolean;filePresent:boolean;version?:string;sha256?:string;running:boolean;exitCode?:number|null;platform:string;arch:string}
function ManagedPackage({app}:{app:App}){
 const en=useLocale()==='en',root=endpoint+'/'+encodeURIComponent(app.id),path=root+'/package'
 const [opened,setOpened]=useState(false),[source,setSource]=useState(''),[version,setVersion]=useState(app.manifest.version),[sha,setSha]=useState('')
 const [token,setToken]=useState(''),[trust,setTrust]=useState(false),[busy,setBusy]=useState(false),[error,setError]=useState('')
 const [repo,setRepo]=useState(''),[catalog,setCatalog]=useState<ReleaseAsset[]>([]),[lookup,setLookup]=useState(false),[catalogNote,setCatalogNote]=useState('')
 const findRelease=async()=>{
  if(lookup||!repo.trim())return
  setLookup(true);setCatalog([]);setCatalogNote('');setError('')
  try{const result=await get<{tag:string;assets:ReleaseAsset[]}>('/api/extensions/releases?repo='+encodeURIComponent(repo.trim()));setCatalog(result.assets);setCatalogNote(result.assets.length?(en?'Select the file matching your platform.':'현재 운영체제와 CPU에 맞는 실행 파일을 선택하세요.'):(en?'No standalone asset with a SHA-256 digest.':'SHA-256이 제공되는 단독 실행 파일이 없습니다.'))}
  catch(e){setError((e as Error).message)}finally{setLookup(false)}
 }
 const inFlight=useRef(false)
 const pkg=useRemote<PackageState|null>(opened?path:null,null)
 const grant=useRemote<Grant|null>(opened?root+'/grant':null,null)
 useEffect(()=>{if(!opened||!pkg.value?.running)return;const timer=window.setInterval(()=>void pkg.reload(),2000);return()=>window.clearInterval(timer)},[opened,pkg.value?.running,pkg.reload])
 const act=async(fn:()=>Promise<unknown>)=>{
  if(inFlight.current)return
  inFlight.current=true;setBusy(true);setError('')
  try{await fn();await pkg.reload();refresh(endpoint)}
  catch(e){setError((e as Error).message)}finally{inFlight.current=false;setBusy(false);setToken('')}
 }
 const installed=Boolean(pkg.value?.installed),running=Boolean(pkg.value?.running),ready=opened&&!pkg.loading&&!pkg.error&&Boolean(pkg.value)
 return <div className="w-full border-t border-divider pt-2">
  <Button variant="ghost" disabled={busy} onClick={()=>{setOpened(!opened);setToken('');setTrust(false)}}>{opened?(en?'Close installation':'설치 관리 닫기'):(en?'Installation and process':'설치·실행 관리')}</Button>
  {opened&&<div className="mt-2 grid gap-3">
   <Notice message={error||pkg.error||grant.error}/>
   {pkg.loading?<p role="status" className="text-xs text-muted">{en?'Checking installation…':'설치 상태 확인 중…'}</p>:pkg.value&&<div className="flex flex-wrap items-center gap-2"><StateTag online={installed&&pkg.value.filePresent}>{installed?(en?'Installed':'설치됨'):(en?'Not installed':'미설치')}</StateTag><StateTag online={running}>{running?(en?'Running':'실행 중'):(en?'Stopped':'정지')}</StateTag><span className="text-xs text-muted">{pkg.value.platform}/{pkg.value.arch} {pkg.value.version||''} {pkg.value.exitCode!=null?' · exit '+pkg.value.exitCode:''}</span></div>}
   {installed&&!pkg.value?.filePresent&&<Notice message={en?'Executable missing. Reinstall the package.':'실행 파일이 없습니다. 다시 설치하세요.'}/>}
   {installed&&<p className="break-all text-xs text-muted">SHA-256: {pkg.value?.sha256}</p>}
   <p className="text-xs text-muted">{en?'Install a trusted standalone executable (up to 64 MiB), not an archive. Runs with your OS user privileges; this is not a sandbox.':'신뢰하는 단독 실행 파일(최대 64MiB)을 설치합니다. 압축 파일은 지원하지 않습니다. 현재 OS 사용자 권한으로 실행되며 샌드박스가 아닙니다.'}</p>
   <label className="flex items-start gap-2 text-xs"><input type="checkbox" checked={trust} onChange={e=>setTrust(e.target.checked)} disabled={busy}/>{en?'I trust this executable and its publisher.':'이 실행 파일과 배포자를 신뢰하며 실행 권한을 허용합니다.'}</label>
   {!app.enabled&&<div className="grid gap-2">
    <FormField label={en?'GitHub repository':'GitHub 저장소'} hint="owner/repository"><TextInput aria-label={en?'GitHub repository':'GitHub 저장소'} value={repo} disabled={lookup||busy} onChange={e=>{setRepo(e.target.value);setCatalog([]);setCatalogNote('')}}/></FormField>
    <Button disabled={lookup||busy||!repo.trim()} onClick={()=>void findRelease()}>{lookup?(en?'Checking release…':'릴리스 확인 중…'):(en?'Find latest release':'최신 릴리스 확인')}</Button>
    {catalogNote&&<p role="status" className="text-xs text-muted">{catalogNote}</p>}
    {catalog.length>0&&<div className="grid gap-1 max-h-48 overflow-y-auto">{catalog.map(asset=><Button key={asset.source} variant="ghost" disabled={busy} className="justify-start whitespace-normal break-all text-left" onClick={()=>{setSource(asset.source);setVersion(asset.version);setSha(asset.sha256);setTrust(false)}}>{asset.name} · v{asset.version} · {(asset.bytes/1024/1024).toFixed(1)} MiB{pkg.value?.sha256===asset.sha256?(en?' · Installed':' · 현재 설치됨'):''}</Button>)}</div>}
    <FormField label={en?'Executable source':'실행 파일 출처'} hint={en?'Absolute local path or pinned GitHub release URL':'절대 로컬 경로 또는 버전이 지정된 GitHub 릴리스 URL'}><TextInput aria-label={en?'Executable source':'실행 파일 출처'} value={source} onChange={e=>setSource(e.target.value)} disabled={busy}/></FormField>
    <FormField label={en?'Package version':'설치 버전'}><TextInput aria-label={en?'Package version':'설치 버전'} value={version} onChange={e=>setVersion(e.target.value)} disabled={busy}/></FormField>
    <FormField label="SHA-256"><TextInput aria-label="SHA-256" value={sha} onChange={e=>setSha(e.target.value)} disabled={busy}/></FormField>
    <div className="flex flex-wrap gap-2"><Action disabled={!ready||busy||!trust||!source.trim()||!version.trim()||!/^[a-f0-9]{64}$/i.test(sha)} onClick={()=>void act(()=>post(path,{source:source.trim(),version:version.trim(),sha256:sha,trustExecutable:true}))}>{installed?(en?'Update package':'업데이트'):(en?'Install package':'설치')}</Action>
    {installed&&<Button disabled={busy||!ready} variant="ghost" onClick={()=>{if(confirm(en?'Uninstall the executable? App registration and run history remain.':'실행 파일만 제거할까요? 앱 등록과 실행 기록은 유지됩니다.'))void act(()=>del(path))}}>{en?'Uninstall':'설치 제거'}</Button>}</div>
   </div>}
   {app.enabled&&!running&&<p className="text-xs text-muted">{en?'Disable the app before installing or updating.':'설치하거나 업데이트하려면 먼저 앱을 비활성화하세요.'}</p>}
   {installed&&app.enabled&&!running&&<div className="grid gap-2"><FormField label={en?'App credential':'앱 실행 인증키'}><TextInput aria-label={en?'App credential':'앱 실행 인증키'} type="password" autoComplete="off" value={token} onChange={e=>setToken(e.target.value)} disabled={busy}/></FormField>
    {!grant.loading&&!grant.value&&<p className="text-xs text-muted">{en?'Issue an app credential below first.':'아래 앱별 인증키 관리에서 키를 먼저 발급하세요.'}</p>}
    <Action disabled={!ready||busy||!trust||!grant.value||token.length!==64||!pkg.value?.filePresent} onClick={()=>void act(()=>post(root+'/process/start',{token,trustExecutable:true}))}>{en?'Start app':'앱 실행'}</Action></div>}
   {running&&<Button disabled={busy} onClick={()=>void act(()=>post(root+'/process/stop',{}))}>{en?'Stop app':'앱 종료'}</Button>}
   <Button variant="ghost" disabled={busy||pkg.refreshing} onClick={()=>void pkg.reload()}>{en?'Refresh process status':'실행 상태 새로고침'}</Button>
  </div>}
 </div>
}

type SharedArtifact={id:string;name:string;sha256:string;bytes:number}
type TaskRequest={id:string;instruction:string;status:string;model:string;tokenBudget:number;runId?:string;runStatus?:string;canApprove:boolean;canPublish:boolean;publishedMessage?:string;parentRequestId?:string;parentMessage?:string;canShareArtifact?:boolean;artifacts?:SharedArtifact[]}
function TaskRequests({appId,enabled,onRun}:{appId:string;enabled:boolean;onRun:(id:string)=>void}){
 const en=useLocale()==='en',path=endpoint+'/'+encodeURIComponent(appId)+'/requests'
 const [open,setOpen]=useState(false),[selected,setSelected]=useState(''),[workspace,setWorkspace]=useState(''),[rounds,setRounds]=useState('8'),[message,setMessage]=useState(''),[error,setError]=useState(''),[busy,setBusy]=useState(false)
 const requests=useRemote<TaskRequest[]>(path,[])
 const pendingCount=requests.value.filter(r=>r.status==='pending'&&r.canApprove).length
 const row=requests.value.find(r=>r.id===selected)
 const [file,setFile]=useState<File|null>(null),fileInput=useRef<HTMLInputElement>(null)
 const inFlight=useRef(false)
 const act=async(suffix:string,body:unknown)=>{
  if(inFlight.current||!row)return
  inFlight.current=true;setBusy(true);setError('')
  try{await post(path+'/'+row.id+'/'+suffix,body);await requests.reload();if(suffix==='publish')setMessage('');if(suffix==='artifacts'){setFile(null);if(fileInput.current)fileInput.current.value=''}}
  catch(e){setError((e as Error).message)}finally{inFlight.current=false;setBusy(false)}
 }
 const shareFile=async()=>{
  if(!file||!row?.canShareArtifact||busy)return
  if(file.size>2*1024*1024){setError(en?'Maximum file size: 2 MiB':'파일 크기는 최대 2MiB입니다.');return}
  if(!confirm(en?'Share this exact file with the external app?':'선택한 파일을 외부 앱에 전달할까요?'))return
  if(inFlight.current)return
  inFlight.current=true;setBusy(true);setError('')
  try{
   const bytes=new Uint8Array(await file.arrayBuffer())
   let binary=''
   for(let offset=0;offset<bytes.length;offset+=8192)binary+=String.fromCharCode(...bytes.subarray(offset,offset+8192))
   await post(path+'/'+row.id+'/artifacts',{name:file.name,contentBase64:btoa(binary)})
   await requests.reload();setFile(null);if(fileInput.current)fileInput.current.value=''
  }catch(e){setError((e as Error).message)}finally{inFlight.current=false;setBusy(false)}
 }
 return <div className="w-full border-t border-divider pt-2">
  <Button variant="ghost" onClick={()=>setOpen(!open)}>{open?(en?'Close task requests':'작업 제안 닫기'):(en?'Task requests':'앱 작업 제안')}{pendingCount>0&&<span className="rounded-full bg-brand px-2 text-white">{pendingCount}</span>}</Button>
  {open&&<div className="mt-2 grid gap-2">
   <p className="text-xs text-muted">{en?'An external app cannot start agent tools. Review its instruction and select a workspace. Native tool approvals still apply. Only your explicitly shared message and files are returned.':'외부 앱의 제안은 직접 승인하기 전까지 실행되지 않습니다. 작업 공간을 지정하고 내용을 확인하세요. 기존 도구 승인 절차가 적용되며 직접 공유한 메시지와 파일만 앱에 전달됩니다.'}</p>
   <Notice message={error||requests.error}/>
   <Button variant="ghost" disabled={requests.refreshing||busy} onClick={()=>void requests.reload()}>{en?'Refresh requests':'제안 새로고침'}</Button>
   {requests.loading&&<p role="status" className="text-xs text-muted">{en?'Loading requests…':'작업 제안 불러오는 중…'}</p>}
   {!requests.loading&&!requests.value.length&&<p className="text-xs text-muted">{en?'No task requests':'작업 제안 없음'}</p>}
   <div className="grid max-h-64 gap-1 overflow-auto">{requests.value.map(r=><button type="button" key={r.id} disabled={busy} aria-pressed={selected===r.id} className="rounded-lg border border-divider p-2 text-left" onClick={()=>{setSelected(r.id);setMessage('');setFile(null);if(fileInput.current)fileInput.current.value='';setError('')}}><StateTag online={r.status==='approved'}>{en?r.status:r.status==='pending'?'승인 대기':r.status==='approved'?'승인됨':'거절됨'}</StateTag><span className="ml-2 text-xs text-muted">{r.model} · {r.tokenBudget} tokens</span><p className="truncate text-sm">{r.instruction}</p></button>)}</div>
   {row&&<Panel className="grid gap-2 p-3">
    {row.parentRequestId&&<div className="grid gap-1 rounded-lg border border-divider p-2"><span className="text-xs text-muted">{en?'Follow-up to a prior completed task':'이전 완료 작업의 후속 요청'}</span><p className="whitespace-pre-wrap break-words text-xs">{row.parentMessage}</p></div>}
    <strong>{en?'Requested instruction':'제안 원문'}</strong><pre className="max-h-64 overflow-auto whitespace-pre-wrap break-words text-sm">{row.instruction}</pre>
    {row.status==='pending'&&<><FormField label={en?'Owner workspace':'승인 작업 공간'} hint={en?'Absolute path':'절대 경로'}><TextInput aria-label={en?'Owner workspace':'승인 작업 공간'} value={workspace} onChange={e=>setWorkspace(e.target.value)} disabled={busy}/></FormField>
    <FormField label={en?'Maximum model rounds':'최대 모델 호출 횟수'}><TextInput type="number" min={1} max={24} value={rounds} onChange={e=>setRounds(e.target.value)} disabled={busy}/></FormField>
    {!row.canApprove&&<p className="text-xs text-muted">{en?'Permission changed or app disabled. Reject and request a fresh proposal.':'인증 권한이 변경되었거나 앱이 비활성화되어 승인할 수 없습니다.'}</p>}
    <div className="flex gap-2"><Action disabled={busy||!enabled||!row.canApprove||!workspace.trim()||!Number.isInteger(Number(rounds))||Number(rounds)<1||Number(rounds)>24} onClick={()=>{if(confirm(en?'Run the displayed instruction using native agent tools in this workspace?':'표시된 지시를 이 작업 공간에서 에이전트로 실행할까요?'))void act('review',{approve:true,workspaceDir:workspace.trim(),maxRounds:Number(rounds)})}}>{en?'Approve and run':'승인 및 실행'}</Action><Button disabled={busy} onClick={()=>void act('review',{approve:false})}>{en?'Reject':'거절'}</Button></div></>}
    {row.runId&&<div className="flex items-center gap-2"><StateTag>{en?(row.runStatus||'queued'):(labels[row.runStatus||'queued']||row.runStatus)}</StateTag><Button onClick={()=>onRun(row.runId!)}>{en?'Inspect native run':'실행 결과 확인'}</Button></div>}
    {row.publishedMessage!=null?<><strong>{en?'Published message':'공유한 메시지'}</strong><p className="whitespace-pre-wrap break-words text-sm">{row.publishedMessage}</p></>:row.canPublish&&<><FormField label={en?'Message to share':'앱에 공유할 메시지'}><TextArea aria-label={en?'Message to share':'앱에 공유할 메시지'} value={message} onChange={e=>setMessage(e.target.value)} disabled={busy}/></FormField><Action disabled={busy||!message.trim()} onClick={()=>{if(confirm(en?'Publish this exact message to the external app?':'이 메시지를 외부 앱에 공유할까요?'))void act('publish',{message})}}>{en?'Share message':'메시지 공유'}</Action></>}
    {(row.canShareArtifact||Boolean(row.artifacts?.length))&&<div className="grid gap-2 border-t border-divider pt-2">
     <strong>{en?'Shared files':'공유 파일'}</strong>
     {(row.artifacts||[]).map(a=><div key={a.id} className="rounded-lg border border-divider p-2"><p className="break-all text-sm">{a.name} · {a.bytes} B</p><p className="break-all text-xs text-muted">SHA-256: {a.sha256}</p></div>)}
     {row.canShareArtifact&&<><p className="text-xs text-muted">{en?'Select a file you intend to disclose. Up to four files, 2 MiB each.':'직접 공유할 파일을 선택하세요. 최대 4개, 파일당 2MiB입니다.'}</p><input ref={fileInput} type="file" aria-label={en?'File to share':'공유할 파일'} disabled={busy} onChange={e=>setFile(e.target.files?.[0]||null)}/><Action disabled={busy||!file||file.size>2*1024*1024} onClick={()=>void shareFile()}>{en?'Share file':'파일 공유'}</Action></>}
    </div>}
   </Panel>}
  </div>}
 </div>
}
