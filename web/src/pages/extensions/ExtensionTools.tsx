import {useEffect,useState} from 'react'
import {get,post} from '../../api'
import {Button} from '../../components/Primitives'
import {useLocale} from '../../i18n'
type Catalog={enabled?:boolean;digest?:string;tools:{name:string;description:string;schema:unknown}[]}
export function ExtensionTools({pluginId}:{pluginId:string}){
 const en=useLocale()==='en',tr=(ko:string,eng:string)=>en?eng:ko
 const path='/api/plugins/managed/'+encodeURIComponent(pluginId)+'/tools'
 const [grant,setGrant]=useState<Catalog|null>(null),[preview,setPreview]=useState<Catalog|null>(null),[busy,setBusy]=useState(false),[error,setError]=useState('')
 useEffect(()=>{let active=true;get<Catalog>(path).then(r=>{if(active)setGrant(r)}).catch(e=>{if(active)setError((e as Error).message)});return()=>{active=false}},[path])
 const action=async(enabled:boolean|null)=>{if(busy)return;setBusy(true);setError('');try{
  if(enabled===null){const r=await get<Catalog>(path+'/discover');if(!r.digest||!Array.isArray(r.tools))throw Error(tr('도구 정보 오류','Invalid catalog'));setPreview(r)}
  else{setGrant(await post<Catalog>(path,{enabled,confirmed:true,digest:enabled?preview?.digest:null}));setPreview(null)}
 }catch(e){setError((e as Error).message)}finally{setBusy(false)}}
 return <div className="space-y-2 rounded-xl border border-divider bg-canvas p-3">
 <strong className="text-sm">{tr('확장 도구 연결','Extension tools')}</strong>
 <p className="text-muted">{tr('이 프로그램이 제공하는 도구를 도미니온에 등록합니다. 실제 실행은 작업별로 승인합니다.','Register the tools offered by this extension. Execution requires approval.')}</p>
 {grant?.enabled?<><p>{tr('등록됨','Enabled')} · {grant.tools?.length??0}</p><Button size="sm" disabled={busy} onClick={()=>void action(false)}>{tr('등록 해제','Disable tools')}</Button></>:<Button size="sm" disabled={busy||grant===null} onClick={()=>void action(null)}>{tr('제공 도구 확인','Discover tools')}</Button>}
 {preview&&<><div className="max-h-52 space-y-2 overflow-auto">{preview.tools.map(t=><details key={t.name}><summary className="font-semibold break-all">{t.name}</summary><p className="break-words">{t.description}</p><pre className="whitespace-pre-wrap break-all">{JSON.stringify(t.schema,null,2)}</pre></details>)}</div><Button size="sm" disabled={busy} onClick={()=>void action(true)}>{tr('확인한 도구 등록','Register reviewed tools')}</Button><Button size="sm" variant="ghost" disabled={busy} onClick={()=>setPreview(null)}>{tr('취소','Cancel')}</Button></>}
 {error&&<p role="alert" className="break-words text-warning">{error}</p>}
 </div>
}
