import {useCallback,useEffect,useRef,useState} from 'react'
import {Box,Play,Square,Trash2,RefreshCw,ExternalLink,FileText,Plus} from 'lucide-react'
import {get,post,del} from '../../api'
import {Button,TextInput} from '../../components/Primitives'
import {useLocale} from '../../i18n'
import {PluginBridgePanel} from './PluginBridgePanel'

type Plugin={id:string;name:string;image:string;imageId:string|null;state:string;running:boolean;network:boolean;ports:number[];exitCode:number|null;requestedPort?:number|null;versionChange?:{phase:'prepared'|'ready';previousImage:string;previousImageId:string;targetImage:string}}
type Inventory={available:boolean;items:Plugin[]|null;error?:string}
type Logs={stdout:string;stderr:string;truncated:boolean}
export function ManagedPlugins({selectedImage,onImageConsumed}:{selectedImage:string;onImageConsumed:()=>void}){
 const en=useLocale()==='en',tr=(ko:string,eng:string)=>en?eng:ko
 const [inventory,setInventory]=useState<Inventory|null>(null),[busy,setBusy]=useState(''),[error,setError]=useState(''),[notice,setNotice]=useState('')
 const [show,setShow]=useState(false),[name,setName]=useState(''),[image,setImage]=useState(''),[network,setNetwork]=useState(false),[port,setPort]=useState(''),[approved,setApproved]=useState(false)
 const [logs,setLogs]=useState<{name:string;data:Logs}|null>(null)
 const [restoreId,setRestoreId]=useState('')
 const [updating,setUpdating]=useState<Plugin|null>(null),[newImage,setNewImage]=useState('')
 const active=useRef(false),requestId=useRef(''),loadVersion=useRef(0),installFingerprint=useRef(''),formRef=useRef<HTMLDivElement>(null)
 const load=useCallback(async()=>{
  const version=++loadVersion.current
  try{const result=await get<Inventory>('/api/plugins/managed');if(version===loadVersion.current)setInventory(result)}
  catch(e){if(version===loadVersion.current)setInventory({available:false,items:null,error:(e as Error).message})}
 },[])
 useEffect(()=>{void load();return()=>{loadVersion.current++}},[load])
 useEffect(()=>{if(selectedImage){setRestoreId('');setImage(selectedImage);setName(selectedImage.split('/').pop()?.replace(/:$/,'')||'');setApproved(false);setShow(true);onImageConsumed()}},[selectedImage,onImageConsumed])
 useEffect(()=>{if(show)formRef.current?.scrollIntoView({block:'nearest'})},[show])
 const execute=async(key:string,task:()=>Promise<void>)=>{
  if(active.current)return
  active.current=true;setBusy(key);setError('');setNotice('')
  try{await task()}catch(e){setError((e as Error).message)}
  finally{await load();active.current=false;setBusy('')}
 }
 const install=()=>execute('install',async()=>{
  const spec={name:name.trim(),image:image.trim(),network,port:network&&port?Number(port):null}
  const fingerprint=JSON.stringify({...spec,restoreId})
  if(!requestId.current||installFingerprint.current!==fingerprint){requestId.current=restoreId||crypto.randomUUID();installFingerprint.current=fingerprint}
  await post('/api/plugins/managed',{id:requestId.current,...spec,confirmed:approved})
  requestId.current='';setRestoreId('');setShow(false);setName('');setImage('');setPort('');setNetwork(false);setApproved(false)
  setNotice(tr('설치했습니다. 실행 버튼을 누르면 시작합니다.','Installed. Start it when ready.'))
 })
 const operate=(p:Plugin,action:'start'|'stop'|'remove'|'logs')=>execute(p.id,async()=>{
  const id=encodeURIComponent(p.id)
  if(action==='logs'){setLogs({name:p.name,data:await get<Logs>('/api/plugins/managed/'+id+'/logs')});return}
  if(action==='remove'){
   if(!window.confirm(tr('플러그인을 제거할까요? 이미지와 /data 저장 공간은 삭제하지 않습니다.','Remove this plugin? Its image and /data volume will be retained.')))return
   const r=await del('/api/plugins/managed/'+id)
   setNotice(tr('제거했습니다. 데이터 보존 볼륨: ','Removed. Retained data volume: ')+r.volume)
  }else{
   if(action==='start'&&!window.confirm(tr('선택한 외부 프로그램을 실행합니다. 계속할까요?','Run the selected external program?')))return
   const r=action==='start'?await post<Plugin>('/api/plugins/managed/'+id+'/start',{}):await post<Plugin>('/api/plugins/managed/'+id+'/stop',{})
   setNotice(p.name+' · '+r.state)
  }
 })
 const updateVersion=()=>{if(!updating)return;const p=updating;void execute(p.id,async()=>{
  if(!window.confirm(tr('중지된 프로그램을 새 버전으로 교체합니다. 데이터는 유지되지만 이전 버전과 호환되지 않을 수 있습니다.','Replace the stopped app with this version? Data is retained but may not be backward-compatible.')))return
  await post('/api/plugins/managed/'+encodeURIComponent(p.id)+'/update',{image:newImage.trim(),expectedImageId:p.imageId,confirmed:true});setUpdating(null);setNotice(tr('버전 변경 완료. 실행을 눌러 확인하세요.','Version changed. Start to verify.'))
 })}
 const rollback=(p:Plugin)=>execute(p.id,async()=>{
  if(!window.confirm(tr('이전 실행 이미지로 복구합니다. 저장 데이터 자체를 되돌리지는 않습니다.','Restore the previous executable image? This does not restore stored data.')))return
  await post('/api/plugins/managed/'+encodeURIComponent(p.id)+'/rollback',{confirmed:true});setNotice(tr('이전 실행 이미지 복구 완료.','Previous executable image restored.'))
 })
 const restore=(p:Plugin)=>{setRestoreId(p.id);setName(p.name);setImage(p.image);setNetwork(p.network);setPort(p.requestedPort?String(p.requestedPort):'');setApproved(false);setShow(true)}
 const rows=inventory?.items
 const disabled=!!busy||inventory?.available!==true
 return <section className="mb-6 rounded-xl border border-divider bg-surface p-4">
  <div className="flex flex-wrap items-center justify-between gap-3">
   <div><h3 className="flex items-center gap-2 text-base font-bold text-ink"><Box size={20}/>{tr('내 플러그인','My plugins')}{rows&&<span className="text-xs text-muted">· {rows.length}</span>}</h3>
    <p className="mt-1 text-xs text-muted">{tr('외부 프로그램을 Docker에서 독립 실행합니다. MCP·스킬과 별도로 관리됩니다.','Run external apps in Docker, independently of MCP and skills.')}</p></div>
   <div className="flex gap-2"><Button variant="ghost" disabled={!!busy} onClick={()=>void load()}><RefreshCw size={15}/>{tr('새로고침','Refresh')}</Button><Button disabled={disabled} onClick={()=>{setRestoreId('');setShow(!show);setApproved(false)}}><Plus size={16}/>{tr('설치','Install')}</Button></div>
  </div>
  <div className="my-3 rounded-lg bg-canvas p-3 text-xs">
   {!inventory?tr('Docker 확인 중…','Checking Docker…'):inventory.available?tr('Docker 연결됨 · 설치 후 직접 실행','Docker connected · explicit start'):tr('Docker 연결 필요: ','Docker unavailable: ')+inventory.error}
  </div>
  {show&&<div ref={formRef} className="mb-4 space-y-3 rounded-xl border border-divider p-3">
   <h4 className="text-sm font-semibold">{restoreId?tr('데이터를 유지하고 다시 설치','Reinstall with retained data'):tr('새 플러그인 설치','Install a plugin')}</h4>
   {restoreId&&<p className="text-xs text-warning">{tr('기존 /data를 재사용합니다. 다른 이미지 버전으로 변경할 수 있지만 데이터 호환성과 롤백은 자동 검증하지 않습니다.','Reuses /data. A different image version is possible, but data compatibility and rollback are not automatically verified.')}</p>}
   <label className="block text-xs">{tr('표시 이름','Name')}<TextInput value={name} maxLength={120} onChange={e=>{setName(e.target.value);setApproved(false)}} placeholder={tr('예: 음성 비서','e.g. Voice assistant')}/></label>
   <label className="block text-xs">{tr('Docker 이미지와 버전','Docker image and version')}<TextInput value={image} maxLength={255} onChange={e=>{setImage(e.target.value);setApproved(false)}} placeholder="ghcr.io/owner/app:1.2.3"/></label>
   <p className="text-xs text-muted">{tr('공식 배포 이미지의 버전 또는 sha256 digest를 입력하세요. latest는 지원하지 않습니다. 설치는 실행하지 않습니다.','Enter a released version or sha256 digest. No latest tag. Install does not start the program.')}</p>
   <label className="flex items-start gap-2 text-xs"><input type="checkbox" checked={network} onChange={e=>{setNetwork(e.target.checked);setApproved(false)}}/>{tr('네트워크 허용 (인터넷 및 호스트 네트워크 접근 가능)','Allow network (may reach internet and host network)')}</label>
   {network&&<label className="block text-xs">{tr('내 PC에서 열 서비스 포트 (선택)','Service port to expose locally (optional)')}<TextInput type="number" min={1} max={65535} value={port} onChange={e=>{setPort(e.target.value);setApproved(false)}} placeholder="8080"/></label>}
   <p className="text-xs text-muted">{tr('기본 제한: 네트워크 차단, CPU 2개, 메모리 2GB, /data 독립 저장 공간. 호스트 파일과 키는 자동 전달하지 않습니다. 이미지에 따라 추가 설정이 필요할 수 있습니다.','Defaults: no network, 2 CPUs, 2GB RAM, isolated /data volume. No host files or API keys passed automatically. Some images require additional configuration.')}</p>
   <label className="flex items-start gap-2 text-xs"><input type="checkbox" checked={approved} onChange={e=>setApproved(e.target.checked)}/>{tr('이미지 출처와 실행 권한을 확인했습니다.','I reviewed the image source and execution permissions.')}</label>
   <div className="flex flex-wrap gap-2"><Button disabled={disabled||!approved||!name.trim()||!image.trim()||(network&&port!==''&&(!Number.isInteger(Number(port))||Number(port)<1||Number(port)>65535))} onClick={()=>void install()}>{busy==='install'?tr('이미지 받는 중…','Downloading image…'):tr('설치 확인','Install')}</Button><Button variant="ghost" disabled={!!busy} onClick={()=>setShow(false)}>{tr('닫기','Close')}</Button></div>
  </div>}
  {updating&&<div className="mb-4 space-y-2 rounded-lg border border-divider p-3">
   <strong className="text-sm">{updating.name} · {tr('버전 변경','Change version')}</strong>
   <p className="break-all text-xs text-muted">{tr('현재 버전','Current version')}: {updating.image}</p>
   <TextInput aria-label={tr('새 이미지 버전','New image version')} value={newImage} onChange={e=>setNewImage(e.target.value)} placeholder="owner/app:2.0"/>
   <div className="flex gap-2"><Button size="sm" disabled={disabled||!newImage.trim()} onClick={updateVersion}>{tr('버전 교체','Replace version')}</Button><Button size="sm" variant="ghost" disabled={!!busy} onClick={()=>setUpdating(null)}>{tr('닫기','Close')}</Button></div>
  </div>}
  {error&&<p role="alert" className="my-2 break-words text-xs text-danger">{error}</p>}
  {notice&&<p role="status" className="my-2 break-all text-xs text-muted">{notice}</p>}
  {rows?.length===0&&<p className="py-4 text-sm text-muted">{tr('설치된 플러그인이 없습니다. 아래에서 검색하거나 직접 이미지를 설치하세요.','No installed plugins. Search below or install an image directly.')}</p>}
  <div className="grid gap-3 md:grid-cols-2">{rows?.map(p=><article key={p.id} className="min-w-0 rounded-xl border border-divider p-3">
   <div className="flex flex-wrap justify-between gap-2"><strong className="break-all text-sm">{p.name}</strong><span className={p.running?'text-xs text-success':'text-xs text-muted'}>{p.running?tr('실행 중','Running'):p.state==='retained'?tr('데이터 보존됨','Data retained'):p.state==='created'?tr('설치됨','Installed'):p.state==='exited'?tr('중지됨','Stopped')+' · '+p.exitCode:p.state}</span></div>
   <p className="mt-1 break-all text-xs text-muted">{p.image}</p>
   {p.versionChange&&<p className="mt-1 break-all text-xs text-muted">{tr('복구 가능 이미지','Previous image')}: {p.versionChange.previousImage}</p>}
   <p className="mt-1 text-xs text-muted">{p.network?tr('네트워크 허용','Network allowed'):tr('네트워크 차단','Network off')}</p>
   {p.running&&p.ports.map(port=><a key={port} href={'http://127.0.0.1:'+port} target="_blank" rel="noopener noreferrer" className="mt-1 flex gap-1 text-xs text-brand">{tr('서비스 열기','Open service')} · {port}<ExternalLink size={12}/></a>)}
   <div className="mt-3 flex flex-wrap gap-1.5">
    {p.versionChange?.phase==='prepared'?<p className="text-xs text-warning">{tr('버전 변경 복구 필요','Version recovery required')}</p>:p.state==='retained'?<Button size="sm" disabled={disabled} onClick={()=>restore(p)}><Plus size={14}/>{tr('다시 설치·버전 변경','Reinstall / change version')}</Button>:<>
    <Button size="sm" disabled={disabled} onClick={()=>void operate(p,p.running?'stop':'start')}>{p.running?<Square size={14}/>:<Play size={14}/>} {p.running?tr('중지','Stop'):tr('실행','Start')}</Button>
    <Button size="sm" variant="secondary" disabled={disabled} onClick={()=>void operate(p,'logs')}><FileText size={14}/>{tr('로그','Logs')}</Button>
    <Button size="sm" variant="ghost" disabled={disabled||p.running} onClick={()=>void operate(p,'remove')}><Trash2 size={14}/>{tr('제거','Remove')}</Button>
    {!p.running&&<Button size="sm" variant="secondary" disabled={disabled||!p.imageId} onClick={()=>{setUpdating(p);setNewImage(p.image)}}>{tr('버전 변경','Change version')}</Button>}
    </>}
    {p.versionChange&&!p.running&&<Button size="sm" variant="secondary" disabled={disabled} onClick={()=>void rollback(p)}>{tr('이전 버전 복구','Restore previous version')}</Button>}
   </div>
   {p.running&&p.ports.length===1&&<PluginBridgePanel pluginId={p.id}/>}
  </article>)}</div>
  {logs&&<div className="mt-4 rounded-lg border border-divider p-3"><div className="flex items-center justify-between"><strong className="text-sm">{logs.name} · {tr('최근 로그','Recent logs')}</strong><Button variant="ghost" size="sm" onClick={()=>setLogs(null)}>{tr('닫기','Close')}</Button></div><pre className="mt-2 max-h-56 overflow-auto whitespace-pre-wrap break-all text-xs">{logs.data.stdout+'\n'+logs.data.stderr}</pre>{logs.data.truncated&&<p className="text-xs text-warning">{tr('로그가 잘렸습니다.','Logs truncated.')}</p>}</div>}
 </section>
}
