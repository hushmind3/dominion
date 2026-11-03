import {Button,TextInput,TextArea,SelectInput} from '../components/Primitives'
import {useMemo,useState} from 'react'
import {BookOpenText,BrainCircuit,Check,Plus,Search,Trash2} from 'lucide-react'
import {del,post} from '../api'
import {Action,FormField,Headline,Notice,Panel,Subheading,useRemote} from '../components/UI'
type Memory={id:string;text:string;scope:string;createdAt?:string;updatedAt?:string}
export function MemoryPage(){
 const memories=useRemote<Memory[]>('/api/memories',[]);
 const agents=useRemote<any[]>('/api/agents',[]);
 const [query,setQuery]=useState(''),[text,setText]=useState(''),[scope,setScope]=useState('shared'),[error,setError]=useState(''),[notice,setNotice]=useState('');
 const filtered=useMemo(()=>memories.value.filter(x=>(x.text+' '+x.scope).toLowerCase().includes(query.toLowerCase())),[memories.value,query]);
 const save=async()=>{if(!text.trim())return;try{await post('/api/memories',{text:text.trim(),scope});setText('');setError('');setNotice('기억이 저장되었습니다.');void memories.reload()}catch(e){setError((e as Error).message)}};
 const drop=async(id:string)=>{if(!confirm('이 기억을 삭제할까요?'))return;try{await del('/api/memories/'+id);void memories.reload()}catch(e){setError((e as Error).message)}};
 return <div className="mx-auto w-full max-w-[1320px] px-5 py-9 sm:px-8 lg:px-11 lg:pb-20">
  <Headline eyebrow="MEMORY LIBRARY" title="기억을 지식으로." description="기억은 로컬 SQLite에 저장되고 에이전트의 다음 실행에 참고됩니다. 자동 학습·자기개선은 별개 기능입니다."/>
  {error&&<Notice message={error}/>}
  {notice&&<div className="my-3 flex items-center gap-2 rounded-xl border border-emerald-200 bg-emerald-50 p-3 text-sm text-emerald-700"><Check size={16}/>{notice}</div>}
  <div className="memory-layout"><Panel className="memory-editor"><Subheading title="새 기억 등록"/><FormField label="저장할 내용"><TextArea className="min-h-28" rows={6} placeholder="예: 프로젝트 기본 스택은 React + Node.js, UI는 모듈식 컴포넌트로 구성한다." value={text} onChange={e=>setText(e.target.value)}/></FormField><FormField label="적용 범위"><SelectInput  value={scope} onChange={e=>setScope(e.target.value)}><option value="shared">전체 에이전트</option>{agents.value.map(a=><option key={a.id} value={a.id}>{a.name}</option>)}</SelectInput></FormField><Action disabled={!text.trim()} onClick={save}><Plus size={16}/> 기억 저장</Action><p className="memory-hint"><BrainCircuit size={16}/> 현재 기억 검색은 텍스트 기반이며 의미 검색·시간적 정합성 평가·자동 리플렉션은 아직 연결되지 않았습니다.</p></Panel>
  <div><div className="memory-heading"><h3>저장된 기억 <span>{memories.value.length}</span></h3><div className="directory-search"><Search size={16}/><TextInput value={query} onChange={e=>setQuery(e.target.value)} placeholder="기억 검색"/></div></div><div className="memory-list">{filtered.map(m=><Panel key={m.id} className="memory-entry"><div className="memory-entry-top"><BookOpenText size={18}/><span>{m.scope==='shared'?'전체 공통':agents.value.find(a=>a.id===m.scope)?.name||m.scope}</span><Button size="icon" variant="ghost" onClick={()=>drop(m.id)} title="삭제"><Trash2 size={16}/></Button></div><p>{m.text}</p><small>{m.createdAt?new Date(m.createdAt).toLocaleString('ko-KR'):''}</small></Panel>)}{!filtered.length&&<Panel><div className="flex min-h-40 flex-col items-center justify-center gap-2 p-7 text-center text-slate-500"><BookOpenText size={24}/><strong>해당 기억이 없습니다.</strong></div></Panel>}</div></div></div>
 </div>
}
