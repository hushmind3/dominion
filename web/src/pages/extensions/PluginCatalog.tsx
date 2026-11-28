// React adaptation of OpenClaw plugin-card/detail-shell/install-action (MIT).
// Original CSS, category SVGs and license are retained in vendor/openclaw.
import {useEffect,useState} from 'react'
import {ArrowRight,BadgeCheck,Blocks,Check,ChevronRight,Flame,LoaderCircle,Plus,Search,Settings,Star,X} from 'lucide-react'
import {get,post} from '../../api'
import {Button,TextInput} from '../../components/Primitives'
import {DialogBackdrop,DialogPanel} from '../../components/Dialog'
import {Notice,useRemote} from '../../components/UI'
import {ChatRichText} from '../../components/ChatRichText'
import {ConnectedAccounts} from '../../components/ConnectedAccounts'
import {useLocale} from '../../i18n'
import {ConnectionsPage} from '../Connections'
import {SdkExtensionsPanel} from './SdkExtensionsPanel'
import {McpPluginSettings} from './McpPluginSettings'
import {PluginConfiguration} from './PluginConfiguration'
import {PluginAssistant} from './PluginAssistant'
import {usePluginDirectory,type Category,type Plugin} from './catalog/usePluginDirectory'
import {loadArtwork} from './catalog/artwork'
import {usePluginInstall,type Installation} from './catalog/usePluginInstall'
import {registryCategoryIcon} from '../../vendor/openclaw/RegistryCategoryIcon'
import '../../vendor/openclaw/plugins.css'
import '../../vendor/openclaw/install-action.css'
import './plugin-workspace.css'

