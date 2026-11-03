import {useEffect,useMemo,useState} from 'react'
import {ArrowDownToLine,ArrowRight,BookOpenText,BrainCircuit,Check,Cloud,Compass,Cpu,Download,ExternalLink,Eye,HardDrive,LoaderCircle,Pause,Play,Plus,RefreshCw,Server,Square,Trash2,Wrench} from 'lucide-react'
import {del,get,post,type Provider} from '../api'
import {Alert,Badge,Button,Card,CodeText,Dialog,Divider,Empty,Grid,Highlight,IconButton,Input,KeyValue,ListItem,Page,PageHeader,Pane,Progress,Row,ScrollList,SearchBox,Section,Select,Selectable,Split,Stack,Tabs,Text,Textarea,Toggle} from '../design/Primitives'
import {notifyDataChanged,useOperation,useResource} from '../design/useResource'
type Model={id:string;downloads:number;likes:number;updatedAt:string;task:string;tags:string[];gated:boolean;description:string;capabilities:{vision:boolean;tools:boolean;reasoning:boolean}}
type Variant={id:string;format:'gguf'|'mlx';label:string;quantization:string;bytes:number;files:string[];shards:number}
type Detail=Model&{revision:string;architecture:string;license:string;readme:string;options:Variant[]}
type Owned={id:string;repo:string;revision:string;optionId:string;format:'gguf'|'mlx';label:string;files:string[];bytes:number;installedAt:string}
type Job={id:string;repo:string;optionId:string;status:string;downloaded:number;total:number;current:string;error?:string}
type Runtime={status:string;id?:string;repo?:string;name?:string;error?:string;capabilities?:{gguf:{available:boolean};mlx:{available:boolean};memory:{total:number;free:number};gpu?:{kind:string;free:number}|null}}
const size=(n=0)=>n>=1e9?(n/1e9).toFixed(2)+' GB':n>=1e6?(n/1e6).toFixed(0)+' MB':n+' B'
const compact=(n=0)=>n>=1e6?(n/1e6).toFixed(1)+'M':n>=1e3?(n/1e3).toFixed(0)+'K':String(n)
const title=(repo:string)=>repo.split('/').at(-1)||repo
const inProgress=(x?:Job)=>x?.status==='resolving'||x?.status==='downloading'
function ModelJobs({job,onManage}:{job:Job;onManage:(id:string,action:'pause'|'resume'|'cancel')=>void}){
 return <Stack gap="sm"><Progress value={job.downloaded} total={job.total||1} label={job.status==='downloading'?'다운로드 중':job.status} detail={size(job.downloaded)+' / '+size(job.total)}/><Text tone="subtle" size="xs">{job.error||job.current||'파일 처리 중'}</Text><Row gap="xs" wrap>{inProgress(job)&&<Button size="sm" onClick={()=>onManage(job.id,'pause')}><Pause size={14}/> 일시정지</Button>}{['paused','failed','cancelled'].includes(job.status)&&<Button size="sm" onClick={()=>onManage(job.id,'resume')}><Play size={14}/> 재개</Button>}{(inProgress(job)||job.status==='paused')&&<Button size="sm" variant="ghost" onClick={()=>onManage(job.id,'cancel')}><Square size={14}/> 중단</Button>}</Row></Stack>
}
function Catalog({openChat}:{openChat:()=>void}){
 const [tab,setTab]=useState('discover')
 const [query,setQuery]=useState('')
 const [format,setFormat]=useState('all')
 const [sort,setSort]=useState('downloads')
 const [catalog,setCatalog]=useState<Model[]>([])
 const [selected,setSelected]=useState('')
 const [detail,setDetail]=useState<Detail|null>(null)
 const [variant,setVariant]=useState('')
 const [readme,setReadme]=useState(false)
 const [searching,setSearching]=useState(false)
 const [loadingDetail,setLoadingDetail]=useState(false)
 const [error,setError]=useState('')
 const [context,setContext]=useState('4096')
 const [gpu,setGpu]=useState(true)
 const installed=useResource<Owned[]>('/api/model-library/installed',[])
 const downloads=useResource<Job[]>('/api/model-library/jobs',[])
 const runner=useResource<Runtime>('/api/model-library/runtime',{status:'idle'})
 const operation=useOperation()
 useEffect(()=>{const t=setInterval(()=>{void installed.reload();void downloads.reload();void runner.reload()},2400);return()=>clearInterval(t)},[])
 useEffect(()=>{
  let live=true
  const handle=setTimeout(async()=>{setSearching(true)
   try{const params=new URLSearchParams({q:query,format,sort});const rows=await get<Model[]>('/api/model-library/catalog?'+params);if(live){setCatalog(rows);setSelected(old=>rows.some(m=>m.id===old)?old:rows[0]?.id||'');setError('')}}
   catch(e){if(live){setCatalog([]);setSelected('');setError((e as Error).message)}}finally{if(live)setSearching(false)}
  },query?340:70)
  return()=>{live=false;clearTimeout(handle)}
 },[query,format,sort])
 useEffect(()=>{
  let live=true;setDetail(null);setVariant('');setReadme(false)
  if(!selected){setLoadingDetail(false);return}
  setLoadingDetail(true)
  get<Detail>('/api/model-library/details?id='+encodeURIComponent(selected)).then(data=>{if(!live)return;setDetail(data);setVariant(data.options.find(o=>format==='all'||o.format===format)?.id||data.options[0]?.id||'')}).catch(e=>{if(live)setError((e as Error).message)}).finally(()=>{if(live)setLoadingDetail(false)})
  return()=>{live=false}
 },[selected])
 const options=detail?.options.filter(o=>format==='all'||o.format===format)||[]
 const choice=options.find(o=>o.id===variant)||options[0]
 const owned=detail&&choice?installed.value.find(x=>x.repo===detail.id&&x.optionId===choice.id&&x.revision===detail.revision):undefined
 const job=detail&&choice?downloads.value.find(x=>x.repo===detail.id&&x.optionId===choice.id):undefined
 const runtime=runner.value
 const capabilities=runtime.capabilities
 const manage=(id:string,action:'pause'|'resume'|'cancel')=>void operation.run(()=>post('/api/model-library/jobs/'+id+'/'+action,{}))
 const download=()=>{if(detail&&choice)void operation.run(()=>post('/api/model-library/download',{repo:detail.id,optionId:choice.id}))}
 const load=(id:string)=>void operation.run(()=>post('/api/model-library/runtime/load',{id,context:Number(context),gpu}))
 const unload=()=>void operation.run(()=>post('/api/model-library/runtime/unload',{}))
 const remove=(x:Owned)=>{if(window.confirm(title(x.repo)+' 파일을 디스크에서 삭제하시겠습니까?'))void operation.run(()=>del('/api/model-library/installed/'+x.id))}
 const latest=downloads.value.filter(x=>x.status!=='completed')
 const selectedReady=runtime.status==='ready'
 return <Stack gap="lg">
  <Card pad="md" tone="soft"><Row justify="between" wrap><Stack gap="xs"><Text size="xs" tone="brand" weight="bold">MODEL REGISTRY · LOCAL RUNTIME</Text><h2 className="text-2xl font-extrabold tracking-tight text-ink">찾고, 설치하고, 바로 실행하세요.</h2><Text tone="muted">GGUF·MLX 모델을 내 PC에 저장하고 관리합니다.</Text></Stack><Row gap="sm"><Badge tone={selectedReady?'positive':'neutral'}>{selectedReady?'● 모델 실행 중':runtime.status==='loading'?'◉ 로딩 중':'○ 대기 중'}</Badge>{selectedReady&&<Button size="sm" variant="primary" onClick={openChat}>대화 열기 <ArrowRight size={15}/></Button>}</Row></Row></Card>
  <Card pad="none"><Tabs value={tab} onChange={setTab} items={[{value:'discover',label:'모델 탐색'},{value:'installed',label:'설치된 모델',count:installed.value.length},{value:'jobs',label:'다운로드',count:latest.length}]}/>
   {error&&<Pane><Alert message={error} onClose={()=>setError('')}/></Pane>}
   {operation.error&&<Pane><Alert message={operation.error} onClose={()=>operation.setError('')}/></Pane>}
   {tab==='discover'&&<Split left={<Stack gap="none"><Pane><Stack gap="sm"><Row gap="sm"><SearchBox value={query} onChange={setQuery} placeholder="모델명 또는 제작사 검색"/>{searching&&<LoaderCircle size={17} className="animate-spin text-brand"/>}</Row><Row gap="sm" wrap><Select label="형식" value={format} onChange={e=>setFormat(e.target.value)} items={[{value:'all',label:'GGUF + MLX'},{value:'gguf',label:'GGUF'},{value:'mlx',label:'MLX'}]}/><Select label="정렬" value={sort} onChange={e=>setSort(e.target.value)} items={[{value:'downloads',label:'다운로드순'},{value:'likes',label:'추천순'},{value:'lastModified',label:'업데이트순'}]}/></Row></Stack></Pane><Divider/><Pane><Row justify="between"><Text tone="muted" weight="bold" size="sm">{query?'검색 결과':'추천 모델'}</Text><Badge>{catalog.length}개</Badge></Row></Pane><ScrollList height="lg">{catalog.map(m=><Selectable key={m.id} selected={m.id===selected} onClick={()=>setSelected(m.id)}><span className="grid size-10 shrink-0 place-items-center rounded-xl bg-brand-soft text-brand"><Cpu size={20}/></span><Stack gap="xs" className="flex-1 overflow-hidden"><span className="truncate text-[12px] font-bold text-ink">{title(m.id)}</span><span className="truncate text-[10px] text-subtle">{m.id.split('/')[0]}</span><Row gap="xs" wrap><Text size="xs" tone="subtle">↓ {compact(m.downloads)}</Text><Text size="xs" tone="subtle">♡ {compact(m.likes)}</Text>{m.gated&&<Badge tone="warning">승인 필요</Badge>}</Row></Stack></Selectable>)}{!catalog.length&&!searching&&<Empty title="모델을 찾지 못했습니다" description="검색어나 파일 형식을 바꿔보세요."/>}{!catalog.length&&searching&&<Empty icon={<LoaderCircle size={20} className="animate-spin"/>} title="검색 중"/>}</ScrollList><Pane><Text size="xs" tone="subtle">모델 정보 제공: Hugging Face 공개 저장소</Text></Pane></Stack>} right={<Pane><Stack gap="lg">{loadingDetail?<Empty icon={<LoaderCircle className="animate-spin" size={23}/>} title="모델 정보 불러오는 중"/>:detail?<><Row justify="between" align="start"><Stack gap="xs"><Text size="xs" weight="bold" tone="brand">MODEL DETAILS</Text><h3 className="break-words text-[22px] font-bold tracking-tight">{title(detail.id)}</h3><Text size="xs" tone="subtle">{detail.id}</Text></Stack><Button size="sm" variant="secondary" onClick={()=>window.open('https://huggingface.co/'+detail.id,'_blank','noopener,noreferrer')}><ExternalLink size={16}/> 원본</Button></Row>
    <Row gap="xs" wrap><Badge>↓ {compact(detail.downloads)} 다운로드</Badge><Badge>♡ {compact(detail.likes)} 추천</Badge><Badge>{detail.updatedAt?new Date(detail.updatedAt).toLocaleDateString('ko-KR'):'날짜 없음'}</Badge></Row>
    <Highlight title="모델 정보" icon={<BrainCircuit size={15}/>}><Stack gap="sm"><p className="whitespace-pre-line">{(detail.description||detail.readme.replace(/^---[\s\S]*?---/,'').replace(/[#*\[\]>]/g,'').trim()||'모델 정보를 제공하지 않습니다.').slice(0,420)}</p><Row gap="sm" wrap><Badge>{detail.architecture||'아키텍처 미표기'}</Badge><Badge>{detail.license||'라이선스 미표기'}</Badge>{detail.capabilities.vision&&<Badge tone="brand"><Eye size={12}/> Vision</Badge>}{detail.capabilities.tools&&<Badge tone="brand"><Wrench size={12}/> Tools</Badge>}{detail.capabilities.reasoning&&<Badge tone="brand"><BrainCircuit size={12}/> Reasoning</Badge>}</Row></Stack></Highlight>
    <Section title="다운로드 옵션" subtitle="실제 저장소 파일에서 확인한 버전입니다."><ScrollList height="sm">{options.length?options.map(o=><Selectable key={o.id} selected={choice?.id===o.id} onClick={()=>setVariant(o.id)}><Badge tone="brand">{o.format.toUpperCase()}</Badge><Stack gap="xs" className="flex-1 overflow-hidden"><Text size="sm" weight="bold">{o.label}</Text><Text size="xs" tone="subtle">{o.quantization||'양자화 미표기'}{o.shards>1?' · '+o.shards+'개 파일':''}</Text></Stack><Text size="sm" weight="semibold">{size(o.bytes)}</Text>{choice?.id===o.id&&<Check size={17} className="text-brand"/>}</Selectable>):<Empty title="사용 가능한 파일이 없습니다" description="현재 선택한 형식의 GGUF·MLX 파일이 없습니다."/>}</ScrollList></Section>
    {choice&&<Stack gap="sm"><Row justify="between" wrap><Text tone="muted" size="sm">디스크 용량 {size(choice.bytes)}</Text>{capabilities?.gpu&&<Text tone="subtle" size="xs">메모리 여유 {size(capabilities.gpu.free)} (추정)</Text>}</Row>{job&&job.status!=='completed'&&<ModelJobs job={job} onManage={manage}/>}<Row justify="end" wrap>{owned&&<Badge tone="positive"><Check size={14}/> 설치됨</Badge>}{owned?<Button variant="primary" disabled={operation.working||runtime.status==='loading'||!capabilities?.[owned.format].available} loading={operation.working} onClick={()=>load(owned.id)}><Play size={16}/>{capabilities?.[owned.format].available?'모델 실행':'실행 엔진 필요'}</Button>:<Button variant="primary" disabled={Boolean(inProgress(job))||job?.status==='paused'||detail.gated} loading={operation.working} onClick={download}><Download size={16}/>{detail.gated?'접근 승인 필요':'다운로드'} · {size(choice.bytes)}</Button>}</Row></Stack>}
    <Row justify="between"><Text weight="bold" size="sm">README</Text><Button size="sm" variant="ghost" onClick={()=>setReadme(x=>!x)}>{readme?'접기':'펼치기'}</Button></Row>{readme&&<CodeText>{detail.readme.slice(0,7500)||'README가 없습니다.'}</CodeText>}
   </>:<Empty title="왼쪽에서 모델을 선택하세요"/>}</Stack></Pane>}/>}
   {tab==='installed'&&<Pane><Stack gap="sm">{installed.value.length?installed.value.map(x=><Card key={x.id} pad="sm"><ListItem title={title(x.repo)} subtitle={x.repo+' · '+x.label+' · '+size(x.bytes)} leading={<HardDrive size={20}/>} meta={<Badge>{x.format.toUpperCase()}</Badge>} actions={<><Button size="sm" variant="primary" disabled={!capabilities?.[x.format].available||operation.working} onClick={()=>load(x.id)}><Play size={14}/>{runtime.id===x.id&&selectedReady?'다시 실행':'실행'}</Button><IconButton icon={<Trash2 size={16}/>} label="모델 삭제" size="sm" onClick={()=>remove(x)}/></>}/></Card>):<Empty icon={<HardDrive size={23}/>} title="설치된 모델이 없습니다" description="모델 탐색에서 원하는 GGUF·MLX 모델을 다운로드하세요." action={<Button onClick={()=>setTab('discover')}>모델 탐색 <ArrowRight size={15}/></Button>}/>}</Stack></Pane>}
   {tab==='jobs'&&<Pane><Stack gap="md">{downloads.value.length?downloads.value.map(j=><Card key={j.id} pad="sm"><Stack gap="sm"><Row justify="between"><Text weight="bold">{title(j.repo)}</Text><Badge tone={j.status==='completed'?'positive':j.status==='failed'?'negative':'neutral'}>{j.status}</Badge></Row><Text tone="subtle" size="xs">{j.optionId}</Text><ModelJobs job={j} onManage={manage}/></Stack></Card>):<Empty title="다운로드 작업이 없습니다"/>}</Stack></Pane>}
  </Card>
  <Card><Row justify="between" wrap><Stack gap="xs"><Text weight="bold">로컬 실행 엔진</Text><Text tone="subtle" size="xs">GGUF {capabilities?.gguf.available?'준비됨':'llama-server 설치 필요'} · MLX {capabilities?.mlx.available?'준비됨':'Apple Silicon + mlx-lm 필요'}</Text>{runtime.error&&<Text tone="negative" size="xs">{runtime.error}</Text>}</Stack><Row gap="sm" wrap><Select label="컨텍스트" value={context} onChange={e=>setContext(e.target.value)} items={['2048','4096','8192','16384','32768'].map(v=>({value:v,label:Number(v)/1024+'K'}))}/><Toggle label="GGUF GPU 자동 배치" checked={gpu} onChange={setGpu}/>{selectedReady&&<><Button onClick={unload} disabled={operation.working}><Square size={15}/> 언로드</Button><Button variant="primary" onClick={openChat}>대화 열기 <ArrowRight size={15}/></Button></>}</Row></Row></Card>
 </Stack>
}
function ProviderConnections(){
 const providers=useResource<Provider[]>('/api/providers',[])
 const [open,setOpen]=useState(false),[editing,setEditing]=useState<Provider|null>(null)
 const [name,setName]=useState(''),[baseUrl,setBaseUrl]=useState(''),[kind,setKind]=useState('openai'),[key,setKey]=useState(''),[local,setLocal]=useState(false)
 const [probed,setProbed]=useState<Record<string,string>>({})
 const operation=useOperation()
 const list=providers.value.filter(p=>p.id!=='dominion-native')
 const begin=(p?:Provider)=>{setEditing(p||null);setName(p?.name||'');setBaseUrl(p?.baseUrl||'');setKind(p?.kind||'openai');setKey('');setLocal(Boolean(p?.local));setOpen(true)}
 const save=()=>void operation.run(async()=>{await post('/api/providers',{id:editing?.id,name,baseUrl,kind,apiKey:key||undefined,local,enabled:true});setOpen(false);await providers.reload()})
 const probe=(p:Provider)=>void operation.run(async()=>{const m=await get<{id:string}[]>('/api/providers/'+p.id+'/models');setProbed(x=>({...x,[p.id]:m.length+'개 모델 연결 확인'}))})
 const remove=(p:Provider)=>{if(window.confirm(p.name+' 연결을 삭제하시겠습니까?'))void operation.run(()=>del('/api/providers/'+p.id))}
 return <Section title="외부 모델 API · 선택 사항" subtitle="자체 로컬 모델 관리와 별도로 외부 공급자를 연결할 수 있습니다." actions={<Button onClick={()=>begin()}><Plus size={15}/> 제공자 추가</Button>}>
  {operation.error&&<Alert message={operation.error}/>}
  <Grid cols={3}>{list.map(p=><Card key={p.id} pad="sm"><Stack gap="md"><Row justify="between"><Row gap="sm"><Cloud size={18} className="text-brand"/><Text weight="bold">{p.name}</Text></Row><Badge tone={p.enabled?'positive':'neutral'}>{p.enabled?'활성':'비활성'}</Badge></Row><Text tone="subtle" size="xs">{p.baseUrl}</Text><Row justify="between" gap="sm" wrap><Text tone="muted" size="xs">{probed[p.id]||'상태 미검사'}</Text><Row gap="xs"><Button size="sm" onClick={()=>probe(p)}>연결 확인</Button><IconButton label="제공자 수정" size="sm" icon={<Server size={15}/>} onClick={()=>begin(p)}/><IconButton label="제공자 삭제" size="sm" icon={<Trash2 size={15}/>} onClick={()=>remove(p)}/></Row></Row></Stack></Card>)}</Grid>
  {!list.length&&<Empty title="등록된 외부 제공자가 없습니다" description="필요할 때만 연결하세요."/>}
  <Dialog open={open} onClose={()=>setOpen(false)} title={editing?'제공자 수정':'외부 API 제공자 연결'} footer={<><Button onClick={()=>setOpen(false)}>취소</Button><Button variant="primary" onClick={save} disabled={!name.trim()||!baseUrl.trim()} loading={operation.working}>저장</Button></>}><Stack><Input label="이름" value={name} onChange={e=>setName(e.target.value)} placeholder="예: OpenRouter"/><Input label="API Base URL" value={baseUrl} onChange={e=>setBaseUrl(e.target.value)} placeholder="https://api.example.com/v1"/><Select label="프로토콜" value={kind} onChange={e=>setKind(e.target.value)} items={[{value:'openai',label:'OpenAI 호환'},{value:'ollama',label:'Ollama'}]}/><Input label="API Key (기존 키 유지하려면 빈칸)" type="password" value={key} onChange={e=>setKey(e.target.value)}/><Toggle checked={local} onChange={setLocal} label="로컬 엔드포인트"/></Stack></Dialog>
 </Section>
}
export function ModelHub({openChat=()=>{}}:{openChat?:()=>void}){
 return <Page wide><PageHeader eyebrow="MODEL WORKSPACE" title="모델 라이브러리" description="모델 탐색부터 다운로드, 설치, 로컬 실행까지 한곳에서 관리합니다."/><Stack gap="xl"><Catalog openChat={openChat}/><ProviderConnections/></Stack></Page>
}
