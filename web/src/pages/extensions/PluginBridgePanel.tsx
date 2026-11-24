import {useEffect,useState} from 'react'
import {get,post} from '../../api'
import {Button} from '../../components/Primitives'
import {useLocale} from '../../i18n'
import {Link2,RefreshCw} from 'lucide-react'
import {UniversalConnection} from './UniversalConnection'

type Grant={enabled:boolean;expiresAt?:number;remaining:number;lastError?:string;activeRequest?:string}
type Request={requestId:string;instruction:string;requestDigest:string;runId?:string;status:string;answer?:string;answerDigest?:string;delivery:string}
export function PluginBridgePanel({pluginId}:{pluginId:string}){
 const en=useLocale()==='en',tr=(ko:string,english:string)=>en?english:ko
 const [request,setRequest]=useState<Request|null>(null),[busy,setBusy]=useState(false),[notice,setNotice]=useState(''),[mode,setMode]=useState<'chat'|'workspace'>('chat')
 const [open,setOpen]=useState(false),[checked,setChecked]=useState(false),[grant,setGrant]=useState<Grant|null>(null)
 const id=encodeURIComponent(pluginId),rid=request?encodeURIComponent(request.requestId):''
 useEffect(()=>{
  if(!rid||!request||!['queued','running','awaiting_approval'].includes(request.status))return
  let active=true
  const interval=setInterval(()=>{void get<Request>('/api/plugins/managed/'+id+'/bridge/'+rid).then(r=>{if(active)setRequest(r)}).catch(e=>{if(active)setNotice((e as Error).message)})},2500)
  return()=>{active=false;clearInterval(interval)}
 },[id,rid,request?.status])
 useEffect(()=>{
  if(!open)return
  let active=true
  const load=()=>void get<Grant>('/api/plugins/managed/'+id+'/auto-chat').then(g=>{if(active)setGrant(g)}).catch(()=>{})
  load();const timer=setInterval(load,4000);return()=>{active=false;clearInterval(timer)}
 },[id,open])
 const action=async(fn:()=>Promise<void>)=>{if(busy)return;setBusy(true);setNotice('');try{await fn()}catch(e){setNotice((e as Error).message)}finally{setBusy(false)}}
 const toggleAuto=()=>action(async()=>{
  const enabled=!grant?.enabled
  if(enabled&&!window.confirm(tr('30분 동안 최대 20번의 대화 요청을 자동 처리하고 답변을 이 프로그램으로 전달합니다. 도구·공유 기억·스킬은 사용하지 않습니다. 모델 요금이 발생할 수 있습니다.','For 30 minutes, automatically process up to 20 chat turns and share answers with this app. No tools, shared memory or skills. Model charges may apply.')))return
  setGrant(await post<Grant>('/api/plugins/managed/'+id+'/auto-chat',{enabled,confirmed:true}))
 })
 const pull=()=>action(async()=>{
  const r=await post<{request:Request|null}>('/api/plugins/managed/'+id+'/bridge/pull',{})
  setRequest(r.request);setChecked(false);setMode('chat')
  if(!r.request)setNotice(tr('대기 중인 요청이 없습니다.','No pending request.'))
 })
 const approve=()=>action(async()=>{
  if(!request||!checked)return
  setRequest(await post<Request>('/api/plugins/managed/'+id+'/bridge/'+rid+'/approve',{confirmed:true,requestDigest:request.requestDigest,mode}))
  setChecked(false)
 })
 const deliver=()=>action(async()=>{
  if(!request||!checked)return
  setRequest(await post<Request>('/api/plugins/managed/'+id+'/bridge/'+rid+'/deliver',{confirmed:true,answerDigest:request.answerDigest}))
  setChecked(false)
 })
 return <div className="mt-3 border-t border-divider pt-2">
  <Button size="sm" variant="ghost" onClick={()=>setOpen(!open)}><Link2 size={15}/>{tr('도미니온 작업 연결','DOMINION task connection')}</Button>
  {open&&<div className="mt-2 space-y-2 text-xs">
   <UniversalConnection pluginId={pluginId}/>
   <details><summary className="cursor-pointer text-muted">{tr('이전 SDK v1 연결 (구버전 앱)','Legacy SDK v1 connection')}</summary>
   <p className="text-muted">{tr('SDK 연결 규격을 지원하는 외부 프로그램에서 요청을 받아 도미니온이 처리합니다. 승인한 요청과 답변만 전달합니다.','Pull requests from compatible apps and send only owner-approved answers.')}</p>
   <div className="rounded-lg bg-canvas p-2 space-y-2"><strong>{tr('음성·자동 대화 연결','Voice / automatic chat')}</strong><p className="text-muted">{grant?.enabled?tr('남은 대화: ','Turns remaining: ')+grant.remaining:tr('꺼짐 · 수동 승인만 사용','Off · manual review only')}</p>{grant?.lastError&&<p className="break-words text-warning">{grant.lastError}</p>}<Button size="sm" disabled={busy||grant===null} onClick={()=>void toggleAuto()}>{grant?.enabled?tr('자동 대화 중지','Stop automatic chat'):tr('30분 자동 대화 허용','Allow automatic chat for 30 minutes')}</Button></div>
   <Button size="sm" disabled={busy} onClick={()=>void pull()}><RefreshCw size={14}/>{tr('요청 가져오기','Fetch request')}</Button>
   {notice&&<p role="status" className="break-words text-warning">{notice}</p>}
   {request&&<div className="space-y-2 rounded-lg bg-canvas p-3">
    <p className="font-semibold">{tr('외부 프로그램 요청','External app request')} · {request.status}</p>
    <p className="max-h-32 overflow-auto whitespace-pre-wrap break-words">{request.instruction}</p>
    {!request.runId&&<>
     <label className="flex gap-2"><input type="radio" checked={mode==='chat'} onChange={()=>{setMode('chat');setChecked(false)}}/>{tr('대화만 (도구 없음)','Conversation only (no tools)')}</label>
     <label className="flex gap-2"><input type="radio" checked={mode==='workspace'} onChange={()=>{setMode('workspace');setChecked(false)}}/>{tr('작업 폴더 읽기·쓰기 (셸·MCP 없음)','Workspace read/write (no shell or MCP)')}</label>
     <label className="flex gap-2"><input type="checkbox" checked={checked} onChange={e=>setChecked(e.target.checked)}/>{tr('이 요청과 권한으로 작업 실행을 승인합니다.','Approve this request and scope.')}</label>
     <Button size="sm" disabled={busy||!checked} onClick={()=>void approve()}>{tr('실행 승인','Approve run')}</Button>
    </>}
    {request.runId&&<><p className="break-all text-muted">Run: {request.runId}</p><a href="#/tasks" className="text-brand">{tr('작업 관리에서 승인·진행 확인','Open Tasks for progress and approvals')}</a></>}
    {request.status==='completed'&&request.answerDigest&&<>
     <div className="max-h-48 overflow-auto whitespace-pre-wrap break-words border-t border-divider pt-2">{request.answer}</div>
     {request.delivery==='sent'?<p className="text-success">{tr('답변 전달 완료','Answer delivered')}</p>:<>
      {request.delivery==='delivery_unknown'&&<p className="text-warning">{tr('이전 전달 결과가 불명확합니다. 같은 요청 ID로 재시도합니다.','Prior delivery is uncertain; retry uses the same request ID.')}</p>}
      <label className="flex gap-2"><input type="checkbox" checked={checked} onChange={e=>setChecked(e.target.checked)}/>{tr('위 답변을 외부 프로그램에 전달합니다.','Share the displayed answer with this app.')}</label>
      <Button size="sm" disabled={busy||!checked} onClick={()=>void deliver()}>{tr('답변 전달','Deliver answer')}</Button>
     </>}
    </>}
   </div>}
  </details></div>}
 </div>
}
