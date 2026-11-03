import {useEffect,useState} from 'react'
import {Check,Clapperboard,ExternalLink,LoaderCircle,Play,RefreshCw,Upload} from 'lucide-react'
import {Button,TextInput,TextArea,SelectInput} from '../components/Primitives'
import {Action,FormField,Headline,Notice,Panel,StateTag,Subheading,useRemote} from '../components/UI'
import {get,post} from '../api'

type Workflow={id:string;name:string;bytes:number}
type VideoJob={id:string;status:string;progress:number;prompt:string;width:number;height:number;frames:number;fps:number;seconds?:number;seed:number;error?:string;result?:{file:string;filename:string;url:string}}
type VideoStatus={config:{comfyUrl:string;workflowDir:string;outputDir:string};comfy:{reachable:boolean;error?:string};workflows:Workflow[];jobs:VideoJob[]}
type UploadJob={id:string;status:string;progress:number;sent:number;total:number;title:string;privacyStatus:string;error?:string;result?:{id:string;url:string;title:string;status:string;privacyStatus:string}}
type YoutubeStatus={connected:boolean;uploadAllowed:boolean;account?:{displayName:string;scopes:string}|null;uploads:UploadJob[]}
const ratios={'9:16':{width:720,height:1280,label:'쇼츠 · 9:16'},'16:9':{width:1280,height:720,label:'가로 · 16:9'},'1:1':{width:960,height:960,label:'정사각 · 1:1'}} as const
const terminal=(status:string)=>['completed','failed','cancelled'].includes(status)

