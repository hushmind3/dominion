import {useCallback,useEffect,useState} from 'react'
import {Activity,BookOpenText,Braces,Check,Code2,Cpu,Plug,RefreshCw,ShieldCheck,Terminal} from 'lucide-react'
import {get} from '../api'
import {Button,PageContainer} from '../components/Primitives'
import {Headline,Notice,Panel,StateTag} from '../components/UI'

type Model={id:string;model:string;origin:string;keyConfigured:boolean;
 inputKrwPerMillion:number;cachedInputKrwPerMillion:number;outputKrwPerMillion:number}
type Config={models:Model[];default:string}
type Tool={name:string;description:string;permission:string;source:string;parameters?:unknown}
type Tools={tools:Tool[];mcpServers:number}
type Run={id:string;input:{prompt:string;agentId?:string};status:string;steps:number;tokens:number;costKrw:number;createdAt:string}
const faded='text-[12px] text-[#7C7469]'
const money=(value:number)=>Number(value||0).toLocaleString('ko-KR',{maximumFractionDigits:2})+'원'
function useRemote<T>(url:string,initial:T){
 const [value,setValue]=useState(initial),[error,setError]=useState(''),[loading,setLoading]=useState(true)
 const reload=useCallback(async()=>{
  try{setValue(await get<T>(url));setError('')}
  catch(e){setError((e as Error).message)}finally{setLoading(false)}
 },[url])
 useEffect(()=>{void reload()},[reload])
 return {value,error,loading,reload}
}
function Header({title,onRefresh}:{title:string;onRefresh:()=>void}){
 return <div className="mb-3 flex items-center justify-between gap-2"><h2 className="text-[13px] font-bold">{title}</h2>
  <Button variant="ghost" size="sm" onClick={onRefresh}><RefreshCw size={13}/> 새로고침</Button></div>
}
export function NativeModels(){
 const remote=useRemote<Config>('/api/providers',{models:[],default:'auto'})
 return <PageContainer><Headline eyebrow="MODEL ROUTING" title="모델 허브"
  description="현재 Rust 실행 엔진에 실제 등록된 제공자와 원화 단가를 확인합니다. 일반 작업은 자동·절약·품질 우선 정책으로 라우팅됩니다."/>
  <Panel className="p-5"><Header title="설정된 모델" onRefresh={()=>void remote.reload()}/><Notice message={remote.error}/>
  <div className="grid gap-3 md:grid-cols-2">{remote.value.models.map(model=><article key={model.id} className="rounded-xl border border-[#E7E0D5] bg-white p-4">
   <div className="mb-3 flex items-center justify-between gap-3"><span className="inline-flex items-center gap-2 font-bold"><Cpu size={16}/>{model.id==='quality'?'품질 우선 모델':'절약형 모델'}</span><StateTag online={true}>설정됨</StateTag></div>
   <h3 className="break-words text-[15px] font-semibold">{model.model}</h3>
   <p className={'mt-1 break-words '+faded}>연결 원본: {model.origin||'비공개'}</p>
   <p className={'mt-1 '+faded}>API 키: {model.keyConfigured?'서버에 별도 등록됨':'없음 / 로컬 연결'}</p>
   <div className="mt-4 grid gap-2 rounded-xl bg-[#F6F3EC] p-3 text-[12px]">
    <div className="flex justify-between gap-2"><span>입력 100만 토큰</span><strong>{money(model.inputKrwPerMillion)}</strong></div>
    <div className="flex justify-between gap-2"><span>캐시 입력 100만 토큰</span><strong>{money(model.cachedInputKrwPerMillion)}</strong></div>
    <div className="flex justify-between gap-2"><span>출력 100만 토큰</span><strong>{money(model.outputKrwPerMillion)}</strong></div>
   </div>
  </article>)}</div>
  {!remote.loading&&!remote.value.models.length&&<p className={faded}>현재 사용 가능한 설정 항목이 없습니다.</p>}
  </Panel>
  <Panel className="mt-4 p-5"><h2 className="mb-3 text-[13px] font-bold">연결 설정</h2>
   <p className={faded}>프로세스 환경변수 DOMINION_MODEL_URL / DOMINION_MODEL / DOMINION_API_KEY로 주 모델을 설정합니다. 절약형 모델은 DOMINION_ECONOMY_MODEL_URL 및 DOMINION_ECONOMY_MODEL로 연결합니다.</p>
   <p className={'mt-2 '+faded}>DOMINION_INPUT_KRW_PER_MILLION, DOMINION_CACHED_INPUT_KRW_PER_MILLION, DOMINION_OUTPUT_KRW_PER_MILLION으로 원화 요율을 설정하면 작업별 비용과 제한이 연동됩니다. 단가가 0원이면 청구액을 추정할 수 없으므로 실제 무료 사용을 의미하지 않습니다.</p>
  </Panel>
 </PageContainer>
}
export function NativeExtensions(){
 const tools=useRemote<Tools>('/api/tools',{tools:[],mcpServers:0})
 const [search,setSearch]=useState('')
 const filtered=tools.value.tools.filter(item=>
  (item.name+' '+item.description+' '+item.source).toLowerCase().includes(search.toLowerCase()))
 const read=tools.value.tools.filter(item=>item.source==='native').length
 const external=tools.value.tools.filter(item=>item.source==='mcp').length
 return <PageContainer><Headline eyebrow="MCP & TOOLS" title="확장 도구"
  description="Rust 실행 엔진이 광고하는 내장 도구와 실시간 MCP 서버 도구를 확인합니다. 권한은 각 작업에서 다시 지정합니다."/>
  <div className="mb-4 grid grid-cols-3 gap-3">{[
   {label:'네이티브 도구',value:read,icon:Code2},
   {label:'MCP 서버',value:tools.value.mcpServers,icon:Plug},
   {label:'연결된 MCP 도구',value:external,icon:Braces}
  ].map(item=><div key={item.label} className="rounded-xl border border-[#E7E0D5] bg-white p-4">
   <span className={'flex items-center justify-between gap-1 '+faded}>{item.label}<item.icon size={15}/></span>
   <strong className="mt-3 block text-2xl">{item.value}</strong>
  </div>)}</div>
  <Panel className="p-5"><Header title="실행 엔진 도구 목록" onRefresh={()=>void tools.reload()}/>
   <Notice message={tools.error}/>
   <input aria-label="도구 검색" value={search} onChange={e=>setSearch(e.target.value)}
    placeholder="도구 이름, 기능 검색" className="mb-3 w-full rounded-xl border border-[#E7E0D5] px-4 py-2.5 text-[12px]"/>
   <div className="grid gap-2 md:grid-cols-2">{filtered.map(item=><div key={item.name} className="rounded-xl border border-[#ECE5DB] p-3 text-[12px]">
    <div className="flex items-center justify-between gap-2"><strong className="break-all">{item.name}</strong><span className={'shrink-0 '+faded}>{item.source==='mcp'?'MCP':'Rust'}</span></div>
    <p className="mt-2 text-[#5C544A]">{item.description}</p>
    <div className={'mt-2 inline-flex items-center gap-1 '+faded}><ShieldCheck size={12}/> 필요 권한: {item.permission}</div>
   </div>)}</div>
   {!filtered.length&&<p className={faded}>검색 조건에 해당하는 도구가 없습니다.</p>}
  </Panel>
  <Panel className="mt-4 p-5"><h2 className="mb-2 font-bold">MCP 연결</h2>
   <p className={faded}>서버 연결 목록은 런타임 설정 DOMINION_MCP_SERVERS에서 읽어옵니다. 외부 도구는 작업별 MCP 허용이 있어야 노출되며, 실제 호출은 Rust가 실행 직전 권한과 응답 형식을 검사합니다.</p>
  </Panel>
 </PageContainer>
}
export function NativeDeveloper(){
 const runs=useRemote<Run[]>('/api/tasks',[])
 const [selected,setSelected]=useState(''),[events,setEvents]=useState<unknown[]>([])
 const [error,setError]=useState('')
 useEffect(()=>{if(!selected){setEvents([]);return}
  let live=true
  const stream=new EventSource('/api/tasks/'+encodeURIComponent(selected)+'/events')
  stream.onmessage=message=>{if(!live)return;try{const event=JSON.parse(message.data);setEvents(value=>[...value,event].slice(-500));if(event.type==='finished')stream.close()}catch{}}
  stream.onerror=()=>{if(live)setError('이벤트 스트림 연결이 일시적으로 끊겼습니다.')}
  return()=>{live=false;stream.close()}
 },[selected])
 return <PageContainer><Headline eyebrow="EXECUTION INSPECTOR" title="개발자 · 실행 추적"
  description="실제 SQLite 작업 영수증과 단계별 이벤트를 관찰합니다. 모델의 텍스트를 검증 증거로 간주하지 않습니다."/>
  <div className="grid gap-4 lg:grid-cols-[minmax(250px,.8fr)_minmax(0,1.4fr)]">
   <Panel className="p-4"><Header title="실행 목록" onRefresh={()=>void runs.reload()}/><Notice message={runs.error}/>
    <div className="max-h-[550px] space-y-2 overflow-auto">{runs.value.map(item=><Button key={item.id} variant="bare"
      onClick={()=>setSelected(item.id)} className={'block w-full rounded-lg border p-3 text-left text-[12px] '+(selected===item.id?'border-[#C96442] bg-[#F9EFE8]':'border-[#ECE5DB]')}>
     <div className="line-clamp-2 font-semibold">{item.input.prompt}</div><p className={'mt-1 '+faded}>{item.status} · {item.tokens.toLocaleString()}토큰 · {money(item.costKrw)}</p>
    </Button>)}</div>
   </Panel>
   <Panel className="p-4"><div className="mb-3 flex items-center justify-between"><h2 className="font-bold">영속 실행 이벤트 · {events.length}건</h2>{selected&&<span className={faded}>{selected.slice(0,8)}</span>}</div>
    <Notice message={error}/>
    <div className="max-h-[600px] space-y-2 overflow-auto">{events.map((event,index)=><pre key={index} className="overflow-x-auto whitespace-pre-wrap break-all rounded-lg border border-[#ECE5DB] bg-[#F6F3EC] p-3 text-[11px]">{JSON.stringify(event,null,2)}</pre>)}</div>
   </Panel>
  </div>
 </PageContainer>
}
