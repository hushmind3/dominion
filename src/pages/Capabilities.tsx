import {StatusPill} from '../components/StatusPill'
import {Button,TextInput} from '../components/Primitives'
import {useMemo,useState} from 'react'
import {Activity,ArrowRight,Blocks,BookOpenText,BrainCircuit,Check,ChevronDown,Cloud,Compass,ExternalLink,KeyRound,Layers3,MessageCircle,RefreshCw,Search,ShieldCheck,SlidersHorizontal,TerminalSquare,Workflow} from 'lucide-react'
import {get,post} from '../api'
import {Action,FormField,Headline,Notice,Panel,Subheading,useRemote} from '../components/UI'
type ChannelField={key:string;label:string;type:'secret'|'text'|'url';required?:boolean;placeholder?:string}
type Item={namespace:string;id:string;sourceId?:string;name:string;description?:string;status?:string;available?:boolean;features?:string[];docs?:string;origin?:string;category?:string;mode?:string;fields?:ChannelField[];instructions?:string[]}
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
 const [family,setFamily]=useState('all'),[query,setQuery]=useState(''),[error,setError]=useState(''),[details,setDetails]=useState<Item|null>(null),[result,setResult]=useState<any>(null),[working,setWorking]=useState(false),[channelForm,setChannelForm]=useState<Record<string,string>>({}),[channelBusy,setChannelBusy]=useState(false);
 const rows=useMemo(()=>data.value.items.filter(x=>(family==='all'||x.namespace===family)&&[x.name,x.namespace,x.description,x.id].join(' ').toLowerCase().includes(query.toLowerCase())),[data.value.items,family,query]);
 const counts=useMemo(()=>Object.fromEntries(families.map(x=>[x.id,x.id==='all'?data.value.items.length:data.value.items.filter(v=>v.namespace===x.id).length])),[data.value.items]);
 const invoke=async(item:Item,operation:string)=>{setWorking(true);setError('');try{setResult(await post('/api/capabilities/'+item.namespace+'/'+item.id+'/'+operation,{}));void data.reload()}catch(e){setError((e as Error).message)}finally{setWorking(false)}};
 const openItem=(x:Item)=>{setDetails(x);setResult(null);setError('');setChannelForm({name:x.namespace==='channel'?x.name:''})};
 const connectChannel=async()=>{
  if(!details||channelBusy)return;
  setChannelBusy(true);setError('');
  try{
   for(const field of details.fields||[])if(field.required&&!String(channelForm[field.key]||'').trim())throw Error(field.label+' 값이 필요합니다');
   const record:any={name:channelForm.name||details.name,kind:details.id,enabled:true,agentId:'general'};
   for(const field of details.fields||[])record[field.key]=channelForm[field.key]||'';
   setResult(await post('/api/channels',record));
   setChannelForm({name:details.name});void data.reload();
  }catch(e){setError((e as Error).message)}finally{setChannelBusy(false)}
 };
 const connectAccount=async(provider:string)=>{
  const popup=window.open('','dominion_oauth','width=650,height=780');
  try{const r=await get<{url:string}>('/api/oauth/'+provider+'/start');if(popup)popup.location.href=r.url;else window.location.href=r.url}catch(e){popup?.close();setError((e as Error).message)}
 };
 return <div className="mx-auto w-full max-w-[1320px] px-5 py-6 sm:px-8 lg:px-6 lg:pb-20">
  <Headline eyebrow="CONNECTION HUB" title="연결 · 채널" description="모델, MCP, 스킬, 메신저, OAuth 계정을 한곳에서 연결하고 실제 상태를 확인합니다." actions={<Action variant="secondary" onClick={()=>data.reload()}><RefreshCw size={16}/> 다시 검색</Action>}/>
  <div className="mb-5 flex items-center gap-4 rounded-xl border border-blue-100 bg-blue-50 p-5"><div className="grid size-12 shrink-0 place-items-center rounded-xl bg-blue-100 text-blue-600"><Layers3 size={22}/></div><div><strong>실행 가능한 능력과 카탈로그 항목을 구분합니다.</strong><p>이 화면은 실제 설정과 확장 매니페스트를 읽습니다. 준비되지 않은 채널은 코드가 없으므로 호출할 수 없으며, 모델은 실제 서버를 조회해 상태를 확인합니다.</p></div></div>
  {data.error&&<Notice message={data.error}/>}
  <div className="mb-5 flex flex-wrap items-center gap-3"><Search size={18}/><TextInput placeholder="모든 모델 · 메신저 · 도구 통합 검색" value={query} onChange={e=>setQuery(e.target.value)}/></div>
  <div className="mb-6 flex flex-wrap items-center gap-2 border-b border-slate-200 pb-3">{families.map(f=>{const Icon=f.icon;return <Button key={f.id} variant={family===f.id?'primary':'ghost'} onClick={()=>setFamily(f.id)}><Icon size={16}/>{f.label}<span>{counts[f.id]||0}</span></Button>})}</div>
  <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 xl:grid-cols-3">{rows.map(item=>{
   const fam=families.find(f=>f.id===item.namespace);
   const Icon=fam?.icon||Layers3;
   return <Button className="flex min-w-0 flex-col gap-3 rounded-xl border border-slate-200 bg-white p-5 text-left shadow-sm transition-shadow hover:shadow-md" onClick={()=>openItem(item)} key={item.namespace+':'+item.id}>
    <div className="flex items-start justify-between gap-3"><span className="grid size-11 shrink-0 place-items-center rounded-xl bg-blue-50 text-blue-600"><Icon size={22}/></span><StatusPill status={item.available?'ready':'pending'}>{item.available?'사용 준비':'미설치 · 미설정'}</StatusPill></div>
    <strong>{item.name}</strong><p>{item.description||item.namespace+' integration'}</p><div className="flex flex-wrap gap-1.5"><span>{fam?.label||item.namespace}</span>{(item.features||[]).slice(0,2).map(f=><span key={f}>{f}</span>)}</div>
    <div className="mt-auto flex items-center justify-between gap-3 border-t border-slate-100 pt-3 text-xs text-slate-500"><small>{item.status||'catalog'}</small><ArrowRight size={16}/></div>
   </Button>
  })}</div>
  {!rows.length&&<Panel><div className="flex min-h-40 flex-col items-center justify-center gap-2 p-5 text-center text-slate-500"><strong>해당 기능이 없습니다.</strong><p>연결된 계정과 확장 목록을 확인하세요.</p></div></Panel>}
  {details&&<div className="fixed inset-0 z-50 flex justify-end bg-slate-950/50" onMouseDown={()=>setDetails(null)}><aside className="flex h-full w-full max-w-xl flex-col overflow-y-auto bg-white p-4 shadow-sm" onMouseDown={e=>e.stopPropagation()}>
   <div className="flex items-center justify-between gap-3 border-b border-slate-200 pb-4"><div><small>{details.namespace.toUpperCase()} · CAPABILITY</small><h2>{details.name}</h2></div><Button size="icon" variant="ghost" onClick={()=>setDetails(null)}>✕</Button></div>
   <p className="py-5 text-sm leading-6 text-slate-600">{details.description||'등록된 기능'}</p>
   <div className="flex items-center justify-between gap-3 border-b border-slate-100 py-3 text-sm text-slate-600"><span>연결 ID</span><strong>{details.id}</strong></div>
   <div className="flex items-center justify-between gap-3 border-b border-slate-100 py-3 text-sm text-slate-600"><span>상태</span><strong>{details.status||'catalog'}</strong></div>
   <div className="flex items-center justify-between gap-3 border-b border-slate-100 py-3 text-sm text-slate-600"><span>실행 어댑터</span><strong>{details.available?'준비됨':'미설치 또는 비활성'}</strong></div>
   {details.docs&&<a className="inline-flex items-center gap-1.5 font-semibold text-blue-600 hover:underline" target="_blank" rel="noreferrer" href={details.docs}>원문 문서 보기 <ExternalLink size={15}/></a>}
   {details.namespace==='channel'&&<div className="mt-4 border-t border-slate-100 pt-4">
    <div className="mb-3 flex items-center justify-between gap-3"><strong className="text-sm">연결 설정</strong><span className={'rounded-md px-2 py-1 text-[10px] font-semibold '+(details.available?'bg-emerald-50 text-emerald-700':'bg-amber-50 text-amber-700')}>{details.available?'드라이버 준비됨':'드라이버 미구현'}</span></div>
    {details.available?<div className="grid gap-3">
     <FormField label="연결 이름"><TextInput value={channelForm.name||''} onChange={e=>setChannelForm({...channelForm,name:e.target.value})}/></FormField>
     {(details.fields||[]).map(field=><FormField key={field.key} label={field.label+(field.required?' *':'')}><TextInput type={field.type==='secret'?'password':field.type==='url'?'url':'text'} value={channelForm[field.key]||''} placeholder={field.placeholder||''} onChange={e=>setChannelForm({...channelForm,[field.key]:e.target.value})}/></FormField>)}
     <Action disabled={channelBusy} onClick={()=>void connectChannel()}>{channelBusy?'저장 중':'연결 저장 및 시작'}</Action>
    </div>:<p className="rounded-lg border border-amber-200 bg-amber-50 p-3 text-xs leading-5 text-amber-800">이 채널은 카탈로그에만 있고 실행 드라이버가 아직 없습니다. 실제 연결은 지원되는 채널에서만 가능합니다.</p>}
    {(details.instructions?.length||0)>0&&<ol className="mt-3 list-decimal space-y-1 pl-5 text-[11px] leading-5 text-slate-500">{(details.instructions||[]).map((text,index)=><li key={index}>{text}</li>)}</ol>}
   </div>}
   {error&&<Notice message={error}/>}
   {result&&<Panel className="rounded-xl border border-slate-200 bg-slate-50 p-4 text-sm leading-6"><strong>실제 실행 결과</strong><pre>{JSON.stringify(result,null,2).slice(0,12000)}</pre></Panel>}
   <div className="mt-5 flex flex-wrap justify-end gap-2">
    {details.namespace==='model'&&details.available&&<Action disabled={working} onClick={()=>invoke(details,'models')}><Activity size={16}/> 실제 모델 조회</Action>}
    {details.namespace==='mcp'&&details.available&&<Action disabled={working} onClick={()=>invoke(details,'tools')}><Activity size={16}/> MCP 도구 연결 검사</Action>}
    {details.namespace==='skill'&&<Action disabled={working} onClick={()=>invoke(details,details.available?'deactivate':'activate')}>{details.available?'스킬 비활성화':'스킬 활성화'}</Action>}
    {details.namespace==='account'&&details.available&&<Action disabled={working} onClick={()=>void connectAccount(details.id)}><KeyRound size={16}/> 계정 연결</Action>}
    {familesPage(details.namespace)&&details.namespace!=='channel'&&<Action variant="secondary" onClick={()=>{setDetails(null);navigate(familesPage(details.namespace)!)}}>설정 화면 열기 <ArrowRight size={16}/></Action>}
   </div>
  </aside></div>}
 </div>
}
function familesPage(kind:string):Page|null{return ({channel:'connections',model:'models',mcp:'extensions',skill:'extensions',account:'connections',tool:'extensions'} as Record<string,Page>)[kind]||null}
