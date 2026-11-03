import {useMemo,useState} from 'react'
import {Activity,ArrowRight,Blocks,BookOpenText,BrainCircuit,ExternalLink,KeyRound,Layers3,MessageSquareMore,RefreshCw,Server,TerminalSquare} from 'lucide-react'
import {post} from '../api'
import {Alert,Badge,Button,Card,CodeText,Drawer,Empty,Grid,Highlight,KeyValue,Page,PageHeader,Row,SearchBox,Section,Stack,Tabs,Text} from '../design/Primitives'
import {useOperation,useResource} from '../design/useResource'
type Item={namespace:string;id:string;sourceId?:string;name:string;description?:string;status?:string;available?:boolean;features?:string[];docs?:string;origin?:string}
type Page='models'|'connections'|'extensions'|'memory'
const families=[{value:'all',label:'전체'},{value:'channel',label:'메신저'},{value:'model',label:'모델'},{value:'mcp',label:'MCP'},{value:'skill',label:'스킬'},{value:'account',label:'OAuth'},{value:'tool',label:'내장 도구'}]
const sections:Record<string,Page>={channel:'connections',model:'models',mcp:'extensions',skill:'extensions',account:'connections',tool:'extensions'}
export function CapabilitiesPage({navigate}:{navigate:(page:Page)=>void}){
 const catalog=useResource<{items:Item[];updatedAt:string}>('/api/capabilities',{items:[],updatedAt:''})
 const op=useOperation()
 const [family,setFamily]=useState('all'),[query,setQuery]=useState(''),[selected,setSelected]=useState<Item|null>(null),[result,setResult]=useState<unknown>(null)
 const shown=useMemo(()=>catalog.value.items.filter(x=>(family==='all'||x.namespace===family)&&[x.name,x.namespace,x.id,x.description||''].join(' ').toLowerCase().includes(query.toLowerCase())),[catalog.value.items,family,query])
 const invoke=(item:Item,action:string)=>void op.run(async()=>{const result=await post('/api/capabilities/'+item.namespace+'/'+item.id+'/'+action,{});setResult(result);await catalog.reload()})
 return <Page><PageHeader eyebrow="CAPABILITY FABRIC" title="기능 디렉터리" description="모델·메신저·MCP·스킬·도구의 실제 등록 및 실행 가능 상태를 조회합니다." actions={<Button onClick={()=>void catalog.reload()}><RefreshCw size={16}/> 갱신</Button>}/>
 <Stack gap="lg">{catalog.error&&<Alert message={catalog.error}/>}
 <Highlight icon={<Layers3 size={16}/>} title="실제 실행 상태 구분">카탈로그 항목과 사용 가능한 실행 어댑터를 구분합니다. 등록만 되어 있는 기능은 준비 상태로 표시하지 않습니다.</Highlight>
 <Card><Stack gap="md"><SearchBox value={query} onChange={setQuery} placeholder="모든 연결, 도구, 모델을 검색"/><Tabs value={family} onChange={setFamily} items={families.map(f=>({...f,count:f.value==='all'?catalog.value.items.length:catalog.value.items.filter(x=>x.namespace===f.value).length}))}/></Stack></Card>
 <Grid cols={3}>{shown.map(item=><Card key={item.namespace+':'+item.id}><Stack gap="md"><Row justify="between"><span className="grid size-11 place-items-center rounded-xl bg-brand-soft text-brand"><Layers3 size={23}/></span><Badge tone={item.available?'positive':'warning'}>{item.available?'사용 준비':'미설정 · 미구현'}</Badge></Row><Stack gap="xs"><Text weight="bold" size="lg">{item.name}</Text><Text tone="muted">{(item.description||'설명 없음').slice(0,145)}</Text></Stack><Row wrap gap="xs"><Badge>{item.namespace}</Badge>{(item.features||[]).slice(0,3).map(f=><Badge key={f}>{f}</Badge>)}</Row><Row justify="between"><Text tone="subtle" size="xs">{item.status||'catalog'}</Text><Button size="sm" variant="quiet" onClick={()=>{setSelected(item);setResult(null)}}>자세히 <ArrowRight size={14}/></Button></Row></Stack></Card>)}</Grid>
 {!shown.length&&<Empty title="해당 기능이 없습니다" description="필터를 변경하거나 연결 설정을 확인하세요."/>}
 </Stack>
 <Drawer open={Boolean(selected)} onClose={()=>setSelected(null)} title={selected?.name||'기능 상세'} footer={<Row wrap gap="sm"><Button onClick={()=>setSelected(null)}>닫기</Button>{selected&&sections[selected.namespace]&&<Button variant="primary" onClick={()=>{navigate(sections[selected.namespace]);setSelected(null)}}>관리 화면 열기 <ArrowRight size={14}/></Button>}</Row>}>
 {selected&&<Stack gap="md"><Text tone="muted">{selected.description||'등록된 기능'}</Text><Card tone="subtle" pad="sm"><Stack gap="sm"><KeyValue label="ID" value={selected.id}/><KeyValue label="종류" value={selected.namespace}/><KeyValue label="상태" value={selected.status||'catalog'}/><KeyValue label="실행 어댑터" value={selected.available?'연결됨':'사용 불가'}/></Stack></Card>
 {selected.docs&&<Button onClick={()=>window.open(selected.docs,'_blank','noopener,noreferrer')}><ExternalLink size={15}/> 공식 문서</Button>}
 {op.error&&<Alert message={op.error} onClose={()=>op.setError('')}/>}
 {result!==null&&<CodeText>{JSON.stringify(result,null,2).slice(0,15000)}</CodeText>}
 {selected.namespace==='model'&&selected.available&&<Button variant="primary" loading={op.working} onClick={()=>invoke(selected,'models')}><Activity size={15}/> 실제 모델 확인</Button>}
 {selected.namespace==='mcp'&&selected.available&&<Button variant="primary" loading={op.working} onClick={()=>invoke(selected,'tools')}><Activity size={15}/> 도구 발견</Button>}
 {selected.namespace==='skill'&&<Button variant="primary" loading={op.working} onClick={()=>invoke(selected,selected.available?'deactivate':'activate')}>{selected.available?'스킬 비활성화':'스킬 활성화'}</Button>}
 </Stack>}
 </Drawer></Page>
}
