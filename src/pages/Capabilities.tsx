import {Button,TextInput} from '../components/Primitives'
import {useMemo,useState} from 'react'
import {Activity,ArrowRight,Blocks,BookOpenText,BrainCircuit,Check,ChevronDown,Cloud,Compass,ExternalLink,KeyRound,Layers3,MessageCircle,RefreshCw,Search,ShieldCheck,SlidersHorizontal,TerminalSquare,Workflow} from 'lucide-react'
import {post} from '../api'
import {Action,Headline,Notice,Panel,Subheading,useRemote} from '../components/UI'
type Item={namespace:string;id:string;sourceId?:string;name:string;description?:string;status?:string;available?:boolean;features?:string[];docs?:string;origin?:string}
type Page='models'|'connections'|'extensions'|'memory'
const families:{id:string;label:string;icon:any;page?:Page}[]=[
 {id:'all',label:'전체',icon:Layers3},
 {id:'channel',label:'메신저',icon:MessageCircle,page:'connections'},
 {id:'model',label:'모델',icon:BrainCircuit,page:'models'},
 {id:'mcp',label:'MCP',icon:Blocks,page:'extensions'},
 {id:'skill',label:'스킬',icon:BookOpenText,page:'extensions'},
 {id:'account',label:'OAuth',icon:KeyRound,page:'connections'},
 {id:'tool',label:'내장 도구',icon:TerminalSquare,page:'extensions'}
]
export function CapabilitiesPage({navigate}:{navigate:(page:Page)=>void}){
 const data=useRemote<{items:Item[];updatedAt:string}>('/api/capabilities',{items:[],updatedAt:''});
 const [family,setFamily]=useState('all'),[query,setQuery]=useState(''),[error,setError]=useState(''),[details,setDetails]=useState<Item|null>(null),[result,setResult]=useState<any>(null),[working,setWorking]=useState(false);
 const rows=useMemo(()=>data.value.items.filter(x=>(family==='all'||x.namespace===family)&&[x.name,x.namespace,x.description,x.id].join(' ').toLowerCase().includes(query.toLowerCase())),[data.value.items,family,query]);
 const counts=useMemo(()=>Object.fromEntries(families.map(x=>[x.id,x.id==='all'?data.value.items.length:data.value.items.filter(v=>v.namespace===x.id).length])),[data.value.items]);
 const invoke=async(item:Item,operation:string)=>{setWorking(true);setError('');try{setResult(await post('/api/capabilities/'+item.namespace+'/'+item.id+'/'+operation,{}));void data.reload()}catch(e){setError((e as Error).message)}finally{setWorking(false)}};
 const openItem=(x:Item)=>{setDetails(x);setResult(null);setError('')};
 return <div className="content-wrap">
  <Headline eyebrow="DOMINION CAPABILITY FABRIC" title="능력을 한곳에서 발견하고 실행하세요." description="모델, 메신저, MCP, 기억 스킬, OAuth 계정과 도구를 서로 다른 앱에서 복사하지 않고 하나의 독립 커널에서 조회합니다." actions={<Action variant="secondary" onClick={()=>data.reload()}><RefreshCw size={16}/> 다시 검색</Action>}/>
  <div className="section-intro"><div className="section-intro-icon"><Layers3 size={22}/></div><div><strong>실행 가능한 능력과 카탈로그 항목을 구분합니다.</strong><p>이 화면은 실제 설정과 확장 매니페스트를 읽습니다. 준비되지 않은 채널은 코드가 없으므로 호출할 수 없으며, 모델은 실제 서버를 조회해 상태를 확인합니다.</p></div></div>
  {data.error&&<Notice message={data.error}/>}
  <div className="cap-search"><Search size={18}/><TextInput placeholder="모든 모델 · 메신저 · 도구 통합 검색" value={query} onChange={e=>setQuery(e.target.value)}/></div>
  <div className="cap-tabs">{families.map(f=>{const Icon=f.icon;return <Button key={f.id} className={family===f.id?'active':''} onClick={()=>setFamily(f.id)}><Icon size={16}/>{f.label}<span>{counts[f.id]||0}</span></Button>})}</div>
  <div className="cap-grid">{rows.map(item=>{
   const fam=families.find(f=>f.id===item.namespace);
   const Icon=fam?.icon||Layers3;
   return <Button className="cap-tile" onClick={()=>openItem(item)} key={item.namespace+':'+item.id}>
    <div className="cap-tile-top"><span className="cap-symbol"><Icon size={22}/></span><span className={'cap-state '+(item.available?'ready':'pending')}>{item.available?'사용 준비':'미설치 · 미설정'}</span></div>
    <strong>{item.name}</strong><p>{item.description||item.namespace+' integration'}</p><div className="cap-tags"><span>{fam?.label||item.namespace}</span>{(item.features||[]).slice(0,2).map(f=><span key={f}>{f}</span>)}</div>
    <div className="cap-tile-foot"><small>{item.status||'catalog'}</small><ArrowRight size={16}/></div>
   </Button>
  })}</div>
  {!rows.length&&<Panel><div className="empty"><strong>해당 기능이 없습니다.</strong><p>연결된 계정과 확장 목록을 확인하세요.</p></div></Panel>}
  {details&&<div className="drawer-backdrop" onMouseDown={()=>setDetails(null)}><aside className="detail-drawer" onMouseDown={e=>e.stopPropagation()}>
   <div className="drawer-head"><div><small>{details.namespace.toUpperCase()} · CAPABILITY</small><h2>{details.name}</h2></div><Button className="icon-button" onClick={()=>setDetails(null)}>✕</Button></div>
   <p className="drawer-intro">{details.description||'등록된 기능'}</p>
   <div className="setting-info"><span>연결 ID</span><strong>{details.id}</strong></div>
   <div className="setting-info"><span>상태</span><strong>{details.status||'catalog'}</strong></div>
   <div className="setting-info"><span>실행 어댑터</span><strong>{details.available?'준비됨':'미설치 또는 비활성'}</strong></div>
   {details.docs&&<a className="external-link" target="_blank" rel="noreferrer" href={details.docs}>원문 문서 보기 <ExternalLink size={15}/></a>}
   {error&&<Notice message={error}/>}
   {result&&<Panel className="cap-result"><strong>실제 실행 결과</strong><pre>{JSON.stringify(result,null,2).slice(0,12000)}</pre></Panel>}
   <div className="cap-actions">
    {details.namespace==='model'&&details.available&&<Action disabled={working} onClick={()=>invoke(details,'models')}><Activity size={16}/> 실제 모델 조회</Action>}
    {details.namespace==='mcp'&&details.available&&<Action disabled={working} onClick={()=>invoke(details,'tools')}><Activity size={16}/> MCP 도구 연결 검사</Action>}
    {details.namespace==='skill'&&<Action disabled={working} onClick={()=>invoke(details,details.available?'deactivate':'activate')}>{details.available?'스킬 비활성화':'스킬 활성화'}</Action>}
    {familesPage(details.namespace)&&<Action variant="secondary" onClick={()=>{setDetails(null);navigate(familesPage(details.namespace)!)}}>설정 화면 열기 <ArrowRight size={16}/></Action>}
   </div>
  </aside></div>}
 </div>
}
function familesPage(kind:string):Page|null{return ({channel:'connections',model:'models',mcp:'extensions',skill:'extensions',account:'connections',tool:'extensions'} as Record<string,Page>)[kind]||null}
