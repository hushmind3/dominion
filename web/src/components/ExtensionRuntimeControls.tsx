import {useState} from 'react'
import {Check,RefreshCw,SlidersHorizontal} from 'lucide-react'
import {post} from '../api'
import {useLocale} from '../i18n'
import {Panel,refresh,useRemote} from './UI'

type ExtensionKey='mcp'|'memory'|'skills'|'channels'|'strategies'|'connectors'
type RuntimePolicy={enabled:Partial<Record<ExtensionKey,boolean>>;configured:Partial<Record<ExtensionKey,boolean>>;runtime?:string}
const features:{key:ExtensionKey;ko:string;en:string;koDesc:string;enDesc:string}[]=[
 {key:'mcp',ko:'외부 MCP',en:'External MCP',koDesc:'연결된 외부 MCP 서버·도구 사용',enDesc:'Connected external MCP servers and tools'},
 {key:'memory',ko:'장기 기억',en:'Long-term memory',koDesc:'대화 외의 기억 검색·학습 (대화 기록은 유지)',enDesc:'Recall and learning (chat history remains)'},
 {key:'skills',ko:'스킬',en:'Skills',koDesc:'선택형 스킬 탐색·지침',enDesc:'Optional skill search and instructions'},
 {key:'channels',ko:'메시지 채널',en:'Message channels',koDesc:'Discord·Telegram 등 외부 채널 처리',enDesc:'External channels such as Discord and Telegram'},
 {key:'strategies',ko:'전략·적응',en:'Strategies & adaptation',koDesc:'선택형 전략·학습·개선 단계',enDesc:'Optional strategies, learning and adaptation'},
 {key:'connectors',ko:'내장 연결 도구',en:'Built-in connectors',koDesc:'웹·브라우저·GitHub·계정 연결 도구',enDesc:'Web, browser, GitHub and account connectors'},
]
/** Existing settings screen, not another tab or navigation route. Policy is
 * a replaceable object: preserve unrelated keys on each update. */
export function ExtensionRuntimeControls(){
 const locale=useLocale()
 const en=locale==='en'
 const remote=useRemote<RuntimePolicy>('/api/extensions/runtime',{enabled:{},configured:{}},false)
 const [saving,setSaving]=useState<ExtensionKey|'reset'|null>(null)
 const [error,setError]=useState('')
 const [saved,setSaved]=useState(false)
 const save=async(next:Partial<Record<ExtensionKey,boolean>>,key:ExtensionKey|'reset')=>{
  if(saving||remote.loading)return
  setSaving(key);setError('');setSaved(false)
  try{
   const result=await post<RuntimePolicy>('/api/extensions/runtime',next)
   remote.setValue({enabled:result.enabled||{},configured:result.configured||{}})
   setSaved(true)
   refresh('/api/extensions/runtime','/api/tools')
  }catch(err){setError((err as Error).message)}
  finally{setSaving(null)}
 }
 return <Panel className="min-w-0 p-5">
  <div className="flex flex-wrap items-center justify-between gap-3">
   <div className="flex items-center gap-2"><SlidersHorizontal size={18} className="text-[#A4583E]"/>
    <h3 className="m-0 text-sm font-bold">{en?'Runtime features':'실행 기능 제어'}</h3>
   </div>
   <button type="button" disabled={Boolean(saving)||remote.loading}
    onClick={()=>void save({},'reset')}
    className="inline-flex items-center gap-1.5 rounded-lg border border-[#E7E0D5] px-2.5 py-1.5 text-[11px] text-[#5C544A] hover:bg-[#FAF7F1] disabled:opacity-50">
    <RefreshCw size={13}/>{en?'Restore defaults':'기본값 복원'}
   </button>
  </div>
  <p className="mt-2 text-[11px] leading-5 text-[#8A8176]">{en
   ?'Enable or disable optional product capabilities. The native Rust engine, conversation records and stored credentials are not removed.'
   :'부가기능만 켜고 끕니다. Rust 실행 엔진·기존 대화·저장된 연결 정보는 삭제되지 않습니다.'}</p>
  {remote.loading&&<p role="status" className="mt-3 text-[11px] text-[#8A8176]">{en?'Loading settings…':'설정 불러오는 중…'}</p>}
  {remote.error&&<p role="alert" className="mt-2 text-xs text-rose-700">{remote.error}</p>}
  <div className="mt-3 grid gap-2">
   {features.map(feature=><label key={feature.key} className="flex cursor-pointer items-center justify-between gap-3 rounded-xl border border-[#EFE9DF] bg-white px-3 py-2.5">
    <span className="min-w-0"><strong className="block text-[12px] text-[#3A342D]">{en?feature.en:feature.ko}</strong>
     <small className="block text-[10px] leading-4 text-[#8A8176]">{en?feature.enDesc:feature.koDesc}</small>
    </span>
    <input type="checkbox" role="switch" checked={remote.value.enabled[feature.key]!==false}
     disabled={remote.loading||Boolean(saving)||Boolean(remote.error)}
     aria-label={en?feature.en:feature.ko}
     onChange={event=>void save({...remote.value.configured,[feature.key]:event.target.checked},feature.key)}
     className="size-4 shrink-0 accent-[#A4583E]"/>
   </label>)}
  </div>
  {saved&&<p role="status" className="mt-3 flex items-center gap-1 text-[11px] text-emerald-700"><Check size={13}/>{en?'Feature policy saved':'실행 기능 설정을 저장했습니다.'}</p>}
  {error&&<p role="alert" className="mt-3 break-words text-[11px] text-rose-700">{error}</p>}
 </Panel>
}
