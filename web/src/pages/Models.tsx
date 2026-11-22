import {ArrowRight, Cloud, FileKey2, HardDrive, KeyRound, LoaderCircle, Plus, RefreshCw, Settings2, UserRound} from 'lucide-react'
import {Button, PageContainer} from '../components/Primitives'
import {Notice} from '../components/UI'
import {useLocale} from '../i18n'
import {providerConfigured} from '../features/conversations/resolveRoute'
import {providerPresets, type Tab} from '../features/models/catalog'
import {useModelHub} from '../features/models/useModelHub'
import {ConnectionDialog, ConnectionTools} from '../features/models/ConnectionTools'
import {PageHeader, ViewTabs, SearchField, EmptyState} from '../components/ui/WorkspaceUI'

export function ModelHub() {
 const en=useLocale()==='en'
 const tr=(ko:string,english:string)=>en?english:ko
 const hub=useModelHub()
 const {providers, tab, query, setQuery, counts, open, openPreset, probe, selectTab}=hub
 const entries=providers.value.filter(provider=>(provider.name+' '+(provider.model||'')).toLocaleLowerCase().includes(query.trim().toLocaleLowerCase()))
 const choices=providerPresets.filter(preset=>!preset.local&&preset.name.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase())).sort((a,b)=>a.name.localeCompare(b.name))
 const routes:Array<{id:Tab;label:string}>=[{id:'providers',label:tr('등록한 AI','Your AIs')},{id:'connect',label:tr('AI 연결 추가','Add a connection')},{id:'local',label:tr('이 컴퓨터의 AI','On this computer')},{id:'catalog',label:tr('서비스·모델 목록','Services & models')}]
 const methods=[
  {icon:KeyRound,title:tr('API 키로 연결','Use an API key'),description:tr('사용할 서비스를 선택하고 연결 정보를 입력합니다.','Choose a service and enter its connection details.'),run:()=>selectTab('connect')},
  {icon:UserRound,title:tr('계정으로 연결','Connect an account'),description:tr('지원되는 서비스의 로그인과 기존 인증을 관리합니다.','Manage supported account sign-in and existing credentials.'),run:()=>selectTab('oauth')},
  {icon:HardDrive,title:tr('이 컴퓨터에서 실행','Run on this computer'),description:tr('설치한 모델, 다운로드와 실행 상태를 확인합니다.','Manage installed models, downloads and runtime status.'),run:()=>selectTab('local')},
  {icon:Cloud,title:tr('서비스·모델 둘러보기','Browse services & models'),description:tr('목록을 확인하고 사용할 연결을 직접 고릅니다.','Explore the catalog and choose a connection yourself.'),run:()=>selectTab('catalog')},
 ]
 return <PageContainer>
  <PageHeader title={tr('사용할 AI와 연결을 관리하세요','Manage your AIs and connections')} description={tr('등록 정보와 실제 연결 확인 결과를 구분해서 표시합니다.','Saved configuration and verified connection results are shown separately.')} actions={<Button variant="primary" onClick={()=>open()}><Plus size={17}/>{tr('직접 연결 추가','Add custom connection')}</Button>}/>
  <ViewTabs value={tab} options={routes.map(item=>({...item,count:item.id==='providers'?providers.value.length:undefined}))} onChange={next=>{setQuery('');selectTab(next)}} label={tr('AI 연결 화면','AI connection views')}/>

  {(tab==='providers'||tab==='connect')&&<div className="mb-6 flex max-w-xl"><SearchField value={query} onChange={setQuery} label={tr('AI·서비스 검색','Search AIs and services')} placeholder={tr('AI 이름이나 모델로 검색','Search by AI or model')}/></div>}
  {tab==='providers'&&<>
   {providers.loading&&!providers.value.length?<div className="flex min-h-40 items-center justify-center gap-3 text-sm text-muted" role="status"><LoaderCircle size={22} className="animate-spin"/>{tr('연결 정보를 불러오는 중','Loading connections')}</div>:providers.error?<Notice message={providers.error} onRetry={()=>void providers.reload()}/>:!providers.value.length?<EmptyState icon={Cloud} title={tr('아직 등록한 AI가 없습니다','No AIs connected yet')} description={tr('아래에서 연결 방식을 선택하세요. 사용할 서비스는 직접 고를 수 있습니다.','Choose a connection method below, then select the service you want to use.')}/>:<div className="grid grid-cols-1 gap-4 lg:grid-cols-2">
    {entries.map(provider=>{const checked=counts[provider.id];const configured=providerConfigured(provider);const verified=Boolean(checked&&!checked.busy&&!checked.error&&!checked.info&&checked.list.length);return <article key={provider.id} className="flex min-w-0 flex-col gap-5 rounded-2xl border border-divider bg-surface p-5 sm:p-6 [&>header]:flex [&>header]:items-center [&>header]:gap-3 [&>header>div]:min-w-0 [&>header>div]:flex-1 [&_h3]:text-base [&_h3]:font-semibold [&>footer]:mt-auto [&>footer]:flex [&>footer]:flex-wrap [&>footer]:items-center [&>footer]:justify-between [&>footer]:gap-3 [&>footer]:border-t [&>footer]:border-divider [&>footer]:pt-4">
     <header><span className="grid size-11 shrink-0 place-items-center rounded-xl border border-divider bg-canvas text-muted">{provider.local?<HardDrive size={22}/>:<Cloud size={22}/>}</span><div><h3>{provider.name}</h3><span className="mt-1 block text-xs text-muted data-[state=verified]:text-success data-[state=error]:text-danger" data-state={checked?.error?'error':verified?'verified':configured?'saved':'pending'}>{provider.enabled===false?tr('사용 중지','Disabled'):checked?.error?tr('확인 필요','Needs attention'):verified?tr('연결 확인됨','Connection verified'):configured?tr('연결 정보 등록됨','Configuration saved'):tr('연결 설정 필요','Setup needed')}</span></div><Button variant="ghost" size="icon" onClick={()=>provider.kind==='cli'?selectTab('oauth'):open(provider)} aria-label={tr(provider.name+' 설정','Configure '+provider.name)}><Settings2 size={18}/></Button></header>
     <div className="grid gap-1.5 [&>span]:text-xs [&>span]:text-muted [&>strong]:break-words [&>strong]:text-sm [&>strong]:font-medium"><span>{tr('기본 모델','Default model')}</span><strong>{provider.model||tr('선택하지 않음','Not selected')}</strong></div>
     {checked?.info&&<p className="text-sm leading-7 text-muted">{checked.info}</p>}
     {checked?.error&&<p className="text-sm leading-7 text-danger" role="alert">{checked.error}</p>}
     {verified&&<p className="text-sm text-success">{tr(checked.list.length+'개 모델 확인',checked.list.length+' models available')}</p>}
     <footer><Button variant="ghost" disabled={checked?.busy} onClick={()=>void probe(provider)}>{checked?.busy?<LoaderCircle size={16} className="animate-spin"/>:<RefreshCw size={16}/>} {tr('연결 확인','Check connection')}</Button>
      {configured?<a className="inline-flex min-h-10 items-center gap-2 rounded-xl bg-brand px-4 py-2 text-sm font-semibold text-white hover:bg-brand-hover" href={'#/chat?provider='+encodeURIComponent(provider.id)+(provider.model?'&model='+encodeURIComponent(provider.model):'')}>{tr('대화하기','Chat')}<ArrowRight size={16}/></a>:<Button variant="secondary" onClick={()=>open(provider)}>{tr('설정하기','Configure')}</Button>}
     </footer>
    </article>})}
    {!entries.length&&<p className="flex min-h-40 items-center justify-center gap-3 text-sm text-muted">{tr('검색 결과가 없습니다.','No matching connections.')}</p>}
   </div>}
   <section className="mt-8 [&>h3]:mb-4 [&>h3]:text-base [&>h3]:font-semibold [&>div]:grid [&>div]:grid-cols-1 [&>div]:gap-3 md:[&>div]:grid-cols-2" aria-label={tr('연결 방법','Connection methods')}><h3>{tr('연결 방법','Connection methods')}</h3><div>{methods.map(({icon:Icon,title,description,run})=><Button variant="bare" className="flex min-w-0 items-center gap-4 rounded-2xl border border-divider bg-surface p-5 text-left transition-colors hover:border-brand [&>svg]:shrink-0 [&>svg]:text-brand [&>span]:min-w-0 [&>span]:flex-1 [&_strong]:block [&_strong]:text-sm [&_strong]:font-semibold [&_small]:mt-1.5 [&_small]:block [&_small]:text-xs [&_small]:leading-6 [&_small]:text-muted" key={title} onClick={run}><Icon size={22}/><span><strong>{title}</strong><small>{description}</small></span><ArrowRight size={17}/></Button>)}</div></section>
  </>}
  {tab==='connect'&&<section className="provider-source-section"><div className="mb-5 flex flex-wrap items-center justify-between gap-3 [&>h3]:text-lg [&>h3]:font-semibold"><h3>{tr('연결할 서비스를 선택하세요','Choose a service')}</h3><Button variant="secondary" onClick={()=>selectTab('oauth')}><UserRound size={16}/>{tr('계정 연결','Connect an account')}</Button></div>
   <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 xl:grid-cols-3 [&>button]:flex [&>button]:items-center [&>button]:gap-3 [&>button]:rounded-xl [&>button]:border [&>button]:border-divider [&>button]:bg-surface [&>button]:p-4 [&>button]:text-left [&>button:hover]:border-brand [&_strong]:min-w-0 [&_strong]:flex-1 [&_strong]:text-sm [&_strong]:font-medium">{choices.map(preset=><Button variant="bare" key={preset.name} onClick={()=>openPreset(preset)}><span className="grid size-9 shrink-0 place-items-center rounded-lg bg-brand-soft text-sm font-semibold text-brand-hover">{preset.name.slice(0,1)}</span><strong>{preset.name}</strong><ArrowRight size={16}/></Button>)}</div>
   <Button variant="secondary" className="mt-5" onClick={()=>open()}><FileKey2 size={17}/>{tr('직접 주소와 연결 정보 입력','Enter an endpoint and connection details')}</Button>
  </section>}
  {!['providers','connect'].includes(tab)&&<><div className="mb-5 flex flex-wrap items-center justify-between gap-3 [&>h3]:text-lg [&>h3]:font-semibold"><h3>{tab==='oauth'?tr('계정 연결 관리','Account connections'):tab==='local'?tr('설치·다운로드·실행','Installations, downloads & runtime'):tab==='catalog'?tr('서비스·모델 목록','Services & models'):tr('모델 찾기','Find models')}</h3><Button variant="ghost" onClick={()=>selectTab('providers')}>{tr('등록한 AI로 돌아가기','Back to your AIs')}</Button></div><ConnectionTools hub={hub}/></>}
  <nav className="mt-8 flex flex-wrap gap-1 border-t border-divider pt-4 [&>button]:text-xs" aria-label={tr('추가 연결 기능','Additional connection options')}><Button variant="ghost" onClick={()=>selectTab('oauth')}>{tr('계정 연결','Accounts')}</Button><Button variant="ghost" onClick={()=>selectTab('free')}>{tr('무료 모델 조회','Free model lookup')}</Button><Button variant="ghost" onClick={()=>selectTab('huggingface')}>{tr('모델 파일 찾기','Find model files')}</Button><Button variant="ghost" onClick={()=>open()}>{tr('직접 연결 설정','Custom endpoint')}</Button></nav>
  <ConnectionDialog hub={hub}/>
 </PageContainer>
}
