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
 const channels=useRemote<any[]>('/api/channels',[]);
 const [family,setFamily]=useState('all'),[query,setQuery]=useState(''),[showPlanned,setShowPlanned]=useState(false),[error,setError]=useState(''),[details,setDetails]=useState<Item|null>(null),[result,setResult]=useState<any>(null),[working,setWorking]=useState(false),[channelForm,setChannelForm]=useState<Record<string,string>>({}),[channelBusy,setChannelBusy]=useState(false),[refreshing,setRefreshing]=useState(false),[lastRefresh,setLastRefresh]=useState(''),[probe,setProbe]=useState<Record<string,string>>({});
 const visibleItems=data.value.items.filter(item=>item.namespace!=='channel'||item.available||showPlanned);
 const rows=useMemo(()=>visibleItems.filter(x=>(family==='all'||x.namespace===family)&&[x.name,x.namespace,x.description,x.id].join(' ').toLowerCase().includes(query.toLowerCase())),[data.value.items,family,query,showPlanned]);
 const counts=useMemo(()=>Object.fromEntries(families.map(x=>[x.id,x.id==='all'?visibleItems.length:visibleItems.filter(v=>v.namespace===x.id).length])),[data.value.items,showPlanned]);
 const statusFor=(item:Item)=>{
  if(item.namespace==='tool')return {online:true,label:'내장 도구'};
  if(item.namespace==='skill')return {online:Boolean(item.available),label:item.available?'활성':'비활성'};
  if(item.namespace==='channel')return {online:Boolean(item.available),label:item.available?'드라이버 준비':'드라이버 없음'};
  if(item.namespace==='model')return {online:Boolean(item.available),label:item.available?'등록됨':'비활성'};
  if(item.namespace==='mcp')return {online:Boolean(item.available),label:item.available?'등록됨':'비활성'};
  if(item.namespace==='account')return {online:Boolean(item.available),label:item.available?'OAuth 설정됨':'설정 필요'};
  return {online:Boolean(item.available),label:item.available?'준비됨':'미설정'};
 };
 const invoke=async(item:Item,operation:string)=>{setWorking(true);setError('');try{setResult(await post('/api/capabilities/'+item.namespace+'/'+item.id+'/'+operation,{}));void data.reload()}catch(e){setError((e as Error).message)}finally{setWorking(false)}};
 const openItem=(x:Item)=>{setDetails(x);setResult(null);setError('');setChannelForm({name:x.namespace==='channel'?x.name:''})};
 const refreshAll=async()=>{
  setRefreshing(true);
  try{await Promise.all([data.reload(),channels.reload()]);setLastRefresh(new Date().toLocaleTimeString())}finally{setRefreshing(false)}
 };
 const diagnoseLive=async(id:string)=>{
  setProbe(x=>({...x,[id]:'진단 중'}));
  try{const r=await post<any>('/api/channels/'+id+'/diagnose',{});setProbe(x=>({...x,[id]:r.verified?'인증 확인됨':r.message||'연결 상태 확인 필요'}))}
  catch(e){setProbe(x=>({...x,[id]:(e as Error).message}))}
 };
 const connectChannel=async()=>{
  if(!details||channelBusy)return;
  setChannelBusy(true);setError('');
  try{
   for(const field of details.fields||[])if(field.required&&!String(channelForm[field.key]||'').trim())throw Error(field.label+' 값이 필요합니다');
   const record:any={name:channelForm.name||details.name,kind:details.id,enabled:true,agentId:'general'};
   for(const field of details.fields||[])record[field.key]=channelForm[field.key]||'';
   setResult(await post('/api/channels',record));
   setChannelForm({name:details.name});void data.reload();void channels.reload();
  }catch(e){setError((e as Error).message)}finally{setChannelBusy(false)}
 };
 const connectAccount=async(provider:string)=>{
  const popup=window.open('','dominion_oauth','width=650,height=780');
  try{const r=await get<{url:string}>('/api/oauth/'+provider+'/start');if(popup)popup.location.href=r.url;else window.location.href=r.url}catch(e){popup?.close();setError((e as Error).message)}
 };
 return <div className="mx-auto w-full max-w-[1320px] px-5 py-6 sm:px-8 lg:px-6 lg:pb-20">
  <Headline eyebrow="CONNECTION HUB" title="연결 · 채널" description="모델, MCP, 스킬, 메신저, OAuth 계정을 한곳에서 연결하고 실제 상태를 확인합니다." actions={<div className="flex items-center gap-3"><span className="text-[11px] text-[#8A8176]">{lastRefresh?'마지막 갱신 '+lastRefresh:'아직 갱신 전'}</span><Action variant="secondary" disabled={refreshing} onClick={()=>void refreshAll()}><RefreshCw size={16} className={refreshing?'animate-spin':''}/> {refreshing?'갱신 중':'새로고침'}</Action></div>}/>
  <Panel className="mb-5 p-4"><Subheading title="실제 연결" extra={<span className="text-[11px] text-[#8A8176]">{channels.value.length}개</span>}/>{channels.value.length?<div className="grid gap-2">{channels.value.map((channel:any)=><div key={channel.id} className="flex flex-wrap items-center justify-between gap-3 rounded-xl border border-[#E7E0D5] bg-[#FAF7F1] px-3 py-2.5"><div className="min-w-0"><strong className="text-[12px] text-[#3A342D]">{channel.name}</strong><small className="ml-2 text-[11px] text-[#8A8176]">{channel.kind} · {channel.status}{channel.runtimeError?' · '+channel.runtimeError:''}</small>{probe[channel.id]&&<small className="ml-2 text-[11px] text-[#C96442]">{probe[channel.id]}</small>}</div><div className="flex items-center gap-2"><span className={'size-2 rounded-full '+(channel.status==='connected'?'bg-emerald-500':'bg-[#B8AFA2]')}/><Button size="sm" variant="ghost" onClick={()=>void diagnoseLive(channel.id)}>진단</Button></div></div>)}</div>:<p className="text-[12px] text-[#8A8176]">아직 연결된 채널이 없습니다. 아래 카탈로그에서 사용할 채널을 선택해 설정하세요.</p>}</Panel>
  <div className="mb-5 flex items-center gap-4 rounded-xl border border-[#EED9CC] bg-[#F7E8E1] p-5"><div className="grid size-12 shrink-0 place-items-center rounded-xl bg-[#F1DCCF] text-[#C96442]"><Layers3 size={22}/></div><div><strong>실제 연결과 카탈로그를 구분합니다.</strong><p>위쪽은 실제로 저장·실행 중인 연결이고, 아래는 설치 가능한 기능 목록입니다. 채널을 선택하면 토큰 입력과 연결 시작까지 이 화면에서 처리합니다.</p></div></div>
  {data.error&&<Notice message={data.error}/>}
  <div className="mb-5 flex flex-wrap items-center gap-3"><Search size={18}/><TextInput placeholder="모든 모델 · 메신저 · 도구 통합 검색" value={query} onChange={e=>setQuery(e.target.value)}/></div>
  <div className="mb-6 flex flex-wrap items-center gap-2 border-b border-[#E7E0D5] pb-3">{families.map(f=>{const Icon=f.icon;return <Button key={f.id} variant={family===f.id?'primary':'ghost'} onClick={()=>setFamily(f.id)}><Icon size={16}/>{f.label}<span>{counts[f.id]||0}</span></Button>})}{(family==='all'||family==='channel')&&<Button variant={showPlanned?'secondary':'ghost'} onClick={()=>setShowPlanned(value=>!value)} className="ml-auto">{showPlanned?'미구현 숨기기':'미구현 포함'}</Button>}</div>
  <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 xl:grid-cols-3">{rows.map(item=>{
   const fam=families.find(f=>f.id===item.namespace);
   const Icon=fam?.icon||Layers3;
   const status=statusFor(item);
   return <Button className="flex min-w-0 flex-col gap-3 rounded-2xl border border-[#E7E0D5] bg-[#FFFDF9] p-5 text-left shadow-[0_1px_2px_rgba(36,33,29,.04)] transition-shadow hover:shadow-md" onClick={()=>openItem(item)} key={item.namespace+':'+item.id}>
    <div className="flex items-start justify-between gap-3"><span className="grid size-11 shrink-0 place-items-center rounded-xl bg-[#F7E8E1] text-[#C96442]"><Icon size={22}/></span><StatusPill status={status.online?'ready':'pending'}>{status.label}</StatusPill></div>
    <strong>{item.name}</strong><p>{item.description||item.namespace+' integration'}</p><div className="flex flex-wrap gap-1.5"><span>{fam?.label||item.namespace}</span>{(item.features||[]).slice(0,2).map(f=><span key={f}>{f}</span>)}</div>
    <div className="mt-auto flex items-center justify-between gap-3 border-t border-[#EFE9DF] pt-3 text-xs text-[#7C7469]"><small>{item.status||'catalog'}</small><ArrowRight size={16}/></div>
   </Button>
  })}</div>
  {!rows.length&&<Panel><div className="flex min-h-40 flex-col items-center justify-center gap-2 p-5 text-center text-[#7C7469]"><strong>해당 기능이 없습니다.</strong><p>연결된 계정과 확장 목록을 확인하세요.</p></div></Panel>}
  {details&&<div className="fixed inset-0 z-50 flex justify-end bg-slate-950/50" onMouseDown={()=>setDetails(null)}><aside className="flex h-full w-full max-w-xl flex-col overflow-y-auto bg-white p-4 shadow-sm" onMouseDown={e=>e.stopPropagation()}>
   <div className="flex items-center justify-between gap-3 border-b border-[#E7E0D5] pb-4"><div><small>{details.namespace.toUpperCase()} · CAPABILITY</small><h2>{details.name}</h2></div><Button size="icon" variant="ghost" onClick={()=>setDetails(null)}>✕</Button></div>
   <p className="py-5 text-sm leading-6 text-[#5C544A]">{details.description||'등록된 기능'}</p>
   <div className="flex items-center justify-between gap-3 border-b border-[#EFE9DF] py-3 text-sm text-[#5C544A]"><span>연결 ID</span><strong>{details.id}</strong></div>
   <div className="flex items-center justify-between gap-3 border-b border-[#EFE9DF] py-3 text-sm text-[#5C544A]"><span>상태</span><strong>{details.status||'catalog'}</strong></div>
   <div className="flex items-center justify-between gap-3 border-b border-[#EFE9DF] py-3 text-sm text-[#5C544A]"><span>{details.namespace==='channel'?'채널 드라이버':details.namespace==='account'?'OAuth 앱':'실행 어댑터'}</span><strong>{details.available?(details.namespace==='channel'?'드라이버 준비':details.namespace==='account'?'설정됨':'준비됨'):'미설치 또는 비활성'}</strong></div>
   {details.docs&&<a className="inline-flex items-center gap-1.5 font-semibold text-[#C96442] hover:underline" target="_blank" rel="noreferrer" href={details.docs}>원문 문서 보기 <ExternalLink size={15}/></a>}
   {details.namespace==='channel'&&<div className="mt-4 border-t border-[#EFE9DF] pt-4">
    <div className="mb-3 flex items-center justify-between gap-3"><strong className="text-sm">연결 설정</strong><span className={'rounded-md px-2 py-1 text-[10px] font-semibold '+(details.available?'bg-emerald-50 text-emerald-700':'bg-amber-50 text-amber-700')}>{details.available?'드라이버 준비됨':'드라이버 미구현'}</span></div>
    {details.available?<div className="grid gap-3">
     <FormField label="연결 이름"><TextInput value={channelForm.name||''} onChange={e=>setChannelForm({...channelForm,name:e.target.value})}/></FormField>
     {(details.fields||[]).map(field=><FormField key={field.key} label={field.label+(field.required?' *':'')}><TextInput type={field.type==='secret'?'password':field.type==='url'?'url':'text'} value={channelForm[field.key]||''} placeholder={field.placeholder||''} onChange={e=>setChannelForm({...channelForm,[field.key]:e.target.value})}/></FormField>)}
     <Action disabled={channelBusy} onClick={()=>void connectChannel()}>{channelBusy?'저장 중':'연결 저장 및 시작'}</Action>
    </div>:<p className="rounded-lg border border-amber-200 bg-amber-50 p-3 text-xs leading-5 text-amber-800">이 채널은 카탈로그에만 있고 실행 드라이버가 아직 없습니다. 실제 연결은 지원되는 채널에서만 가능합니다.</p>}
    {(details.instructions?.length||0)>0&&<ol className="mt-3 list-decimal space-y-1 pl-5 text-[11px] leading-5 text-[#7C7469]">{(details.instructions||[]).map((text,index)=><li key={index}>{text}</li>)}</ol>}
   </div>}
   {error&&<Notice message={error}/>}
   {result&&<Panel className="rounded-xl border border-[#E7E0D5] bg-[#FAF7F1] p-4 text-sm leading-6"><strong>실제 실행 결과</strong><pre>{JSON.stringify(result,null,2).slice(0,12000)}</pre></Panel>}
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