export function VideoPage({navigate}:{navigate:(page:any)=>void}){
 const video=useRemote<VideoStatus>('/api/video/status',{config:{comfyUrl:'',workflowDir:'',outputDir:''},comfy:{reachable:false},workflows:[],jobs:[]})
 const youtube=useRemote<YoutubeStatus>('/api/youtube/status',{connected:false,uploadAllowed:false,account:null,uploads:[]})
 const [workflow,setWorkflow]=useState(''),[prompt,setPrompt]=useState(''),[negative,setNegative]=useState(''),[ratio,setRatio]=useState<keyof typeof ratios>('9:16'),[seconds,setSeconds]=useState(4),[fps,setFps]=useState(16),[seed,setSeed]=useState('-1')
 const [comfyUrl,setComfyUrl]=useState(''),[savingConfig,setSavingConfig]=useState(false)
 const [videoJob,setVideoJob]=useState<VideoJob|null>(null),[generating,setGenerating]=useState(false),[error,setError]=useState('')
 const [uploadFile,setUploadFile]=useState(''),[uploadTitle,setUploadTitle]=useState(''),[uploadDescription,setUploadDescription]=useState(''),[uploadPrivacy,setUploadPrivacy]=useState('private'),[uploadTags,setUploadTags]=useState('')
 const [uploadJob,setUploadJob]=useState<UploadJob|null>(null),[uploading,setUploading]=useState(false)
 useEffect(()=>{if(!video.value.config.comfyUrl)return;setComfyUrl(video.value.config.comfyUrl)},[video.value.config.comfyUrl])
 useEffect(()=>{if(!workflow&&video.value.workflows[0])setWorkflow(video.value.workflows[0].id)},[video.value.workflows,workflow])
 useEffect(()=>{
  if(!videoJob||terminal(videoJob.status))return
  const timer=setInterval(async()=>{
   try{const next=await get<VideoJob>('/api/video/jobs/'+videoJob.id);setVideoJob(next);if(terminal(next.status)){void video.reload();clearInterval(timer)}}catch{}
  },2000)
  return()=>clearInterval(timer)
 },[videoJob?.id,videoJob?.status])
 useEffect(()=>{
  if(!uploadJob||terminal(uploadJob.status))return
  const timer=setInterval(async()=>{
   try{const next=await get<UploadJob>('/api/youtube/uploads/'+uploadJob.id);setUploadJob(next);if(terminal(next.status)){void youtube.reload();clearInterval(timer)}}catch{}
  },1500)
  return()=>clearInterval(timer)
 },[uploadJob?.id,uploadJob?.status])
 const saveComfy=async()=>{setSavingConfig(true);setError('');try{await post('/api/video/config',{comfyUrl});await video.reload()}catch(e){setError((e as Error).message)}finally{setSavingConfig(false)}}
 const generate=async()=>{
  if(!workflow){setError('영상 모델(워크플로)을 선택하세요.');return}
  if(!prompt.trim()){setError('영상 프롬프트를 입력하세요.');return}
  setGenerating(true);setError('');setUploadFile('');setUploadTitle('');setUploadDescription('');
  try{
   const dimensions=ratios[ratio]
   setVideoJob(await post<VideoJob>('/api/video/generate',{workflow,prompt,negative,width:dimensions.width,height:dimensions.height,seconds,fps,seed:Number(seed)}))
  }catch(e){setError((e as Error).message)}finally{setGenerating(false)}
 }
 const useResult=()=>{
  if(!videoJob?.result)return
  setUploadFile(videoJob.result.file)
  setUploadTitle(videoJob.prompt.slice(0,90))
  setUploadDescription(videoJob.prompt)
 }
 const pick=async()=>{try{const picked=await (window as any).dominionDesktop?.pickVideo?.();if(picked){setUploadFile(picked);if(!uploadTitle)setUploadTitle(String(picked).split(/[\\/]/).pop()?.replace(/\.[^.]+$/,'')||'')}}catch(e){setError((e as Error).message)}}
 const startUpload=async()=>{
  if(!youtube.value.uploadAllowed){setError('Google 계정을 YouTube 업로드 권한으로 먼저 연결하세요.');return}
  if(!uploadFile.trim()||!uploadTitle.trim()){setError('영상 파일과 제목을 입력하세요.');return}
  setUploading(true);setError('')
  try{setUploadJob(await post<UploadJob>('/api/youtube/upload',{path:uploadFile,title:uploadTitle,description:uploadDescription,privacyStatus:uploadPrivacy,tags:uploadTags}));void youtube.reload()}
  catch(e){setError((e as Error).message)}finally{setUploading(false)}
 }
 return <div className="mx-auto w-full max-w-[1400px] px-5 py-6 sm:px-8 lg:px-6 lg:pb-20">
  <Headline eyebrow="VIDEO PRODUCTION" title="영상" description="영상 모델(ComfyUI 워크플로)을 선택해 쇼츠·숏폼을 생성하고, 결과를 바로 YouTube에 업로드합니다. 채팅 모델과는 별개입니다." actions={<Button variant="secondary" onClick={()=>{void video.reload();void youtube.reload()}}><RefreshCw size={16}/> 상태 새로고침</Button>}/>
  {error&&<Notice message={error}/>}
  <div className="mb-5 grid grid-cols-1 gap-3 md:grid-cols-2">
   <Panel className={'p-4 '+(video.value.comfy.reachable?'border-emerald-200 bg-emerald-50/50':'border-amber-200 bg-amber-50/50')}><div className="flex items-center justify-between gap-3"><div className="flex items-center gap-2"><Clapperboard size={18} className={video.value.comfy.reachable?'text-emerald-700':'text-amber-700'}/><strong className="text-[13px] text-[#24211D]">ComfyUI 영상 엔진</strong></div><StateTag online={video.value.comfy.reachable}>{video.value.comfy.reachable?'연결됨':'연결 안 됨'}</StateTag></div><p className="mt-2 text-[11px] leading-5 text-[#5C544A]">{video.value.comfy.reachable?'워크플로를 선택해 영상을 생성할 수 있습니다.':'ComfyUI를 실행한 뒤 URL을 저장하세요.'}</p>{!video.value.comfy.reachable&&<div className="mt-2 flex gap-2"><TextInput value={comfyUrl} onChange={e=>setComfyUrl(e.target.value)} placeholder="http://127.0.0.1:8188"/><Button variant="secondary" disabled={savingConfig||!comfyUrl.trim()} onClick={()=>void saveComfy()}>{savingConfig?'저장 중':'저장'}</Button></div>}</Panel>
   <Panel className={'p-4 '+(youtube.value.uploadAllowed?'border-emerald-200 bg-emerald-50/50':'border-amber-200 bg-amber-50/50')}><div className="flex items-center justify-between gap-3"><div className="flex items-center gap-2"><Upload size={18} className={youtube.value.uploadAllowed?'text-emerald-700':'text-amber-700'}/><strong className="text-[13px] text-[#24211D]">YouTube 업로드</strong></div><StateTag online={youtube.value.uploadAllowed}>{youtube.value.uploadAllowed?'준비됨':youtube.value.connected?'권한 필요':'연결 필요'}</StateTag></div><p className="mt-2 text-[11px] leading-5 text-[#5C544A]">{youtube.value.uploadAllowed?'계정 '+youtube.value.account?.displayName+'으로 업로드할 수 있습니다.':youtube.value.connected?'업로드 권한으로 Google에 다시 로그인하세요.':'Google OAuth 앱과 계정을 먼저 연결하세요.'}</p><Button variant="ghost" className="mt-2 !px-0 !text-[11px] text-[#C96442]" onClick={()=>navigate('capabilities')}>계정 연동 설정 <ExternalLink size={13}/></Button></Panel>
  </div>
  <div className="grid grid-cols-1 items-start gap-5 xl:grid-cols-[minmax(0,1fr)_440px]">
   <Panel className="p-5">
    <Subheading title="영상 생성" extra={<span className="text-[11px] text-[#8A8176]">{video.value.workflows.length}개 워크플로</span>}/>
    {video.value.workflows.length?<div className="grid gap-4">
     <FormField label="영상 모델 · 워크플로"><SelectInput value={workflow} onChange={e=>setWorkflow(e.target.value)}>{video.value.workflows.map(item=><option key={item.id} value={item.id}>{item.name}</option>)}</SelectInput></FormField>
     <FormField label="프롬프트"><TextArea rows={5} value={prompt} onChange={e=>setPrompt(e.target.value)} placeholder="예: 비 오는 서울 골목, 시네마틱, 천천히 걷는 고양이, 4초" className="min-h-28"/></FormField>
     <FormField label="네거티브 프롬프트"><TextInput value={negative} onChange={e=>setNegative(e.target.value)} placeholder="low quality, blurry, watermark"/></FormField>
     <div className="grid grid-cols-2 gap-3 sm:grid-cols-4"><FormField label="화면 비율"><SelectInput value={ratio} onChange={e=>setRatio(e.target.value as keyof typeof ratios)}>{(Object.keys(ratios) as Array<keyof typeof ratios>).map(key=><option key={key} value={key}>{ratios[key].label}</option>)}</SelectInput></FormField><FormField label="길이(초)"><TextInput type="number" min={1} max={30} value={seconds} onChange={e=>setSeconds(Number(e.target.value))}/></FormField><FormField label="FPS"><TextInput type="number" min={1} max={60} value={fps} onChange={e=>setFps(Number(e.target.value))}/></FormField><FormField label="시드"><TextInput value={seed} onChange={e=>setSeed(e.target.value)} placeholder="-1 = 랜덤"/></FormField></div>
     <div className="flex flex-wrap items-center justify-between gap-3"><p className="text-[11px] leading-5 text-[#8A8176]">워크플로에 {`{{prompt}}`} {`{{width}}`} {`{{height}}`} {`{{frames}}`} {`{{fps}}`} {`{{seed}}`} 자리표시자가 필요합니다.</p><Action disabled={generating||!video.value.comfy.reachable||!workflow||!prompt.trim()} onClick={()=>void generate()}>{generating?<><LoaderCircle size={15} className="animate-spin"/> 요청 중</>:<><Play size={15}/> 영상 생성</>}</Action></div>
    </div>:<div className="rounded-xl border border-dashed border-[#D8CFC2] bg-[#FAF7F1] p-5 text-[12px] leading-6 text-[#5C544A]"><strong className="text-[#3A342D]">영상 워크플로가 없습니다.</strong><p className="mt-1">ComfyUI에서 Wan·HunyuanVideo·LTX 계열 워크플로를 API 형식으로 저장한 JSON을 아래 폴더에 넣으세요.</p><code className="mt-2 block break-all rounded-lg bg-[#FFFDF9] p-2 text-[11px] text-[#A94E30]">{video.value.config.workflowDir}</code><p className="mt-2">{`{{prompt}}`} 같은 자리표시자를 넣으면 이 화면에서 프롬프트가 자동으로 주입됩니다.</p><Button variant="secondary" className="mt-3" onClick={()=>void video.reload()}>폴더 확인 · 새로고침</Button></div>}
   </Panel>
   <div className="grid gap-5">
    <Panel className="p-5"><Subheading title="생성 결과" extra={videoJob?<StateTag online={videoJob.status==='completed'}>{videoJob.status}</StateTag>:<span className="text-[11px] text-[#8A8176]">대기</span>}/>{!videoJob?<div className="flex min-h-44 flex-col items-center justify-center gap-2 rounded-xl border border-dashed border-[#D8CFC2] bg-[#FAF7F1] text-[12px] text-[#8A8176]"><Clapperboard size={24}/>영상을 생성하면 여기에 표시됩니다.</div>:<div className="grid gap-3">{!terminal(videoJob.status)&&<><div className="flex justify-between text-[11px] text-[#7C7469]"><span>{videoJob.status==='queued'?'ComfyUI 대기 중':videoJob.status==='downloading'?'결과 저장 중':'생성 중'}</span><span>{videoJob.progress}%</span></div><div className="h-2 overflow-hidden rounded-full bg-[#F3EEE5]"><div className="h-full bg-[#C96442]" style={{width:Math.max(4,videoJob.progress)+'%'}}/></div></>}{videoJob.result&&<video className="max-h-[340px] w-full rounded-xl bg-black" src={videoJob.result.url} controls playsInline/>}{videoJob.status==='completed'&&videoJob.result&&<div className="flex flex-wrap items-center justify-between gap-2"><code className="min-w-0 flex-1 truncate rounded bg-[#FAF7F1] p-2 text-[10px] text-[#5C544A]">{videoJob.result.file}</code><Action onClick={useResult}><Upload size={14}/> 업로드 폼에 채우기</Action></div>}{videoJob.status==='failed'&&<Notice message={videoJob.error||'영상 생성에 실패했습니다.'}/>}</div>}</Panel>
    <Panel className="p-5"><Subheading title="YouTube 업로드" extra={<span className="text-[11px] text-[#8A8176]">{uploadFile?'파일 준비됨':'파일 선택'}</span>}/><div className="grid gap-3"><FormField label="영상 파일"><div className="flex flex-col gap-2 sm:flex-row"><TextInput value={uploadFile} onChange={e=>setUploadFile(e.target.value)} placeholder="생성 결과 또는 로컬 영상 경로"/><Button variant="secondary" onClick={()=>void pick()}>선택</Button></div></FormField><FormField label="제목"><TextInput value={uploadTitle} onChange={e=>setUploadTitle(e.target.value)} placeholder="영상 제목" maxLength={100}/></FormField><FormField label="설명"><TextArea rows={3} value={uploadDescription} onChange={e=>setUploadDescription(e.target.value)} placeholder="설명"/></FormField><div className="grid grid-cols-2 gap-3"><FormField label="공개"><SelectInput value={uploadPrivacy} onChange={e=>setUploadPrivacy(e.target.value)}><option value="private">비공개</option><option value="unlisted">일부 공개</option><option value="public">공개</option></SelectInput></FormField><FormField label="태그"><TextInput value={uploadTags} onChange={e=>setUploadTags(e.target.value)} placeholder="쉼표 구분"/></FormField></div><Action disabled={uploading||!uploadFile.trim()||!uploadTitle.trim()} onClick={()=>void startUpload()}>{uploading?<><LoaderCircle size={15} className="animate-spin"/> 요청 중</>:<><Upload size={15}/> YouTube 업로드</>}</Action>{uploadJob&&<div className="rounded-lg border border-[#E7E0D5] bg-[#FAF7F1] p-3 text-[11px]"><div className="flex justify-between"><span>{uploadJob.status}</span><span>{uploadJob.progress}%</span></div>{uploadJob.result?.url&&<a className="mt-1 inline-flex items-center gap-1 font-semibold text-[#C96442]" href={uploadJob.result.url} target="_blank" rel="noreferrer">YouTube에서 열기 <ExternalLink size={12}/></a>}{uploadJob.error&&<p className="mt-1 text-rose-600">{uploadJob.error}</p>}</div>}</div></Panel>
    <Panel className="p-5"><Subheading title="최근 생성" extra={<span className="text-[11px] text-[#8A8176]">{video.value.jobs.length}개</span>}/>{video.value.jobs.length?<div className="grid gap-2">{video.value.jobs.slice().reverse().map(item=><Button type="button" variant="ghost" key={item.id} onClick={()=>setVideoJob(item)} className="!h-auto !justify-start rounded-xl border border-[#E7E0D5] bg-[#FAF7F1] px-3 py-2 text-left"><div className="flex w-full items-center justify-between gap-2"><strong className="truncate text-[12px] text-[#3A342D]">{item.prompt||item.id.slice(0,8)}</strong><StateTag online={item.status==='completed'}>{item.status}</StateTag></div><small className="text-[10px] text-[#8A8176]">{item.width}×{item.height} · {item.seconds||''}{item.frames?item.frames+'f':''}</small></Button>)}</div>:<p className="text-[12px] text-[#8A8176]">아직 생성한 영상이 없습니다.</p>}</Panel>
   </div>
  </div>
 </div>
}
