import {useState} from 'react'
import {Activity,ArrowRight,Blocks,BookOpenText,Check,Cloud,Compass,ExternalLink,Plus,Search,Server,Settings2,Trash2} from 'lucide-react'
import {del,get,post} from '../api'
import {Alert,Badge,Button,Card,CodeText,Dialog,Empty,Grid,Highlight,IconButton,Input,ListItem,Page,PageHeader,Row,SearchBox,Section,Select,Stack,Tabs,Text,Textarea,Toggle} from '../design/Primitives'
import {useOperation,useResource} from '../design/useResource'
type Mcp={id:string;name:string;transport:'http'|'stdio';url?:string;command?:string;args?:string[];enabled:boolean;token?:string}
type Skill={id:string;name:string;instructions:string;enabled:boolean;status?:string}
type Registry={name:string;description:string;url?:string;packageName?:string}
export function ExtensionsPage(){
 const mcp=useResource<Mcp[]>('/api/mcp',[])
 const skills=useResource<Skill[]>('/api/skills',[])
 const operation=useOperation()
 const [tab,setTab]=useState('market')
 const [query,setQuery]=useState('')
 const [found,setFound]=useState<Registry[]>([])
 const [notice,setNotice]=useState('')
 const [editing,setEditing]=useState<Partial<Mcp>|null>(null)
 const [args,setArgs]=useState('')
 const [skill,setSkill]=useState<Partial<Skill>|null>(null)
 const [tools,setTools]=useState<any[]>([])
 const search=()=>void operation.run(async()=>{
  if(query.trim().length<3)throw Error('3글자 이상 입력하세요')
  setFound(await get<Registry[]>('/api/mcp/catalog?q='+encodeURIComponent(query.trim())))
 })
 const editMcp=(v?:Mcp)=>{setEditing(v?{...v}:{name:'',transport:'stdio',command:'npx',args:[],url:'',enabled:true});setArgs((v?.args||[]).join('\n'))}
 const saveMcp=()=>void operation.run(async()=>{if(!editing?.name)throw Error('이름이 필요합니다');await post('/api/mcp',{...editing,args:args.split('\n').map(x=>x.trim()).filter(Boolean)});setEditing(null);await mcp.reload()})
 const install=(v:Registry)=>{if(window.confirm(v.name+' MCP 서버를 등록하시겠습니까?'))void operation.run(async()=>{await post('/api/mcp',{name:v.name,transport:v.packageName?'stdio':'http',command:'npx',args:v.packageName?['-y',v.packageName]:[],url:v.url||''});setNotice(v.name+' 등록됨');await mcp.reload()})}
 const discover=()=>void operation.run(async()=>{setTools(await get('/api/mcp/discover'));setNotice('도구 검색을 마쳤습니다.')})
 const remove=(v:Mcp)=>{if(window.confirm(v.name+' 연결을 제거할까요?'))void operation.run(async()=>{await del('/api/mcp/'+v.id);await mcp.reload()})}
 const saveSkill=()=>void operation.run(async()=>{if(!skill?.name?.trim())throw Error('스킬 이름이 필요합니다');await post('/api/skills',skill);setSkill(null);await skills.reload()})
 return <Page><PageHeader eyebrow="EXTENSION SYSTEM" title="MCP · 스킬" description="도구를 연결하고 에이전트의 지침과 능력을 확장합니다." actions={<Button variant="primary" onClick={()=>editMcp()}><Plus size={16}/> MCP 추가</Button>}/>
  <Stack gap="lg">{operation.error&&<Alert message={operation.error} onClose={()=>operation.setError('')}/>}
   {notice&&<Alert message={notice} tone="positive" onClose={()=>setNotice('')}/>}
   <Card pad="none"><Tabs value={tab} onChange={setTab} items={[{value:'market',label:'도구 탐색'},{value:'installed',label:'연결된 MCP',count:mcp.value.length},{value:'skills',label:'스킬',count:skills.value.length}]}/>
   {tab==='market'&&<div className="p-4 sm:p-6"><Stack gap="lg"><Highlight title="MCP Registry" icon={<Compass size={16}/>}>공식 Registry에서 메타데이터를 검색합니다. 검색된 항목과 실제 설치된 연결은 별도로 표시됩니다.</Highlight><Row gap="sm"><SearchBox value={query} onChange={setQuery} placeholder="MCP 이름 검색 · 예: browser, GitHub"/><Button variant="primary" disabled={operation.working} onClick={search}><Search size={16}/> 검색</Button></Row><Grid cols={2}>{found.map(x=><Card key={x.name} pad="sm"><Stack gap="md"><Row justify="between"><Row gap="sm"><Server size={20} className="text-brand"/><Text weight="bold">{x.name}</Text></Row><Badge>{x.packageName?'npm · stdio':'remote'}</Badge></Row><Text tone="muted">{x.description||'MCP server'}</Text><Text tone="subtle" size="xs">{x.packageName||x.url||'설치 정보 없음'}</Text><Button variant="quiet" disabled={!x.packageName&&!x.url} onClick={()=>install(x)}><Plus size={15}/> 연결 등록</Button></Stack></Card>)}</Grid>{!found.length&&<Empty title="MCP를 검색해 보세요" description="검색 결과에서 로컬 실행 또는 원격 서버를 등록할 수 있습니다."/>}</Stack></div>}
   {tab==='installed'&&<div className="p-4 sm:p-6"><Stack gap="md"><Row justify="between" wrap><Text weight="bold">연결된 MCP 서버</Text><Button onClick={discover} disabled={operation.working}><Activity size={15}/> 도구 검사</Button></Row><Card pad="none">{mcp.value.map(x=><ListItem key={x.id} title={x.name} subtitle={x.transport==='stdio'?x.command+' '+(x.args||[]).join(' '):x.url} leading={<Blocks size={20}/>} meta={<Badge tone={tools.some(y=>y.mcpId===x.id&&!y.error)?'positive':'neutral'}>{tools.some(y=>y.mcpId===x.id&&!y.error)?'도구 연결됨':'등록됨'}</Badge>} actions={<><IconButton size="sm" label="수정" icon={<Settings2 size={16}/>} onClick={()=>editMcp(x)}/><IconButton size="sm" label="삭제" icon={<Trash2 size={16}/>} onClick={()=>remove(x)}/></>}/>)}{!mcp.value.length&&<Empty title="등록된 MCP 서버가 없습니다"/>}</Card>{tools.length>0&&<Card><Stack gap="sm"><Text weight="bold">검색된 도구 {tools.filter(t=>!t.error).length}개</Text>{tools.map((t,i)=><Row key={i} justify="between"><Text size="sm">{t.name}</Text><Text size="xs" tone={t.error?'negative':'subtle'}>{t.error||t.description||'ready'}</Text></Row>)}</Stack></Card>}</Stack></div>}
   {tab==='skills'&&<div className="p-4 sm:p-6"><Stack gap="md"><Row justify="between"><Text weight="bold">지식과 스킬</Text><Button variant="primary" onClick={()=>setSkill({name:'',instructions:'',enabled:true})}><Plus size={15}/> 스킬 작성</Button></Row><Grid cols={3}>{skills.value.map(s=><Card key={s.id} pad="sm"><Stack gap="md"><Row justify="between"><BookOpenText size={22} className="text-brand"/><Badge tone={s.enabled?'positive':'neutral'}>{s.enabled?'활성':'비활성'}</Badge></Row><Text weight="bold">{s.name}</Text><Text tone="muted">{s.instructions.slice(0,145)}</Text><Button size="sm" onClick={()=>setSkill({...s})}>편집 <ArrowRight size={14}/></Button></Stack></Card>)}</Grid>{!skills.value.length&&<Empty title="저장된 스킬이 없습니다"/>}</Stack></div>}
  </Card></Stack>
  <Dialog open={Boolean(editing)} onClose={()=>setEditing(null)} title="MCP 서버 설정" wide footer={<><Button onClick={()=>setEditing(null)}>취소</Button><Button variant="primary" onClick={saveMcp} disabled={!editing?.name?.trim()||operation.working}><Check size={15}/> 저장</Button></>}>
   {editing&&<Stack><Input label="이름" value={editing.name||''} onChange={e=>setEditing({...editing,name:e.target.value})}/><Select label="연결 방식" value={editing.transport||'stdio'} onChange={e=>setEditing({...editing,transport:e.target.value as Mcp['transport']})} items={[{value:'stdio',label:'stdio · 로컬 프로세스'},{value:'http',label:'HTTP · 원격 MCP'}]}/>{editing.transport==='http'?<><Input label="서버 URL" value={editing.url||''} onChange={e=>setEditing({...editing,url:e.target.value})}/><Input label="Bearer Token" type="password" value={editing.token||''} onChange={e=>setEditing({...editing,token:e.target.value})}/></>:<><Input label="실행 명령" value={editing.command||''} onChange={e=>setEditing({...editing,command:e.target.value})}/><Textarea label="명령 인수 (한 줄씩)" value={args} onChange={e=>setArgs(e.target.value)}/></>}<Toggle label="활성화" checked={Boolean(editing.enabled)} onChange={v=>setEditing({...editing,enabled:v})}/></Stack>}
  </Dialog>
  <Dialog open={Boolean(skill)} onClose={()=>setSkill(null)} title="스킬 편집" wide footer={<><Button onClick={()=>setSkill(null)}>취소</Button><Button variant="primary" onClick={saveSkill} disabled={!skill?.name?.trim()}>저장</Button></>}>
   {skill&&<Stack><Input label="이름" value={skill.name||''} onChange={e=>setSkill({...skill,name:e.target.value})}/><Textarea label="실행 지침" rows={10} value={skill.instructions||''} onChange={e=>setSkill({...skill,instructions:e.target.value})}/><Toggle checked={Boolean(skill.enabled)} label="에이전트 지침에 적용" onChange={v=>setSkill({...skill,enabled:v,status:v?'approved':'proposed'})}/></Stack>}
  </Dialog>
 </Page>
}
