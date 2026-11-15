import {useCallback,useEffect,useState,type FormEvent} from 'react'
import {Bot,Plus,RefreshCw,Trash2} from 'lucide-react'
import {get,post,del} from '../api'
import {Button,PageContainer,TextArea,TextInput} from '../components/Primitives'
import {FormField,Headline,Notice,Panel} from '../components/UI'

export type AgentPreset={
 id:string;name:string;description:string;instructions:string;createdAt:string
}
/** The main workspace visual language, connected to Rust-owned persona records. */
export function NativeAgents(){
 const [agents,setAgents]=useState<AgentPreset[]>([])
 const [name,setName]=useState(''),[description,setDescription]=useState(''),[instructions,setInstructions]=useState('')
 const [busy,setBusy]=useState(false),[error,setError]=useState(''),[selected,setSelected]=useState('')
 const reload=useCallback(async()=>{try{setAgents(await get<AgentPreset[]>('/api/agents'));setError('')}catch(e){setError((e as Error).message)}},[])
 useEffect(()=>{void reload()},[reload])
 const create=async(e:FormEvent)=>{e.preventDefault();setBusy(true);setError('')
  try{
   const record=await post<AgentPreset>('/api/agents',{name,description,instructions})
   setAgents(previous=>[record,...previous]);setSelected(record.id)
   setName('');setDescription('');setInstructions('')
  }catch(e){setError((e as Error).message)}finally{setBusy(false)}
 }
 const remove=async(id:string)=>{setBusy(true);setError('')
  try{await del('/api/agents/'+encodeURIComponent(id));setAgents(old=>old.filter(a=>a.id!==id));if(id===selected)setSelected('')}
  catch(e){setError((e as Error).message)}finally{setBusy(false)}
 }
 const active=agents.find(a=>a.id===selected)
 return <PageContainer>
  <Headline eyebrow="NATIVE AGENT PROFILES" title="에이전트" description="각 에이전트의 전문성·작업 방식을 저장하고 실행 화면에서 선택하세요. 모델 호출·도구 권한·완료 판정은 공통 Rust 엔진이 수행합니다."/>
  <div className="grid gap-4 lg:grid-cols-[minmax(0,1.2fr)_minmax(0,1fr)]">
   <Panel className="p-5"><h2 className="mb-4 text-[14px] font-bold">새 에이전트 만들기</h2>
    <form className="grid gap-3" onSubmit={e=>void create(e)}>
     <FormField label="이름"><TextInput required maxLength={80} value={name} onChange={e=>setName(e.target.value)} placeholder="예: 코드 검토 전문가"/></FormField>
     <FormField label="설명"><TextInput maxLength={500} value={description} onChange={e=>setDescription(e.target.value)} placeholder="예: 변경 전후 테스트와 회귀 위험 점검"/></FormField>
     <FormField label="전문 역할과 행동 지침"><TextArea required maxLength={8000} rows={7} value={instructions} onChange={e=>setInstructions(e.target.value)} placeholder="반드시 실제 파일을 확인하고 검증 증거가 없는 변경은 완료로 보고하지 않습니다."/></FormField>
     <div className="flex flex-wrap items-center justify-between gap-2"><small className="text-[#7C7469]">권한은 실행할 때 별도로 승인합니다.</small><Button variant="primary" type="submit" disabled={busy}><Plus size={14}/> {busy?'저장 중…':'에이전트 저장'}</Button></div>
    </form>
   </Panel>
   <div className="grid content-start gap-4"><Panel className="p-5">
    <div className="mb-4 flex items-center justify-between"><h2 className="font-bold">저장된 에이전트 · {agents.length}</h2><Button variant="ghost" size="sm" onClick={()=>void reload()}><RefreshCw size={14}/> 갱신</Button></div>
    <div className="grid max-h-[470px] gap-2 overflow-auto">{agents.map(a=><Button key={a.id} variant="bare" onClick={()=>setSelected(a.id)} className={'flex items-center gap-3 rounded-xl border p-3 text-left '+(selected===a.id?'border-[#D7A78B] bg-[#F9EFE8]':'border-[#ECE5DB]')}>
     <span className="grid size-9 shrink-0 place-items-center rounded-lg bg-[#F6F3EC]"><Bot size={17}/></span><span className="min-w-0"><strong className="block text-[12px]">{a.name}</strong><small className="block truncate text-[#7C7469]">{a.description||'사용자 설정 에이전트'}</small></span>
    </Button>)}
    {!agents.length&&<p className="py-8 text-center text-[12px] text-[#9C9388]">저장된 에이전트가 없습니다.</p>}</div>
   </Panel>
   {active&&<Panel className="p-5"><div className="mb-3 flex items-center justify-between"><h2 className="font-bold">{active.name}</h2><Button variant="danger" size="sm" disabled={busy} onClick={()=>void remove(active.id)}><Trash2 size={13}/> 삭제</Button></div><p className="mb-2 text-[12px] text-[#7C7469]">{active.description}</p><pre className="max-h-56 overflow-auto whitespace-pre-wrap break-all rounded-lg bg-[#F6F3EC] p-3 text-[12px]">{active.instructions}</pre></Panel>}
   </div>
  </div><Notice message={error}/>
 </PageContainer>
}
