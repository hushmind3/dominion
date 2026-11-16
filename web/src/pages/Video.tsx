import {useEffect,useMemo,useState} from 'react'
import {
 ArrowDown,ArrowRight,ArrowUp,Check,CheckCircle2,Clapperboard,Clock3,
 Download,ExternalLink,Film,FolderOpen,Layers3,LoaderCircle,Plus,
 RefreshCw,RotateCcw,Settings2,ShieldCheck,SlidersHorizontal,
 Sparkles,Trash2,Upload,Video as VideoIcon,WandSparkles
} from 'lucide-react'
import {Button,TextInput,TextArea,SelectInput} from '../components/Primitives'
import {FormField,Headline,Notice,Panel,StateTag,useRemote} from '../components/UI'
import {openOAuth,post,type Provider} from '../api'
import {jobFinished,useJob,type JobState} from '../hooks/useJob'
import {useLocale} from '../i18n'

type VideoEngine='native'|'comfyui'
type Ratio='9:16'|'16:9'|'1:1'
type StudioTab='create'|'library'|'publish'|'settings'
type StudioMode='quick'|'scenes'
type VideoModel={id:string;name:string;kind:string;available:boolean;hint:string;missingFiles?:number}
type VideoConfig={engine:VideoEngine;comfyUrl:string;voice?:string;narration?:boolean;videoModel?:string;resolution?:string;steps?:number;comfyWorkflow?:unknown;comfyWorkflowName?:string;comfyModelsDir?:string;ffmpegPath?:string}
type VideoJob=JobState & {
 kind:'video'|'youtube_upload';title?:string;brief?:string;engine?:VideoEngine;
 phase?:string;progress?:number;outputUrl?:string|null;createdAt?:string;
 duration?:number;aspectRatio?:Ratio;storyboard?:string[]|null;continuity?:string|null;
 negativePrompt?:string|null;reuseVideoJobId?:string|null;maxNewScenes?:number|null;
 renderParams?:{fps?:number;seed?:number;width?:number;height?:number};
 sceneProgress?:{index?:number|null;total?:number|null;rendered?:number|null;reused?:number|null};
 renderWorkload?:{frames?:number;pixelFrames?:number;megapixelFrames?:number;estimatedMoneyCost?:number|null};
 result?:{bytes?:number;sceneCount?:number;renderedScenes?:number;reusedScenes?:number;reusedFrames?:number;generatedFrames?:number;renderSeconds?:number;url?:string}
}
type UploadJob=JobState & {progress?:number;result?:{url?:string}}
type VideoStatus={config:VideoConfig;modelDirectory?:string;native?:{available:boolean};comfy:{reachable:boolean;available?:boolean;missingNodes?:string[]};workflows:{id:string;name:string;bytes:number}[];jobs:VideoJob[]}
type YoutubeStatus={connected:boolean;uploadAllowed:boolean;account?:{displayName:string;scopes:string}|null;uploads:UploadJob[]}
type Draft={title:string;prompt:string;negative:string;ratio:Ratio;seconds:number;fps:number;seed:string;scenes:string[];continuity:string;mode:StudioMode}
const ratios:Record<Ratio,{label:string;w:number;h:number}>={
 '9:16':{label:'세로 · 쇼츠 / 릴스',w:360,h:640},
 '16:9':{label:'가로 · 유튜브',w:640,h:360},
 '1:1':{label:'정사각형 · 피드',w:480,h:480}
}
const draftKey='dominion.video.studio.draft.v1'
const defaults:Draft={title:'',prompt:'',negative:'',ratio:'9:16',seconds:6,fps:16,seed:'-1',scenes:[''],continuity:'',mode:'quick'}
function loadDraft():Draft {
 try{
  const saved=JSON.parse(localStorage.getItem(draftKey)||'null') as Partial<Draft>|null
  if(!saved || typeof saved!=='object')return defaults
  return {...defaults,
   title:typeof saved.title==='string'?saved.title:'',
   prompt:typeof saved.prompt==='string'?saved.prompt:'',
   negative:typeof saved.negative==='string'?saved.negative:'',
   ratio:saved.ratio && saved.ratio in ratios?saved.ratio:'9:16',
   seconds:Number.isInteger(saved.seconds)&&Number(saved.seconds)>=3&&Number(saved.seconds)<=180?Number(saved.seconds):6,
   fps:Number.isInteger(saved.fps)&&Number(saved.fps)>=1&&Number(saved.fps)<=60?Number(saved.fps):16,
   seed:typeof saved.seed==='string'?saved.seed:'-1',
   mode:saved.mode==='scenes'?'scenes':'quick',
   scenes:Array.isArray(saved.scenes)&&saved.scenes.some(v=>typeof v==='string')?saved.scenes.filter((v):v is string=>typeof v==='string').slice(0,8):[''],
   continuity:typeof saved.continuity==='string'?saved.continuity:''
  }
 }catch{return defaults}
}
const running=(status?:string)=>Boolean(status && !jobFinished(status))
const statusText=(status?:string)=>({
 completed:'완료',running:'제작 중',queued:'대기 중',failed:'실패',
 cancelled:'취소됨',needs_review:'확인 필요',interrupted:'중단됨',downloading:'다운로드 중'
} as Record<string,string>)[status||'']||'상태 확인 중'
const phaseText=(phase?:string)=>({
 storyboard:'장면 준비 중',submitting:'렌더러에 요청 중',rendering:'장면 제작 중',
 encoding:'영상 합성 중',complete:'완료',completed:'완료',uploading:'업로드 중'
} as Record<string,string>)[phase||'']||'처리 중'
const fmtBytes=(bytes=0)=>bytes>=1024**3?(bytes/1024**3).toFixed(1)+' GB':bytes>=1024**2?(bytes/1024**2).toFixed(1)+' MB':bytes>0?Math.round(bytes/1024)+' KB':'크기 확인 중'
const shortDate=(iso?:string)=>{
 if(!iso)return ''
 const d=new Date(iso);return Number.isNaN(d.valueOf())?'':d.toLocaleDateString('ko-KR',{month:'short',day:'numeric',hour:'2-digit',minute:'2-digit'})
}
const panelTitle='text-[15px] font-bold tracking-tight text-[#302A24]'
const soft='rounded-2xl border border-[#E7E0D5] bg-[#FBF8F3]'
const focus='focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[#C96442] focus-visible:ring-offset-2'
const tabStyle='inline-flex min-h-10 items-center gap-2 rounded-xl px-3 py-2 text-[13px] font-semibold transition-colors'