type Detail={readme?:string|null;version?:{pluginManifestSummary?:{bundledSkills?:Surface[];channels?:Surface[];mcpServers?:Surface[];contracts?:Record<string,Surface[]>}}}
type Surface=string|{name?:string;id?:string;description?:string}
export function PluginCatalog({initialCategory='all'}:{initialCategory?:string}={}){
 const en=useLocale()==='en'
 const [query,setQuery]=useState(''),[search,setSearch]=useState(''),[filter,setFilter]=useState(initialCategory),[installedOnly,setInstalledOnly]=useState(false),[selected,setSelected]=useState<Plugin>(),[progress,setProgress]=useState<string>(),[administration,setAdministration]=useState(false)
 useEffect(()=>{const timer=setTimeout(()=>setSearch(query.trim()),300);return()=>clearTimeout(timer)},[query])
 const catalog=usePluginDirectory(filter,search,installedOnly),operations=usePluginInstall(()=>void catalog.reload())
 const current=selected&&(catalog.items.find(item=>item.name===selected.name)||selected)
 const overview=filter==='all'&&!search&&!installedOnly
 const shelves=overview?[
  {id:'featured',label:en?'Featured':'추천',items:catalog.items.filter(item=>item.featured).sort((a,b)=>(a.featuredRank??0)-(b.featuredRank??0)).slice(0,8)},
  {id:'popular',label:en?'Popular':'인기',items:catalog.items.filter(item=>item.trending).sort((a,b)=>(a.trendingRank??0)-(b.trendingRank??0)).slice(0,8)},
  ...catalog.categories.map(category=>({id:category.slug,label:category.label,items:catalog.items.filter(item=>item.categories?.includes(category.slug)).slice(0,8)})),
 ]:[{id:filter,label:installedOnly?(en?'Installed plugins':'설치된 플러그인'):catalog.categories.find(category=>category.slug===filter)?.label||(filter==='featured'?(en?'Featured':'추천'):filter==='popular'?(en?'Popular':'인기'):(en?'Plugins':'플러그인')),items:catalog.items}]
 const install=async(plugin:Plugin)=>{
  if(plugin.connection){try{await post('/api/mcp',{name:plugin.displayName,transport:plugin.connection.packageName?'stdio':'http',command:plugin.connection.packageName?'npx':undefined,args:plugin.connection.packageName?['-y',plugin.connection.packageName]:[],url:plugin.connection.url||'',enabled:false});await catalog.reload();setSelected(plugin)}catch(e){alert((e as Error).message)}return}
  if(plugin.nativeKind){setSelected(plugin);return}
  if(!plugin.latestVersion)return
  await operations.install(plugin.name,plugin.latestVersion);setProgress(plugin.name)
 }
 return <section aria-label={en?'Plugin catalog':'플러그인 둘러보기'} className="plugin-workspace-content">
  {current?<PluginDetails item={current} en={en} onBack={()=>setSelected(undefined)} onChanged={()=>void catalog.reload()} onInstall={()=>void install(current)} operation={operations.entries.get(current.name)} onProgress={()=>setProgress(current.name)}/>:<div className="plugin-directory-scroll">
   <div className="mb-6 flex items-center gap-4"><label className="plugin-search"><Search size={23}/><TextInput aria-label={en?'Search plugins':'플러그인 검색'} placeholder={en?'Search plugins':'플러그인 검색'} value={query} onChange={e=>setQuery(e.target.value)} className="!border-0 !bg-transparent !text-base !shadow-none focus:!ring-0"/>{query&&<Button variant="ghost" size="icon" aria-label={en?'Clear search':'검색 지우기'} onClick={()=>setQuery('')}><X size={16}/></Button>}</label><Button variant="secondary" size="icon" aria-label={en?'Plugin administration':'플러그인 관리 설정'} onClick={()=>setAdministration(true)}><Settings size={21}/></Button></div>
   <PluginFilters categories={catalog.categories} selected={filter} onSelect={setFilter} en={en}/>
   <label className="mb-6 flex items-center gap-2 text-xs text-muted"><input type="checkbox" checked={installedOnly} onChange={e=>setInstalledOnly(e.target.checked)}/>{en?'Installed only':'설치된 항목만'}</label>
   <Notice message={catalog.error||operations.error} onRetry={()=>void catalog.reload()} retrying={catalog.loading}/>
   {catalog.loading&&<p role="status" className="my-5 flex items-center gap-2 text-sm text-muted"><LoaderCircle size={17} className="animate-spin"/>{en?'Loading registry…':'레지스트리를 불러오는 중…'}</p>}
   {shelves.filter(shelf=>shelf.items.length).map(shelf=><section key={shelf.id} aria-label={shelf.label} className="mb-9"><div className="mb-4 flex items-center justify-between"><h2 className="text-lg font-semibold">{shelf.label}</h2>{overview&&<Button variant="ghost" size="sm" onClick={()=>setFilter(shelf.id)}>{en?'View all':'모두 보기'}<ArrowRight size={14}/></Button>}</div><div className="plugin-catalog-grid">{shelf.items.map(item=><PluginCard key={item.name} item={item} en={en} operation={operations.entries.get(item.name)} onOpen={()=>setSelected(item)} onInstall={()=>void install(item)} onProgress={()=>setProgress(item.name)}/>)}</div></section>)}
   {!catalog.loading&&!catalog.items.length&&<p role="status" className="p-10 text-center text-muted">{en?'No matching plugins. Try another search or category.':'일치하는 플러그인이 없어요. 검색어나 분야를 바꿔 보세요.'}</p>}
   {catalog.hasMore&&<div className="my-5 text-center"><Button variant="secondary" disabled={catalog.moreBusy} onClick={()=>void catalog.loadMore()}>{catalog.moreBusy?<LoaderCircle size={16} className="animate-spin"/>:null}{en?'Show more':'더 보기'}</Button></div>}
   <p className="mt-8 text-xs text-muted">ClawHub · MCP Registry <a href="https://clawhub.ai" target="_blank" rel="noreferrer" className="underline">ClawHub</a></p>
  </div>}
  {progress&&operations.entries.get(progress)&&<InstallProgress operation={operations.entries.get(progress)!} en={en} onClose={()=>setProgress(undefined)}/>}
  {administration&&<DialogBackdrop onDismiss={()=>setAdministration(false)}><DialogPanel size="wide" aria-label={en?'Plugin administration':'플러그인 관리 설정'} className="p-6"><PluginAdministration onChanged={()=>void catalog.reload()} onClose={()=>setAdministration(false)}/></DialogPanel></DialogBackdrop>}
 </section>
}
function PluginFilters({categories,selected,onSelect,en}:{categories:Category[];selected:string;onSelect:(id:string)=>void;en:boolean}){
 return <nav aria-label={en?'Plugin categories':'플러그인 분야'} className="plugin-category-filters">{[{slug:'all',label:en?'All':'전체',icon:<Blocks size={20}/>},{slug:'featured',label:en?'Featured':'추천',icon:<Star size={20}/>},{slug:'popular',label:en?'Popular':'인기',icon:<Flame size={20}/>}].map(item=><button type="button" key={item.slug} aria-pressed={selected===item.slug} onClick={()=>onSelect(item.slug)}>{item.icon}{item.label}</button>)}{categories.map(category=><button type="button" key={category.slug} title={category.description} aria-pressed={selected===category.slug} onClick={()=>onSelect(category.slug)}>{registryCategoryIcon(category.icon)}{category.label}</button>)}</nav>
}
function artwork(value?:string|null){if(!value)return undefined;try{const url=new URL(value,'https://clawhub.ai');return url.protocol==='https:'?'/api/plugins/artwork?src='+encodeURIComponent(url.href):undefined}catch{return undefined}}
function PluginArtwork({item,large=false}:{item:Plugin;large?:boolean}){
 const sources=[...new Set([artwork(item.icon),artwork(item.ownerImage)].filter(Boolean))] as string[]
 const [index,setIndex]=useState(0),[data,setData]=useState<string>()
 const source=sources[index]
 useEffect(()=>setIndex(0),[item.icon,item.ownerImage])
 useEffect(()=>{let live=true;setData(undefined);if(source)void loadArtwork(source).then(value=>{if(live)setData(value)}).catch(()=>{if(live)setIndex(value=>value+1)});return()=>{live=false}},[source])
 return <span className={'plugin-catalog-card__art '+(large?'plugin-artwork-large':'')}>{data?<img src={data} alt="" loading="lazy" onError={()=>setIndex(value=>value+1)} className="h-full w-full object-contain"/>:<span className="plugin-artwork-fallback">{(item.displayName||item.name).slice(0,2).toUpperCase()}</span>}</span>
}
function PluginCard({item,en,onOpen,onInstall,operation,onProgress}:{item:Plugin;en:boolean;onOpen:()=>void;onInstall:()=>void;operation?:Installation;onProgress:()=>void}){
 return <article className="plugin-catalog-card"><button type="button" className="plugin-catalog-card__primary-link" aria-label={(en?'Details: ':'상세 보기: ')+(item.displayName||item.name)} onClick={onOpen}/><div className="plugin-catalog-card__content"><div className="installed-plugins-card__head"><PluginArtwork item={item}/><div className="installed-plugins-card__identity"><div className="plugin-card-title-row"><h3>{item.displayName||item.name}</h3>{item.ownerOfficial&&<BadgeCheck className="size-4 shrink-0 text-blue-600" aria-label={en?'Verified publisher':'확인된 게시자'}/>}</div><span className="plugin-card-author">{item.ownerHandle?'@'+item.ownerHandle:item.host?.packageName||item.latestVersion||''}</span></div><div className="plugin-catalog-card__action">{item.installed?<span className={'plugin-state-dot '+(item.host?.state==='needs-setup'?'needs-setup':item.host?.state==='error'?'has-error':item.enabled?'is-enabled':'')} role="img" aria-label={item.host?.state==='needs-setup'?(en?'Setup required':'추가 설정 필요'):item.host?.state==='error'?(en?'Needs attention':'확인 필요'):item.enabled?(en?'Enabled':'활성화됨'):(en?'Disabled':'비활성화됨')}/>:<InstallButton en={en} operation={operation} onInstall={onInstall} onProgress={onProgress}/>}</div></div><p className="installed-plugins-card__summary">{item.summary}</p></div></article>
}
function InstallButton({en,operation,onInstall,onProgress}:{en:boolean;operation?:Installation;onInstall:()=>void;onProgress:()=>void}){
 return <Button variant="secondary" className="plugin-catalog-card__install" aria-busy={operation?.status==='running'} onClick={operation&&operation.status!=='completed'?onProgress:onInstall}>{operation?.status==='running'?<LoaderCircle size={15} className="animate-spin"/>:null}{operation?.status==='running'?(en?'Installing':'설치 중'):operation?.status==='failed'?(en?'Check status':'상태 확인'):(en?'Install':'설치')}</Button>
}
function InstallProgress({operation,en,onClose}:{operation:Installation;en:boolean;onClose:()=>void}){
 const [now,setNow]=useState(Date.now())
 useEffect(()=>{if(operation.status!=='running')return;const timer=setInterval(()=>setNow(Date.now()),1000);return()=>clearInterval(timer)},[operation.status])
 return <DialogBackdrop onDismiss={onClose}><DialogPanel aria-label={en?'Installation progress':'설치 진행 상황'} className="plugin-install-progress !max-w-xl"><div className="plugin-install-progress__header"><strong>{en?'Installation progress':'설치 진행 상황'}</strong><span>{Math.floor(((operation.finishedAt||now)-operation.startedAt)/1000)}{en?'s':'초'}</span><Button variant="ghost" size="icon" onClick={onClose} aria-label={en?'Close installation progress':'설치 진행 상황 닫기'}><X size={17}/></Button></div><ul className="plugin-install-progress__activities">{operation.activities.map((activity,index)=><li key={index} className="plugin-install-progress__activity">{activity.status==='completed'||!activity.status&&operation.status==='completed'?<Check size={22} className="text-emerald-700"/>:activity.status!=='failed'&&operation.status==='running'?<LoaderCircle size={22} className="animate-spin text-brand"/>:<X size={22} className="text-rose-700"/>}<span>{activity.label}</span></li>)}</ul>{operation.status==='completed'&&<p role="status" className="mt-4 font-semibold text-emerald-800">{en?'Installed and applied':'설치 및 적용 완료'}</p>}<Notice message={operation.error||''}/></DialogPanel></DialogBackdrop>
}
function PluginDetails({item,en,onBack,onChanged,onInstall,operation,onProgress}:{item:Plugin;en:boolean;onBack:()=>void;onChanged:()=>void;onInstall:()=>void;operation?:Installation;onProgress:()=>void}){
 const remote=!item.connection&&!item.appId&&!item.nativeKind&&!item.nativeTools
 const detail=useRemote<Detail>(remote?'/api/plugins/catalog?view=detail&id='+encodeURIComponent(item.name):null,{},false)
 const [asking,setAsking]=useState(false),[settings,setSettings]=useState(false),[busy,setBusy]=useState(false),[error,setError]=useState('')
 const summary=detail.value.version?.pluginManifestSummary
 const contents=[...Object.entries(summary?.contracts||{}).filter(([,items])=>Array.isArray(items)).map(([name,items])=>({name,items})),{name:en?'Skills':'스킬',items:summary?.bundledSkills||[]},{name:en?'Channels':'채널',items:summary?.channels||[]},{name:en?'Connections':'연결',items:summary?.mcpServers||[]},...(item.nativeTools?[{name:en?'Included tools':'포함된 도구',items:item.nativeTools}]:[])].filter(group=>group.items.length)
 const action=async(method:string,params:Record<string,unknown>)=>{if(busy)return;setBusy(true);setError('');try{await post('/api/plugins/host/action',{method,params});onChanged();if(method==='plugins.uninstall')onBack()}catch(e){setError((e as Error).message)}finally{setBusy(false)}}
 return <div className={'plugin-detail-layout '+(asking?'has-assistant':'')}><div className="plugin-detail-scroll"><nav className="plugins-settings-breadcrumb" aria-label={en?'Plugin navigation':'플러그인 경로'}><button type="button" onClick={onBack}>{en?'Plugins':'플러그인'}</button><ChevronRight size={16}/><span>{item.displayName||item.name}</span></nav><section className="plugin-catalog-detail plugin-catalog-detail--no-sidebar"><div className="plugin-catalog-detail__hero"><PluginArtwork item={item} large/><div className="plugin-catalog-detail__heading"><h1 className="plugin-detail-title">{item.displayName||item.name}</h1><p className="plugin-detail-author">{item.ownerHandle?'@'+item.ownerHandle:item.host?.packageName||'Dominion'} {item.ownerOfficial&&<BadgeCheck className="inline size-5 text-blue-600"/>}</p><p className="plugin-catalog-detail__summary">{item.summary}</p><div className="plugin-catalog-detail__actions">{!item.installed&&<InstallButton en={en} operation={operation} onInstall={onInstall} onProgress={onProgress}/>}<Button onClick={()=>setAsking(true)}>{en?'Ask about this plugin':'이 플러그인에 대해 질문하기'}</Button>{item.host&&<><Button variant="secondary" disabled={busy} onClick={()=>void action('plugins.setEnabled',{pluginId:item.host!.id,enabled:!item.enabled})}>{item.enabled?(en?'Disable':'비활성화'):(en?'Enable':'활성화')}</Button>{item.host.removable&&<Button variant="secondary" disabled={busy} onClick={()=>void action('plugins.uninstall',{pluginId:item.host!.id})}>{en?'Remove':'제거'}</Button>}<Button variant="secondary" size="icon" aria-label={en?'Plugin settings':'플러그인 설정'} onClick={()=>setSettings(value=>!value)}><Settings size={20}/></Button></>}{item.nativeTools&&<Button variant="secondary" onClick={()=>setSettings(value=>!value)}>{en?'Connected accounts':'연결된 계정'}</Button>}</div></div></div>
  <Notice message={error||detail.error} onRetry={()=>void detail.reload()}/>{detail.loading&&<p role="status" className="my-4 text-muted">{en?'Loading plugin contents…':'플러그인 구성을 불러오는 중…'}</p>}
  {settings&&item.host&&<PluginConfiguration id={item.host.id} onChanged={onChanged}/>}{settings&&item.nativeTools&&<ConnectedAccounts/>}
  {item.connection&&(item.installed?<McpPluginSettings connection={item.connection} onChanged={onChanged} onRemoved={onBack}/>:<p className="my-5 text-sm text-muted">{item.connection.packageName||item.connection.url}</p>)}
  {item.appId&&<SdkExtensionsPanel appId={item.appId}/>}{item.nativeKind&&<ConnectionsPage kind={item.nativeKind} embedded/>}
  {contents.length>0&&<section className="plugin-catalog-detail__panel"><h2 className="mb-5 border-b border-divider pb-4 text-xl font-semibold">{en?'Included features':'이 플러그인에 포함된 기능'}</h2>{contents.map(group=><div key={group.name} className="mb-6"><h3 className="mb-3 text-sm text-muted">{group.name} · {group.items.length}</h3><ul className="divide-y divide-divider">{group.items.map((surface,index)=><li key={index} className="py-4"><strong className="text-sm">{typeof surface==='string'?surface:surface.name||('id' in surface?surface.id:undefined)}</strong>{typeof surface!=='string'&&surface.description&&<p className="mt-2 text-sm leading-6 text-muted">{surface.description}</p>}</li>)}</ul></div>)}</section>}
  {detail.value.readme&&<section className="plugin-catalog-detail__readme-section"><h2 className="mb-5 border-b border-divider pb-4 text-xl font-semibold">README</h2><ChatRichText text={detail.value.readme}/></section>}
 </section></div>{asking&&<PluginAssistant key={item.name} plugin={item} onClose={()=>setAsking(false)} onChanged={onChanged}/>}</div>
}
function PluginAdministration({onChanged,onClose}:{onChanged:()=>void;onClose:()=>void}){
 const en=useLocale()==='en',host=useRemote<{installed:boolean;running:boolean;version:string}>('/api/plugins/host',{installed:false,running:false,version:''})
 const [mode,setMode]=useState<'menu'|'connection'|'app'>('menu'),[error,setError]=useState('')
 return <><div className="mb-5 flex items-center justify-between"><h2 className="text-lg font-semibold">{en?'Plugin administration':'플러그인 관리 설정'}</h2><Button variant="ghost" size="icon" aria-label={en?'Close plugin administration':'플러그인 관리 설정 닫기'} onClick={onClose}><X size={18}/></Button></div><Notice message={error||host.error}/>{mode==='menu'?<div className="grid gap-4"><p className="text-sm text-muted">{host.value.running?(en?'Plugin host is running':'플러그인 호스트 실행 중'):(en?'The plugin host starts automatically on installation.':'설치 버튼을 누르면 필요한 호스트가 자동으로 준비됩니다.')} {host.value.version}</p><Button variant="secondary" onClick={()=>setMode('connection')}><Plus size={16}/>{en?'Connect a custom plugin':'직접 플러그인 연결'}</Button><Button variant="secondary" onClick={()=>setMode('app')}>{en?'Register an SDK app':'SDK 앱 등록'}</Button>{!host.value.running&&<Button variant="ghost" onClick={()=>void post('/api/plugins/host/start',{}).then(()=>{void host.reload();onChanged()}).catch(e=>setError(e.message))}>{en?'Check plugin host':'플러그인 호스트 확인'}</Button>}{host.value.running&&<Button variant="ghost" onClick={()=>void post('/api/plugins/host/stop',{}).then(()=>{void host.reload();onChanged()}).catch(e=>setError(e.message))}>{en?'Stop plugin host':'플러그인 호스트 중지'}</Button>}</div>:mode==='connection'?<McpPluginSettings connection={{name:'',transport:'http',enabled:false}} onChanged={onChanged} onRemoved={onClose}/>:<SdkExtensionsPanel registrationOnly onRegistered={()=>{onChanged();onClose()}}/>}</>
}
