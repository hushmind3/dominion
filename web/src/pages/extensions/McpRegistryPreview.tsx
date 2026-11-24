import {useEffect,useRef,useState} from 'react'
import {get} from '../../api'
import {Button,TextInput} from '../../components/Primitives'
import {useLocale} from '../../i18n'
import {registryDraft,type RegistryMcp} from '../../features/mcp/quickCommands'

/** Discovery only. All connections and execution belong to the installed extension. */
export function McpRegistryPreview(){
 const en=useLocale()==='en',tr=(ko:string,eng:string)=>en?eng:ko
 const initial=new URLSearchParams(window.location.hash.split('?')[1]).get('q')||''
 const [q,setQ]=useState(initial),[rows,setRows]=useState<RegistryMcp[]|null>(null),[busy,setBusy]=useState(false),[error,setError]=useState(''),[selected,setSelected]=useState<RegistryMcp|null>(null)
 const active=useRef<AbortController|null>(null)
 const search=async(term:string)=>{
  if(term.trim().length>160){setError(tr('검색어가 너무 깁니다.','Search query too long.'));return}
  active.current?.abort();const controller=new AbortController();active.current=controller
  setBusy(true);setError('');setRows(null);setSelected(null)
  try{const result=await get<RegistryMcp[]>('/api/mcp/catalog?q='+encodeURIComponent(term.trim()),controller.signal)
   if(active.current===controller&&!controller.signal.aborted){if(!Array.isArray(result))throw Error('Invalid registry response');setRows(result)}
  }catch(e){if(active.current===controller&&!controller.signal.aborted)setError((e as Error).message)}
  finally{if(active.current===controller)setBusy(false)}
 }
 useEffect(()=>{void search(initial);return()=>active.current?.abort()},[])
 const draft=selected&&registryDraft(selected)
 return <section className="space-y-3 rounded-xl border border-divider p-4">
  <h3 className="font-semibold">{tr('공식 MCP 레지스트리','Official MCP registry')}</h3>
  <p className="text-xs text-muted">{tr('서버를 찾고 설정을 확인합니다. 실제 연결과 실행은 설치된 독립 커넥터에서 관리합니다.','Find server settings here. Connections and execution belong to the installed independent connector.')}</p>
  <form className="flex gap-2" onSubmit={e=>{e.preventDefault();void search(q)}}><TextInput aria-label={tr('MCP 서버 검색','Search MCP servers')} maxLength={160} value={q} onChange={e=>setQ(e.target.value)}/><Button type="submit" disabled={busy}>{tr('검색','Search')}</Button></form>
  <button type="button" className="text-sm text-brand underline" disabled={busy} onClick={()=>{setQ('Hugging Face');setSelected({name:'Hugging Face',url:'https://huggingface.co/mcp'});setRows(null);setError('')}}>{tr('Hugging Face 공식 서버 설정','Hugging Face official server settings')}</button>
  {busy&&<p role="status" className="text-sm text-muted">{tr('레지스트리 검색 중…','Searching registry…')}</p>}{error&&<p role="alert" className="text-danger text-sm">{error}</p>}
  {rows&&<div className="max-h-80 divide-y divide-divider overflow-auto">{rows.length===0&&<p className="text-sm text-muted">{tr('검색 결과 없음','No results')}</p>}{rows.map(row=><div key={row.name} className="flex flex-wrap items-center gap-2 py-2"><div className="min-w-0 flex-1"><strong className="break-all text-sm">{row.name}</strong><p className="break-words text-xs text-muted">{row.description}</p></div><Button size="sm" variant="secondary" onClick={()=>setSelected(row)}>{tr('설정 확인','Review settings')}</Button></div>)}</div>}
  {selected&&<div className="space-y-2 rounded-lg border border-divider p-3"><strong className="break-all">{selected.name}</strong>
   {draft?<><p className="text-xs text-muted">{tr('아래 설정을 설치된 커넥터의 프로그램 설정에 입력하세요. 인증정보는 직접 입력합니다.','Enter these settings in the installed connector. Enter credentials separately.')}</p><textarea aria-label={tr('서버 설정 초안','Server configuration draft')} className="w-full rounded border border-divider bg-canvas p-2 font-mono text-xs" rows={7} readOnly value={JSON.stringify(draft,null,2)}/></>:<p className="text-xs text-warning">{tr('자동 설정 초안을 만들 수 없습니다. 원본 서버의 필수 설정을 직접 확인하세요.','No safe configuration draft. Review required settings in the original server documentation.')}</p>}
   <a href="#/connections?view=plugins&extensionView=installed" className="text-sm text-brand underline">{tr('설치된 확장 관리','Manage installed extensions')}</a>
  </div>}
 </section>
}
