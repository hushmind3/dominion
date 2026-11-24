import {useMemo,useState} from 'react'
import {Code2,RefreshCw,Link2,Copy,ShieldCheck} from 'lucide-react'
import {Button,SelectInput,TextArea} from '../../components/Primitives'
import {Panel,Notice,StateTag,useRemote} from '../../components/UI'
import {post} from '../../api'
import {useLocale} from '../../i18n'
import {SdkPluginRegistry} from './SdkPluginRegistry'
import {type SdkRun,sdkActive,sdkNeedsReview,sdkVerified,sdkSources,sdkControls} from '../../utils/sdkIntegration'

const endpoint='/api/sdk/runs'
const labels:Record<string,[string,string]>={
 queued:['대기','Queued'],running:['실행 중','Running'],awaiting_approval:['승인 대기','Awaiting approval'],
 in_doubt:['도구 결과 확인 필요','Effect unknown'],interrupted:['중단됨','Interrupted'],
 needs_review:['완료 증거 검토 필요','Needs review'],completed:['응답 완료','Completed'],
 failed:['실패','Failed'],cancelled:['취소됨','Cancelled'],exhausted:['예산 소진','Exhausted'],denied:['거부됨','Denied'],
}

/** This is the owner view of the actual SDK API, not a fabricated plugin
 * presence registry. Embedded SDKs in another process are not observable. */
