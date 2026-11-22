import {RefreshCw,SlidersHorizontal} from 'lucide-react'
import {useLocale} from '../i18n'
import {Button} from './Primitives'
import {Panel,useRemote} from './UI'

type Status={mcp?:number;skills?:number;memories?:number;providers?:number;agents?:number}
type Channel={id:string}
type Tool={name:string;source?:string}
const format=(value:number|undefined,en:boolean)=>value===undefined?(en?'Unavailable':'조회 불가'):value.toLocaleString()

/** The Sub2 harness does not implement Sub4's global extension_policy switch.
 * Report real native capabilities; never acknowledge a setting that would
 * not affect tool admission or the model's execution policy. */
export function ExtensionRuntimeControls(){
 const en=useLocale()==='en'
 const status=useRemote<Status>('/api/status',{})
 const channels=useRemote<Channel[]>('/api/channels',[])
 const tools=useRemote<Tool[]>('/api/tools',[])
 const loading=status.loading||channels.loading||tools.loading
 const rows=[
  {key:'mcp',ko:'외부 MCP',en:'External MCP',count:status.error?undefined:status.value.mcp,
   detailKo:'저장된 MCP 연결 수',detailEn:'Saved MCP connections'},
  {key:'memory',ko:'장기 기억',en:'Long-term memory',count:status.error?undefined:status.value.memories,
   detailKo:'저장된 기억 기록',detailEn:'Saved memory records'},
  {key:'skills',ko:'스킬',en:'Skills',count:status.error?undefined:status.value.skills,
   detailKo:'등록된 스킬 수',detailEn:'Registered skills'},
  {key:'channels',ko:'메시지 채널',en:'Message channels',count:channels.error?undefined:channels.value.length,
   detailKo:'저장된 채널 수',detailEn:'Configured channels'},
  {key:'connectors',ko:'내장 도구',en:'Built-in tools',
   count:tools.error?undefined:tools.value.filter(tool=>tool.source==='builtin').length,
   detailKo:'실제 Rust 도구 정의',detailEn:'Native Rust tool definitions'},
 ]
 const reload=()=>{void status.reload();void channels.reload();void tools.reload()}
 return <Panel className="min-w-0 p-5">
  <div className="flex flex-wrap items-center justify-between gap-3">
   <h3 className="flex items-center gap-2 text-sm font-bold text-ink">
    <SlidersHorizontal size={17} className="text-brand"/>
    {en?'Native feature status':'네이티브 기능 현황'}
   </h3>
   <Button type="button" variant="secondary" size="sm" onClick={reload} disabled={loading}>
    <RefreshCw size={14}/>{en?'Refresh':'새로고침'}
   </Button>
  </div>
  <p className="mt-2 text-xs leading-5 text-muted">
   {en?'This is the actual Sub2 runtime inventory. Capabilities and permission controls are managed by its native Rust services, not unsupported frontend switches.':
    '서브2 Rust 엔진에 실제 연결된 기능을 표시합니다. 실행 권한은 기존 네이티브 설정에서 관리하며, 작동하지 않는 일괄 스위치는 노출하지 않습니다.'}
  </p>
  {(status.error||channels.error||tools.error)&&<p role="alert" className="mt-3 text-xs text-rose-700">
   {[status.error,channels.error,tools.error].filter(Boolean).join(' · ')}
  </p>}
  <dl className="mt-4 grid gap-2">
   {rows.map(row=><div key={row.key} className="flex items-center justify-between gap-3 rounded-xl border border-divider bg-surface px-3 py-3">
    <div><dt className="text-xs font-semibold text-ink">{en?row.en:row.ko}</dt>
     <dd className="mt-1 text-xs text-muted">{en?row.detailEn:row.detailKo}</dd>
    </div>
    <strong className="text-lg font-bold tabular-nums text-ink">{loading?'…':format(row.count,en)}</strong>
   </div>)}
  </dl>
 </Panel>
}
