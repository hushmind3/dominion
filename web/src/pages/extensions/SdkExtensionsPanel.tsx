import {useRef,useState} from 'react'
import {del,post} from '../../api'
import {useLocale} from '../../i18n'
import {Action,FormField,Notice,Panel,Subheading,useRemote,refresh} from '../../components/UI'
import {Button,TextInput,TextArea} from '../../components/Primitives'
type Manifest={schemaVersion:1;id:string;name:string;version:string;description?:string}
type App={id:string;manifest:Manifest;enabled:boolean}
const endpoint='/api/extensions/apps'
const empty:Manifest={schemaVersion:1,id:'',name:'',version:'1.0.0',description:''}
export function SdkExtensionsPanel(){
 const en=useLocale()==='en'
 const apps=useRemote<App[]>(endpoint,[])
 const [draft,setDraft]=useState<Manifest>(empty),[open,setOpen]=useState(false)
 const [busy,setBusy]=useState(false),[error,setError]=useState('')
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
  <Notice message={error||apps.error} onRetry={apps.error?()=>void apps.reload():undefined}/>
  {open&&<Panel className="mb-3 grid gap-3 p-4">
   <FormField label={en?'App ID':'앱 ID'} hint="example.assistant"><TextInput value={draft.id} onChange={e=>setDraft({...draft,id:e.target.value})} disabled={busy}/></FormField>
   <FormField label={en?'Name':'앱 이름'}><TextInput value={draft.name} onChange={e=>setDraft({...draft,name:e.target.value})} disabled={busy}/></FormField>
   <FormField label={en?'Version':'버전'}><TextInput value={draft.version} onChange={e=>setDraft({...draft,version:e.target.value})} disabled={busy}/></FormField>
   <FormField label={en?'Description':'설명'}><TextArea value={draft.description} onChange={e=>setDraft({...draft,description:e.target.value})} disabled={busy}/></FormField>
   <div className="flex gap-2"><Action disabled={busy||!draft.id||!draft.name||!draft.version} onClick={()=>void save()}>{en?'Save registration':'등록 저장'}</Action><Button variant="ghost" onClick={()=>setOpen(false)} disabled={busy}>{en?'Cancel':'취소'}</Button></div>
  </Panel>}
  {apps.loading?<p role="status" className="text-xs text-muted">{en?'Loading…':'불러오는 중…'}</p>:!apps.error&&!apps.value.length?<p className="text-xs text-muted">{en?'No registered apps.':'등록된 외부 SDK 앱이 없습니다.'}</p>:null}
  <div className="grid gap-2">{apps.value.map(app=><Panel key={app.id} className="flex flex-wrap items-center gap-3 p-4">
    <div className="min-w-0 flex-1"><strong>{app.manifest.name}</strong><p className="break-all text-xs text-muted">{app.id} · {app.manifest.version}</p><p className="text-xs text-muted">{app.manifest.description}</p><p className="text-xs">{app.enabled?(en?'Enabled for new SDK requests':'SDK 요청 허용'):(en?'Disabled':'비활성')}</p></div>
    <Button disabled={busy||Boolean(apps.error)} onClick={()=>void change(()=>post(endpoint+'/'+encodeURIComponent(app.id)+'/state',{enabled:!app.enabled}))}>{app.enabled?(en?'Disable':'비활성화'):(en?'Enable':'활성화')}</Button>
    <Button variant="ghost" disabled={busy||app.enabled||Boolean(apps.error)} onClick={()=>remove(app)}>{en?'Unregister':'등록 해제'}</Button>
  </Panel>)}</div>
 </section>
}
