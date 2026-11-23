import {useState, type FormEvent} from 'react'
import {CheckCircle2,LoaderCircle,Package,PlugZap,Search,Settings2,ShieldAlert,X} from 'lucide-react'
import {Button,TextInput} from '../Primitives'
import type {InstalledMcp,RegistryMcp} from '../../features/mcp/quickCommands'

type Props={
 installed:InstalledMcp[];results:RegistryMcp[];busy:boolean;notice:string;lastQuery:string
 onClose:()=>void;onSearch:(query:string)=>void;onAdd:(entry:RegistryMcp)=>void
 onToggle:(entry:InstalledMcp,enabled:boolean)=>void;onRemove:(entry:InstalledMcp)=>void
 onManage:()=>void
}

/** Reuses the existing MCP registry and collection API; no parallel plugin runtime. */
export function McpQuickConnect({installed,results,busy,notice,lastQuery,onClose,onSearch,onAdd,onToggle,onRemove,onManage}:Props){
 const [query,setQuery]=useState(lastQuery)
 const search=(event:FormEvent)=>{event.preventDefault();if(query.trim())onSearch(query.trim())}
 return <section role="region" aria-label="MCP 빠른 연결" className="mb-3 max-h-[min(380px,50dvh)] overflow-y-auto rounded-2xl border border-divider bg-surface p-3 shadow-panel">
  <div className="mb-3 flex items-center gap-2">
   <PlugZap size={18} className="shrink-0 text-brand"/>
   <strong className="min-w-0 flex-1 text-sm">MCP 빠른 연결</strong>
   <Button variant="ghost" size="sm" onClick={onManage}><Settings2 size={14}/>전체 관리</Button>
   <Button variant="ghost" size="icon" onClick={onClose} aria-label="MCP 빠른 연결 닫기"><X size={15}/></Button>
  </div>
  <form onSubmit={search} className="mb-3 flex gap-2">
   <TextInput aria-label="MCP 레지스트리 검색어" value={query} onChange={e=>setQuery(e.target.value)}
    placeholder="Parallel, GitHub, Exa…" className="min-w-0 flex-1"/>
   <Button type="submit" size="sm" disabled={busy||!query.trim()}><Search size={14}/>검색</Button>
  </form>
  {notice&&<div role="status" className="mb-3 flex items-start gap-2 rounded-lg bg-canvas px-3 py-2 text-xs leading-5 text-ink">
    {busy?<LoaderCircle size={15} className="shrink-0 animate-spin"/>:<CheckCircle2 size={15} className="shrink-0"/>}{notice}
   </div>}
  {results.length>0&&<div className="mb-3 rounded-xl border border-divider p-2">
   <h3 className="px-2 pb-1 text-xs font-semibold">레지스트리 검색 결과 · {results.length}개</h3>
   {results.slice(0,20).map(item=>{
    const exists=installed.some(server=>server.name.toLowerCase()===item.name.toLowerCase())
    return <div key={item.name} className="flex items-center gap-2 border-t border-divider/60 px-2 py-2">
     <div className="min-w-0 flex-1"><strong className="block break-all text-xs">{item.name}</strong>
      {item.description&&<p className="mt-0.5 line-clamp-2 text-[11px] leading-4 text-muted">{item.description}</p>}
      <p className="mt-0.5 break-all text-[10px] text-muted">{item.url||item.packageName||'설치 정보 없음'}</p>
     </div>
     <Button size="sm" disabled={busy||exists||(!item.url&&!item.packageName)} onClick={()=>onAdd(item)}>
      {item.packageName&&!item.url?<Package size={14}/>:<PlugZap size={14}/>}
      {exists?'등록됨':item.url?'연결':'설치'}
     </Button>
    </div>
   })}
  </div>}
  <div className="space-y-1">
   <div className="flex items-center justify-between px-1"><h3 className="text-xs font-semibold">등록된 MCP · {installed.length}개</h3>
    <span className="text-[10px] text-muted">개별 사용 설정</span></div>
   {installed.length===0&&<p className="rounded-lg bg-canvas p-3 text-xs text-muted">설치된 MCP가 없습니다. 위에서 검색해 연결할 수 있습니다.</p>}
   {installed.map(entry=><div key={entry.id} className="flex flex-wrap items-center gap-2 rounded-lg bg-canvas px-2 py-2">
     <div className="min-w-0 flex-1"><strong className="block break-all text-xs">{entry.name}</strong>
      <p className="break-all text-[10px] text-muted">{entry.transport==='http'?entry.url:'로컬 명령 실행'}</p></div>
     <Button size="sm" variant={entry.enabled!==false?'secondary':'ghost'} disabled={busy}
      aria-label={entry.name+' '+(entry.enabled!==false?'사용 중지':'사용')}
      onClick={()=>onToggle(entry,entry.enabled===false)}>{entry.enabled!==false?'켜짐':'꺼짐'}</Button>
     <Button size="sm" variant="ghost" disabled={busy} aria-label={entry.name+' 연결 삭제'}
      onClick={()=>onRemove(entry)}><X size={14}/></Button>
    </div>)}
  </div>
  <div className="mt-3 flex items-start gap-2 text-[11px] leading-5 text-muted"><ShieldAlert size={14} className="mt-0.5 shrink-0"/>
   MCP를 등록했다고 곧바로 연결 성공을 의미하지는 않습니다. 로그인이나 별도 설정이 필요할 수 있습니다.</div>
 </section>
}