export function VideoPage({navigate}:{navigate:(page:any)=>void}){
 useLocale()
 const video=useRemote<VideoStatus>('/api/video/status',{config:{engine:'native',comfyUrl:''},native:{available:false},comfy:{reachable:false},workflows:[],jobs:[]},false)
 const jobs=useRemote<VideoJob[]>('/api/video/jobs',[],true)
 const outputs=useRemote<VideoJob[]>('/api/video/outputs',[],true)
 const catalog=useRemote<{videoModels:VideoModel[]}>('/api/video/catalog',{videoModels:[]})
 const downloads=useRemote<{downloads:{id:string;model:string;status:string;progress:number;bytes:number;total:number;filesTotal?:number;filesCompleted?:number;currentFile?:string|null;error?:string}[]}>('/api/video/models/downloads',{downloads:[]})
 const youtube=useRemote<YoutubeStatus>('/api/youtube/status',{connected:false,uploadAllowed:false,account:null,uploads:[]})
 const oauth=useRemote<{id:string;configured:boolean;callback?:string}[]>('/api/oauth/providers',[])
 const providers=useRemote<Provider[]>('/api/providers',[])
 const [draft,setDraft]=useState<Draft>(loadDraft)
 const [tab,setTab]=useState<StudioTab>('create')
 const [engineChoice,setEngineChoice]=useState<VideoEngine|null>(null)
 const [comfyUrl,setComfyUrl]=useState('')
 const [modelDir,setModelDir]=useState('')
 const [ffmpegPath,setFfmpegPath]=useState('')
 const [voice,setVoice]=useState('Yuna')
 const [narration,setNarration]=useState(true)
 const [reuseId,setReuseId]=useState('')
 const [regenerate,setRegenerate]=useState<number[]>([])
 const [budgetEnabled,setBudgetEnabled]=useState(false)
 const [maxNewScenes,setMaxNewScenes]=useState(1)
 const [advanced,setAdvanced]=useState(false)
 const [autoPlan,setAutoPlan]=useState(true)
 const [currentId,setCurrentId]=useState('')
 const [submitted,setSubmitted]=useState<VideoJob|null>(null)
 const [uploadFile,setUploadFile]=useState('')
 const [uploadTitle,setUploadTitle]=useState('')
 const [uploadDescription,setUploadDescription]=useState('')
 const [uploadPrivacy,setUploadPrivacy]=useState('private')
 const [uploadTags,setUploadTags]=useState('')
 const [uploadId,setUploadId]=useState('')
 const [uploadSubmission,setUploadSubmission]=useState<UploadJob|null>(null)
 const [gid,setGid]=useState('')
 const [gsecret,setGsecret]=useState('')
 const [gBusy,setGBusy]=useState(false)
 const [savingConfig,setSavingConfig]=useState(false)
 const [preparingModel,setPreparingModel]=useState('')
 const [generating,setGenerating]=useState(false)
 const [uploading,setUploading]=useState(false)
 const [refreshing,setRefreshing]=useState(false)
 const [error,setError]=useState('')
 const engine:VideoEngine=engineChoice??(video.value.config.engine==='comfyui'?'comfyui':'native')
 const usingComfy=engine==='comfyui'
 const active=useJob<VideoJob>(currentId?'/api/video/jobs/'+encodeURIComponent(currentId):null,submitted?.id===currentId?submitted:null)
 const activeUpload=useJob<UploadJob>(uploadId?'/api/youtube/uploads/'+encodeURIComponent(uploadId):null,uploadSubmission)
 const sortedJobs=useMemo(()=>[...jobs.value].filter(j=>j.kind==='video').sort((a,b)=>String(b.createdAt||'').localeCompare(String(a.createdAt||''))),[jobs.value])
 const videoJob=active.value??sortedJobs.find(j=>j.id===currentId)??sortedJobs[0]??null
 const completed=useMemo(()=>sortedJobs.filter(j=>j.status==='completed'&&Boolean(j.outputUrl)),[sortedJobs])
 const selectedOutput=completed.find(j=>j.id===uploadFile)??outputs.value.find(j=>j.id===uploadFile)??completed[0]??outputs.value[0]??null
 const uploadJob=activeUpload.value
 const statusConfig=video.value.config
 const installed=catalog.value.videoModels.find(model=>model.id===statusConfig.videoModel)
 const downloadState=downloads.value.downloads.find(row=>row.model===statusConfig.videoModel)
 const savedWorkflow=video.value.workflows[0]
 const google=oauth.value.find(provider=>provider.id==='youtube')
 const sceneItems=draft.scenes.map(s=>s.trim()).filter(Boolean)
 const actualScenes=draft.mode==='scenes'?sceneItems:[draft.prompt.trim()].filter(Boolean)
 const maxDuration=usingComfy&&draft.mode==='quick'?9:180
 const invalidTime=usingComfy&&draft.mode==='scenes'&&sceneItems.length>1&&draft.seconds<sceneItems.length*3
 const invalidScenes=draft.mode==='scenes'&&(!sceneItems.length||sceneItems.length>8||draft.scenes.some(s=>!s.trim())||draft.scenes.some(s=>new TextEncoder().encode(s).length>4000))
 const invalidSeed=usingComfy&&(!draft.seed.trim()||!Number.isSafeInteger(Number(draft.seed))||Number(draft.seed)<-1)
 const validDuration=Number.isInteger(draft.seconds)&&draft.seconds>=3&&draft.seconds<=maxDuration
 const validFps=Number.isInteger(draft.fps)&&draft.fps>=1&&draft.fps<=60
 const validBrief=new TextEncoder().encode(actualScenes.join('\n\n')).length<=16000
 const planner=providers.value.find(provider=>provider.enabled!==false&&Boolean(provider.model?.trim()))
 const planningEnabled=usingComfy&&draft.mode==='quick'&&autoPlan&&Boolean(planner)&&draft.seconds>=6&&(!budgetEnabled||maxNewScenes>=2)
 const multiSceneRequiresFfmpeg=usingComfy&&((draft.mode==='scenes'&&sceneItems.length>1)||planningEnabled)&&!video.value.native?.available
 const ready=usingComfy?Boolean(video.value.comfy.available):Boolean(video.value.native?.available)
 const prerequisite=usingComfy
  ? !statusConfig.comfyUrl?.trim()?'먼저 ComfyUI 서버 주소를 등록해 주세요.'
   : !video.value.comfy.reachable?'ComfyUI 서버에 연결되지 않았습니다. 주소와 실행 상태를 확인해 주세요.'
   : video.value.comfy.missingNodes?.length?'ComfyUI에 GGUF 확장 노드가 설치되지 않았습니다. 영상 모델 카드 아래의 설치 안내를 확인해 주세요.'
   : !video.value.comfy.available?'영상 모델을 설치하거나 API 워크플로를 등록해 주세요.':''
  : !video.value.native?.available?'네이티브 영상 제작에는 FFmpeg 설치가 필요합니다.':''
 const submittedState=sortedJobs.find(j=>j.id===submitted?.id)??(active.value?.id===submitted?.id?active.value:submitted)
 const busy=generating||Boolean(submittedState&&running(submittedState.status))
 const canGenerate=ready&&!busy&&actualScenes.length>0&&validBrief&&!invalidScenes&&!invalidTime&&!invalidSeed&&!multiSceneRequiresFfmpeg&&validDuration&&(!usingComfy||validFps)
 const suggestedMaxNew=actualScenes.length
 const progress=typeof videoJob?.progress==='number'?Math.max(0,Math.min(100,Math.round(videoJob.progress))):null
 const editingSource=sortedJobs.find(j=>j.id===reuseId)
 const ratioWidth=ratios[draft.ratio].w
 const ratioHeight=ratios[draft.ratio].h

 useEffect(()=>{try{localStorage.setItem(draftKey,JSON.stringify(draft))}catch{}},[draft])
 useEffect(()=>{setComfyUrl(statusConfig.comfyUrl||'')},[statusConfig.comfyUrl])
 useEffect(()=>{setModelDir(statusConfig.comfyModelsDir||'')},[statusConfig.comfyModelsDir])
 useEffect(()=>{setFfmpegPath(statusConfig.ffmpegPath||'')},[statusConfig.ffmpegPath])
 useEffect(()=>{setVoice(statusConfig.voice||'Yuna');setNarration(statusConfig.narration!==false)},[statusConfig.voice,statusConfig.narration])
 useEffect(()=>{if(currentId||!sortedJobs.length)return;setCurrentId(sortedJobs[0].id)},[currentId,sortedJobs])
 useEffect(()=>{if(active.value?.status==='completed'){void outputs.reload();void jobs.reload()}},[active.value?.status,outputs.reload,jobs.reload])
 useEffect(()=>{
  if(!sortedJobs.some(j=>running(j.status)))return
  const timer=window.setInterval(()=>void jobs.reload(),4000)
  return()=>window.clearInterval(timer)
 },[sortedJobs,jobs.reload])
 useEffect(()=>{if(downloadState?.status==='completed'){void catalog.reload();void video.reload()}},[downloadState?.status,catalog.reload,video.reload])
 // Keep real model availability fresh while a download is pending. The UI
 // re-reads both install receipts and the disk-backed catalog.
 useEffect(()=>{
  if(!downloads.value.downloads.some(row=>row.status==='downloading'))return
  const timer=window.setInterval(()=>void downloads.reload(),3000)
  return()=>window.clearInterval(timer)
 },[downloads.value.downloads,downloads.reload])

 const patchDraft=(patch:Partial<Draft>)=>setDraft(previous=>({...previous,...patch}))
 const reload=async()=>{
  setRefreshing(true);setError('')
  try{await Promise.allSettled([video.reload(),jobs.reload(),outputs.reload(),catalog.reload(),downloads.reload(),youtube.reload(),oauth.reload(),providers.reload()])}
  finally{setRefreshing(false)}
 }
 const saveConfig=async(patch:Partial<VideoConfig>)=>{
  if(savingConfig)return
  setSavingConfig(true);setError('')
  try{await post('/api/video/config',patch);await Promise.all([video.reload(),catalog.reload()])}
  catch(e){setError((e as Error).message)}
  finally{setSavingConfig(false)}
 }
 const changeEngine=async(next:VideoEngine)=>{
  setEngineChoice(next);await saveConfig({engine:next})
 }
 const importWorkflow=async(file:File)=>{
  if(savingConfig)return
  if(file.size>1024*1024){setError('워크플로는 1 MB 이하의 API 형식 JSON이어야 합니다.');return}
  try{
   const text=await file.text()
   const parsed=JSON.parse(text) as Record<string,unknown>
   if(!parsed||typeof parsed!=='object'||Array.isArray(parsed)||!Object.keys(parsed).length||!Object.values(parsed).every(node=>
    Boolean(node&&typeof node==='object'&&!Array.isArray(node)&&'class_type' in node&&'inputs' in node)
   ))throw Error('ComfyUI API 형식 JSON이 필요합니다. 일반 UI 워크플로 JSON은 그대로 실행할 수 없습니다.')
   await saveConfig({engine:'comfyui',comfyWorkflow:parsed,comfyWorkflowName:file.name})
  }catch(e){setError((e as Error).message)}
 }
 // The first-time setup is part of video creation: choose a model,
 // freeze a built-in workflow, and install its weights without leaving the studio.
 const prepareVideoModel=async(model:VideoModel)=>{
  if(savingConfig||preparingModel)return
  setPreparingModel(model.id);setError('')
  try{
   const nextUrl=statusConfig.comfyUrl?.trim()||'http://127.0.0.1:8188'
   await post('/api/video/config',{
    engine:'comfyui',videoModel:model.id,
    comfyUrl:nextUrl,comfyWorkflow:null,comfyWorkflowName:''
   })
   setEngineChoice('comfyui')
   setComfyUrl(nextUrl)
   await Promise.all([video.reload(),catalog.reload()])
   if(!model.available){
    const record=downloads.value.downloads.find(item=>item.model===model.id)
    if(record?.status!=='downloading'){
     await post('/api/video/models/'+encodeURIComponent(model.id)+'/download',{})
    }
    await downloads.reload()
   }
  }catch(e){setError((e as Error).message)}
  finally{setPreparingModel('')}
 }
 const reveal=async(id:string)=>{
  try{await post('/api/video/jobs/'+encodeURIComponent(id)+'/reveal',{})}catch(e){setError((e as Error).message)}
 }
 const mutateScene=(index:number,text:string)=>patchDraft({scenes:draft.scenes.map((scene,i)=>i===index?text:scene)})
 const swapScene=(index:number,other:number)=>{
  const next=[...draft.scenes]
  ;[next[index],next[other]]=[next[other],next[index]]
  patchDraft({scenes:next});setRegenerate([])
 }
 const addScene=()=>{
  if(draft.scenes.length>=8)return
  patchDraft({scenes:[...draft.scenes,''],seconds:Math.max(draft.seconds,(draft.scenes.length+1)*3)})
  setMaxNewScenes(v=>Math.max(v,draft.scenes.length+1))
 }
 const generate=async()=>{
  if(!canGenerate)return
  if(budgetEnabled&&(!Number.isInteger(maxNewScenes)||maxNewScenes<0||maxNewScenes>actualScenes.length)){
   setError('새로 생성할 장면 수는 0부터 전체 장면 수까지 입력해 주세요.');return
  }
  if(budgetEnabled&&maxNewScenes===0&&(!reuseId||actualScenes.length===1)){
   setError('렌더 0회는 2장면 이상의 이전 영상에서 검증된 장면만 재사용할 때 적용할 수 있습니다.');return
  }
  setGenerating(true);setError('')
  try{
   const newJob=await post<VideoJob>('/api/video/jobs',{
    engine,title:draft.title.trim(),brief:actualScenes.join('\n\n'),
    aspectRatio:draft.ratio,duration:draft.seconds,
    ...(usingComfy?{
     // Quick mode remains ONE scene even when the idea contains blank lines.
     // Only the storyboard mode opts into multiple remote GPU renders.
     scenes:actualScenes,
     negativePrompt:draft.negative,fps:draft.fps,seed:Number(draft.seed),
     continuity:draft.continuity,
     autoStoryboard:planningEnabled&&!reuseId,
     ...(planningEnabled?{planningProviderId:planner?.id}:{}),
     ...(reuseId?{reuseVideoJobId:reuseId,regenerateScenes:regenerate}:{}),
     ...(budgetEnabled?{maxNewScenes}: {})
    }:{})
   })
   setSubmitted(newJob);setCurrentId(newJob.id);setTab('create')
   void jobs.reload()
  }catch(e){setError((e as Error).message)}finally{setGenerating(false)}
 }
 const editJob=(job:VideoJob)=>{
  const scenes=job.storyboard?.length?job.storyboard:
   (job.brief||'').split('\n\n').map(s=>s.trim()).filter(Boolean)
  patchDraft({
   title:job.title||'',prompt:job.brief||'',scenes:scenes.length?scenes:[''],
   mode:job.engine==='comfyui'?'scenes':'quick',
   ratio:job.aspectRatio&&job.aspectRatio in ratios?job.aspectRatio:'9:16',
   seconds:job.duration??6,fps:job.renderParams?.fps??16,
   seed:'-1',negative:job.negativePrompt||'',continuity:job.continuity||''
  })
  if(job.engine==='comfyui'){
   setEngineChoice('comfyui')
   void saveConfig({engine:'comfyui'})
   setReuseId(job.id);setBudgetEnabled(false);setRegenerate([])
  }else{setReuseId('');setEngineChoice('native');void saveConfig({engine:'native'})}
  setCurrentId(job.id);setTab('create')
 }
 const chooseForUpload=(job:VideoJob)=>{
  if(!job.outputUrl)return
  setUploadFile(job.id)
  setUploadTitle((job.title||job.brief||'').slice(0,100))
  setUploadDescription(job.brief||'')
  setTab('publish')
 }
 const connectYoutube=async()=>{setError('');try{await openOAuth('youtube')}catch(e){setError((e as Error).message)}}
 const saveGoogle=async()=>{
  if(!gid.trim()||!gsecret.trim())return
  setGBusy(true);setError('')
  try{
   await post('/api/oauth/providers/youtube',{clientId:gid.trim(),clientSecret:gsecret.trim()})
   setGsecret('');await oauth.reload()
  }catch(e){setError((e as Error).message)}finally{setGBusy(false)}
 }
 const startUpload=async()=>{
  if(!selectedOutput?.outputUrl||!uploadTitle.trim()||uploading)return
  if(!youtube.value.uploadAllowed){setError('YouTube 계정을 먼저 연결해 주세요.');return}
  setUploading(true);setError('')
  try{
   const job=await post<UploadJob>('/api/video/jobs/'+encodeURIComponent(selectedOutput.id)+'/upload',{
    title:uploadTitle.trim(),description:uploadDescription,privacyStatus:uploadPrivacy,tags:uploadTags
   })
   setUploadSubmission(job);setUploadId(job.id)
  }catch(e){setError((e as Error).message)}finally{setUploading(false)}
 }

 const errorMessage=error||video.error||jobs.error||outputs.error||active.error||activeUpload.error
 const sections:[StudioTab,string,typeof Clapperboard][]=[
  ['create','영상 만들기',WandSparkles],['library','프로젝트',Layers3],
  ['publish','게시하기',Upload],['settings','환경 설정',Settings2]
 ]
 const previewJob=videoJob
 const previewWidth=previewJob?.aspectRatio&&previewJob.aspectRatio in ratios?ratios[previewJob.aspectRatio].w:ratioWidth
 const previewHeight=previewJob?.aspectRatio&&previewJob.aspectRatio in ratios?ratios[previewJob.aspectRatio].h:ratioHeight
 return <div className="mx-auto w-full max-w-[1536px] min-w-0 overflow-x-hidden px-4 pb-16 pt-5 sm:px-6 lg:px-8">
  <Headline eyebrow="DOMINION · VIDEO STUDIO" title="영상 스튜디오" description="아이디어부터 장면 구성, 생성, 수정, 게시까지 한곳에서 작업합니다."
   actions={<Button variant="secondary" disabled={refreshing} onClick={()=>void reload()}><RefreshCw size={15} className={refreshing?'animate-spin':''}/> 새로고침</Button>}/>
  {errorMessage&&<Notice message={errorMessage}/>}
  <section className="relative mb-5 overflow-hidden rounded-[24px] bg-[#202A38] p-5 text-white shadow-[0_14px_44px_rgba(23,34,52,.14)] sm:p-7">
   <div className="pointer-events-none absolute -right-16 -top-32 h-72 w-72 rounded-full bg-[#EAA178]/15 blur-3xl"/>
   <div className="relative flex flex-col justify-between gap-5 lg:flex-row lg:items-center">
    <div className="max-w-xl">
     <div className="mb-2 inline-flex items-center gap-2 rounded-full border border-white/20 bg-white/10 px-3 py-1.5 text-[11px] font-semibold text-[#FFE4CD]"><Sparkles size={13}/> 아이디어를 영상으로</div>
     <h2 className="text-[23px] font-bold leading-tight tracking-tight sm:text-[27px]">만들고, 확인하고, 필요한 장면만 다시 만드세요.</h2>
     <p className="mt-2 text-[12px] leading-6 text-[#CDD5DF]">간단한 자막 영상부터 여러 장면으로 구성된 AI 영상까지. 완성된 결과는 프로젝트에서 다시 열 수 있습니다.</p>
    </div>
    <div className="grid grid-cols-2 gap-2 sm:min-w-[290px]">
     <div className="rounded-xl border border-white/15 bg-white/10 px-4 py-3">
      <span className="text-[11px] text-[#CAD5DF]">제작 환경</span>
      <div className="mt-1 flex items-center gap-1.5 text-[13px] font-semibold">{ready?<CheckCircle2 size={15} className="text-[#A7DFB2]"/>:<Clock3 size={15} className="text-[#F7CC8F]"/>}{ready?'생성 준비 완료':'설정 필요'}</div>
     </div>
     <div className="rounded-xl border border-white/15 bg-white/10 px-4 py-3">
      <span className="text-[11px] text-[#CAD5DF]">완료한 영상</span>
      <div className="mt-1 text-[19px] font-bold tabular-nums">{completed.length}<span className="ml-1 text-[11px] font-medium text-[#CAD5DF]">개</span></div>
     </div>
    </div>
   </div>
  </section>
  <nav role="tablist" aria-label="영상 작업 메뉴" className="mb-5 flex flex-wrap gap-1 rounded-2xl border border-[#E7E0D5] bg-[#F5F0E8] p-1.5">
   {sections.map(([name,label,Icon])=><button key={name} type="button" role="tab" aria-selected={tab===name}
    onClick={()=>setTab(name)}
    className={tabStyle+' '+focus+' '+(tab===name?'bg-white text-[#A44C2F] shadow-sm':'text-[#6F675D] hover:bg-white/60')}>
    <Icon size={15}/>{label}{name==='library'&&sortedJobs.length>0&&<span className="rounded-full bg-[#F2E7DA] px-1.5 py-0.5 text-[10px] text-[#8E5942]">{sortedJobs.length}</span>}
   </button>)}
  </nav>

  {tab==='create'&&<>
   <div className="mb-4 grid gap-2 sm:grid-cols-3">
    {[
     ['01','아이디어 / 스토리보드',true],
     ['02','영상 규격 / 비용 관리',ready],
     ['03','미리보기 / 수정',Boolean(previewJob)]
    ].map(([number,label,active])=><div key={String(number)} className={'flex items-center gap-3 rounded-xl border p-3 '+(active?'border-[#E5C0AD] bg-[#FFF6EF]':'border-[#E7E0D5] bg-white')}>
     <span className={'flex size-8 shrink-0 items-center justify-center rounded-lg text-[11px] font-bold '+(active?'bg-[#C96442] text-white':'bg-[#F3EEE5] text-[#8E8174]')}>{number}</span>
     <span className="text-[12px] font-semibold text-[#4A4036]">{label}</span>
    </div>)}
   </div>
   <Panel className="mb-4 border-[#E2C4AD] bg-[#FFFDF9] p-4 sm:p-5">
    <div className="flex flex-wrap items-start justify-between gap-3">
     <div><span className="inline-flex items-center gap-1.5 rounded-full bg-[#F6E7D9] px-2.5 py-1 text-[10px] font-bold text-[#955231]"><Sparkles size={12}/> 턴키 AI 영상</span>
      <h3 className="mt-2 text-[15px] font-bold text-[#342A24]">영상 모델 선택 · 다운로드</h3>
      <p className="mt-1 text-[12px] text-[#796F63]">이곳에서 모델을 준비한 뒤 주제만 입력하세요. 별도 메뉴로 이동할 필요 없습니다.</p>
     </div>
     <StateTag online={Boolean(usingComfy&&ready)}>{usingComfy&&ready?'AI 영상 제작 준비 완료':'모델·렌더러 준비'}</StateTag>
    </div>
    <div className="mt-4 grid gap-3 md:grid-cols-2">
     {catalog.value.videoModels.length===0?<p className="text-[12px] text-[#807568]">영상 모델 목록을 불러오는 중입니다. <button className="underline" type="button" onClick={()=>void catalog.reload()}>다시 불러오기</button></p>:
      catalog.value.videoModels.map(model=>{
       const record=downloads.value.downloads.find(item=>item.model===model.id)
       const isDownloading=record?.status==='downloading'
       const selected=statusConfig.videoModel===model.id&&usingComfy
       const installedNow=model.available||record?.status==='completed'
       return <div key={model.id} className={'rounded-2xl border p-4 '+(selected?'border-[#CB8B67] bg-[#FFF5EC]':'border-[#E6D9CB] bg-white')}>
        <div className="flex flex-wrap items-center justify-between gap-2">
         <strong className="text-[13px] text-[#3A3028]">{model.name}</strong>
         <span className={'rounded-full px-2 py-1 text-[10px] font-bold '+(model.available?'bg-[#E7F2E5] text-[#467448]':'bg-[#FAEBD9] text-[#8D5837]')}>{model.available?'설치 완료':isDownloading?'다운로드 중':'설치 필요'}</span>
        </div>
        <p className="mt-2 text-[11px] leading-5 text-[#7D7064]">{model.kind==='gguf'?'경량 4비트 · ComfyUI-GGUF 확장 노드가 필요합니다.':'원본 fp16 · 더 많은 RAM/VRAM이 필요합니다.'} {model.hint}</p>
        <Button variant={selected&&model.available?'secondary':'primary'} size="sm" className="mt-3 w-full"
         disabled={Boolean(preparingModel)||savingConfig||isDownloading||Boolean(selected&&model.available)}
         onClick={()=>void prepareVideoModel(model)}>
         {preparingModel===model.id?<><LoaderCircle size={14} className="animate-spin"/> 준비 중</>:
          isDownloading?<><LoaderCircle size={14} className="animate-spin"/> 모델 다운로드 진행 중</>:
          selected&&model.available?<><Check size={14}/> 이 모델 사용 중</>:
          model.available?<><CheckCircle2 size={14}/> 설치된 모델 선택</>:
          <><Download size={14}/> 모델 선택하고 다운로드</>}
        </Button>
        {isDownloading&&<div className="mt-3 grid gap-1.5" role="status" aria-live="polite">
         <div className="flex justify-between gap-2 text-[11px] text-[#695D50]"><span>파일 {record.filesCompleted??0}/{record.filesTotal??'?'} · {fmtBytes(record.bytes)}</span><span className="tabular-nums">{record.progress??0}%</span></div>
         <div role="progressbar" aria-label={model.name+' 다운로드'} aria-valuemin={0} aria-valuemax={100} aria-valuenow={record.progress??0} className="h-2 overflow-hidden rounded-full bg-[#F2E5D8]"><div className="h-full rounded-full bg-[#C96442] transition-all" style={{width:Math.max(0,Math.min(100,record.progress??0))+'%'}}/></div>
         {record.currentFile&&<span className="truncate text-[10px] text-[#907F71]" title={record.currentFile}>{record.currentFile}</span>}
        </div>}
        {record?.status==='failed'&&<p role="alert" className="mt-2 text-[11px] text-rose-700">{record.error||'모델 다운로드에 실패했습니다. 다시 시도할 수 있습니다.'}</p>}
       </div>
      })}
    </div>
    <div className="mt-3 flex flex-wrap items-end gap-3 rounded-xl bg-[#F7F2EB] p-3">
     <div className="min-w-[205px] flex-1"><label className="mb-1.5 block text-[11px] font-semibold text-[#5D5145]" htmlFor="studio-comfy-url">AI 렌더링 서버 · ComfyUI</label><TextInput id="studio-comfy-url" value={comfyUrl} onChange={e=>setComfyUrl(e.target.value)} placeholder="http://127.0.0.1:8188"/></div>
     <Button variant="secondary" disabled={savingConfig||!comfyUrl.trim()} onClick={()=>void saveConfig({comfyUrl:comfyUrl.trim()})}><RefreshCw size={14}/> 연결 확인</Button>
     <span className={'text-[11px] font-semibold '+(video.value.comfy.reachable?'text-[#42774D]':'text-[#906349]')}>{video.value.comfy.reachable?'● 연결됨':'○ 서버 연결 필요'}</span>
    </div>
    {!video.value.comfy.reachable&&<p className="mt-2 text-[11px] leading-5 text-[#836D5B]">모델 파일 다운로드는 바로 시작할 수 있습니다. AI 렌더링에는 실행 중인 ComfyUI 서버도 필요합니다. 기본 연결 주소는 127.0.0.1:8188입니다.</p>}
    {usingComfy&&video.value.comfy.reachable&&Boolean(video.value.comfy.missingNodes?.length)&&<div role="alert" className="mt-3 flex flex-wrap items-center justify-between gap-3 rounded-xl border border-amber-200 bg-amber-50 p-3 text-[12px] text-amber-900">
     <span>GGUF 영상 생성에 필요한 ComfyUI 확장 노드가 없습니다: {video.value.comfy.missingNodes?.join(', ')}. 모델 파일만으로는 렌더링할 수 없습니다.</span>
     <a href="https://github.com/city96/ComfyUI-GGUF" target="_blank" rel="noreferrer" className="font-bold underline">GGUF 확장 설치 안내 <ExternalLink size={12} className="inline"/></a>
    </div>}
    <p className="mt-1 text-[11px] leading-5 text-[#887769]">모델 파일은 DOMINION을 실행 중인 컴퓨터에 저장됩니다. 원격 ComfyUI를 사용하면 서버의 모델 경로와 일치시켜 주세요.</p>
   </Panel>
   <div className="grid items-start gap-5 xl:grid-cols-[minmax(0,1.45fr)_minmax(320px,0.8fr)]">
    <div className="grid min-w-0 content-start gap-4">
     <Panel className="p-4 sm:p-6">
      <div className="mb-4 flex flex-wrap items-center justify-between gap-3">
       <div><h3 className={panelTitle}>1. 어떤 영상을 만드시겠어요?</h3><p className="mt-1 text-[12px] text-[#82796E]">원하는 수준에 맞춰 제작 방식을 선택하세요.</p></div>
       <span className="rounded-full bg-[#F1ECE4] px-3 py-1 text-[11px] text-[#6B6257]">입력 내용은 이 기기에 임시 저장</span>
      </div>
      <div className="mb-5 grid gap-3 sm:grid-cols-2">
       <button type="button" onClick={()=>patchDraft({mode:'quick'})} aria-pressed={draft.mode==='quick'}
        className={'rounded-2xl border p-4 text-left transition-all '+focus+' '+(draft.mode==='quick'?'border-[#C96442] bg-[#FFF4EC] ring-1 ring-[#C96442]/30':'border-[#E2DACF] bg-white hover:border-[#D7BEAD]')}>
        <span className="inline-flex items-center gap-2 text-[13px] font-bold text-[#393029]"><Sparkles size={18} className="text-[#C96442]"/> 간편 제작</span>
        <p className="mt-1 text-[11px] leading-5 text-[#756B60]">주제 한 번 입력하고 바로 생성. 빠른 샘플과 자막 영상에 추천합니다.</p>
       </button>
       <button type="button" onClick={()=>patchDraft({mode:'scenes',scenes:draft.scenes.some(s=>s.trim())?draft.scenes:[draft.prompt]})} aria-pressed={draft.mode==='scenes'}
        className={'rounded-2xl border p-4 text-left transition-all '+focus+' '+(draft.mode==='scenes'?'border-[#C96442] bg-[#FFF4EC] ring-1 ring-[#C96442]/30':'border-[#E2DACF] bg-white hover:border-[#D7BEAD]')}>
        <span className="inline-flex items-center gap-2 text-[13px] font-bold text-[#393029]"><Layers3 size={18} className="text-[#C96442]"/> 스토리보드 제작</span>
        <p className="mt-1 text-[11px] leading-5 text-[#756B60]">장면을 나누고 수정한 뒤 필요한 부분만 다시 만들 수 있습니다.</p>
       </button>
      </div>
      <div className="grid gap-4">
       <FormField label="프로젝트 제목" hint="영상 목록과 게시 정보에 사용합니다."><TextInput value={draft.title} maxLength={90} onChange={e=>patchDraft({title:e.target.value})} placeholder="예: 서울의 새벽, 15초 여행 쇼츠"/></FormField>
       {draft.mode==='quick'?<FormField label="영상 아이디어 / 대본" hint="핵심 장면, 분위기, 움직임을 구체적으로 적으면 결과를 비교하기 쉽습니다.">
        <TextArea value={draft.prompt} onChange={e=>patchDraft({prompt:e.target.value})} rows={5} placeholder="예: 따뜻한 아침 햇살, 푸른 바다 위로 카메라가 천천히 이동하는 장면. 부드러운 시네마틱 분위기."/>
       </FormField>:<>
        <div className="flex flex-wrap items-center justify-between gap-2">
         <div><h4 className="text-[13px] font-bold text-[#363029]">장면별 스토리보드</h4><p className="text-[11px] text-[#807466]">최대 8장면 · 장면당 최소 3초 · 위에서 아래 순서대로 재생</p></div>
         <Button variant="secondary" size="sm" disabled={draft.scenes.length>=8} onClick={addScene}><Plus size={14}/> 장면 추가</Button>
        </div>
        <div className="grid gap-2">
         {draft.scenes.map((scene,index)=><div key={index} className={'rounded-xl border bg-[#FEFDFB] p-3 '+(regenerate.includes(index)?'border-[#D6A27C]':'border-[#E7DED3]')}>
          <div className="mb-2 flex items-center justify-between gap-2">
           <div className="flex items-center gap-2"><span className="rounded-md bg-[#F4E5D8] px-2 py-1 text-[11px] font-bold text-[#975335]">SCENE {index+1}</span>{reuseId&&usingComfy&&<label className="flex items-center gap-1.5 text-[11px] text-[#6B6257]"><input type="checkbox" checked={regenerate.includes(index)} onChange={e=>setRegenerate(prev=>e.target.checked?[...prev,index]:prev.filter(i=>i!==index))}/> 새로 렌더</label>}</div>
           <div className="flex items-center gap-1">
            <Button variant="ghost" size="icon" aria-label={'장면 '+(index+1)+' 위로'} disabled={index===0} onClick={()=>swapScene(index,index-1)}><ArrowUp size={14}/></Button>
            <Button variant="ghost" size="icon" aria-label={'장면 '+(index+1)+' 아래로'} disabled={index===draft.scenes.length-1} onClick={()=>swapScene(index,index+1)}><ArrowDown size={14}/></Button>
            <Button variant="ghost" size="icon" aria-label={'장면 '+(index+1)+' 삭제'} disabled={draft.scenes.length<=1} onClick={()=>{patchDraft({scenes:draft.scenes.filter((_,i)=>i!==index)});setRegenerate([])}}><Trash2 size={14}/></Button>
           </div>
          </div>
          <TextArea aria-label={'장면 '+(index+1)+' 설명'} value={scene} onChange={e=>mutateScene(index,e.target.value)} rows={2} placeholder={'장면 '+(index+1)+'에서 보여줄 모습과 카메라 움직임을 적으세요.'}/>
          <div className="mt-1 text-right text-[10px] tabular-nums text-[#8E8174]">{new TextEncoder().encode(scene).length.toLocaleString('ko-KR')} / 4,000 byte</div>
         </div>)}
        </div>
        {usingComfy&&<FormField label="일관된 캐릭터 / 영상 스타일" hint="모든 장면에 같은 인물·의상·색감 등을 유지하도록 전달합니다."><TextArea rows={2} maxLength={2000} value={draft.continuity} onChange={e=>patchDraft({continuity:e.target.value})} placeholder="예: 같은 주인공, 남색 재킷, 따뜻한 필름 색감, 자연스러운 움직임"/></FormField>}
       </>}
       {draft.mode==='quick'&&usingComfy&&<div className="rounded-xl border border-[#E4D4C7] bg-[#FAF6F0] p-3">
        <label className="flex items-center gap-2 text-[12px] font-bold text-[#47392F]"><input type="checkbox" checked={autoPlan} onChange={e=>setAutoPlan(e.target.checked)}/> AI가 자동으로 대본·장면 구성</label>
        <p className="mt-1 text-[11px] leading-5 text-[#786B5D]">연결된 텍스트 AI가 장면을 기획하고, 영상 엔진이 장면별 생성·합성을 진행합니다. 대본 기획에 모델 토큰이 사용될 수 있습니다.</p>
        {autoPlan&&<p className="mt-1 text-[11px] font-semibold text-[#9A6544]">{!planner?'사용 가능한 텍스트 AI 제공자가 없어 현재는 단일 장면으로 생성됩니다.':draft.seconds<6?'6초 이상부터 장면 자동 기획을 사용합니다.':budgetEnabled&&maxNewScenes<2?'새 렌더 예산 2장면 이상일 때 자동 기획을 사용합니다.':!video.value.native?.available?'장면 합성을 위해 FFmpeg 설치가 필요합니다.':'자동 기획 준비됨 · '+(planner.name||planner.id)}</p>}
       </div>}
       <div className="flex flex-wrap gap-2">
        <Button variant="ghost" size="sm" onClick={()=>{if(window.confirm('영상 아이디어와 장면 내용을 초기화하시겠습니까?')){setDraft({...defaults});setReuseId('');setRegenerate([])}}}><RotateCcw size={14}/> 새 프로젝트</Button>
        {reuseId&&<span className="inline-flex items-center gap-1.5 rounded-xl bg-[#EAF3E8] px-3 py-2 text-[11px] font-medium text-[#466647]"><CheckCircle2 size={13}/> 이전 프로젝트를 기반으로 수정 중 <button type="button" className="ml-1 underline" onClick={()=>{setReuseId('');setRegenerate([])}}>연결 해제</button></span>}
       </div>
      </div>
     </Panel>

     <Panel className="p-4 sm:p-6">
      <div className="mb-4 flex flex-wrap items-center justify-between gap-2">
       <h3 className={panelTitle}>2. 영상 설정</h3>
       <Button variant="ghost" size="sm" onClick={()=>setTab('settings')}><Settings2 size={14}/> 환경 설정</Button>
      </div>
      <div className="mb-4 grid gap-2 sm:grid-cols-2">
       <button type="button" aria-pressed={engine==='native'} onClick={()=>void changeEngine('native')}
        className={'flex items-start gap-3 rounded-xl border p-3 text-left '+focus+' '+(engine==='native'?'border-[#C96442] bg-[#FFF4EC]':'border-[#E5DDD1] hover:border-[#D7BEAD]')}>
        <Film size={20} className="mt-0.5 shrink-0 text-[#C96442]"/><span><strong className="block text-[12px] text-[#382F29]">자막 · 카드 영상</strong><small className="mt-1 block text-[11px] text-[#7C7165]">FFmpeg로 로컬 합성 · AI 렌더 불필요</small></span>
       </button>
       <button type="button" aria-pressed={engine==='comfyui'} onClick={()=>void changeEngine('comfyui')}
        className={'flex items-start gap-3 rounded-xl border p-3 text-left '+focus+' '+(engine==='comfyui'?'border-[#C96442] bg-[#FFF4EC]':'border-[#E5DDD1] hover:border-[#D7BEAD]')}>
        <VideoIcon size={20} className="mt-0.5 shrink-0 text-[#C96442]"/><span><strong className="block text-[12px] text-[#382F29]">AI 장면 생성</strong><small className="mt-1 block text-[11px] text-[#7C7165]">ComfyUI · 로컬/연결 서버 GPU 렌더</small></span>
       </button>
      </div>
      <fieldset className="mb-4">
       <legend className="mb-2 text-[12px] font-semibold text-[#4A4036]">화면 비율</legend>
       <div className="grid grid-cols-3 gap-2">
        {(Object.keys(ratios) as Ratio[]).map(key=><button key={key} type="button" aria-pressed={draft.ratio===key} onClick={()=>patchDraft({ratio:key})}
         className={'flex min-h-20 flex-col items-center justify-center gap-2 rounded-xl border p-2 text-center '+focus+' '+(draft.ratio===key?'border-[#C96442] bg-[#FFF5EC] text-[#A54F33]':'border-[#E5DDD1] bg-white text-[#74685C]')}>
         <span style={{aspectRatio:key,width:key==='16:9'?30:key==='1:1'?23:15,height:key==='16:9'?17:key==='1:1'?23:27}} className={'block border-2 '+(draft.ratio===key?'border-[#C96442]':'border-[#B5A99A]')}/>
         <span className="text-[11px] font-bold">{key}</span>
        </button>)}
       </div>
       <p className="mt-2 text-[11px] text-[#82796E]">{ratios[draft.ratio].label} · 실제 픽셀 크기는 모델/렌더 설정에 따릅니다.</p>
      </fieldset>
      <div className="grid gap-3 sm:grid-cols-2">
       <FormField label="영상 길이 · 초" hint={usingComfy&&draft.mode==='scenes'?'전체 장면의 합계 시간':'기본 6초 · 최대 '+maxDuration+'초'}><TextInput type="number" min={3} max={maxDuration} step={1} value={draft.seconds} onChange={e=>patchDraft({seconds:Number(e.target.value)})}/></FormField>
       {usingComfy?<FormField label="프레임 / 초 (FPS)" hint="프레임이 많을수록 생성 작업량이 증가합니다."><SelectInput value={draft.fps} onChange={e=>patchDraft({fps:Number(e.target.value)})}>{[8,12,16,20,24,30].map(n=><option value={n} key={n}>{n} FPS</option>)}</SelectInput></FormField>:
        <div className={soft+' flex items-center gap-2 p-3 text-[12px] text-[#766B60]'}><CheckCircle2 size={17} className="text-[#58724D]"/> 자막 카드 방식은 추가 모델 API 호출 없이 합성합니다.</div>}
      </div>
      {usingComfy&&<div className="mt-4 grid gap-2">
       <button type="button" className={tabStyle+' '+focus+' !px-0 !text-[#8B543A]'} aria-expanded={advanced} onClick={()=>setAdvanced(!advanced)}><SlidersHorizontal size={15}/> {advanced?'고급 옵션 닫기':'고급 렌더 옵션 열기'} <ArrowRight size={14} className={advanced?'rotate-90 transition-transform':'transition-transform'}/></button>
       {advanced&&<div className="grid gap-3 rounded-xl border border-[#E7E0D5] bg-[#FAF7F1] p-3">
        <FormField label="제외할 요소 (Negative Prompt)"><TextInput value={draft.negative} onChange={e=>patchDraft({negative:e.target.value})} placeholder="blur, watermark, low quality"/></FormField>
        <FormField label="시드 · 같은 장면을 재현하는 값" hint="-1은 새 프로젝트에 랜덤 시드를 적용합니다. 이전 프로젝트를 수정할 때는 기존 시드를 이어받습니다.">
         <div className="flex gap-2"><TextInput value={draft.seed} onChange={e=>patchDraft({seed:e.target.value})}/><Button variant="secondary" size="sm" onClick={()=>patchDraft({seed:'-1'})}>랜덤</Button></div>
        </FormField>
       </div>}
       <div className={soft+' p-4'}>
        <div className="flex flex-wrap items-center justify-between gap-2"><strong className="text-[12px] text-[#3D352E]">새 장면 렌더 예산</strong><label className="flex items-center gap-2 text-[12px] font-semibold"><input type="checkbox" checked={budgetEnabled} onChange={e=>{setBudgetEnabled(e.target.checked);setMaxNewScenes(suggestedMaxNew)}}/> 상한 사용</label></div>
        <p className="mt-1 text-[11px] leading-5 text-[#817366]">외부 GPU가 실제로 새로 생성할 장면 수를 제한합니다. 이전 영상을 재사용하면 비용을 줄일 수 있습니다.</p>
        {budgetEnabled&&<div className="mt-3 flex items-center gap-3"><TextInput type="number" min={reuseId?0:1} max={suggestedMaxNew} value={maxNewScenes} onChange={e=>setMaxNewScenes(Number(e.target.value))} aria-label="최대 신규 렌더 장면 수" className="max-w-[105px]"/><span className="text-[12px] text-[#756A5D]">/ 전체 {suggestedMaxNew}장면 · 상한 초과 시 생성이 중지됩니다.</span></div>}
        {editingSource&&<p className="mt-2 text-[11px] text-[#597452]"><ShieldCheck size={13} className="mr-1 inline"/> 장면 재사용은 서버에서 기존 파일 지문과 SHA-256 검증 후 결정합니다. 내용·시드·규격이 바뀌면 새 렌더가 필요할 수 있습니다.</p>}
       </div>
      </div>}
      {invalidTime&&<p role="alert" className="mt-3 text-[12px] font-semibold text-rose-700">장면 {sceneItems.length}개의 최소 길이는 {sceneItems.length*3}초입니다. 시간을 늘리거나 장면을 줄여주세요.</p>}
      {invalidScenes&&<p role="alert" className="mt-3 text-[12px] font-semibold text-rose-700">모든 장면에 설명을 입력해 주세요. 최대 8개, 각 4,000바이트입니다.</p>}
      {!validBrief&&<p role="alert" className="mt-3 text-[12px] font-semibold text-rose-700">전체 영상 설명은 16,000바이트 이하로 작성해 주세요.</p>}
      {multiSceneRequiresFfmpeg&&<p role="alert" className="mt-3 text-[12px] font-semibold text-rose-700">여러 AI 장면을 합치려면 FFmpeg를 설치해야 합니다.</p>}
     </Panel>
     <Panel className="p-4 sm:p-6">
      <div className="flex flex-wrap items-center justify-between gap-2"><h3 className={panelTitle}>3. 제작 준비</h3><StateTag online={ready}>{ready?'생성 가능':'설정 확인'}</StateTag></div>
      <div className="mt-3 grid gap-2 sm:grid-cols-2">
       <div className={soft+' p-3'}><span className="block text-[11px] text-[#807568]">렌더 방식</span><strong className="mt-1 block text-[13px] text-[#3D342E]">{usingComfy?'AI 장면 생성 (ComfyUI)':'자막 · 카드 합성 (FFmpeg)'}</strong></div>
       <div className={soft+' p-3'}><span className="block text-[11px] text-[#807568]">예상 처리량 · 비용이 아님</span><strong className="mt-1 block text-[13px] text-[#3D342E]">{usingComfy?(draft.seconds*draft.fps).toLocaleString('ko-KR')+' frames':'로컬 합성'}</strong></div>
      </div>
      {prerequisite&&<div role="status" className="mt-3 flex flex-wrap items-center justify-between gap-3 rounded-xl border border-amber-200 bg-amber-50 p-3 text-[12px] text-amber-900"><span>{prerequisite}</span><Button size="sm" variant="secondary" onClick={()=>setTab('settings')}>설정으로 이동 <ArrowRight size={14}/></Button></div>}
      <div className="mt-4 flex flex-col justify-between gap-3 sm:flex-row sm:items-center">
       <p className="max-w-md text-[11px] leading-5 text-[#81766B]">{usingComfy?'실제 금전 비용은 연결 서버의 GPU·서비스 요금에 따릅니다. 임의의 가격을 표시하지 않습니다.':'영상은 로컬 도구로 만들어지며, 게시하기 전에는 외부에 공개되지 않습니다.'}</p>
       <Button variant="primary" className="min-h-11 shrink-0 !px-6" disabled={!canGenerate} onClick={()=>void generate()}>
        {generating?<><LoaderCircle size={16} className="animate-spin"/> 요청 중</>:<><PlayIcon/> 영상 생성하기 <ArrowRight size={16}/></>}
       </Button>
      </div>
      {busy&&<p role="status" className="mt-2 text-[11px] text-[#86664D]">현재 작업이 진행 중입니다. 다른 장면을 제출하기 전에 기존 작업 결과를 확인해 주세요.</p>}
     </Panel>
    </div>
    <aside className="grid min-w-0 content-start gap-4">
     <Panel className="p-4 sm:p-5">
      <div className="mb-3 flex flex-wrap items-center justify-between gap-2"><h3 className={panelTitle}>실시간 미리보기</h3>{previewJob&&<span className={'rounded-full px-2.5 py-1 text-[11px] font-bold '+(previewJob.status==='completed'?'bg-[#EAF4E9] text-[#456A46]':'bg-[#F8EEE3] text-[#A1623D]')}>{statusText(previewJob.status)}</span>}</div>
      {!previewJob?<div className="flex min-h-72 flex-col items-center justify-center gap-3 rounded-2xl border border-dashed border-[#D9CFBF] bg-[#FBF8F4] p-4 text-center"><div className="flex size-14 items-center justify-center rounded-2xl bg-[#F2E2D6]"><Clapperboard size={27} className="text-[#B46645]"/></div><strong className="text-[13px] text-[#55483C]">첫 번째 영상을 만들어 보세요</strong><p className="max-w-[250px] text-[11px] leading-5 text-[#83776A]">제작을 시작하면 여기서 진행 상태와 완성된 영상을 확인할 수 있습니다.</p></div>:<>
       <div className="flex min-h-[230px] items-center justify-center overflow-hidden rounded-2xl bg-[#171D28] p-3" style={{maxHeight:440}}>
        {previewJob.outputUrl?<video key={previewJob.id} controls playsInline preload="metadata" src={previewJob.outputUrl} className="max-h-[420px] max-w-full rounded-xl object-contain" style={{aspectRatio:String(previewWidth)+' / '+String(previewHeight)}}/>:
         <div className="flex min-h-[225px] flex-col items-center justify-center gap-3 px-4 text-center text-white"><LoaderCircle size={27} className={running(previewJob.status)?'animate-spin text-[#F5BF9B]':'text-[#E9B28A]'}/><strong className="text-[13px]">{statusText(previewJob.status)}</strong><span className="text-[11px] text-[#B9C1CA]">{phaseText(previewJob.phase)}</span></div>}
       </div>
       <div className="mt-3 flex items-start justify-between gap-2"><div className="min-w-0"><strong className="block truncate text-[13px] text-[#3B342D]" title={previewJob.title}>{previewJob.title||'이름 없는 영상'}</strong><span className="mt-1 block text-[11px] text-[#897B6D]">{shortDate(previewJob.createdAt)} · {previewJob.duration||'-'}초 · {previewJob.aspectRatio||'9:16'}</span></div>{previewJob.outputUrl&&<div className="flex shrink-0 gap-1"><a href={previewJob.outputUrl} download={(previewJob.title||"dominion-video")+".mp4"} aria-label="영상 MP4 다운로드" title="영상 MP4 다운로드" className={"inline-flex size-9 items-center justify-center rounded-lg text-[#77695E] hover:bg-[#F1ECE5] "+focus}><Download size={17}/></a><Button variant="ghost" size="icon" aria-label="영상 저장 폴더 열기" onClick={()=>void reveal(previewJob.id)}><FolderOpen size={17}/></Button></div>}</div>
       {running(previewJob.status)&&<div className="mt-3" role="status" aria-live="polite">
        <div className="mb-1.5 flex justify-between gap-2 text-[11px] text-[#75685A]"><span>{phaseText(previewJob.phase)}{typeof previewJob.sceneProgress?.index==='number'&&previewJob.sceneProgress.total?' · '+(previewJob.sceneProgress.index+1)+'/'+previewJob.sceneProgress.total+' 장면':''}</span><span className="tabular-nums">{progress===null?'진행률 측정 중':progress+'%'}</span></div>
        <div role="progressbar" aria-label="영상 생성 진행률" aria-valuemin={0} aria-valuemax={100} aria-valuenow={progress??undefined} className="h-2 overflow-hidden rounded-full bg-[#EFE7DE]"><div className={'h-full rounded-full bg-[#C96442] transition-all '+(progress===null?'w-1/3 animate-pulse':'')} style={progress===null?undefined:{width:String(progress)+'%'}}/></div>
       </div>}
       {previewJob.storyboard&&previewJob.storyboard.length>1&&<div className="mt-3">
        <div className="mb-2 flex items-center justify-between gap-2"><span className="text-[11px] font-semibold text-[#5B4D40]">장면 구성 · {previewJob.storyboard.length}개</span>{previewJob.sceneProgress?.total&&<span className="text-[10px] text-[#8D7C6F]">새 렌더 {previewJob.sceneProgress.rendered??0} · 재사용 {previewJob.sceneProgress.reused??0}</span>}</div>
        <div className="flex gap-2 overflow-x-auto pb-2" aria-label="영상 장면 타임라인">{previewJob.storyboard.map((scene,index)=><div key={index} className={'w-36 shrink-0 rounded-lg border p-2 '+(previewJob.sceneProgress?.index===index&&running(previewJob.status)?'border-[#C96442] bg-[#FFF3EB]':'border-[#E4DDD3] bg-[#FAF7F1]')}><span className="block text-[10px] font-bold text-[#A36141]">{String(index+1).padStart(2,'0')}</span><p className="mt-1 line-clamp-2 text-[11px] leading-4 text-[#675B50]">{scene}</p></div>)}</div>
       </div>}
       {previewJob.result&&previewJob.status==='completed'&&<div className={soft+' mt-3 grid grid-cols-2 gap-3 p-3'}>
        <div><span className="block text-[10px] text-[#81776B]">결과 파일</span><strong className="text-[12px] text-[#473A30]">{fmtBytes(previewJob.result.bytes)}</strong></div>
        <div><span className="block text-[10px] text-[#81776B]">재사용한 장면</span><strong className="text-[12px] text-[#507A4C]">{previewJob.result.reusedScenes??0}개</strong></div>
        <div><span className="block text-[10px] text-[#81776B]">새로 만든 장면</span><strong className="text-[12px] text-[#473A30]">{previewJob.result.renderedScenes??'-'}개</strong></div>
        <div><span className="block text-[10px] text-[#81776B]">동일 작업량 기준</span><strong className="text-[12px] text-[#473A30]">{(previewJob.renderWorkload?.frames??0).toLocaleString('ko-KR')} frames</strong></div>
       </div>}
       {previewJob.error&&<Notice message={previewJob.error}/>}
       {previewJob.status==='completed'&&previewJob.outputUrl&&<div className="mt-3 grid gap-2 sm:grid-cols-2">
        <Button variant="secondary" onClick={()=>editJob(previewJob)}><RotateCcw size={15}/> 장면 다시 편집</Button>
        <Button variant="primary" onClick={()=>chooseForUpload(previewJob)}><Upload size={15}/> 게시 준비</Button>
       </div>}
      </>}
     </Panel>
     <Panel className="p-4 sm:p-5">
      <div className="mb-3 flex items-center justify-between gap-2"><h3 className={panelTitle}>최근 작업</h3><Button variant="ghost" size="sm" onClick={()=>setTab('library')}>전체 보기 <ArrowRight size={14}/></Button></div>
      {!sortedJobs.length?<p className="rounded-xl bg-[#F9F5EF] p-4 text-[12px] text-[#807366]">아직 제작한 영상이 없습니다.</p>:<div className="grid gap-2">{sortedJobs.slice(0,5).map(job=><button key={job.id} type="button" onClick={()=>setCurrentId(job.id)} aria-current={videoJob?.id===job.id?'true':undefined}
       className={'flex w-full items-center gap-3 rounded-xl border px-3 py-3 text-left '+focus+' '+(videoJob?.id===job.id?'border-[#D6A27C] bg-[#FFF6ED]':'border-[#E9E2D9] bg-white hover:bg-[#FAF7F1]')}>
       <span className="flex size-9 shrink-0 items-center justify-center rounded-lg bg-[#F2E8DE]"><Film size={16} className="text-[#AA6A46]"/></span><span className="min-w-0 flex-1"><strong className="block truncate text-[12px] text-[#453A31]">{job.title||job.brief||'이름 없는 영상'}</strong><span className="text-[10px] text-[#897C70]">{shortDate(job.createdAt)} · {job.engine==='comfyui'?'AI 생성':'자막 영상'}</span></span><span className="shrink-0 text-[10px] font-semibold text-[#8B6A51]">{statusText(job.status)}</span>
       </button>)}</div>}
     </Panel>
    </aside>
   </div>
  </>}

  {tab==='library'&&<div className="grid gap-4">
   <div className="flex flex-wrap items-end justify-between gap-3"><div><h2 className="text-[19px] font-bold text-[#342C25]">프로젝트 라이브러리</h2><p className="mt-1 text-[12px] text-[#817468]">이전 결과를 다시 열고, 장면 일부를 수정하거나 게시할 수 있습니다.</p></div><Button variant="primary" onClick={()=>setTab('create')}><Plus size={16}/> 새 영상</Button></div>
   {!sortedJobs.length?<Panel className="flex min-h-64 flex-col items-center justify-center gap-3 p-6 text-center"><Clapperboard size={32} className="text-[#BE825F]"/><strong className="text-[14px] text-[#453B30]">첫 프로젝트를 만들어 보세요</strong><p className="text-[12px] text-[#83776B]">완성된 영상과 작업 중인 영상이 이곳에 쌓입니다.</p><Button variant="primary" onClick={()=>setTab('create')}>영상 만들기 <ArrowRight size={14}/></Button></Panel>:
    <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-3">{sortedJobs.map(job=><Panel key={job.id} className="overflow-hidden p-0">
     <div className="flex min-h-[190px] items-center justify-center bg-[#222A35] p-3">{job.outputUrl?<video src={job.outputUrl} controls playsInline preload="metadata" className="max-h-[230px] max-w-full rounded-lg"/>:<div className="flex flex-col items-center gap-3 py-10 text-center text-white"><Film size={26} className="text-[#EEB38D]"/><span className="text-[12px]">{statusText(job.status)}</span>{running(job.status)&&<LoaderCircle size={14} className="animate-spin"/>}</div>}</div>
     <div className="grid gap-3 p-4">
      <div className="flex items-center justify-between gap-3"><strong className="min-w-0 truncate text-[14px] text-[#3C322A]">{job.title||job.brief||'이름 없는 영상'}</strong><span className="shrink-0 text-[10px] text-[#8D7764]">{statusText(job.status)}</span></div>
      <span className="text-[11px] text-[#8A7D70]">{shortDate(job.createdAt)} · {job.duration||'-'}초 · {job.engine==='comfyui'?'ComfyUI':'FFmpeg'}</span>
      {job.error&&<p className="line-clamp-2 text-[11px] text-rose-600">{job.error}</p>}
      <div className="flex flex-wrap gap-2"><Button size="sm" variant="secondary" onClick={()=>{setCurrentId(job.id);setTab('create')}}>상태·미리보기</Button><Button size="sm" variant="secondary" onClick={()=>editJob(job)}><RotateCcw size={13}/> 편집</Button>{job.outputUrl&&<Button size="sm" variant="primary" onClick={()=>chooseForUpload(job)}>게시 준비 <ArrowRight size={13}/></Button>}</div>
     </div>
    </Panel>)}</div>}
  </div>}

  {tab==='publish'&&<div className="grid items-start gap-5 lg:grid-cols-[minmax(0,1fr)_minmax(310px,0.8fr)]">
   <Panel className="p-4 sm:p-6">
    <div className="mb-4 flex items-center justify-between gap-3"><h2 className={panelTitle}>YouTube 게시 준비</h2><span className="rounded-full bg-[#E9F0E5] px-3 py-1 text-[11px] font-bold text-[#55734A]">기본값 비공개</span></div>
    {!completed.length&&!outputs.value.length?<div className="flex min-h-36 flex-col items-center justify-center gap-3 rounded-xl border border-dashed border-[#D9CDBF] text-center"><p className="text-[12px] text-[#7D7266]">게시할 완료 영상이 없습니다. 먼저 영상을 제작해 주세요.</p><Button variant="primary" onClick={()=>setTab('create')}>영상 만들기 <ArrowRight size={14}/></Button></div>:<div className="grid gap-4">
     <FormField label="게시할 영상"><SelectInput value={selectedOutput?.id||''} onChange={e=>{const job=completed.find(j=>j.id===e.target.value)??outputs.value.find(j=>j.id===e.target.value);if(job){setUploadFile(job.id);setUploadTitle(job.title||'');setUploadDescription(job.brief||'')}}}>{(completed.length?completed:outputs.value).map(job=><option value={job.id} key={job.id}>{job.title||job.brief||'이름 없는 영상'} · {fmtBytes(job.result?.bytes)}</option>)}</SelectInput></FormField>
     <FormField label="영상 제목"><TextInput value={uploadTitle} maxLength={100} onChange={e=>setUploadTitle(e.target.value)} placeholder="게시할 제목을 적어 주세요"/></FormField>
     <FormField label="영상 설명"><TextArea rows={4} value={uploadDescription} onChange={e=>setUploadDescription(e.target.value)} placeholder="영상 설명을 적어 주세요"/></FormField>
     <div className="grid gap-3 sm:grid-cols-2"><FormField label="공개 범위"><SelectInput value={uploadPrivacy} onChange={e=>setUploadPrivacy(e.target.value)}><option value="private">비공개 · 나만 보기</option><option value="unlisted">일부 공개 · 링크 공유</option><option value="public">전체 공개</option></SelectInput></FormField><FormField label="태그"><TextInput value={uploadTags} onChange={e=>setUploadTags(e.target.value)} placeholder="여행, 브이로그, 일상"/></FormField></div>
     <Button className="min-h-11" variant="primary" disabled={uploading||Boolean(uploadSubmission&&running(uploadJob?.status))||!youtube.value.uploadAllowed||!selectedOutput?.outputUrl||!uploadTitle.trim()} onClick={()=>void startUpload()}>{uploading?<LoaderCircle size={15} className="animate-spin"/>:<Upload size={15}/>} YouTube에 업로드</Button>
     {!youtube.value.uploadAllowed&&<p role="status" className="text-[12px] text-[#976342]">아래에서 YouTube 계정을 연결하면 업로드 버튼이 활성화됩니다.</p>}
     {uploadJob&&<div className={soft+' p-3'} role="status"><div className="flex items-center justify-between gap-2 text-[12px]"><strong>{statusText(uploadJob.status)}</strong><span>{uploadJob.progress==null?'':uploadJob.progress+'%'}</span></div>{uploadJob.result?.url&&<a href={uploadJob.result.url} target="_blank" rel="noreferrer" className="mt-2 inline-flex items-center gap-1.5 text-[12px] font-semibold text-[#A55133]">YouTube에서 열기 <ExternalLink size={13}/></a>}{uploadJob.error&&<Notice message={uploadJob.error}/>}</div>}
    </div>}
   </Panel>
   <div className="grid gap-4">
    <Panel className="p-4 sm:p-5">
     <div className="mb-2 flex items-center justify-between gap-3"><h3 className={panelTitle}>계정 연결</h3><StateTag online={youtube.value.uploadAllowed}>{youtube.value.uploadAllowed?'업로드 준비됨':'연결 필요'}</StateTag></div>
     <p className="text-[12px] leading-5 text-[#776B60]">영상 제작과 게시 권한은 별개입니다. 사용자가 업로드 버튼을 누를 때만 게시 요청이 실행됩니다.</p>
     {youtube.value.account?.displayName&&<p className="mt-2 text-[12px] font-semibold text-[#45684C]">{youtube.value.account.displayName}</p>}
     <div className="mt-3 flex flex-wrap gap-2"><Button variant="secondary" onClick={()=>void connectYoutube()}>YouTube 연결 <ExternalLink size={14}/></Button><Button variant="ghost" onClick={()=>void youtube.reload()}>연결 상태 확인 <RefreshCw size={14}/></Button></div>
     {!google?.configured&&<div className="mt-4 grid gap-3 rounded-xl bg-[#FAF7F1] p-3"><p className="text-[11px] text-[#807467]">최초 연결 시 Google OAuth 앱의 Client ID와 Secret이 필요합니다.</p><TextInput aria-label="Google Client ID" value={gid} onChange={e=>setGid(e.target.value)} placeholder="Client ID"/><TextInput type="password" aria-label="Google Client Secret" value={gsecret} onChange={e=>setGsecret(e.target.value)} placeholder="Client Secret"/><Button variant="secondary" disabled={gBusy||!gid.trim()||!gsecret.trim()} onClick={()=>void saveGoogle()}>{gBusy?'저장 중':'연결 정보 저장'}</Button>{google?.callback&&<div className="text-[10px] break-all text-[#756A60]">리디렉션 주소: {google.callback} <button type="button" className="ml-1 underline" onClick={()=>void navigator.clipboard.writeText(google.callback||'')}>복사</button></div>}</div>}
    </Panel>
    {selectedOutput?.outputUrl&&<Panel className="p-4 sm:p-5"><h3 className={panelTitle}>업로드할 영상</h3><video className="mt-3 max-h-[320px] w-full rounded-xl bg-black" src={selectedOutput.outputUrl} controls playsInline preload="metadata"/><p className="mt-2 text-[11px] text-[#81766B]">{selectedOutput.title||'영상'} · {fmtBytes(selectedOutput.result?.bytes)}</p><Button variant="ghost" size="sm" onClick={()=>void reveal(selectedOutput.id)}><FolderOpen size={14}/> 파일 위치 열기</Button></Panel>}
   </div>
  </div>}

  {tab==='settings'&&<div className="grid items-start gap-4 lg:grid-cols-2">
   <Panel className="p-4 sm:p-6"><div className="mb-4 flex items-center justify-between gap-2"><h2 className={panelTitle}>영상 엔진과 연결</h2><StateTag online={ready}>{ready?'사용 가능':'확인 필요'}</StateTag></div>
    <FormField label="영상 엔진"><SelectInput value={engine} disabled={savingConfig} onChange={e=>void changeEngine(e.target.value as VideoEngine)}><option value="native">로컬 FFmpeg · 자막 카드 영상</option><option value="comfyui">ComfyUI · AI 장면 생성</option></SelectInput></FormField>
    <div className={soft+' mt-4 p-3'}><div className="flex items-center justify-between gap-2"><strong className="text-[12px] text-[#453B32]">FFmpeg</strong><StateTag online={Boolean(video.value.native?.available)}>{video.value.native?.available?'설치됨':'미설치'}</StateTag></div><p className="mt-1 text-[11px] text-[#7E7366]">자막 영상 제작과 여러 AI 장면을 하나의 MP4로 합칠 때 사용합니다.</p>
     <div className="mt-3 flex flex-col gap-2 sm:flex-row"><TextInput aria-label="FFmpeg 실행 파일 경로" value={ffmpegPath} onChange={e=>setFfmpegPath(e.target.value)} placeholder="자동 검색 · 또는 FFmpeg 실행 파일의 전체 경로"/><Button variant="secondary" disabled={savingConfig} className="shrink-0" onClick={()=>void saveConfig({ffmpegPath:ffmpegPath.trim()})}>경로 저장</Button></div>
     <p className="mt-1 text-[11px] text-[#81766B]">비워 두면 시스템에서 자동으로 검색합니다. FFmpeg가 설치됐는데 인식되지 않을 때 실행 파일 경로를 지정하세요.</p>
    </div>
    <div className="mt-4 grid gap-2"><div className="flex items-center justify-between gap-2"><strong className="text-[12px] text-[#453B32]">ComfyUI 서버</strong><StateTag online={video.value.comfy.reachable}>{video.value.comfy.reachable?'연결됨':'연결 안 됨'}</StateTag></div><div className="flex gap-2"><TextInput aria-label="ComfyUI 주소" value={comfyUrl} onChange={e=>setComfyUrl(e.target.value)} placeholder="http://127.0.0.1:8188"/><Button variant="secondary" disabled={savingConfig||!comfyUrl.trim()} onClick={()=>void saveConfig({comfyUrl:comfyUrl.trim()})}>저장</Button></div><p className="text-[11px] text-[#81766B]">ComfyUI가 실행 중인 로컬 또는 원격 HTTP(S) 주소를 입력하세요.</p></div>
    <div className="mt-4 rounded-xl border border-[#E7E0D5] p-3"><h3 className="mb-2 text-[12px] font-bold text-[#453A31]">외부 워크플로 가져오기</h3>{savedWorkflow&&<div className="mb-3 rounded-lg bg-[#F0F4EA] p-3"><div className="flex flex-wrap items-center justify-between gap-2"><p className="text-[11px] font-semibold text-[#597450]"><Check size={13} className="mr-1 inline"/>{savedWorkflow.name} · 등록된 워크플로 사용 중</p><Button size="sm" variant="ghost" disabled={savingConfig} onClick={()=>void saveConfig({comfyWorkflow:null,comfyWorkflowName:''})}>등록 해제</Button></div><p className="mt-1 text-[11px] text-[#6B795F]">등록한 API 워크플로가 기본 모델보다 먼저 실행됩니다. 내장 모델로 바꾸려면 등록을 해제하세요.</p></div>}<label className={'inline-flex cursor-pointer items-center gap-2 rounded-xl border border-[#DCCCBF] bg-[#FFFDF9] px-3 py-2 text-[12px] font-semibold text-[#694A38] '+focus}><Upload size={14}/> API 워크플로 JSON 선택<input type="file" accept=".json,application/json" className="sr-only" disabled={savingConfig} onChange={e=>{const file=e.currentTarget.files?.[0];e.currentTarget.value='';if(file)void importWorkflow(file)}}/></label><p className="mt-2 text-[11px] text-[#81766B]">ComfyUI에서 API 형식으로 내보낸 JSON을 사용합니다. 일반 UI 그래프 JSON과 구별됩니다.</p></div>
   </Panel>
   <div className="grid gap-4">
    <Panel className="p-4 sm:p-6"><h2 className={panelTitle}>고급 모델·렌더 경로</h2><p className="mt-1 text-[12px] text-[#807469]">일반 모델 선택과 다운로드는 영상 만들기 화면에서 진행합니다.</p>
     <div className="mt-3 grid gap-3">
      <div className="rounded-xl border border-[#E7E0D5] bg-[#FAF7F1] p-3">
       <FormField label="ComfyUI 모델 설치 폴더"><div className="flex flex-col gap-2 sm:flex-row"><TextInput value={modelDir} onChange={e=>setModelDir(e.target.value)} aria-label="모델 설치 폴더 경로" placeholder={video.value.modelDirectory||'ComfyUI/models'}/><Button variant="secondary" disabled={savingConfig} className="shrink-0" onClick={()=>void saveConfig({comfyModelsDir:modelDir.trim()})}>경로 저장</Button></div></FormField>
       <p className="mt-2 break-all text-[11px] leading-5 text-[#817569]">현재 다운로드 위치: {video.value.modelDirectory||'서버에서 확인 중'}</p>
       <p className="mt-1 text-[11px] leading-5 text-[#817569]">다운로드는 DOMINION이 실행되는 컴퓨터의 이 폴더에 저장됩니다. 원격 ComfyUI를 연결했다면 원격 서버의 실제 모델 경로와 일치하는지 확인해 주세요.</p>
      </div>
      <p className="text-[11px] leading-5 text-[#827567]">모델 선택과 다운로드는 영상 만들기 메뉴에서 진행합니다.</p>
      <div className="grid gap-3 sm:grid-cols-2"><FormField label="기본 렌더 해상도"><SelectInput disabled={savingConfig} value={statusConfig.resolution||'480x832'} onChange={e=>void saveConfig({resolution:e.target.value})}>{['256x448','384x672','480x832','832x480','448x448'].map(x=><option key={x} value={x}>{x}</option>)}</SelectInput></FormField><FormField label="샘플링 스텝"><SelectInput disabled={savingConfig} value={statusConfig.steps??20} onChange={e=>void saveConfig({steps:Number(e.target.value)})}>{[4,8,12,20,30,40].map(x=><option key={x} value={x}>{x} steps</option>)}</SelectInput></FormField></div>
     </div>
    </Panel>
    <Panel className="p-4 sm:p-6"><h2 className={panelTitle}>내레이션</h2><div className="mt-3 grid gap-3 sm:grid-cols-2"><FormField label="음성"><SelectInput value={voice} disabled={savingConfig} onChange={e=>{setVoice(e.target.value);void saveConfig({voice:e.target.value})}}><option value="Yuna">Yuna</option></SelectInput></FormField><label className="mt-6 flex items-center gap-2 text-[12px] text-[#4B4035]"><input type="checkbox" checked={narration} disabled={savingConfig} onChange={e=>{setNarration(e.target.checked);void saveConfig({narration:e.target.checked})}}/> 내레이션 자동 생성</label></div><p className="mt-2 text-[11px] text-[#81766B]">지원되는 음성 엔진이 있으면 음성을 합성합니다. 지원되지 않는 환경에서는 무음으로 영상을 생성합니다.</p></Panel>
   </div>
  </div>}
 </div>
}
function PlayIcon(){return <Clapperboard size={16}/>}