export function SdkIntegrationPanel(){
 const en=useLocale()==='en'
 const tr=(ko:string,eng:string)=>en?eng:ko
 const runs=useRemote<SdkRun[]>(endpoint,[])
 const [source,setSource]=useState('')
 const [selected,setSelected]=useState('')
 const [busy,setBusy]=useState(false)
 const [error,setError]=useState('')
 const [notice,setNotice]=useState('')
 const [observation,setObservation]=useState('')
 const [applied,setApplied]=useState('')
 const [limit,setLimit]=useState(15)
 const groups=useMemo(()=>sdkSources(runs.value),[runs.value])
 const ordered=useMemo(()=>[...runs.value].sort((a,b)=>(b.updatedAt||b.createdAt||'').localeCompare(a.updatedAt||a.createdAt||'')),[runs.value])
 const filtered=source?ordered.filter(run=>JSON.stringify([run.source?.kind||'',run.source?.id||''])===source):ordered
 const run=filtered.find(item=>item.id===selected)
 const detail=useRemote<SdkRun|null>(run?endpoint+'/'+encodeURIComponent(run.id):null,null)
 const current=detail.value?.id===run?.id?detail.value:run
 const controls=current?sdkControls(current):null
 const state=(item:SdkRun)=>sdkVerified(item)?tr('검증 완료','Verified'):(labels[item.status]?.[en?1:0]||item.status)
 const time=(value?:string)=>value?new Date(value).toLocaleString(en?'en-US':'ko-KR'):'—'
 const act=async(action:string,body:unknown={},confirmation?:string)=>{
  if(!current||busy||detail.error||detail.loading||!detail.value)return
  if(confirmation&&!window.confirm(confirmation))return
  setBusy(true);setError('');setNotice('')
  try{
   await post(endpoint+'/'+encodeURIComponent(current.id)+'/'+action,body)
   setNotice(tr('요청을 처리했습니다. 서버 상태를 다시 확인합니다.','Request accepted. Refreshing server state.'))
   setObservation('');setApplied('')
   await Promise.all([runs.reload(),detail.reload()])
  }catch(e){setError((e as Error).message);await Promise.all([runs.reload(),detail.reload()])}
  finally{setBusy(false)}
 }
 const copy=async(value:string)=>{
  try{await navigator.clipboard.writeText(value);setNotice(tr('복사했습니다.','Copied.'));setError('')}
  catch(e){setError((e as Error).message)}
 }
 const base=window.location.origin
 const sample=JSON.stringify({instruction:tr('확인할 작업을 입력하세요','Enter a task'),source:{kind:'plugin',id:'my-plugin'},idempotencyKey:'replace-with-unique-task-id'},null,2)
 return <Panel className="mb-4 p-4" aria-label={tr('SDK 연결 및 실행 기록','SDK connections and run history')}>
  <div className="flex flex-wrap items-start justify-between gap-3">
   <div className="min-w-0">
    <h3 className="m-0 flex items-center gap-2 text-[15px] font-bold text-[#24211D]"><Code2 size={18}/>{tr('SDK 연동 관리','SDK integrations')}</h3>
    <p className="mt-1 text-[11px] leading-5 text-[#7C7469]">{tr('외부 프로그램이 이 도미니온 서버에 제출한 실제 작업을 확인하고 관리합니다.','Inspect and manage real tasks submitted to this DOMINION server by external programs.')}</p>
   </div>
   <div className="flex flex-wrap items-center gap-2">
    <StateTag online={!runs.loading&&!runs.error}>{runs.loading?tr('조회 중','Checking'):runs.error?tr('조회 실패','Unavailable'):tr('SDK API 응답 확인','SDK API responding')}</StateTag>
    <Button size="sm" onClick={()=>{void runs.reload();void detail.reload()}}><RefreshCw size={14}/>{tr('새로고침','Refresh')}</Button>
   </div>
  </div>
  <SdkPluginRegistry onSelect={id=>{setSource(JSON.stringify(['sdk_plugin',id]));setSelected('');setLimit(15)}}/>
  {runs.error&&<Notice message={runs.error}/>}
  {error&&<Notice message={error}/>}
  {notice&&<p role="status" className="mt-2 text-xs text-[#5C544A]">{notice}</p>}
  {!runs.loading&&!runs.error&&<div className="my-3 grid grid-cols-2 gap-2 sm:grid-cols-4">
   {[[tr('기록된 작업','Recorded runs'),runs.value.length],[tr('진행 중','Active'),runs.value.filter(sdkActive).length],
     [tr('확인 필요','Needs attention'),runs.value.filter(sdkNeedsReview).length],[tr('검증 성공','Verified'),runs.value.filter(sdkVerified).length]].map(([label,value])=>
    <div key={label} className="rounded-xl border border-[#E7E0D5] p-3"><span className="block text-[11px] text-[#7C7469]">{label}</span><strong className="text-lg">{value}</strong></div>)}
  </div>}
  <details className="my-3 rounded-xl border border-[#E7E0D5] p-3">
   <summary className="cursor-pointer text-[12px] font-semibold"><Link2 size={14} className="mr-1 inline"/>{tr('외부 프로그램 연결 방법','Connect an external program')}</summary>
   <p className="mt-2 text-xs leading-5">{tr('TypeScript SDK 또는 HTTP API로 연결하세요. Rust 내장 SDK는 별도 프로세스에서 실행되며 이 서버로 작업을 보내지 않으면 여기에 나타나지 않습니다.','Connect through the TypeScript SDK or HTTP API. A standalone embedded Rust SDK is not visible unless it submits work to this server.')}</p>
   <div className="mt-2 flex flex-wrap items-center gap-2 text-xs"><code className="break-all">{base+endpoint}</code><Button size="sm" onClick={()=>void copy(base+endpoint)}><Copy size={13}/>{tr('주소 복사','Copy URL')}</Button></div>
   <p className="mt-2 text-[11px] text-[#7C7469]">{tr('POST 요청 예시. 관리자 토큰을 설정한 서버에서는 기존 X-Dominion-Token 인증이 필요합니다. 작업마다 새 중복 방지 키를 사용하세요.','POST body example. Configured servers require the existing X-Dominion-Token authentication. Use a new retry key per logical task.')}</p>
   <pre className="mt-2 overflow-x-auto whitespace-pre-wrap break-all rounded-lg bg-[#F3EEE5] p-3 text-[11px]">{sample}</pre>
   <Button size="sm" onClick={()=>void copy(sample)}><Copy size={13}/>{tr('요청 예시 복사','Copy request')}</Button>
  </details>
  <p className="mb-2 flex items-start gap-1.5 text-[11px] leading-5 text-[#7C7469]"><ShieldCheck size={15} className="mt-0.5 shrink-0"/>
   {tr('작업 출처는 외부 프로그램이 제출한 이름입니다. 설치·온라인 상태 또는 권한을 증명하지 않습니다. 원격 장치 등록과 연결 해제는 아래 원격 워커에서 관리합니다.','Source labels are supplied by callers, not proof of installation, online presence or permissions. Manage enrolled remote devices in the remote worker panel below.')}</p>
  {!runs.loading&&!runs.error&&<>
   <div className="mb-2 flex flex-wrap items-center gap-2">
    <span className="text-xs font-semibold">{tr('실행 기록','Run history')}</span>
    <SelectInput className="!w-auto max-w-full !py-1.5" aria-label={tr('작업 출처 필터','Filter run source')} value={source} onChange={e=>{setSource(e.target.value);setSelected('');setLimit(15);setObservation('');setApplied('')}}>
     <option value="">{tr('모든 출처','All sources')}</option>
     {groups.map(group=><option key={group.key} value={group.key}>{group.kind?group.kind+' / '+group.id:tr('출처 미지정','Unspecified source')} ({group.runs.length})</option>)}
    </SelectInput>
   </div>
   {!filtered.length&&<p className="rounded-xl border border-dashed border-[#E7E0D5] p-4 text-center text-xs text-[#7C7469]">{tr('아직 기록된 SDK 작업이 없습니다. 외부 프로그램이 작업을 제출하면 여기에 표시됩니다.','No SDK runs recorded. Submitted external tasks will appear here.')}</p>}
   <div className="grid gap-1.5">
    {filtered.slice(0,limit).map(item=><button type="button" key={item.id} aria-pressed={selected===item.id} onClick={()=>{setSelected(selected===item.id?'':item.id);setObservation('');setApplied('');setError('');setNotice('')}} className={'min-w-0 rounded-lg border p-2.5 text-left '+(selected===item.id?'border-[#C96442] bg-[#F7E8E1]':'border-[#E7E0D5] hover:bg-[#F3EEE5]')}>
     <div className="flex flex-wrap items-center justify-between gap-2"><strong className="min-w-0 break-words text-xs">{item.instruction?.slice(0,160)||item.id}</strong><StateTag online={sdkVerified(item)}>{state(item)}</StateTag></div>
     <p className="mt-1 break-all text-[11px] text-[#7C7469]">{item.source?.kind?item.source.kind+' / '+item.source.id:tr('출처 미지정','Unspecified source')} · {item.model||'—'} · {time(item.updatedAt||item.createdAt)}</p>
    </button>)}
   </div>
   {filtered.length>limit&&<Button className="mt-2" size="sm" onClick={()=>setLimit(n=>n+15)}>{tr('더 보기','Show more')} ({filtered.length-limit})</Button>}
  </>}
  {current&&controls&&<div className="mt-3 rounded-xl border border-[#E7E0D5] bg-white p-3">
   <div className="flex flex-wrap items-center justify-between gap-2"><strong className="text-sm">{tr('작업 상세','Run details')}</strong><StateTag online={sdkVerified(current)}>{state(current)}</StateTag></div>
   <p className="mt-2 break-all text-[11px] text-[#7C7469]">ID: {current.id}{current.runRef?.executionId&&' · '+current.runRef.executionId}</p>
   <p className="mt-1 text-xs">{tr('모델 호출','Model calls')}: {current.checkpoint?.rounds??'—'} · {tr('도구 호출','Tool calls')}: {current.checkpoint?.toolCalls??'—'}</p>
   {current.error&&<Notice message={current.error}/>}
   {detail.error&&<Notice message={detail.error}/>}
   {detail.loading&&<p className="text-xs">{tr('상세 조회 중','Loading details')}</p>}
   {current.answer&&<pre className="mt-2 max-h-60 overflow-auto whitespace-pre-wrap break-words rounded-lg bg-[#F3EEE5] p-2 text-xs">{current.answer}</pre>}
   {controls.approve&&current.checkpoint?.pendingCall&&<div className="mt-2 rounded-lg border border-[#E7E0D5] p-2">
    <p className="text-xs font-semibold">{tr('승인 요청 도구','Tool awaiting approval')}: {current.checkpoint.pendingCall.name}</p>
    <pre className="max-h-40 overflow-auto whitespace-pre-wrap break-all text-[11px]">{JSON.stringify(current.checkpoint.pendingCall.arguments,null,2)}</pre>
    <Button size="sm" disabled={busy||detail.loading||!!detail.error||!detail.value} onClick={()=>void act('approve',{callId:current.checkpoint?.pendingCall?.id},tr('이 도구와 인자를 확인했고 실행을 승인하시겠습니까?','Approve this exact tool and arguments?'))}>{tr('도구 승인','Approve tool')}</Button>
   </div>}
   {controls.reconcile&&<div className="mt-2 grid gap-2 rounded-lg border border-amber-200 p-2">
    <p className="text-xs">{tr('실제 외부 결과를 직접 확인한 경우에만 기록하세요.','Record an observation only after independently inspecting the actual effect.')}</p>
    <SelectInput value={applied} onChange={e=>setApplied(e.target.value)} aria-label={tr('실제 도구 효과','Observed effect')}><option value="">{tr('결과 선택','Select observed result')}</option><option value="yes">{tr('실제로 적용됨','Applied')}</option><option value="no">{tr('적용되지 않음','Not applied')}</option></SelectInput>
    <TextArea value={observation} onChange={e=>setObservation(e.target.value)} maxLength={8192} placeholder={tr('직접 확인한 결과와 근거','Observed result and evidence')} aria-label={tr('확인 근거','Evidence')}/>
    <Button size="sm" disabled={busy||detail.loading||!!detail.error||!detail.value||!applied||!observation.trim()} onClick={()=>void act('reconcile',{applied:applied==='yes',details:observation.trim()},tr('이 확인 결과를 기록하시겠습니까?','Record this observation?'))}>{tr('확인 결과 저장','Save observation')}</Button>
   </div>}
   <div className="mt-3 flex flex-wrap gap-2">
    {controls.cancel&&<Button size="sm" variant="danger" disabled={busy||detail.loading||!!detail.error||!detail.value} onClick={()=>void act('cancel',{},tr('이 SDK 작업을 중단하시겠습니까?','Cancel this SDK run?'))}>{tr('작업 중단','Cancel')}</Button>}
    {controls.resume&&<Button size="sm" disabled={busy||detail.loading||!!detail.error||!detail.value} onClick={()=>void act('resume',{},tr('이 작업을 재개하시겠습니까? 모델 호출 비용이 발생할 수 있습니다.','Resume this run? Model calls may incur costs.'))}>{tr('작업 재개','Resume')}</Button>}
    {controls.recheck&&<Button size="sm" disabled={busy||detail.loading||!!detail.error||!detail.value} onClick={()=>void act('resume')}>{tr('완료 증거 재확인','Recheck completion evidence')}</Button>}
   </div>
  </div>}
 </Panel>
}
