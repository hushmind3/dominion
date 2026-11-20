import { useMemo, useState } from "react";
import { Search, RefreshCw, Server, ExternalLink, ShieldAlert, CheckCircle2, LoaderCircle } from "lucide-react";
import { Button, TextInput } from "../../components/Primitives";
import { t, useLocale } from "../../i18n";
import { get, post, del } from "../../api";

type RegistryItem = { name:string; description?:string; packageName?:string; url?:string };
type Installed = { id:string; name:string };
export function McpRegistryMarket({installed,onChanged}:{installed:Installed[];onChanged:()=>void}) {
 const locale=useLocale(), ko=locale!=="en";
 const [query,setQuery]=useState(""),[items,setItems]=useState<RegistryItem[]>([]);
 const [busy,setBusy]=useState(false),[pending,setPending]=useState(""),[error,setError]=useState(""),[notice,setNotice]=useState("");
 const [filter,setFilter]=useState("all"),[sort,setSort]=useState("name"),[expanded,setExpanded]=useState("");
 const [loaded,setLoaded]=useState(false);
 const fetchRegistry=async(term=query)=>{
  setBusy(true);setError("");
  try {const result=await get<RegistryItem[]>("/api/mcp/catalog"+(term.trim()?"?q="+encodeURIComponent(term.trim()):""));setItems(result);setLoaded(true);}
  catch(e){setError((e as Error).message);}finally{setBusy(false);}
 };
 const visible=useMemo(()=>{
  const filtered=items.filter(x=>filter==="all"||(filter==="remote"?!!x.url:filter==="package"?!!x.packageName:installed.some(i=>i.name===x.name)));
  return [...filtered].sort((a,b)=>sort==="installed"?Number(installed.some(i=>i.name===b.name))-Number(installed.some(i=>i.name===a.name))||a.name.localeCompare(b.name):a.name.localeCompare(b.name));
 },[items,filter,sort,installed]);
 const toggle=async(item:RegistryItem)=>{
  const existing=installed.find(x=>x.name===item.name);
  if(!existing&&!item.url&&!item.packageName)return;
  if(!existing&&!window.confirm(ko?"이 MCP 서버를 등록하시겠습니까? 실행 전 출처와 권한을 확인하세요.":"Register this MCP server? Review its source and permissions before running."))return;
  setPending(item.name);setError("");setNotice("");
  try{
   if(existing){await del("/api/mcp/"+encodeURIComponent(existing.id));setNotice(ko?"등록을 해제했습니다.":"Server removed.");}
   else if(item.packageName){await post("/api/mcp",{name:item.name,transport:"stdio",command:"npx",args:["-y",item.packageName],enabled:false});setNotice(ko?"비활성 상태로 등록했습니다. 검토 후 활성화하세요.":"Registered disabled. Review before enabling.");}
   else if(item.url){await post("/api/mcp",{name:item.name,transport:"http",url:item.url,enabled:false});setNotice(ko?"비활성 상태로 등록했습니다. 검토 후 활성화하세요.":"Registered disabled. Review before enabling.");}
   onChanged();
  }catch(e){setError((e as Error).message);}finally{setPending("");}
 };
 return <section className="space-y-4">
  <div className="flex flex-wrap items-center justify-between gap-3"><div><h3 className="text-lg font-bold">{ko?"MCP 레지스트리":"MCP Registry"}</h3><p className="text-xs text-[#7C7469]">{ko?"공식 MCP Registry에서 서버를 검색하고 검토한 뒤 등록합니다.":"Discover, review and register servers from the official MCP Registry."}</p></div><Button variant="secondary" disabled={busy} onClick={()=>void fetchRegistry()}><RefreshCw size={15}/>{ko?"새로고침":"Refresh"}</Button></div>
  <div className="flex items-center gap-2 rounded-xl border border-[#E7E0D5] bg-white px-3 py-2"><Search size={18}/><TextInput value={query} placeholder={ko?"GitHub, browser, database 등 검색":"Search GitHub, browser, database..."} onChange={e=>setQuery(e.target.value)} onKeyDown={e=>e.key==="Enter"&&void fetchRegistry()}/><Button disabled={busy||!!query.trim()&&query.trim().length<3} onClick={()=>void fetchRegistry()}>{busy?<LoaderCircle size={16} className="animate-spin"/>:ko?"검색":"Search"}</Button></div>
  <div className="flex flex-wrap items-center justify-between gap-2"><div className="flex flex-wrap gap-1.5">{([{id:"all",ko:"전체",en:"All"},{id:"remote",ko:"원격 연결",en:"Remote"},{id:"package",ko:"패키지",en:"Package"},{id:"installed",ko:"등록됨",en:"Registered"}] as const).map(f=><Button key={f.id} variant={filter===f.id?"primary":"secondary"} onClick={()=>setFilter(f.id)}>{ko?f.ko:f.en}</Button>)}</div><div className="flex items-center gap-2 text-xs text-[#7C7469]"><span>{loaded?(ko?`조회 ${visible.length}개 · 최대 180개`:`${visible.length} results · up to 180`):ko?"검색 전":"Not loaded"}</span><select className="rounded-lg border border-[#E7E0D5] bg-white p-2" value={sort} onChange={e=>setSort(e.target.value)}><option value="name">{ko?"이름순":"Name"}</option><option value="installed">{ko?"등록 우선":"Registered first"}</option></select></div></div>
  <div className="flex items-start gap-2 rounded-xl bg-amber-50 p-3 text-xs text-amber-900"><ShieldAlert size={17} className="shrink-0"/><span>{ko?"공식 레지스트리 등록은 코드 안전성 인증이 아닙니다. 별점이나 신뢰도 점수는 제공되지 않으며, 설치 전 패키지와 권한을 확인해야 합니다.":"Registry publication is not a security certification. Ratings are not provided; review packages and permissions before enabling."}</span></div>
  {error&&<p role="alert" className="text-sm text-red-700">{error}</p>}{notice&&<p role="status" className="text-sm text-emerald-700">{notice}</p>}
  <div className="grid gap-3 md:grid-cols-2">{visible.map(item=>{const existing=installed.find(i=>i.name===item.name),open=expanded===item.name;return <article key={item.name} className="min-w-0 rounded-xl border border-[#E7E0D5] bg-white p-4"><div className="flex items-start gap-3"><div className="rounded-lg bg-[#F7E8E1] p-2 text-[#C96442]"><Server size={19}/></div><div className="min-w-0 flex-1"><h4 className="break-all text-sm font-bold">{item.name}</h4><p className="mt-1 line-clamp-2 text-xs text-[#7C7469]">{item.description||"MCP server"}</p></div></div><div className="mt-3 flex flex-wrap gap-1.5 text-[11px]">{existing&&<span className="inline-flex items-center gap-1 rounded bg-emerald-50 px-2 py-1 text-emerald-700"><CheckCircle2 size={12}/>{ko?"등록됨":"Registered"}</span>}{item.url&&<span className="rounded bg-sky-50 px-2 py-1 text-sky-700">HTTP</span>}{item.packageName&&<span className="rounded bg-stone-100 px-2 py-1">npm</span>}{!item.url&&!item.packageName&&<span className="rounded bg-stone-100 px-2 py-1">{ko?"설치 정보 없음":"No install metadata"}</span>}</div>{open&&<div className="mt-3 space-y-2 rounded-lg bg-[#FAF7F1] p-3 text-xs"><p className="break-all">{item.description}</p>{item.packageName&&<p className="break-all">npm: {item.packageName}</p>}{item.url&&<p className="break-all">URL: {item.url}</p>}<a href={"https://registry.modelcontextprotocol.io/"} target="_blank" rel="noopener noreferrer" className="inline-flex items-center gap-1 underline">{ko?"공식 레지스트리":"Official registry"}<ExternalLink size={12}/></a></div>}<div className="mt-4 flex justify-between gap-2"><Button variant="secondary" onClick={()=>setExpanded(open?"":item.name)}>{open?(ko?"접기":"Less"):(ko?"상세 정보":"Details")}</Button><Button variant="secondary" disabled={!!pending||!existing&&!item.url&&!item.packageName} onClick={()=>void toggle(item)}>{pending===item.name?<LoaderCircle size={15} className="animate-spin"/>:existing?(ko?"등록 해제":"Remove"):(ko?"등록":"Register")}</Button></div></article>})}</div>
  {!busy&&!visible.length&&<p className="py-12 text-center text-sm text-[#7C7469]">{loaded?(ko?"일치하는 MCP 서버가 없습니다.":"No matching MCP servers."):(ko?"검색하거나 새로고침을 눌러 공식 목록을 불러오세요.":"Search or refresh to load the official registry.")}</p>}
 </section>;
}
