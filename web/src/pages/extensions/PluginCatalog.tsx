import {useEffect,useState} from 'react'
import {ArrowRight,Blocks,Check,ChevronDown,Download,ExternalLink,Flame,LoaderCircle,Search,Star,X} from 'lucide-react'
import {get,post} from '../../api'
import {Button,TextInput} from '../../components/Primitives'
import {DialogActions,DialogBackdrop,DialogBody,DialogHeader,DialogPanel} from '../../components/Dialog'
import {Notice,useRemote} from '../../components/UI'
import {ChatRichText} from '../../components/ChatRichText'
import {useLocale} from '../../i18n'
import {usePluginDirectory,type Category,type Plugin} from './catalog/usePluginDirectory'
import type {ExtensionTab} from '../Extensions'

export function PluginCatalog({onManage}:{onManage:(tab:ExtensionTab)=>void}){
 const en=useLocale()==='en'
 const [query,setQuery]=useState(''),[search,setSearch]=useState(''),[filter,setFilter]=useState('all'),[selected,setSelected]=useState<Plugin>()
 useEffect(()=>{const timer=setTimeout(()=>setSearch(query.trim()),350);return()=>clearTimeout(timer)},[query])
 const catalog=usePluginDirectory(filter,search)
 const overview=filter==='all'&&!search
 const shelves=overview?[
  {id:'featured',label:en?'Featured':'추천',items:catalog.items.filter(item=>item.featured).sort((a,b)=>(a.featuredRank??0)-(b.featuredRank??0)).slice(0,8)},
  {id:'popular',label:en?'Popular':'인기',items:catalog.items.filter(item=>item.trending).sort((a,b)=>(a.trendingRank??0)-(b.trendingRank??0)).slice(0,8)},
  ...catalog.categories.map(category=>({id:category.slug,label:category.label,items:catalog.items.filter(item=>item.categories?.includes(category.slug)).slice(0,8)})),
 ]:[{id:filter,label:catalog.categories.find(category=>category.slug===filter)?.label||(filter==='featured'?(en?'Featured':'추천'):filter==='popular'?(en?'Popular':'인기'):(en?'Search results':'검색 결과')),items:catalog.items}]
 return <section aria-label={en?'Plugin catalog':'플러그인 둘러보기'} className="min-w-0">
  <PluginSearch value={query} onChange={setQuery} en={en}/>
  <PluginFilters categories={catalog.categories} selected={filter} onSelect={setFilter} en={en}/>
  <Notice message={catalog.error} onRetry={()=>void catalog.reload()} retrying={catalog.loading}/>
  {catalog.loading&&<p role="status" className="my-5 flex items-center gap-2 text-sm text-muted"><LoaderCircle size={17} className="animate-spin"/>{en?'Loading plugin registry…':'플러그인 레지스트리를 불러오는 중…'}</p>}
  {shelves.filter(shelf=>shelf.items.length>0).map(shelf=><section key={shelf.id} aria-label={shelf.label} className="mb-8">
   <div className="mb-4 flex items-center justify-between gap-3"><h2 className="text-lg font-bold">{shelf.label}</h2>{overview&&<Button variant="ghost" size="sm" onClick={()=>setFilter(shelf.id)}>{en?'View all':'모두 보기'}<ArrowRight size={15}/></Button>}</div>
   <PluginGrid items={shelf.items} onSelect={setSelected} en={en}/>
  </section>)}
  {!catalog.loading&&!catalog.items.length&&<p role="status" className="rounded-2xl border border-dashed border-divider p-10 text-center text-muted">{en?'No matching plugins. Try another search or category.':'일치하는 플러그인이 없어요. 검색어나 분야를 바꿔 보세요.'}</p>}
  {catalog.hasMore&&<div className="my-5 text-center"><Button variant="secondary" disabled={catalog.moreBusy} onClick={()=>void catalog.loadMore()}>{catalog.moreBusy?<LoaderCircle size={16} className="animate-spin"/>:<ChevronDown size={16}/>} {en?'Show more':'더 보기'}</Button></div>}
  <p className="mt-8 text-xs text-muted">{en?'Catalog metadata from ClawHub and the public MCP Registry.':'ClawHub·공개 MCP Registry의 카탈로그 데이터를 사용합니다.'} <a className="underline underline-offset-2" href="https://clawhub.ai" target="_blank" rel="noreferrer">ClawHub<ExternalLink size={11} className="ml-1 inline"/></a></p>
  {selected&&<PluginDetails item={selected} en={en} onClose={()=>setSelected(undefined)} onManage={()=>{setSelected(undefined);onManage('installed')}} onAdded={()=>void catalog.installed.reload()}/>}
 </section>
}
function PluginSearch({value,onChange,en}:{value:string;onChange:(value:string)=>void;en:boolean}){
 return <label className="mb-5 flex min-w-0 items-center gap-3 rounded-2xl border border-divider bg-white px-4 py-3 shadow-sm"><Search size={22} className="shrink-0 text-muted"/><TextInput aria-label={en?'Search plugins':'플러그인 검색'} className="!border-0 !bg-transparent !px-0 !text-base !shadow-none focus:!ring-0" placeholder={en?'Search plugins':'플러그인 검색'} value={value} onChange={e=>onChange(e.target.value)}/>{value&&<Button variant="ghost" size="icon" aria-label={en?'Clear search':'검색 지우기'} onClick={()=>onChange('')}><X size={17}/></Button>}</label>
}
function PluginFilters({categories,selected,onSelect,en}:{categories:Category[];selected:string;onSelect:(id:string)=>void;en:boolean}){
 return <div aria-label={en?'Plugin categories':'플러그인 분야'} className="mb-8 flex flex-wrap gap-2">
  {[{slug:'all',label:en?'All':'전체',icon:Blocks},{slug:'featured',label:en?'Featured':'추천',icon:Star},{slug:'popular',label:en?'Popular':'인기',icon:Flame}].map(({slug,label,icon:Icon})=><Button key={slug} variant={selected===slug?'secondary':'ghost'} aria-pressed={selected===slug} onClick={()=>onSelect(slug)} className="!rounded-xl !border !border-divider !px-4 !py-2.5"><Icon size={17}/>{label}</Button>)}
  {categories.map(category=><Button key={category.slug} variant={selected===category.slug?'secondary':'ghost'} title={category.description} aria-pressed={selected===category.slug} onClick={()=>onSelect(category.slug)} className="!rounded-xl !border !border-divider !px-4 !py-2.5">{category.label}</Button>)}
 </div>
}
function artwork(value?:string|null){
 if(!value)return undefined
 try{const url=new URL(value,'https://clawhub.ai');return url.protocol==='https:'?url.href:undefined}catch{return undefined}
}
function PluginArtwork({item}:{item:Plugin}){
 const [failed,setFailed]=useState(false),src=artwork(item.icon||item.ownerImage)
 return <span className="grid size-12 shrink-0 place-items-center overflow-hidden rounded-xl bg-brand/10 text-brand">{src&&!failed?<img src={src} alt="" loading="lazy" referrerPolicy="no-referrer" onError={()=>setFailed(true)} className="size-10 object-contain"/>:<Blocks size={25}/>}</span>
}
function PluginGrid({items,onSelect,en}:{items:Plugin[];onSelect:(item:Plugin)=>void;en:boolean}){
 return <div className="grid grid-cols-[repeat(auto-fill,minmax(245px,1fr))] gap-4">{items.map(item=><button key={item.name} type="button" onClick={()=>onSelect(item)} aria-label={(en?'Details: ':'상세 보기: ')+(item.displayName||item.name)} className="group flex min-w-0 flex-col gap-4 rounded-2xl border border-divider bg-white p-5 text-left shadow-sm transition hover:border-brand/50 hover:shadow-md focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand">
  <span className="flex min-w-0 items-center gap-3"><PluginArtwork item={item}/><span className="min-w-0 flex-1"><strong className="line-clamp-2 break-words text-[15px]">{item.displayName||item.name}</strong><span className="mt-1 block truncate text-xs text-muted">{item.ownerHandle?'@'+item.ownerHandle:item.name}</span></span>{item.installed&&<Check size={17} className="shrink-0 text-emerald-700"/>}</span>
  <span className="line-clamp-3 text-sm leading-6 text-muted">{item.summary}</span>
  <span className="mt-auto flex items-center justify-between gap-3 text-xs text-muted">{item.stats?.downloads!=null?<span className="inline-flex items-center gap-1"><Download size={12}/>{new Intl.NumberFormat(en?'en':'ko',{notation:'compact'}).format(item.stats.downloads)}</span>:<span>{item.latestVersion?'v'+item.latestVersion:''}</span>}<span className="inline-flex items-center gap-1 font-semibold text-brand">{item.installed?(en?'Manage':'설정·관리'):(en?'Explore':'자세히 보기')}<ArrowRight size={13}/></span></span>
 </button>)}</div>
}
type Surface=string|{name?:string;id?:string;description?:string}
type Detail={package?:{summary?:string;compatibility?:Record<string,string>};readme?:string|null;version?:{version?:string;pluginManifestSummary?:{bundledSkills?:Surface[];channels?:Surface[];mcpServers?:Surface[];configFields?:{name:string;required?:boolean;sensitive?:boolean}[];contracts?:Record<string,Surface[]>}}}
function PluginDetails({item,en,onClose,onManage,onAdded}:{item:Plugin;en:boolean;onClose:()=>void;onManage:()=>void;onAdded:()=>void}){
 const detail=useRemote<Detail>(item.connection?null:'/api/plugins/catalog?view=detail&id='+encodeURIComponent(item.name),{},false)
 const tools=useRemote<{server:string;name:string;description:string}[]>(item.connection&&item.enabled?'/api/mcp/discover':null,[],false)
 const [busy,setBusy]=useState(false),[added,setAdded]=useState(item.installed),[error,setError]=useState('')
 const summary=detail.value.version?.pluginManifestSummary
 const contents=[...Object.entries(summary?.contracts||{}).filter(([,items])=>Array.isArray(items)).map(([name,items])=>({name,items})),{name:en?'Skills':'스킬',items:summary?.bundledSkills||[]},{name:en?'Channels':'채널',items:summary?.channels||[]},{name:en?'Connections':'연결',items:summary?.mcpServers||[]},...(item.connection?[{name:en?'Tools':'도구',items:tools.value.filter(tool=>tool.server===item.name)}]:[])].filter(group=>group.items.length)
 const add=async()=>{if(!item.connection||busy)return;setBusy(true);setError('');try{const r=item.connection;await post('/api/mcp',{name:item.name,transport:r.packageName?'stdio':'http',command:r.packageName?'npx':undefined,args:r.packageName?['-y',r.packageName]:[],url:r.url||'',enabled:false});setAdded(true);onAdded()}catch(e){setError((e as Error).message)}finally{setBusy(false)}}
 const canonical='https://clawhub.ai/plugins/'+encodeURIComponent(item.ownerHandle||item.name.split('/')[0].replace('@',''))+'/'+encodeURIComponent(item.name.split('/').pop()||item.name)
 return <DialogBackdrop onDismiss={()=>{if(!busy)onClose()}}><DialogPanel size="wide" aria-label={item.displayName||item.name}>
  <DialogHeader><div className="flex min-w-0 items-center gap-3"><PluginArtwork item={item}/><div className="min-w-0"><h2 className="break-words text-lg font-bold">{item.displayName||item.name}</h2><p className="mt-1 break-all text-xs text-muted">{item.name} {item.latestVersion?'· v'+item.latestVersion:''}</p></div></div><Button variant="ghost" size="icon" disabled={busy} onClick={onClose} aria-label={en?'Close details':'상세 닫기'}><X size={19}/></Button></DialogHeader>
  <DialogBody><p className="text-sm leading-7">{item.summary}</p><Notice message={error||detail.error||tools.error} onRetry={()=>{void detail.reload();void tools.reload()}}/>
   {detail.loading&&<p role="status" className="flex items-center gap-2 text-sm text-muted"><LoaderCircle size={16} className="animate-spin"/>{en?'Loading plugin contents…':'플러그인 구성을 불러오는 중…'}</p>}
   {contents.length>0&&<section className="rounded-2xl border border-divider p-4"><h3 className="mb-4 text-sm font-bold">{en?'Included features':'이 플러그인에 포함된 기능'}</h3><div className="grid gap-5">{contents.map(group=><div key={group.name}><h4 className="mb-2 text-xs font-semibold text-muted">{group.name} · {group.items.length}</h4><ul className="grid gap-2">{group.items.map((surface,index)=><li key={index} className="rounded-xl bg-surface p-3 text-sm"><strong className="break-all font-medium">{typeof surface==='string'?surface:surface.name||('id' in surface?surface.id:undefined)}</strong>{typeof surface!=='string'&&surface.description&&<p className="mt-1 text-xs leading-5 text-muted">{surface.description}</p>}</li>)}</ul></div>)}</div></section>}
   {Boolean(summary?.configFields?.length)&&<section><h3 className="mb-2 text-sm font-bold">{en?'Configuration':'설정 항목'}</h3><div className="flex flex-wrap gap-2">{summary?.configFields?.map(field=><span key={field.name} className="rounded-lg border border-divider px-3 py-2 text-xs">{field.name}{field.required?' *':''}{field.sensitive?' · '+(en?'Secret':'비밀 정보'):''}</span>)}</div></section>}
   {item.connection?<div className="rounded-xl bg-surface p-4 text-sm"><h3 className="font-semibold">{en?'Connection':'연결 대상'}</h3><code className="my-2 block break-all">{item.connection.packageName||item.connection.url}</code><p className="text-xs leading-5 text-muted">{en?'Adding this plugin registers it without starting a process. Review its command and credentials in settings before enabling.':'추가한 뒤 설정에서 명령·인증을 확인하고 활성화하세요. 추가만으로 프로세스를 실행하지 않습니다.'}</p></div>:<p role="status" className="rounded-xl bg-amber-50 p-4 text-sm leading-6 text-amber-900">{en?'This package requires an OpenClaw-compatible plugin host connection.':'이 패키지를 실행하려면 OpenClaw 호환 플러그인 호스트 연결이 필요합니다.'}</p>}
   {detail.value.readme&&<details className="rounded-xl border border-divider p-4"><summary className="cursor-pointer text-sm font-semibold">{en?'Documentation':'사용 설명'}</summary><div className="mt-4"><ChatRichText text={detail.value.readme}/></div></details>}
   {added&&!item.installed&&<p role="status" className="text-sm text-emerald-800">{en?'Added. Review and enable in connection settings.':'추가했어요. 연결 설정에서 확인하고 활성화하세요.'}</p>}
  </DialogBody><DialogActions>{!item.connection&&<a href={canonical} target="_blank" rel="noreferrer" className="mr-auto inline-flex items-center gap-1 text-sm text-brand">ClawHub<ExternalLink size={14}/></a>}<Button variant="ghost" disabled={busy} onClick={onClose}>{en?'Close':'닫기'}</Button>{item.connection&&(added?<Button onClick={onManage}><Check size={16}/>{en?'Configure':'설정·관리'}</Button>:<Button disabled={busy||!item.connection.packageName&&!item.connection.url} onClick={()=>void add()}>{busy?<LoaderCircle size={16} className="animate-spin"/>:<Blocks size={16}/>} {en?'Add':'추가'}</Button>)}</DialogActions>
 </DialogPanel></DialogBackdrop>
}
