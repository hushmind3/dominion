import {useEffect,useState} from 'react'
import {ArrowDownToLine,ArrowRight,Check,CheckCircle2,ChevronDown,Clapperboard,Clock3,Download,ExternalLink,Film,FolderOpen,HardDrive,LoaderCircle,MonitorPlay,Play,RefreshCw,Settings2,ShieldCheck,Sparkles,Upload,Wallet,Wifi,WifiOff,Youtube} from 'lucide-react'
import {Button,TextInput,TextArea,SelectInput} from '../components/Primitives'
import {Action,FormField,Headline,Notice,Panel,StateTag,Subheading,useRemote} from '../components/UI'
import {openOAuth,post} from '../api'
import {jobFinished,useJob,type JobState} from '../hooks/useJob'
import {VideoAutopilot} from '../components/VideoAutopilot'
import {t,useLocale} from '../i18n'

type Workflow={id:string;name:string;bytes:number}
type VideoJob=JobState & {progress?:number;kind:'video';title?:string;brief:string;outputUrl?:string;result?:{bytes:number};duration?:number;aspectRatio?:string;createdAt?:string;engine?:VideoEngine;phase?:string;renderBudget?:{mode?:string;estimatedWorkReductionPercent?:number;frames?:number;steps?:number}}
type VideoEngine='native'|'comfyui'
type VideoStatus={config:{engine:VideoEngine;comfyUrl:string;voice?:string;narration?:boolean;videoModel?:string;resolution?:string;steps?:number;economy?:boolean;ffmpegPath?:string;useModel?:boolean};native?:{available:boolean};comfy:{reachable:boolean;available?:boolean};workflows:Workflow[];jobs:VideoJob[]}
type VideoModel={id:string;name:string;kind:string;available:boolean;hint:string}
type UploadJob=JobState & {progress?:number;result?:{url:string}}
type YoutubeStatus={connected:boolean;uploadAllowed:boolean;account?:{displayName:string;scopes:string}|null;uploads:UploadJob[]}
const ratios={'9:16':{width:360,height:640,label:'vid.ratio.shorts'},'16:9':{width:640,height:360,label:'vid.ratio.landscape'},'1:1':{width:480,height:480,label:'vid.ratio.square'}} as const
const terminal=jobFinished
const statusLabel=(status:string)=>status==='completed'?t('vid.status.completed'):status==='running'?t('vid.status.running'):status==='queued'?t('vid.status.queued'):status==='downloading'?t('vid.status.downloading'):status==='failed'?t('vid.status.failed'):status
const fmtBytes=(bytes=0)=>bytes>=1024**3?(bytes/1024**3).toFixed(1)+' GB':bytes>=1024**2?Math.round(bytes/1024**2)+' MB':'0 MB'

