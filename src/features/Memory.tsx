import {useMemo,useState} from 'react'
import {BookOpenText,BrainCircuit,Plus,Search,Trash2} from 'lucide-react'
import {del,post,type Agent} from '../api'
import {Alert,Badge,Button,Card,Dialog,Empty,Input,ListItem,Page,PageHeader,Row,SearchBox,Select,Stack,Text,Textarea} from '../design/Primitives'
import {useOperation,useResource} from '../design/useResource'
type Memory={id:string;text:string;scope:string;createdAt?:string;updatedAt?:string}
export function MemoryPage(){
 const memories=useResource<Memory[]>('/api/memories',[])
 const agents=useResource<Agent[]>('/api/agents',[])
 const op=useOperation()
 const [query,setQuery]=useState('')
 const [editor,setEditor]=useState(false)
 const [content,setContent]=useState('')
 const [scope,setScope]=useState('shared')
 const shown=useMemo(()=>memories.value.filter(m=>m.text.toLowerCase().includes(query.toLowerCase())),[memories.value,query])
 const save=()=>void op.run(async()=>{if(!content.trim())throw Error('기억 내용을 입력하세요');await post('/api/memories',{text:content.trim(),scope});setEditor(false);setContent('');await memories.reload()})
 const remove=(m:Memory)=>{if(window.confirm('이 기억을 삭제할까요?'))void op.run(async()=>{await del('/api/memories/'+m.id);await memories.reload()})}
 return <Page><PageHeader eyebrow="PERSISTENT KNOWLEDGE" title="기억 라이브러리" description="에이전트가 참고할 정보를 직접 관리합니다. 내용은 로컬 데이터베이스에 저장됩니다." actions={<Button variant="primary" onClick={()=>setEditor(true)}><Plus size={16}/> 기억 추가</Button>}/>
  <Stack gap="lg">{op.error&&<Alert message={op.error} onClose={()=>op.setError('')}/>}
   <Card><Row justify="between" wrap><SearchBox value={query} onChange={setQuery} placeholder="기억 내용 검색"/><Badge>{shown.length} / {memories.value.length}개</Badge></Row></Card>
   <Card pad="none">{shown.map(m=><ListItem key={m.id} leading={<BookOpenText size={20}/>} title={m.text.slice(0,130)} subtitle={<Text tone="subtle" size="xs">{m.updatedAt||m.createdAt||'날짜 미기록'}</Text>} meta={<Badge tone={m.scope==='shared'?'brand':'neutral'}>{m.scope==='shared'?'공유':agents.value.find(a=>a.id===m.scope)?.name||m.scope}</Badge>} actions={<Button size="sm" variant="danger" onClick={()=>remove(m)}><Trash2 size={14}/> 삭제</Button>}/>)}
    {!shown.length&&<Empty icon={<Search size={22}/>} title={query?'검색 결과가 없습니다':'저장된 기억이 없습니다'} description="필요한 지식을 추가하면 에이전트가 참고할 수 있습니다."/>}
   </Card>
  </Stack>
  <Dialog open={editor} onClose={()=>setEditor(false)} title="새 기억 저장" footer={<><Button onClick={()=>setEditor(false)}>취소</Button><Button variant="primary" onClick={save} disabled={!content.trim()||op.working}>저장</Button></>}><Stack><Textarea label="기억 내용" value={content} onChange={e=>setContent(e.target.value)} rows={6} placeholder="저장할 정보를 입력하세요"/><Select label="적용 범위" value={scope} onChange={e=>setScope(e.target.value)} items={[{value:'shared',label:'모든 에이전트 공유'},...agents.value.map(a=>({value:a.id,label:a.name}))]}/></Stack></Dialog>
 </Page>
}
