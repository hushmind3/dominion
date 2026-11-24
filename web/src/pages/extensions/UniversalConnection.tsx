import {useEffect,useState} from 'react'
import {get,post} from '../../api'
import {Button,SelectInput} from '../../components/Primitives'
import {useLocale} from '../../i18n'
type Status={enabled:boolean;mode?:string;tools?:string[];lastError?:string;lastStatus?:string;usedToday?:number;dailyLimit?:number;lastRunId?:string}
export function UniversalConnection({pluginId}:{pluginId:string}){
 const en=useLocale()==='en',tr=(ko:string,english:string)=>en?english:ko
 const [status,setStatus]=useState<Status|null>(null),[mode,setMode]=useState('web'),[busy,setBusy]=useState(false),[error,setError]=useState('')
 const path='/api/plugins/managed/'+encodeURIComponent(pluginId)+'/connection'
 useEffect(()=>{let alive=true;const load=()=>void get<Status>(path).then(v=>{if(alive)setStatus(v)}).catch(e=>{if(alive)setError((e as Error).message)});load();const timer=setInterval(load,3000);return()=>{alive=false;clearInterval(timer)}},[path])
 const change=async()=>{if(busy||!status)return;const enabled=!status.enabled
  if(enabled&&!window.confirm(tr('이 프로그램이 선택한 범위에서 도미니온 작업을 요청하고 답변을 받을 수 있습니다. 검색 외 도구는 별도 승인을 받으며 모델 비용이 발생할 수 있습니다.','Allow this app to submit tasks and receive results within the chosen scope? Non-search tools require approval. Model costs may apply.')))return
  setBusy(true);setError('');try{setStatus(await post<Status>(path,{enabled,mode,confirmed:true}))}catch(e){setError((e as Error).message)}finally{setBusy(false)}
 }
 return <div className="space-y-2 rounded-xl border border-divider bg-canvas p-3">
 <strong className="text-sm">{tr('공통 SDK 연결','Universal SDK connection')} · v2</strong>
 <p className="text-muted">{tr('한 번 연결하면 PC를 재시작해도 같은 설치 프로그램의 요청을 자동으로 처리합니다. 앱 업데이트 시 재승인합니다.','Connect once; resume for the same installed app after restart. Updates require review.')}</p>
 {!status?.enabled?<SelectInput value={mode} onChange={e=>setMode(e.target.value)}><option value="chat">{tr('대화','Conversation')}</option><option value="web">{tr('웹 검색·브라우저','Web search and browser')}</option><option value="workspace">{tr('작업 폴더','Workspace')}</option><option value="agent">{tr('전체 에이전트 도구 (작업별 승인)','Agent tools (per-action approval)')}</option></SelectInput>:<><p className="text-success">{tr('연결됨','Connected')} · {status.mode} · {status.usedToday??0}/{status.dailyLimit??100}</p><p className="text-muted">{status.lastStatus||tr('요청 대기 중','Waiting for requests')}</p>{status.lastRunId&&<a href="#/tasks" className="text-brand">{tr('실제 작업 진행 및 승인 확인','View actual tasks and approvals')}</a>}{!!status.tools?.length&&<details><summary>{tr('허용된 도구','Granted tools')} · {status.tools.length}</summary><p className="break-words">{status.tools.join(', ')}</p></details>}</>}
 <Button size="sm" disabled={busy||status===null} onClick={()=>void change()}>{busy?tr('처리 중…','Working…'):status?.enabled?tr('연결 해제','Disconnect'):tr('도미니온에 연결','Connect to DOMINION')}</Button>
 {(error||status?.lastError)&&<p role="alert" className="break-words text-warning">{error||status?.lastError}</p>}
 </div>
}