export function VideoPage({navigate}:{navigate:(page:any)=>void}){
 useLocale()
 const video=useRemote<VideoStatus>('/api/video/status',{config:{engine:'comfyui',comfyUrl:''},comfy:{reachable:false},workflows:[],jobs:[]},false)
 const youtube=useRemote<YoutubeStatus>('/api/youtube/status',{connected:false,uploadAllowed:false,account:null,uploads:[]})
 const oauth=useRemote<{id:string;configured:boolean;callback?:string}[]>('/api/oauth/providers',[])
 const outputs=useRemote<VideoJob[]>('/api/video/outputs',[])
 const catalog=useRemote<{videoModels:VideoModel[]}>('/api/video/catalog',{videoModels:[]})
 const downloads=useRemote<{downloads:{id:string;model:string;status:string;progress:number;bytes:number;total:number;error?:string}[]}>('/api/video/models/downloads',{downloads:[]})
 const [workflow,setWorkflow]=useState(''),[title,setTitle]=useState(''),[prompt,setPrompt]=useState(''),[negative,setNegative]=useState(''),[ratio,setRatio]=useState<keyof typeof ratios>('9:16'),[seconds,setSeconds]=useState(3),[fps,setFps]=useState(16),[seed,setSeed]=useState('-1')
 const [engineChoice,setEngine]=useState<'native'|'comfyui'>(),[comfyUrl,setComfyUrl]=useState(''),[savingConfig,setSavingConfig]=useState(false)
 const [voice,setVoice]=useState('Yuna'),[narration,setNarration]=useState(true),[ffmpegPath,setFfmpegPath]=useState('')
 const [tab,setTab]=useState<'autopilot'|'create'|'jobs'|'publish'|'settings'>('autopilot'),[refreshing,setRefreshing]=useState(false),[gid,setGid]=useState(''),[gsecret,setGsecret]=useState(''),[gBusy,setGBusy]=useState(false)
 const [economy,setEconomy]=useState(false),[advanced,setAdvanced]=useState(false),[selectedJobId,setSelectedJobId]=useState(''),[jobFilter,setJobFilter]=useState<'all'|'active'|'completed'>('all')
 const [videoSubmission,setVideoSubmission]=useState<VideoJob|null>(null),[generating,setGenerating]=useState(false),[error,setError]=useState('')
 const [uploadFile,setUploadFile]=useState(''),[uploadTitle,setUploadTitle]=useState(''),[uploadDescription,setUploadDescription]=useState(''),[uploadPrivacy,setUploadPrivacy]=useState('private'),[uploadTags,setUploadTags]=useState('')
 const [uploadSubmission,setUploadSubmission]=useState<UploadJob|null>(null),[uploading,setUploading]=useState(false)
 const focusId=selectedJobId||videoSubmission?.id||video.value.jobs[0]?.id||outputs.value[0]?.id||''
 const focusInitial=videoSubmission?.id===focusId?videoSubmission:video.value.jobs.find(item=>item.id===focusId)||outputs.value.find(item=>item.id===focusId)||null
 const activeVideo=useJob<VideoJob>(focusId?'/api/video/jobs/'+encodeURIComponent(focusId):null,focusInitial)
 const activeUpload=useJob(uploadSubmission?'/api/youtube/uploads/'+uploadSubmission.id:null,uploadSubmission)
 const videoJob=activeVideo.value??focusInitial??outputs.value[0]
 const uploadJob=activeUpload.value
 const uploadStatus=oauth.value.find(item=>item.id==='youtube')
 const gReady=uploadStatus?.configured
 const gCallback=uploadStatus?.callback
 const selectedOutput=uploadFile
  ? outputs.value.find(item=>item.id===uploadFile)??(videoJob?.id===uploadFile&&videoJob.outputUrl?videoJob:undefined)
  : outputs.value[0]
 const engine=engineChoice??(video.value.config.engine==='native'?'native':'comfyui')
 const usingComfy=engine==='comfyui'
 const selectedModel=catalog.value.videoModels.find(model=>model.id===video.value.config.videoModel)
 const hasWorkflow=video.value.workflows.length>0
 const ready=usingComfy?Boolean(video.value.comfy.reachable&&(hasWorkflow||selectedModel?.available)):Boolean(video.value.native?.available)
 const maxDuration=usingComfy?9:180
 const configDimensions=(video.value.config.resolution||'480x832').split('x').map(Number)
 const longSide=Math.max(configDimensions[0]||480,configDimensions[1]||832)
 const shortSide=Math.min(1024,Math.max(128,Math.round(longSide*9/16/16)*16))
 const squareSide=Math.min(configDimensions[0]||480,configDimensions[1]||832)
 const renderSize=ratio==='1:1'?{width:squareSide,height:squareSide}:ratio==='16:9'?{width:longSide,height:shortSide}:{width:shortSide,height:longSide}
 const economyAllowed=!usingComfy||!hasWorkflow
 const effectiveEconomy=economy&&economyAllowed
 const renderFps=usingComfy&&effectiveEconomy?Math.min(fps,12):fps
 const renderSteps=usingComfy&&effectiveEconomy?Math.min(video.value.config.steps??20,12):video.value.config.steps??20
 const baselineUnits=seconds*fps*(video.value.config.steps??20)*renderSize.width*renderSize.height
 const economyUnits=seconds*renderFps*renderSteps*renderSize.width*renderSize.height
 const reduction=baselineUnits>0?Math.round((1-economyUnits/baselineUnits)*100):0
 const jobs=video.value.jobs.length?video.value.jobs:outputs.value
 const activeCount=jobs.filter(job=>!terminal(job.status)).length
 const completedCount=jobs.filter(job=>job.status==='completed').length
 const filteredJobs=jobs.filter(job=>jobFilter==='all'||(jobFilter==='active'?!terminal(job.status):job.status==='completed'))
 const readinessIssue=!usingComfy?(!video.value.native?.available?'FFmpeg 실행 파일이 필요합니다. 환경설정에서 경로를 지정하세요.':''):
  !video.value.comfy.reachable?'ComfyUI에 연결되지 않았습니다. 환경설정에서 연결 상태를 확인하세요.':
  !hasWorkflow&&!selectedModel?.available?'실행 가능한 영상 모델 또는 ComfyUI 워크플로가 필요합니다.':''
 useEffect(()=>{setComfyUrl(video.value.config.comfyUrl)},[video.value.config.comfyUrl])
 useEffect(()=>{setFfmpegPath(video.value.config.ffmpegPath||'')},[video.value.config.ffmpegPath])
 useEffect(()=>{
  setVoice(video.value.config.voice||'Yuna')
  setNarration(video.value.config.narration!==false)
 },[video.value.config.voice,video.value.config.narration])
 useEffect(()=>{
  if(!video.value.workflows.some(item=>item.id===workflow))setWorkflow(video.value.workflows[0]?.id||'')
 },[video.value.workflows,workflow])
 useEffect(()=>{
  if(activeVideo.value?.status==='completed')void outputs.reload()
 },[activeVideo.value?.status,outputs.reload])
 const saveGoogle=async()=>{
  setGBusy(true);setError('');
  try{await post('/api/oauth/providers/youtube',{clientId:gid.trim(),clientSecret:gsecret.trim()});setGsecret('');await oauth.reload()}
  catch(e){setError((e as Error).message)}finally{setGBusy(false)}
 }
 const connectGoogle=async()=>{
  setError('');
  try{await openOAuth('youtube');await Promise.allSettled([youtube.reload(),oauth.reload()])}catch(e){setError(t('vid.youtubeFailed',{error:(e as Error).message}))}
 }
 const changeEngine=async(next:'native'|'comfyui')=>{
  if(savingConfig)return;
  setSavingConfig(true);setError('');
  try{await post('/api/video/config',{engine:next,comfyUrl:comfyUrl.trim()||video.value.config.comfyUrl});setEngine(next);if(next==='comfyui')setSeconds(current=>Math.min(current,9));await video.reload()}
  catch(e){setError((e as Error).message)}finally{setSavingConfig(false)}
 }
 const saveFfmpeg=async()=>{
  if(savingConfig)return
  setSavingConfig(true);setError('')
  try{await post('/api/video/config',{ffmpegPath:ffmpegPath.trim()});await video.reload()}
  catch(e){setError((e as Error).message)}finally{setSavingConfig(false)}
 }
 const saveComfy=async()=>{
  if(savingConfig)return;
  setSavingConfig(true);setError('');
  try{await post('/api/video/config',{engine:'comfyui',comfyUrl:comfyUrl.trim()});await video.reload()}
  catch(e){setError((e as Error).message)}finally{setSavingConfig(false)}
 }
 const importWorkflow=async(file:File)=>{
  if(savingConfig)return;
  setSavingConfig(true);setError('');
  try{
   if(file.size>1024*1024)throw Error(t('vid.workflowTooBig'))
   const comfyWorkflow:unknown=JSON.parse(await file.text());
   await post('/api/video/config',{engine:'comfyui',comfyWorkflow,comfyWorkflowName:file.name});
   await video.reload();
  }catch(e){setError((e as Error).message)}finally{setSavingConfig(false)}
 }
 const saveAudio=async(patch:{voice?:string;narration?:boolean})=>{
  if(patch.voice!==undefined)setVoice(patch.voice)
  if(patch.narration!==undefined)setNarration(patch.narration)
  setSavingConfig(true);setError('')
  try{await post('/api/video/config',patch);await video.reload()}
  catch(e){setError((e as Error).message)}finally{setSavingConfig(false)}
 }
 const saveModel=async(patch:{videoModel?:string;resolution?:string;steps?:number})=>{
  setSavingConfig(true);setError('')
  try{await post('/api/video/config',patch);await video.reload();await catalog.reload()}
  catch(e){setError((e as Error).message)}finally{setSavingConfig(false)}
 }
 const downloadState=downloads.value.downloads.find(row=>row.model===video.value.config.videoModel)
 useEffect(()=>{
  if(downloadState?.status!=='downloading')return
  const timer=window.setInterval(()=>void downloads.reload(),3000)
  return()=>window.clearInterval(timer)
 },[downloadState?.status,downloads.reload])
 useEffect(()=>{
  if(downloadState?.status==='completed')void Promise.all([catalog.reload(),video.reload()])
 },[downloadState?.status,catalog.reload,video.reload])
 useEffect(()=>{
  if(!jobs.some(job=>!terminal(job.status)))return
  const timer=window.setInterval(()=>void video.reload(),4000)
  return()=>window.clearInterval(timer)
 },[jobs,video.reload])
 const startDownload=async()=>{
  if(!video.value.config.videoModel)return
  setError('')
  try{await post('/api/video/models/'+encodeURIComponent(video.value.config.videoModel)+'/download',{});await downloads.reload()}
  catch(e){setError((e as Error).message)}
 }
 const reveal=async(id:string)=>{
  try{await post('/api/video/jobs/'+id+'/reveal',{})}catch(e){setError((e as Error).message)}
 }
 const generate=async()=>{
  if(!ready||!prompt.trim()||generating)return;
  const requestedSeed=Number(seed);
  if(!Number.isInteger(seconds)||seconds<3||seconds>maxDuration||
   (usingComfy&&(!seed.trim()||!Number.isSafeInteger(requestedSeed)||requestedSeed < -1||!Number.isInteger(fps)||fps<1||fps>60))){
   setError(t('vid.checkValues'));return;
  }
  setGenerating(true);setError('');
  try{
   const created=await post<VideoJob>('/api/video/jobs',{
    engine,title,brief:prompt,aspectRatio:ratio,duration:seconds,economy:effectiveEconomy,
    ...(usingComfy?{negativePrompt:negative,fps,seed:requestedSeed,width:renderSize.width,height:renderSize.height}:{}),
   });
   setVideoSubmission(created);setSelectedJobId(created.id);void video.reload()
  }catch(e){setError((e as Error).message)}finally{setGenerating(false)}
 }
 const useResult=()=>{
  if(!videoJob?.outputUrl)return
  setUploadFile(videoJob.id)
  setUploadTitle((videoJob.title||videoJob.brief).slice(0,90))
  setUploadDescription(videoJob.brief)
  setTab('publish')
 }
 const enterTab=(next:'autopilot'|'create'|'jobs'|'publish'|'settings')=>{
  if(next==='publish'&&!uploadFile){
   const prepared=videoJob?.outputUrl?videoJob:outputs.value[0]
   if(prepared){setUploadFile(prepared.id);setUploadTitle(current=>current||prepared.title||prepared.brief.slice(0,90));setUploadDescription(current=>current||prepared.brief)}
  }
  setTab(next)
 }
 const startUpload=async()=>{
  if(!youtube.value.uploadAllowed){setError(t('vid.connectYoutubeFirst'));return}
  if(!selectedOutput||!uploadTitle.trim()||uploading)return;
  setUploading(true);setError('');
  try{
   setUploadSubmission(await post<UploadJob>('/api/video/jobs/'+selectedOutput.id+'/upload',{
    title:uploadTitle,description:uploadDescription,privacyStatus:uploadPrivacy,tags:uploadTags,
   }));
  }catch(e){setError((e as Error).message)}finally{setUploading(false)}
 }

 const stageNames:Record<string,string>={queued:'작업 대기',storyboard:'스토리보드 구성',rendering:'장면 생성',encoding:'영상 인코딩',submitting:'ComfyUI 제출',downloading:'파일 저장',completed:'생성 완료',failed:'생성 실패',needs_review:'확인 필요',interrupted:'중단됨'}
 const jobStage=(job:VideoJob)=>stageNames[job.phase||'']||statusLabel(job.status)
 const progressValue=Math.max(0,Math.min(100,Number(videoJob?.progress??0)))
 const previewPanel=<Panel className="overflow-hidden">
  <div className="flex items-center justify-between gap-3 border-b border-[#EEE7DC] px-5 py-4">
   <div><h3 className="text-[15px] font-bold text-[#2E2922]">미리보기 · 작업 현황</h3><p className="mt-1 text-[11px] text-[#8A8176]">실제 렌더링 진행 상태가 여기에 표시됩니다.</p></div>
   {videoJob?<StateTag online={videoJob.status==='completed'}>{statusLabel(videoJob.status)}</StateTag>:<span className="text-[11px] text-[#9C9388]">작업 없음</span>}
  </div>
  <div className="p-4 sm:p-5">
   {videoJob?.outputUrl?<div className="flex min-h-56 items-center justify-center overflow-hidden rounded-2xl bg-[#161B20] p-3">
    <video key={videoJob.id} src={videoJob.outputUrl} className="max-h-[430px] w-auto max-w-full rounded-xl" controls playsInline preload="metadata" aria-label="완성된 영상 미리보기"/>
   </div>:<div className="relative flex min-h-64 flex-col items-center justify-center gap-3 overflow-hidden rounded-2xl bg-[radial-gradient(circle_at_25%_20%,#465B65_0%,#242F39_55%,#171F29_100%)] text-center text-white">
    <div className="pointer-events-none absolute inset-0 opacity-20" style={{backgroundImage:'linear-gradient(#fff2 1px,transparent 1px),linear-gradient(90deg,#fff2 1px,transparent 1px)',backgroundSize:'32px 32px'}}/>
    <div className="relative flex size-14 items-center justify-center rounded-2xl border border-white/20 bg-white/10"><Film size={26}/></div>
    <strong className="relative max-w-xs px-4 text-[14px]">{videoJob?(videoJob.title||videoJob.brief||'영상 생성 중'):'새 영상의 시작'}</strong>
    <p className="relative max-w-xs px-6 text-[11px] leading-5 text-white/65">{videoJob?jobStage(videoJob):'주제를 입력하고 생성하면 이곳에서 결과를 확인할 수 있습니다.'}</p>
    {!videoJob&&<span className="relative rounded-full border border-white/20 bg-white/10 px-3 py-1 text-[10px]">{ratio} · {usingComfy?'AI 영상':'자막 카드 영상'}</span>}
   </div>}
   {videoJob&&<div className="mt-4 space-y-3" aria-live="polite">
    <div className="flex items-start justify-between gap-3"><div className="min-w-0"><p className="truncate text-[13px] font-semibold text-[#342E28]">{videoJob.title||videoJob.brief||'영상 작업'}</p><p className="mt-1 text-[11px] text-[#948A7D]">{jobStage(videoJob)}</p></div>{!terminal(videoJob.status)&&<strong className="text-[14px] tabular-nums text-[#A95133]">{progressValue}%</strong>}</div>
    {!terminal(videoJob.status)&&<div role="progressbar" aria-label="영상 생성 진행률" aria-valuenow={progressValue} aria-valuemin={0} aria-valuemax={100} className="h-2.5 overflow-hidden rounded-full bg-[#EFE7DA]"><div className="h-full rounded-full bg-gradient-to-r from-[#CB8059] to-[#C96442] transition-[width] duration-700" style={{width:progressValue+'%'}}/></div>}
    {videoJob.status==='completed'&&videoJob.outputUrl&&<div className="flex flex-wrap gap-2">
     <a href={videoJob.outputUrl} download={(videoJob.title||'dominion-video')+'.mp4'} className="inline-flex min-h-9 items-center gap-2 rounded-xl bg-[#C96442] px-3.5 py-2 text-[12px] font-semibold text-white hover:bg-[#AD5336]"><Download size={15}/> MP4 다운로드</a>
     <Button size="sm" variant="secondary" onClick={()=>void reveal(videoJob.id)}><FolderOpen size={14}/> 폴더 열기</Button>
     <Button size="sm" variant="secondary" onClick={useResult}><Youtube size={15}/> YouTube 게시 준비</Button>
    </div>}
    {videoJob.renderBudget?.mode==='economy'&&<p className="text-[11px] text-[#697B66]">절약 모드 적용 · 예상 연산량 {videoJob.renderBudget.estimatedWorkReductionPercent??0}% 감소</p>}
    {['failed','needs_review','interrupted'].includes(videoJob.status)&&<><Notice message={videoJob.error||'작업을 확인한 다음 재시도해 주세요.'}/><Button size="sm" variant="secondary" onClick={()=>{setTitle(videoJob.title||'');setPrompt(videoJob.brief);setRatio((videoJob.aspectRatio==='16:9'||videoJob.aspectRatio==='1:1'?videoJob.aspectRatio:'9:16'));setTab('create')}}><RefreshCw size={14}/> 설정을 불러와 다시 만들기</Button></>}
   </div>}
  </div>
 </Panel>
 return <main className="mx-auto w-full max-w-[1500px] min-w-0 px-4 py-5 sm:px-7 lg:px-8 lg:py-7">
  <header className="relative mb-5 overflow-hidden rounded-[26px] bg-[#243037] p-6 text-white shadow-[0_14px_35px_rgba(32,41,44,.12)] sm:p-8">
   <div className="pointer-events-none absolute -right-20 -top-24 size-80 rounded-full bg-[#C96442]/15 blur-3xl"/>
   <div className="relative flex flex-wrap items-start justify-between gap-5">
    <div className="min-w-0"><p className="mb-2 flex items-center gap-2 text-[10px] font-bold uppercase tracking-[.19em] text-[#E8AD91]"><Clapperboard size={15}/> DOMINION VIDEO STUDIO</p>
     <h1 className="text-[25px] font-bold tracking-[-.04em] sm:text-[30px]">아이디어에서 완성 영상까지</h1>
     <p className="mt-2 max-w-xl text-[12px] leading-6 text-white/65">주제를 정하고, 생성 방식을 고른 뒤 결과를 확인하세요. 모델 설치부터 YouTube 게시까지 한곳에서 관리합니다.</p>
    </div>
    <div className="flex flex-wrap items-center gap-2">
     <span className={'inline-flex items-center gap-1.5 rounded-full border px-3 py-1.5 text-[11px] '+(video.value.native?.available?'border-emerald-300/30 bg-emerald-300/10 text-emerald-100':'border-white/15 bg-white/5 text-white/70')}><HardDrive size={13}/> 로컬 {video.value.native?.available?'준비':'설정 필요'}</span>
     <span className={'inline-flex items-center gap-1.5 rounded-full border px-3 py-1.5 text-[11px] '+(video.value.comfy.reachable?'border-emerald-300/30 bg-emerald-300/10 text-emerald-100':'border-white/15 bg-white/5 text-white/70')}>{video.value.comfy.reachable?<Wifi size={13}/>:<WifiOff size={13}/>} ComfyUI {video.value.comfy.reachable?'연결됨':'미연결'}</span>
     <button type="button" aria-label="영상 상태 새로고침" disabled={refreshing} onClick={async()=>{setRefreshing(true);try{await Promise.allSettled([video.reload(),outputs.reload(),catalog.reload(),downloads.reload(),youtube.reload(),oauth.reload()])}finally{setRefreshing(false)}}} className="flex size-9 items-center justify-center rounded-xl border border-white/20 bg-white/10 hover:bg-white/20 disabled:opacity-50"><RefreshCw size={16} className={refreshing?'animate-spin':''}/></button>
    </div>
   </div>
   <div className="relative mt-6 grid grid-cols-3 divide-x divide-white/10 rounded-2xl border border-white/10 bg-white/5 py-3">
    <div className="px-4"><strong className="block text-[21px] font-bold tabular-nums">{jobs.length}</strong><span className="text-[10px] text-white/60">전체 작업</span></div>
    <div className="px-4"><strong className="block text-[21px] font-bold tabular-nums">{activeCount}</strong><span className="text-[10px] text-white/60">진행 중</span></div>
    <div className="px-4"><strong className="block text-[21px] font-bold tabular-nums">{completedCount}</strong><span className="text-[10px] text-white/60">완성 영상</span></div>
   </div>
  </header>
  {(error||video.error||outputs.error||activeVideo.error||activeUpload.error)&&<Notice message={error||video.error||outputs.error||activeVideo.error||activeUpload.error}/>}
  <nav aria-label="영상 스튜디오 메뉴" className="mb-5 flex min-w-0 flex-wrap items-center gap-1 rounded-2xl border border-[#E7E0D5] bg-[#FFFDF9] p-1.5">
   {([{key:'autopilot',text:'자율 콘텐츠 제작',icon:Sparkles},{key:'create',text:'수동 제작',icon:Clapperboard},{key:'jobs',text:'작업함',icon:Film},{key:'publish',text:'YouTube 게시',icon:Youtube},{key:'settings',text:'환경설정',icon:Settings2}] as const).map(item=><button type="button" key={item.key} aria-current={tab===item.key?'page':undefined} onClick={()=>enterTab(item.key)} className={'inline-flex min-h-10 items-center justify-center gap-2 rounded-xl px-3.5 text-[12px] font-semibold transition-colors '+(tab===item.key?'bg-[#C96442] text-white shadow-sm':'text-[#746B60] hover:bg-[#F3EEE5]')}><item.icon size={16}/><span>{item.text}</span>{item.key==='jobs'&&activeCount>0&&<span className={'rounded-full px-1.5 py-0.5 text-[10px] '+(tab===item.key?'bg-white/20':'bg-[#F4E4D9] text-[#B45636]')}>{activeCount}</span>}</button>)}
  </nav>
  {tab==='autopilot'&&<VideoAutopilot nativeReady={Boolean(video.value.native?.available)} comfyReady={Boolean(video.value.comfy.available)} onSettings={()=>setTab('settings')} onModelInstalled={video.reload} onPublish={job=>{setUploadFile(job.id);setUploadTitle((job.title||job.brief||'').slice(0,90));setUploadDescription(job.brief||'');setTab('publish')}}/>}
  {tab==='create'&&<div className="grid min-w-0 items-start gap-5 xl:grid-cols-[minmax(0,1.45fr)_minmax(330px,.95fr)]">
   <div className="min-w-0 space-y-5">
    <Panel className="p-5 sm:p-6">
     <div className="mb-4 flex items-center justify-between"><div><p className="text-[10px] font-bold tracking-[.15em] text-[#BB7552]">STEP 01</p><h2 className="mt-1 text-[17px] font-bold">어떻게 만들까요?</h2></div><span className="text-[11px] text-[#8D8377]">실행 엔진 선택</span></div>
     <div className="grid gap-3 sm:grid-cols-2">
      <button type="button" aria-pressed={!usingComfy} disabled={savingConfig} onClick={()=>void changeEngine('native')} className={'group min-w-0 rounded-2xl border p-4 text-left transition-all '+(!usingComfy?'border-[#C96442] bg-[#FFF3EB] ring-2 ring-[#C96442]/10':'border-[#E6DED3] bg-[#FFFEFB] hover:border-[#D2B5A2]')}>
       <div className="flex items-start justify-between gap-2"><span className="flex size-10 items-center justify-center rounded-xl bg-[#F0E3D7] text-[#AD664B]"><Clapperboard size={20}/></span>{!usingComfy?<CheckCircle2 size={19} className="text-[#C96442]"/>:<span className={'rounded-full px-2 py-1 text-[10px] '+(video.value.native?.available?'bg-emerald-50 text-emerald-700':'bg-amber-50 text-amber-800')}>{video.value.native?.available?'사용 가능':'설정 필요'}</span>}</div>
       <strong className="mt-3 block text-[14px] text-[#332C26]">빠른 자막 영상</strong><span className="mt-1 block text-[11px] leading-5 text-[#796E61]">장면 카드·자막·내레이션을 FFmpeg로 합성합니다. AI 영상 모델 없이도 제작할 수 있습니다.</span>
       <span className="mt-3 inline-flex items-center gap-1 text-[10px] font-semibold text-[#AA654A]"><Wallet size={12}/> 기본 렌더링 API 비용 없음</span>
      </button>
      <button type="button" aria-pressed={usingComfy} disabled={savingConfig} onClick={()=>void changeEngine('comfyui')} className={'group min-w-0 rounded-2xl border p-4 text-left transition-all '+(usingComfy?'border-[#C96442] bg-[#FFF3EB] ring-2 ring-[#C96442]/10':'border-[#E6DED3] bg-[#FFFEFB] hover:border-[#D2B5A2]')}>
       <div className="flex items-start justify-between gap-2"><span className="flex size-10 items-center justify-center rounded-xl bg-[#E9E7F4] text-[#67638F]"><Sparkles size={20}/></span>{usingComfy?<CheckCircle2 size={19} className="text-[#C96442]"/>:<span className={'rounded-full px-2 py-1 text-[10px] '+(video.value.comfy.reachable?'bg-emerald-50 text-emerald-700':'bg-amber-50 text-amber-800')}>{video.value.comfy.reachable?'서버 연결됨':'서버 미연결'}</span>}</div>
       <strong className="mt-3 block text-[14px] text-[#332C26]">AI 생성 영상</strong><span className="mt-1 block text-[11px] leading-5 text-[#796E61]">선택한 ComfyUI 워크플로로 움직임이 있는 장면을 생성합니다.</span>
       <span className="mt-3 inline-flex items-center gap-1 text-[10px] font-semibold text-[#766CA2]"><HardDrive size={12}/> 모델·GPU 성능에 따라 시간과 자원 사용</span>
      </button>
     </div>
     {usingComfy&&<div className="mt-4 rounded-xl border border-[#EEE4D9] bg-[#FBF7F0] px-4 py-3">
      <div className="flex flex-wrap items-center justify-between gap-2"><div><strong className="text-[12px]">선택된 생성 구성</strong><p className="mt-1 text-[11px] text-[#81766B]">{hasWorkflow?'사용자 워크플로: '+video.value.workflows[0].name:selectedModel?selectedModel.name:'영상 모델 또는 워크플로를 선택해 주세요.'}</p></div><Button size="sm" variant="secondary" onClick={()=>setTab('settings')}><Settings2 size={13}/> 모델 설정</Button></div>
      {selectedModel&&!selectedModel.available&&!hasWorkflow&&<p className="mt-2 text-[11px] text-amber-700">선택한 모델이 설치되지 않았습니다. 환경설정에서 다운로드할 수 있습니다.</p>}
     </div>}
    </Panel>
    <Panel className="p-5 sm:p-6">
     <div className="mb-4"><p className="text-[10px] font-bold tracking-[.15em] text-[#BB7552]">STEP 02</p><h2 className="mt-1 text-[17px] font-bold">영상 내용을 알려주세요</h2><p className="mt-1 text-[11px] text-[#918679]">어떤 장면을 만들고 싶은지 자연스럽게 입력하시면 됩니다.</p></div>
     <div className="space-y-4">
      <FormField label="제목 · 선택 입력"><TextInput value={title} maxLength={90} onChange={e=>setTitle(e.target.value)} placeholder="예: 도시의 밤, 15초 감성 영상"/></FormField>
      <FormField label="영상 주제 및 장면 설명"><TextArea rows={6} value={prompt} maxLength={16000} onChange={e=>setPrompt(e.target.value)} placeholder="예: 비가 내리는 서울의 밤거리. 네온사인이 젖은 도로에 반사되고 카메라가 천천히 앞으로 이동합니다."/></FormField>
      <div className="flex flex-wrap items-center justify-between gap-2"><span className="text-[11px] text-[#978B7D]">{prompt.trim().length.toLocaleString()} / 16,000자</span>
       <div className="flex flex-wrap gap-1.5"><span className="self-center text-[10px] text-[#918679]">빠른 시작</span>
        {['여행 기록','제품 소개','정보 요약'].map((name,index)=><button key={name} type="button" onClick={()=>setPrompt(['도시 여행에서 만난 풍경과 거리, 이동 장면을 시간 순서대로 보여주세요.','제품의 주요 특징과 사용하는 상황을 장면별로 소개해 주세요.','핵심 내용을 짧은 장면으로 나누고 중요한 정보를 이해하기 쉽게 보여주세요.'][index])} className="rounded-full border border-[#E6DCD0] bg-[#FBF7F1] px-2.5 py-1 text-[10px] text-[#78695A] hover:border-[#C96442] hover:text-[#A95133]">{name}</button>)}
       </div>
      </div>
     </div>
    </Panel>
    <Panel className="p-5 sm:p-6">
     <div className="mb-4"><p className="text-[10px] font-bold tracking-[.15em] text-[#BB7552]">STEP 03</p><h2 className="mt-1 text-[17px] font-bold">화면 형식과 제작 비용</h2></div>
     <div className="grid gap-2 sm:grid-cols-3" role="group" aria-label="영상 화면 비율">
      {(['9:16','16:9','1:1'] as const).map(key=><button type="button" key={key} aria-pressed={ratio===key} onClick={()=>setRatio(key)} className={'flex min-h-24 flex-col items-center justify-center gap-2 rounded-2xl border px-3 py-3 transition-colors '+(ratio===key?'border-[#C96442] bg-[#FFF2EA] text-[#9D5134]':'border-[#E5DCD0] bg-[#FFFEFB] text-[#756B60] hover:bg-[#FAF6F0]')}>
       <span className={'border-2 border-current rounded-sm '+(key==='9:16'?'h-8 w-[18px]':key==='16:9'?'h-[19px] w-9':'size-7')}/><span className="text-[12px] font-bold">{key}</span><span className="text-[10px]">{key==='9:16'?'쇼츠 · 릴스':key==='16:9'?'가로 영상':'정사각형'}</span>
      </button>)}
     </div>
     <div className="mt-4 grid grid-cols-2 gap-3">
      <FormField label="영상 길이 · 초" hint={'최대 '+maxDuration+'초'}><TextInput type="number" min={3} max={maxDuration} step={1} value={seconds} onChange={e=>setSeconds(Number(e.target.value))}/></FormField>
      <div className="rounded-xl border border-[#EBE4D9] bg-[#F9F6F0] px-3 py-2.5"><p className="text-[11px] text-[#827669]">출력 방식</p><p className="mt-1 text-[13px] font-semibold">{usingComfy?'AI 프레임 생성':'로컬 영상 합성'}</p><p className="mt-1 text-[10px] text-[#9A8D7E]">{usingComfy?renderSize.width+' × '+renderSize.height+'px':'FFmpeg H.264 · MP4'}</p></div>
     </div>
     <div className="mt-4 rounded-2xl border border-[#E6DDCF] bg-[#FAF7F0] p-4">
      <div className="flex flex-wrap items-center justify-between gap-3"><div className="flex items-center gap-2"><Wallet size={17} className="text-[#B36F50]"/><div><strong className="text-[13px]">절약 모드</strong><p className="text-[11px] text-[#8B7E71]">적은 자원으로 빠르게 시도하기</p></div></div>
       <button type="button" role="switch" aria-checked={effectiveEconomy} aria-label="영상 절약 모드" disabled={!economyAllowed} onClick={()=>setEconomy(!economy)} className={'relative h-7 w-12 rounded-full transition-colors disabled:cursor-not-allowed disabled:opacity-40 '+(effectiveEconomy?'bg-[#C96442]':'bg-[#CBC2B5]')}><span className={'absolute top-1 size-5 rounded-full bg-white shadow-sm transition-all '+(effectiveEconomy?'left-6':'left-1')}/></button>
      </div>
      {usingComfy?<div className="mt-3 grid grid-cols-3 gap-2 border-t border-[#E9DFD2] pt-3 text-center"><div><strong className="block text-[15px] tabular-nums">{renderFps}</strong><span className="text-[10px] text-[#8B7E71]">FPS</span></div><div><strong className="block text-[15px] tabular-nums">{renderSteps}</strong><span className="text-[10px] text-[#8B7E71]">생성 스텝</span></div><div><strong className="block text-[15px] tabular-nums">{effectiveEconomy?reduction+'%':'기본'}</strong><span className="text-[10px] text-[#8B7E71]">예상 연산량 감소</span></div></div>:<p className="mt-3 border-t border-[#E9DFD2] pt-3 text-[11px] leading-5 text-[#827669]">{effectiveEconomy?'선택적 유료 AI 스토리보드를 사용하지 않습니다.':'로컬 합성은 기본적으로 별도의 영상 생성 API 호출이 필요하지 않습니다.'}</p>}
      <p className="mt-2 text-[10px] text-[#A09385]">{usingComfy&&hasWorkflow?'사용자 워크플로는 임의의 노드를 포함할 수 있어 절약 모드를 자동 적용하지 않습니다.':'연산량은 프레임·스텝·해상도 기준 상대 추정입니다. 실제 생성 시간이나 요금은 아닙니다.'}</p>
     </div>
     {usingComfy&&<div className="mt-4 rounded-xl border border-[#EAE2D8]">
      <button type="button" aria-expanded={advanced} onClick={()=>setAdvanced(!advanced)} className="flex w-full items-center justify-between rounded-xl px-4 py-3 text-left text-[12px] font-semibold text-[#665B50] hover:bg-[#FAF7F1]"><span><Settings2 size={14} className="mr-2 inline"/>고급 생성 설정</span><ChevronDown size={15} className={'transition-transform '+(advanced?'rotate-180':'')}/></button>
      {advanced&&<div className="space-y-3 border-t border-[#EEE6DB] px-4 py-4"><FormField label="네거티브 프롬프트"><TextInput value={negative} onChange={e=>setNegative(e.target.value)} placeholder="blurry, watermark, low quality"/></FormField><div className="grid grid-cols-2 gap-3"><FormField label="기본 FPS"><TextInput type="number" min={1} max={60} value={fps} onChange={e=>setFps(Number(e.target.value))}/></FormField><FormField label="랜덤 시드 (-1)"><TextInput value={seed} onChange={e=>setSeed(e.target.value)}/></FormField></div><Button size="sm" variant="ghost" onClick={()=>setSeed('-1')}>랜덤 시드로 초기화</Button></div>}
     </div>}
     <div className="mt-5 border-t border-[#EEE7DD] pt-5">
      {readinessIssue?<div role="status" className="mb-3 flex flex-wrap items-center justify-between gap-2 rounded-xl border border-amber-200 bg-amber-50 px-4 py-3 text-[11px] text-amber-900"><span>{readinessIssue}</span><Button size="sm" variant="secondary" onClick={()=>setTab('settings')}>설정 열기 <ArrowRight size={13}/></Button></div>:<p className="mb-3 flex items-center gap-2 text-[11px] text-emerald-700"><CheckCircle2 size={15}/> 생성 엔진 준비 완료</p>}
      {Boolean(videoSubmission&&!terminal(activeVideo.value?.status))&&<p role="status" className="mb-3 text-[11px] text-[#94734F]">현재 작업이 진행 중입니다. 작업함에서 상태를 확인할 수 있습니다.</p>}
      <Button variant="primary" className="min-h-12 w-full !rounded-xl !text-[14px]" disabled={generating||Boolean(videoSubmission&&!terminal(activeVideo.value?.status))||!ready||!prompt.trim()||seconds<3||seconds>maxDuration||!Number.isInteger(seconds)||(usingComfy&&(fps<1||fps>60||!Number.isInteger(fps)||!Number.isSafeInteger(Number(seed))||Number(seed)<-1))} onClick={()=>void generate()}>{generating?<><LoaderCircle size={17} className="animate-spin"/> 생성 요청 중</>:<><Play size={17} fill="currentColor"/> 영상 생성 시작 <ArrowRight size={16}/></>}</Button>
      <p className="mt-2 text-center text-[10px] text-[#A09385]">생성은 서버 작업으로 처리되며, 화면을 이동해도 작업 기록이 남습니다.</p>
     </div>
    </Panel>
   </div>
   <aside className="min-w-0 space-y-5 xl:sticky xl:top-5">{previewPanel}
    <Panel className="p-5"><div className="mb-3 flex items-center justify-between gap-2"><h3 className="text-[14px] font-bold">최근 작업</h3><Button size="sm" variant="ghost" onClick={()=>setTab('jobs')}>모두 보기 <ArrowRight size={14}/></Button></div>
     {!jobs.length?<p className="rounded-xl bg-[#F8F4ED] p-4 text-[11px] text-[#8C8073]">아직 작업 기록이 없습니다. 왼쪽에서 첫 영상을 만들어 보세요.</p>:<div className="space-y-2">{jobs.slice(0,5).map(job=><button key={job.id} type="button" onClick={()=>setSelectedJobId(job.id)} className={'flex w-full items-center gap-3 rounded-xl border p-3 text-left transition-colors '+(focusId===job.id?'border-[#D5A086] bg-[#FFF4EC]':'border-[#EBE4DA] hover:bg-[#FAF7F1]')}><span className="flex size-9 shrink-0 items-center justify-center rounded-lg bg-[#EEE7DE] text-[#9F7055]"><Film size={16}/></span><span className="min-w-0 flex-1"><strong className="block truncate text-[12px]">{job.title||job.brief||'이름 없는 작업'}</strong><small className="block text-[10px] text-[#908679]">{statusLabel(job.status)} · {job.progress??0}%</small></span>{focusId===job.id&&<Check size={15} className="text-[#C96442]"/>}</button>)}</div>}
    </Panel>
   </aside>
  </div>}
  {tab==='jobs'&&<div className="grid min-w-0 items-start gap-5 xl:grid-cols-[minmax(0,1.25fr)_minmax(330px,.95fr)]">
   <Panel className="p-5 sm:p-6"><div className="mb-4 flex flex-wrap items-center justify-between gap-3"><div><h2 className="text-[17px] font-bold">작업함</h2><p className="mt-1 text-[11px] text-[#8A8176]">진행 중인 작업과 완성 영상을 선택해 확인하세요.</p></div><Button size="sm" variant="secondary" onClick={()=>void Promise.all([video.reload(),outputs.reload()])}><RefreshCw size={14}/> 새로고침</Button></div>
    <div className="mb-4 flex gap-2" role="group" aria-label="작업 상태 필터">{([{key:'all',label:'전체'},{key:'active',label:'진행 중'},{key:'completed',label:'완료'}] as const).map(item=><button type="button" key={item.key} aria-pressed={jobFilter===item.key} onClick={()=>setJobFilter(item.key)} className={'rounded-full px-3 py-1.5 text-[11px] font-semibold '+(jobFilter===item.key?'bg-[#C96442] text-white':'bg-[#F2EDE5] text-[#7A6F62]')}>{item.label}</button>)}</div>
    {!filteredJobs.length?<div className="flex min-h-52 flex-col items-center justify-center gap-3 rounded-xl border border-dashed border-[#E0D6C9] bg-[#FBF8F3] text-center"><Film size={26} className="text-[#B6A899]"/><p className="text-[12px] text-[#85796D]">{jobFilter==='all'?'아직 영상 작업이 없습니다.':'이 상태의 작업이 없습니다.'}</p><Button variant="secondary" size="sm" onClick={()=>setTab('create')}>영상 만들기</Button></div>:<div className="grid gap-3">{filteredJobs.map(job=><button type="button" key={job.id} onClick={()=>setSelectedJobId(job.id)} className={'flex min-w-0 items-center gap-3 rounded-2xl border p-4 text-left transition-colors '+(focusId===job.id?'border-[#D8A386] bg-[#FFF5EE]':'border-[#ECE5DB] bg-white hover:bg-[#FCF8F3]')}>
      <span className="flex size-12 shrink-0 items-center justify-center rounded-xl bg-[#EEE9E3] text-[#A8795D]"><MonitorPlay size={21}/></span>
      <span className="min-w-0 flex-1"><strong className="block truncate text-[13px]">{job.title||job.brief||'영상 작업'}</strong><small className="mt-1 block text-[11px] text-[#93877A]">{statusLabel(job.status)} · {job.duration??'?'}초 · {job.aspectRatio||'영상'}</small>{!terminal(job.status)&&<span className="mt-2 block h-1.5 overflow-hidden rounded-full bg-[#EAE1D5]"><span className="block h-full rounded-full bg-[#C96442]" style={{width:Math.max(2,job.progress||0)+'%'}}/></span>}</span><span className="text-[11px] font-semibold text-[#B56948]">{job.status==='completed'?'보기':(job.progress??0)+'%'}</span>
     </button>)}</div>}
   </Panel>
   <div className="space-y-4">{previewPanel}{videoJob?.status==='completed'&&<Panel className="p-5"><h3 className="mb-2 text-[13px] font-bold">다음 작업</h3><p className="mb-3 text-[11px] text-[#83776A]">완성된 영상을 확인하고 다시 만들거나 YouTube에 게시할 수 있습니다.</p><div className="flex flex-wrap gap-2"><Button size="sm" variant="secondary" onClick={()=>{setPrompt(videoJob.brief);setTitle(videoJob.title||'');setRatio((videoJob.aspectRatio==='16:9'||videoJob.aspectRatio==='1:1'?videoJob.aspectRatio:'9:16'));setTab('create')}}><Sparkles size={14}/> 이 내용으로 새로 만들기</Button><Button size="sm" variant="secondary" onClick={useResult}><Youtube size={14}/> 게시 준비</Button></div></Panel>}</div>
  </div>}
  {tab==='publish'&&<div className="grid min-w-0 items-start gap-5 lg:grid-cols-[minmax(0,.85fr)_minmax(0,1.15fr)]">
   <Panel className="p-5 sm:p-6"><div className="mb-4 flex items-center gap-3"><span className="flex size-11 items-center justify-center rounded-xl bg-[#FCEAE5] text-[#B95339]"><Youtube size={23}/></span><div><h2 className="text-[16px] font-bold">YouTube 게시</h2><p className="text-[11px] text-[#8A8176]">동영상이 완성된 다음 직접 업로드하세요.</p></div></div>
    <div className="mb-4 rounded-xl border border-[#E9E0D5] bg-[#F9F6F0] p-4"><div className="flex flex-wrap items-center justify-between gap-2"><strong className="text-[12px]">계정 연결 상태</strong><StateTag online={youtube.value.uploadAllowed}>{youtube.value.uploadAllowed?'업로드 가능':'연결 필요'}</StateTag></div>{youtube.value.account?.displayName&&<p className="mt-2 text-[11px] text-[#82776A]">{youtube.value.account.displayName}</p>}<p className="mt-2 text-[11px] leading-5 text-[#8B8072]">계정 연결과 파일 업로드는 별개의 단계입니다. 영상은 자동 게시되지 않습니다.</p><div className="mt-3 flex flex-wrap gap-2"><Button size="sm" variant="primary" onClick={()=>void connectGoogle()}><Youtube size={14}/> 계정 연결</Button><Button size="sm" variant="secondary" onClick={()=>setTab('settings')}>앱 설정</Button></div></div>
    <div className="rounded-xl border border-[#E7E0D5] bg-[#FFFDF9] p-4"><h3 className="mb-2 text-[12px] font-bold">게시 전 확인</h3><div className="space-y-2 text-[11px] text-[#83776A]"><p className="flex items-center gap-2"><CheckCircle2 size={14} className="text-[#789376]"/> 영상 파일 선택</p><p className="flex items-center gap-2"><CheckCircle2 size={14} className="text-[#789376]"/> 제목·설명 작성</p><p className="flex items-center gap-2"><ShieldCheck size={14} className="text-[#789376]"/> 공개 범위 확인 후 직접 업로드</p></div></div>
   </Panel>
   <Panel className="p-5 sm:p-6"><div className="mb-4"><h2 className="text-[16px] font-bold">업로드 설정</h2><p className="mt-1 text-[11px] text-[#8A8176]">완성된 영상만 선택할 수 있습니다.</p></div>
    <div className="space-y-4"><FormField label="영상 파일"><SelectInput aria-label="YouTube에 게시할 영상 선택" value={selectedOutput?.id||''} disabled={!outputs.value.length} onChange={e=>{const picked=outputs.value.find(item=>item.id===e.target.value);if(!picked)return;setUploadFile(picked.id);setUploadTitle(picked.title||'');setUploadDescription(picked.brief)}}>
      {!outputs.value.length&&<option value="">완료된 영상 없음</option>}{outputs.value.map(item=><option key={item.id} value={item.id}>{item.title||'output.mp4'} · {fmtBytes(item.result?.bytes)}</option>)}</SelectInput></FormField>
     <FormField label="제목"><TextInput value={uploadTitle} maxLength={100} onChange={e=>setUploadTitle(e.target.value)} placeholder="YouTube에 표시할 제목"/></FormField>
     <FormField label="설명"><TextArea rows={5} value={uploadDescription} onChange={e=>setUploadDescription(e.target.value)} placeholder="영상 설명을 적어주세요."/></FormField>
     <div className="grid grid-cols-2 gap-3"><FormField label="공개 범위"><SelectInput value={uploadPrivacy} onChange={e=>setUploadPrivacy(e.target.value)}><option value="private">비공개 · 기본</option><option value="unlisted">링크가 있는 사람만</option><option value="public">전체 공개</option></SelectInput></FormField><FormField label="태그"><TextInput value={uploadTags} onChange={e=>setUploadTags(e.target.value)} placeholder="쉼표로 구분"/></FormField></div>
     <Button variant="primary" className="min-h-11 w-full" disabled={uploading||Boolean(uploadSubmission&&!terminal(uploadJob?.status))||!youtube.value.uploadAllowed||!selectedOutput||!uploadTitle.trim()} onClick={()=>void startUpload()}>{uploading?<><LoaderCircle size={16} className="animate-spin"/> 업로드 요청 중</>:<><Upload size={16}/> 확인 후 YouTube 업로드</>}</Button>
     {!youtube.value.uploadAllowed&&<p className="text-[11px] text-amber-700">먼저 Google 앱 및 YouTube 계정을 연결해 주세요.</p>}
     {uploadJob&&<div className="rounded-xl border border-[#E9E0D5] bg-[#F9F6F0] p-3" aria-live="polite"><div className="flex justify-between text-[11px]"><span>{statusLabel(uploadJob.status)}</span><span>{uploadJob.progress??0}%</span></div><div className="mt-2 h-1.5 rounded-full bg-[#EAE2D7]"><div className="h-full rounded-full bg-[#C96442]" style={{width:Math.min(100,uploadJob.progress??0)+'%'}}/></div>{uploadJob.result?.url&&<a href={uploadJob.result.url} target="_blank" rel="noreferrer" className="mt-2 inline-flex items-center gap-1 text-[12px] font-semibold text-[#BC6547]">YouTube에서 보기 <ExternalLink size={13}/></a>}{uploadJob.error&&<Notice message={uploadJob.error}/>}</div>}
    </div>
   </Panel>
  </div>}
  {tab==='settings'&&<div className="grid min-w-0 items-start gap-5 lg:grid-cols-2">
   <div className="space-y-5">
    <Panel className="p-5 sm:p-6"><div className="mb-4 flex items-center gap-2"><Settings2 size={19} className="text-[#C96442]"/><h2 className="text-[16px] font-bold">영상 생성 환경</h2></div>
     <div className="space-y-4"><FormField label="기본 엔진"><SelectInput value={engine} disabled={savingConfig} onChange={e=>void changeEngine(e.target.value as VideoEngine)}><option value="native">빠른 자막 영상 (FFmpeg)</option><option value="comfyui">AI 영상 (ComfyUI)</option></SelectInput></FormField>
     <div className="rounded-xl border border-[#EAE2D9] bg-[#FAF7F2] p-3"><p className="flex items-center gap-2 text-[12px] font-semibold"><HardDrive size={15}/> FFmpeg <StateTag online={Boolean(video.value.native?.available)}>{video.value.native?.available?'사용 가능':'경로 확인 필요'}</StateTag></p><p className="mt-2 text-[11px] text-[#887D70]">로컬 자막 영상 인코딩에 필요합니다. FFmpeg 경로는 서버의 영상 설정을 따릅니다.</p></div>
     <FormField label="FFmpeg 실행 파일 경로"><TextInput aria-label="FFmpeg 실행 파일 경로" value={ffmpegPath} onChange={e=>setFfmpegPath(e.target.value)} placeholder="자동 탐지 · 직접 지정할 경우 절대경로 입력"/></FormField>
     <div className="flex flex-wrap items-center justify-between gap-2"><p className="text-[10px] text-[#8B8072]">비워 두면 PATH에서 자동 탐색합니다.</p><Button size="sm" variant="secondary" disabled={savingConfig} onClick={()=>void saveFfmpeg()}>FFmpeg 경로 저장</Button></div>
     <FormField label="ComfyUI 주소"><TextInput value={comfyUrl} onChange={e=>setComfyUrl(e.target.value)} placeholder="http://127.0.0.1:8188"/></FormField><div className="flex flex-wrap items-center justify-between gap-2"><StateTag online={video.value.comfy.reachable}>{video.value.comfy.reachable?'연결 확인됨':'서버 미연결'}</StateTag><Button size="sm" variant="primary" disabled={savingConfig||!comfyUrl.trim()} onClick={()=>void saveComfy()}>{savingConfig?'저장 중':'주소 저장·연결 확인'}</Button></div></div>
    </Panel>
    <Panel className="p-5 sm:p-6"><h2 className="mb-4 text-[16px] font-bold">내레이션 설정</h2><div className="grid gap-3 sm:grid-cols-2"><FormField label="음성"><SelectInput value={voice} disabled={savingConfig} onChange={e=>void saveAudio({voice:e.target.value})}><option value="Yuna">Yuna</option></SelectInput></FormField><label className="flex items-center gap-3 rounded-xl border border-[#E6DFD5] bg-[#FAF8F4] p-3 text-[12px]"><input type="checkbox" checked={narration} disabled={savingConfig} onChange={e=>void saveAudio({narration:e.target.checked})} className="accent-[#C96442]"/> 내레이션 사용</label></div><p className="mt-3 text-[11px] text-[#8A8176]">운영체제에 지원 음성이 없으면 영상은 무음으로 생성됩니다.</p></Panel>
   </div>
   <div className="space-y-5">
    <Panel className="p-5 sm:p-6"><div className="mb-3 flex flex-wrap items-center justify-between gap-2"><h2 className="text-[16px] font-bold">AI 영상 모델 관리</h2><StateTag online={Boolean(selectedModel?.available)}>{selectedModel?.available?'설치됨':'모델 선택·설치'}</StateTag></div>
     <p className="mb-4 text-[11px] text-[#8A8176]">설치되지 않은 모델도 선택할 수 있습니다. 선택 후 이곳에서 다운로드하세요.</p>
     <div className="grid gap-3 sm:grid-cols-2"><FormField label="모델"><SelectInput value={video.value.config.videoModel||''} disabled={savingConfig} onChange={e=>void saveModel({videoModel:e.target.value})}><option value="">모델 선택</option>{catalog.value.videoModels.map(model=><option key={model.id} value={model.id}>{model.name}{model.available?' · 설치됨':' · 미설치'}</option>)}</SelectInput></FormField><FormField label="해상도 기준"><SelectInput value={video.value.config.resolution||'480x832'} disabled={savingConfig} onChange={e=>void saveModel({resolution:e.target.value})}>{['256x448','384x672','480x832','832x480','448x448'].map(size=><option key={size} value={size}>{size}</option>)}</SelectInput></FormField><FormField label="품질 스텝"><SelectInput value={String(video.value.config.steps??20)} disabled={savingConfig} onChange={e=>void saveModel({steps:Number(e.target.value)})}>{[4,8,12,20,30,40].map(step=><option key={step} value={step}>{step} 스텝</option>)}</SelectInput></FormField></div>
     {selectedModel&&<div className="mt-4 rounded-xl border border-[#EAE1D7] bg-[#FAF7F2] p-4"><div className="flex flex-wrap items-center justify-between gap-2"><div className="min-w-0"><p className="text-[12px] font-bold">{selectedModel.name}</p><p className="mt-1 text-[11px] text-[#908477]">{selectedModel.available?'모델 파일 확인됨':'다운로드 후 사용할 수 있습니다.'}</p></div>{selectedModel.available?<CheckCircle2 size={22} className="text-emerald-700"/>:<Button size="sm" variant="primary" disabled={downloadState?.status==='downloading'} onClick={()=>void startDownload()}>{downloadState?.status==='downloading'?<><LoaderCircle size={13} className="animate-spin"/> 다운로드 중</>:<><ArrowDownToLine size={14}/> 모델 다운로드</>}</Button>}</div>
      {downloadState&&<div className="mt-3"><div className="flex justify-between text-[10px] text-[#85796D]"><span>{downloadState.status==='downloading'?'모델 파일 받는 중':downloadState.status==='completed'?'다운로드 완료':downloadState.status==='failed'?'다운로드 실패':'대기 중'}</span><span>{Math.max(0,Math.min(100,downloadState.progress||0))}% · {fmtBytes(downloadState.bytes)}{downloadState.total?' / '+fmtBytes(downloadState.total):''}</span></div><div role="progressbar" aria-label="모델 다운로드 진행률" aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.max(0,Math.min(100,downloadState.progress||0))} className="mt-2 h-2 overflow-hidden rounded-full bg-[#E7DED2]"><div className="h-full rounded-full bg-[#C96442]" style={{width:Math.max(0,Math.min(100,downloadState.progress||0))+'%'}}/></div>{downloadState.error&&<p className="mt-2 text-[11px] text-rose-700">{downloadState.error}</p>}</div>}
     </div>}
    </Panel>
    <Panel className="p-5 sm:p-6"><h2 className="mb-3 text-[16px] font-bold">사용자 ComfyUI 워크플로</h2><p className="text-[11px] leading-5 text-[#82776C]">{hasWorkflow?'저장된 워크플로: '+video.value.workflows[0].name:'JSON API 형식의 워크플로를 가져오면 기본 모델 없이도 사용할 수 있습니다.'}</p>
     <div className="mt-3 flex flex-wrap items-center gap-2"><label className="inline-flex cursor-pointer items-center gap-2 rounded-xl border border-[#E1D9CC] bg-[#FFFDF9] px-3.5 py-2 text-[12px] font-semibold text-[#3A342D] hover:bg-[#F7F2EA]"><Upload size={14}/> 워크플로 JSON 가져오기<input className="hidden" type="file" accept=".json,application/json" disabled={savingConfig} onChange={e=>{const file=e.currentTarget.files?.[0];e.currentTarget.value='';if(file)void importWorkflow(file)}}/></label><Button size="sm" variant="ghost" onClick={()=>void video.reload()}><RefreshCw size={14}/> 상태 확인</Button></div>
    </Panel>
    <Panel className="p-5 sm:p-6"><div className="mb-3 flex items-center justify-between gap-2"><h2 className="text-[16px] font-bold">YouTube 앱 연결</h2><StateTag online={Boolean(gReady)}>{gReady?'앱 설정됨':'설정 필요'}</StateTag></div><p className="text-[11px] text-[#8A8176]">Google OAuth 앱 정보를 연결한 후 계정을 인증하세요.</p>
     {gCallback&&<div className="mt-3 flex items-center gap-2 rounded-xl bg-[#F5F1EA] p-3"><code className="min-w-0 flex-1 break-all text-[10px]">{gCallback}</code><Button size="sm" variant="secondary" onClick={async()=>{try{await navigator.clipboard.writeText(gCallback)}catch(e){setError((e as Error).message)}}}>복사</Button></div>}
     <div className="mt-4 grid gap-3 sm:grid-cols-2"><FormField label="Client ID"><TextInput value={gid} onChange={e=>setGid(e.target.value)} placeholder="Google OAuth Client ID"/></FormField><FormField label="Client Secret"><TextInput type="password" value={gsecret} onChange={e=>setGsecret(e.target.value)} placeholder="비밀키 입력"/></FormField></div>
     <div className="mt-3 flex flex-wrap gap-2"><Button variant="primary" size="sm" disabled={gBusy||!gid.trim()||!gsecret.trim()} onClick={()=>void saveGoogle()}>{gBusy?'저장 중':'앱 정보 저장'}</Button><Button size="sm" variant="secondary" onClick={()=>void connectGoogle()}><Youtube size={14}/> YouTube 계정 연결</Button></div>
    </Panel>
   </div>
  </div>}
 </main>
}
