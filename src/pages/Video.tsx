import {useEffect,useState} from 'react'
import {Check,Clapperboard,ExternalLink,LoaderCircle,Play,Power,RefreshCw,Upload} from 'lucide-react'
import {Button,TextInput,TextArea,SelectInput} from '../components/Primitives'
import {Action,FormField,Headline,Notice,Panel,StateTag,Subheading,useRemote} from '../components/UI'
import {get,post} from '../api'

type Workflow={id:string;name:string;bytes:number}
type VideoJob={id:string;status:string;progress:number;title?:string;prompt:string;width:number;height:number;frames:number;fps:number;seconds?:number;seed:number;error?:string;result?:{file:string;filename:string;url:string}}
type VideoModel={type:string;name:string;present:boolean;path:string;sizeBytes?:number;download?:string}
type VideoStatus={config:{comfyUrl:string;comfyDir:string;workflowDir:string;outputDir:string};comfy:{reachable:boolean;error?:string};installed?:boolean;workflows:Workflow[];models?:VideoModel[];modelsReady?:boolean;jobs:VideoJob[]}
type UploadJob={id:string;status:string;progress:number;sent:number;total:number;title:string;privacyStatus:string;error?:string;result?:{id:string;url:string;title:string;status:string;privacyStatus:string}}
type YoutubeStatus={connected:boolean;uploadAllowed:boolean;account?:{displayName:string;scopes:string}|null;uploads:UploadJob[]}
const ratios={'9:16':{width:360,height:640,label:'쇼츠 · 9:16'},'16:9':{width:640,height:360,label:'가로 · 16:9'},'1:1':{width:480,height:480,label:'정사각 · 1:1'}} as const
const terminal=(status:string)=>['completed','failed','cancelled'].includes(status)
const statusLabel=(status:string)=>status==='completed'?'완료':status==='running'?'생성 중':status==='queued'?'대기':status==='downloading'?'저장 중':status==='failed'?'실패':status
const fmtBytes=(bytes=0)=>bytes>=1024**3?(bytes/1024**3).toFixed(1)+' GB':bytes>=1024**2?Math.round(bytes/1024**2)+' MB':'0 MB'

export function VideoPage({navigate}:{navigate:(page:any)=>void}){
 const video=useRemote<VideoStatus>('/api/video/status',{config:{comfyUrl:'',comfyDir:'',workflowDir:'',outputDir:''},comfy:{reachable:false},workflows:[],models:[],jobs:[]})
 const youtube=useRemote<YoutubeStatus>('/api/youtube/status',{connected:false,uploadAllowed:false,account:null,uploads:[]})
 const [workflow,setWorkflow]=useState(''),[title,setTitle]=useState(''),[prompt,setPrompt]=useState(''),[negative,setNegative]=useState(''),[ratio,setRatio]=useState<keyof typeof ratios>('9:16'),[seconds,setSeconds]=useState(2),[fps,setFps]=useState(16),[seed,setSeed]=useState('-1')
 const [comfyUrl,setComfyUrl]=useState(''),[comfyDir,setComfyDir]=useState(''),[workflowDir,setWorkflowDir]=useState(''),[outputDir,setOutputDir]=useState(''),[showPaths,setShowPaths]=useState(false),[savingConfig,setSavingConfig]=useState(false),[starting,setStarting]=useState(false),[stopping,setStopping]=useState(false)
 const [videoJob,setVideoJob]=useState<VideoJob|null>(null),[generating,setGenerating]=useState(false),[error,setError]=useState('')
 const [uploadFile,setUploadFile]=useState(''),[uploadTitle,setUploadTitle]=useState(''),[uploadDescription,setUploadDescription]=useState(''),[uploadPrivacy,setUploadPrivacy]=useState('private'),[uploadTags,setUploadTags]=useState('')
 const [uploadJob,setUploadJob]=useState<UploadJob|null>(null),[uploading,setUploading]=useState(false)
 const modelTotal=(video.value.models||[]).reduce((sum,model)=>sum+(model.sizeBytes||0),0)
 useEffect(()=>{const c=video.value.config;if(c.comfyUrl)setComfyUrl(c.comfyUrl);if(c.comfyDir)setComfyDir(c.comfyDir);if(c.workflowDir)setWorkflowDir(c.workflowDir);if(c.outputDir)setOutputDir(c.outputDir)},[video.value.config.comfyUrl,video.value.config.comfyDir,video.value.config.workflowDir,video.value.config.outputDir])
 useEffect(()=>{if(!workflow&&video.value.workflows[0])setWorkflow(video.value.workflows[0].id)},[video.value.workflows,workflow])
 useEffect(()=>{if(videoJob)return;const latest=[...video.value.jobs].reverse().find(job=>job.status==='completed');if(latest)setVideoJob(latest)},[video.value.jobs])
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
 const saveComfy=async()=>{setSavingConfig(true);setError('');try{await post('/api/video/config',{comfyUrl,comfyDir,workflowDir,outputDir});await video.reload()}catch(e){setError((e as Error).message)}finally{setSavingConfig(false)}}
 const startComfy=async()=>{setStarting(true);setError('');try{await post('/api/video/start',{});setTimeout(()=>{void video.reload();setStarting(false)},5000)}catch(e){setError((e as Error).message);setStarting(false)}}
 const stopComfy=async()=>{setStopping(true);setError('');try{await post('/api/video/stop',{});await video.reload()}catch(e){setError((e as Error).message)}finally{setStopping(false)}}
 const reveal=async(file:string)=>{
  const bridge=(window as any).dominionDesktop
  try{
   if(bridge?.showItem){const result=await bridge.showItem(file);if(result&&!result.ok)setError(result.error||'폴더를 열 수 없습니다.');return}
   await post('/api/video/reveal',{file})
  }catch(e){setError((e as Error).message)}
 }
 const openModels=async()=>{
  const bridge=(window as any).dominionDesktop
  try{
   if(bridge?.openPath){const result=await bridge.openPath(video.value.config.comfyDir.replace(/\\+$/,'')+'/models');if(result&&!result.ok)setError(result.error||'모델 폴더를 열 수 없습니다.');return}
   await post('/api/video/open-models',{})
  }catch(e){setError((e as Error).message)}
 }
 const generate=async()=>{
  if(!workflow){setError('영상 모델(워크플로)을 선택하세요.');return}
  if(!prompt.trim()){setError('영상 프롬프트를 입력하세요.');return}
  setGenerating(true);setError('');setUploadFile('');setUploadTitle('');setUploadDescription('');
  try{
   const dimensions=ratios[ratio]
   setVideoJob(await post<VideoJob>('/api/video/generate',{workflow,title,prompt,negative,width:dimensions.width,height:dimensions.height,seconds,fps,seed:Number(seed)}))
  }catch(e){setError((e as Error).message)}finally{setGenerating(false)}
 }
 const useResult=()=>{
  if(!videoJob?.result)return
  setUploadFile(videoJob.result.file)
  setUploadTitle((videoJob.title||videoJob.prompt).slice(0,90))
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
 return <div className="mx-auto w-full max-w-[1400px] overflow-x-hidden px-5 py-4 sm:px-7 lg:px-6 lg:pb-8">
  <Headline eyebrow="VIDEO PRODUCTION" title="영상 제작" description="영상 모델(ComfyUI 워크플로)을 선택해 쇼츠를 생성하고 YouTube에 올립니다. 채팅 모델과는 별개입니다." actions={<Button variant="secondary" onClick={()=>{void video.reload();void youtube.reload()}}><RefreshCw size={16}/> 상태 새로고침</Button>}/>
  {error&&<Notice message={error}/>}
  <div className="mb-4 grid grid-cols-2 gap-3">
   <Panel className={'p-4 '+(video.value.comfy.reachable?'border-emerald-200 bg-emerald-50/50':'border-amber-200 bg-amber-50/50')}>
    <div className="flex items-center justify-between gap-3"><div className="flex items-center gap-2"><Clapperboard size={18} className={video.value.comfy.reachable?'text-emerald-700':'text-amber-700'}/><strong className="text-[13px] text-[#24211D]">ComfyUI 영상 엔진</strong></div><StateTag online={video.value.comfy.reachable}>{video.value.comfy.reachable?'실행 중':'꺼짐'}</StateTag></div>
    <p className="mt-2 text-[11px] leading-5 text-[#5C544A]">{video.value.comfy.reachable?'영상 생성할 때만 켜두면 됩니다. 앱을 닫아도 이 엔진은 따로 실행되므로 아래 버튼으로 종료하세요.':'영상 생성할 때만 시작하세요. 앱과 별도 프로세스라 켜져 있는 동안 GPU 메모리를 사용합니다. 이 PC의 ComfyUI 폴더는 약 14.5GB를 사용하며, 필요 없으면 폴더째 삭제해도 앱의 다른 기능은 그대로 동작합니다.'}</p>
    {video.value.comfy.reachable
     ? <div className="mt-2 flex flex-wrap items-center gap-2"><Button variant="secondary" disabled={stopping} onClick={()=>void stopComfy()}><Power size={14}/> {stopping?'종료 중':'ComfyUI 종료'}</Button><span className="text-[10px] leading-4 text-[#8A8176]">종료하면 VRAM이 바로 반환됩니다.</span></div>
     : <div className="mt-2 flex flex-wrap gap-2"><TextInput value={comfyUrl} onChange={e=>setComfyUrl(e.target.value)} placeholder="http://127.0.0.1:8188"/><Button variant="secondary" disabled={savingConfig||!comfyUrl.trim()} onClick={()=>void saveComfy()}>{savingConfig?'저장 중':'저장'}</Button>{video.value.installed&&<Button variant="primary" disabled={starting} onClick={()=>void startComfy()}>{starting?'시작 중':'ComfyUI 시작'}</Button>}</div>}
   </Panel>
   <div className="col-span-2 -mt-1">
    <Button type="button" variant="ghost" onClick={()=>setShowPaths(v=>!v)} className="!h-7 !px-2 !text-[11px] text-[#5C544A]">{showPaths?'▾':'▸'} 폴더 직접 지정</Button>
    {showPaths&&<Panel className="mt-2 grid gap-2 p-4">
     <FormField label="ComfyUI 설치 폴더" hint="main.py와 .venv가 있는 폴더"><TextInput value={comfyDir} onChange={e=>setComfyDir(e.target.value)} placeholder="~/ComfyUI"/></FormField>
     <FormField label="워크플로 폴더" hint="API 형식 JSON을 넣는 곳"><TextInput value={workflowDir} onChange={e=>setWorkflowDir(e.target.value)}/></FormField>
     <FormField label="출력 폴더" hint="생성된 영상 저장 위치"><TextInput value={outputDir} onChange={e=>setOutputDir(e.target.value)}/></FormField>
     <div className="flex justify-end"><Button variant="secondary" disabled={savingConfig} onClick={()=>void saveComfy()}>{savingConfig?'저장 중':'폴더 저장'}</Button></div>
    </Panel>}
   </div>
   <Panel className={'p-4 '+(youtube.value.uploadAllowed?'border-emerald-200 bg-emerald-50/50':'border-amber-200 bg-amber-50/50')}><div className="flex items-center justify-between gap-3"><div className="flex items-center gap-2"><Upload size={18} className={youtube.value.uploadAllowed?'text-emerald-700':'text-amber-700'}/><strong className="text-[13px] text-[#24211D]">YouTube 업로드</strong></div><StateTag online={youtube.value.uploadAllowed}>{youtube.value.uploadAllowed?'준비됨':youtube.value.connected?'권한 필요':'연결 필요'}</StateTag></div><p className="mt-2 text-[11px] leading-5 text-[#5C544A]">{youtube.value.uploadAllowed?'계정 '+youtube.value.account?.displayName+'으로 업로드할 수 있습니다.':youtube.value.connected?'업로드 권한으로 Google에 다시 로그인하세요.':'Google OAuth 앱과 계정을 먼저 연결하세요.'}</p><Button variant="ghost" className="mt-2 !px-0 !text-[11px] text-[#C96442]" onClick={()=>navigate('capabilities')}>계정 연동 설정 <ExternalLink size={13}/></Button></Panel>
  </div>
  <div className="grid grid-cols-[minmax(0,1fr)_280px] items-start gap-4">
   <div className="grid min-w-0 gap-4">
    <Panel className="min-w-0 p-5">
     <Subheading title="영상 생성" extra={<span className="text-[11px] text-[#8A8176]">{video.value.workflows.length}개 워크플로</span>}/>
     {video.value.workflows.length?<div className="grid gap-4">
      <FormField label="영상 모델 · 워크플로"><SelectInput value={workflow} onChange={e=>setWorkflow(e.target.value)}>{video.value.workflows.map(item=><option key={item.id} value={item.id}>{item.name}</option>)}</SelectInput></FormField>
      <FormField label="영상 제목" hint="이 제목이 생성 파일 이름과 YouTube 제목 기본값이 됩니다."><TextInput value={title} onChange={e=>setTitle(e.target.value)} placeholder="예: 비 오는 서울 골목 산책"/></FormField>
      <FormField label="프롬프트"><TextArea rows={4} value={prompt} onChange={e=>setPrompt(e.target.value)} placeholder="예: 비 오는 서울 골목, 시네마틱, 천천히 걷는 고양이"/></FormField>
      <FormField label="네거티브 프롬프트" hint="영상에 나오면 안 되는 요소를 적습니다. 예: blurry, watermark, low quality, text"><TextInput value={negative} onChange={e=>setNegative(e.target.value)} placeholder="low quality, blurry, watermark"/></FormField>
      <div className="grid grid-cols-2 gap-3 sm:grid-cols-3"><FormField label="화면 비율" hint={`${ratios[ratio].width}×${ratios[ratio].height}px`}><SelectInput value={ratio} onChange={e=>setRatio(e.target.value as keyof typeof ratios)}>{(Object.keys(ratios) as Array<keyof typeof ratios>).map(key=><option key={key} value={key}>{ratios[key].label}</option>)}</SelectInput></FormField><FormField label="길이(초)" hint="최대 9초"><TextInput type="number" min={1} max={9} value={seconds} onChange={e=>setSeconds(Number(e.target.value))}/></FormField><FormField label="FPS" hint="초당 프레임 수. 높을수록 부드럽지만 느립니다."><TextInput type="number" min={1} max={60} value={fps} onChange={e=>setFps(Number(e.target.value))}/></FormField></div>
      <FormField label="시드" hint="같은 숫자를 넣으면 같은 영상이 다시 나옵니다. -1은 생성할 때마다 새 랜덤 값."><div className="flex gap-2"><TextInput value={seed} onChange={e=>setSeed(e.target.value)} placeholder="-1 (랜덤)"/><Button variant="secondary" className="shrink-0" onClick={()=>setSeed('-1')}>랜덤</Button></div></FormField>
      <div className="flex flex-wrap items-center justify-between gap-3"><p className="text-[11px] leading-5 text-[#8A8176]">워크플로에 {`{{prompt}}`} {`{{width}}`} {`{{height}}`} {`{{frames}}`} {`{{fps}}`} {`{{seed}}`} 자리표시자가 필요합니다.</p><Action disabled={generating||!video.value.comfy.reachable||!workflow||!prompt.trim()} onClick={()=>void generate()}>{generating?<><LoaderCircle size={15} className="animate-spin"/> 요청 중</>:<><Play size={15}/> 영상 생성</>}</Action></div>
     </div>:<div className="rounded-xl border border-dashed border-[#D8CFC2] bg-[#FAF7F1] p-5 text-[12px] leading-6 text-[#5C544A]"><strong className="text-[#3A342D]">영상 워크플로가 없습니다.</strong><p className="mt-1">ComfyUI에서 Wan·HunyuanVideo·LTX 계열 워크플로를 API 형식으로 저장한 JSON을 아래 폴더에 넣으세요.</p><code className="mt-2 block break-all rounded-lg bg-[#FFFDF9] p-2 text-[11px] text-[#A94E30]">{video.value.config.workflowDir}</code><Button variant="secondary" className="mt-3" onClick={()=>void video.reload()}>폴더 확인 · 새로고침</Button></div>}
    </Panel>
    <Panel className="min-w-0 p-5">
     <Subheading title="영상 모델 파일" extra={<StateTag online={Boolean(video.value.modelsReady)}>{video.value.modelsReady?'준비됨':(video.value.models||[]).length?'확인 필요':'워크플로 필요'}</StateTag>}/>
     <p className="text-[11px] leading-5 text-[#5C544A]">ComfyUI가 아래 폴더의 모델 파일을 자동으로 인식합니다. 워크플로가 참조하는 파일 이름과 같아야 합니다.</p>
     <code className="mt-2 block break-all rounded-lg bg-[#FAF7F1] p-2 text-[11px] text-[#A94E30]">{video.value.config.comfyDir.replace(/\\+$/,'')+'/models'}</code>
     {(video.value.models||[]).length?<div className="mt-3 grid gap-2">{video.value.models!.map(model=><div key={model.type+model.name} className="flex flex-wrap items-center justify-between gap-2 rounded-xl border border-[#E7E0D5] bg-[#FAF7F1] px-3 py-2"><div className="min-w-0"><strong className="block break-all text-[11px] text-[#3A342D]">{model.name}</strong><small className="text-[10px] text-[#8A8176]">{model.type} · {model.present?'있음':'없음'}{model.present&&model.sizeBytes?' · '+fmtBytes(model.sizeBytes):''}</small></div>{model.present?<Check size={15} className="text-emerald-600"/>:model.download?<a className="inline-flex items-center gap-1 text-[11px] font-semibold text-[#C96442] hover:underline" href={model.download} target="_blank" rel="noreferrer">다운로드 <ExternalLink size={12}/></a>:<span className="text-[10px] text-amber-700">파일 필요</span>}</div>)}</div>:<p className="mt-3 text-[11px] text-[#8A8176]">워크플로를 추가하면 필요한 모델 파일 목록이 여기에 표시됩니다.</p>}
     <p className="mt-2 text-[10px] leading-4 text-[#8A8176]">모델 파일 합계 {fmtBytes(modelTotal)} · Python·PyTorch가 포함된 ComfyUI 설치 폴더는 모델 외에 약 5GB를 더 사용합니다. 앱에는 포함되지 않으며 이 PC에만 있습니다.</p>
     <div className="mt-3 flex flex-wrap gap-2"><Button variant="secondary" onClick={()=>void openModels()}>모델 폴더 열기</Button><Button variant="ghost" onClick={()=>void video.reload()}>다시 확인</Button></div>
    </Panel>
    <Panel className="min-w-0 p-5">
     <Subheading title="최근 생성" extra={<span className="text-[11px] text-[#8A8176]">{video.value.jobs.length}개</span>}/>
     {video.value.jobs.length?<div className="grid gap-2">{video.value.jobs.slice().reverse().map(item=><Button type="button" variant="ghost" key={item.id} onClick={()=>setVideoJob(item)} className="!h-auto !justify-start rounded-xl border border-[#E7E0D5] bg-[#FAF7F1] px-3 py-2.5 text-left"><div className="flex w-full items-center justify-between gap-2"><strong className="truncate text-[12px] text-[#3A342D]">{item.title||item.prompt||item.id.slice(0,8)}</strong><StateTag online={item.status==='completed'}>{statusLabel(item.status)}</StateTag></div><small className="text-[10px] text-[#8A8176]">{item.width}×{item.height}{item.seconds?' · '+item.seconds+'s':''}{item.frames?' · '+item.frames+'f':''}</small></Button>)}</div>:<p className="text-[12px] text-[#8A8176]">아직 생성한 영상이 없습니다.</p>}
    </Panel>
   </div>
   <div className="grid min-w-0 gap-4">
    <Panel className="min-w-0 p-5"><Subheading title="생성 결과" extra={videoJob?<StateTag online={videoJob.status==='completed'}>{statusLabel(videoJob.status)}</StateTag>:<span className="text-[11px] text-[#8A8176]">대기</span>}/>{!videoJob?<div className="flex min-h-48 flex-col items-center justify-center gap-2 rounded-xl border border-dashed border-[#D8CFC2] bg-[#FAF7F1] text-[12px] text-[#8A8176]"><Clapperboard size={24}/>영상을 생성하면 여기에 표시됩니다.</div>:<div className="grid gap-3">{!terminal(videoJob.status)&&<><div className="flex justify-between text-[11px] text-[#7C7469]"><span>{videoJob.status==='queued'?'ComfyUI 대기 중':videoJob.status==='downloading'?'결과 저장 중':'생성 중'}</span><span>{videoJob.progress}%</span></div><div className="h-2 overflow-hidden rounded-full bg-[#F3EEE5]"><div className="h-full bg-[#C96442]" style={{width:Math.max(4,videoJob.progress)+'%'}}/></div></>}{videoJob.result&&<div className="flex justify-center rounded-xl bg-black p-2"><video className="max-h-[520px] w-auto max-w-full rounded-lg" src={videoJob.result.url} controls playsInline/></div>}{videoJob.status==='completed'&&videoJob.result&&<div className="grid gap-2"><div className="flex min-w-0 items-center gap-2 rounded bg-[#FAF7F1] p-2"><span className="min-w-0 flex-1 truncate text-[11px] text-[#5C544A]" title={videoJob.result.file}>{videoJob.result.filename}</span><Button variant="ghost" className="!px-2 !py-1 !text-[10px]" onClick={()=>void reveal(videoJob.result!.file)}>폴더 열기</Button></div><Action onClick={useResult} className="w-full"><Upload size={14}/> 업로드 폼에 채우기</Action></div>}{videoJob.status==='failed'&&<Notice message={videoJob.error||'영상 생성에 실패했습니다.'}/>}</div>}</Panel>
    <Panel className="min-w-0 p-5"><Subheading title="YouTube 업로드" extra={<span className="text-[11px] text-[#8A8176]">{uploadFile?'파일 준비됨':'파일 선택'}</span>}/><div className="grid gap-3"><FormField label="영상 파일"><div className="flex flex-col gap-2"><TextInput value={uploadFile} onChange={e=>setUploadFile(e.target.value)} placeholder="생성 결과 또는 로컬 영상 경로"/><Button variant="secondary" className="shrink-0 whitespace-nowrap" onClick={()=>void pick()}>파일 선택</Button></div></FormField><FormField label="제목"><TextInput value={uploadTitle} onChange={e=>setUploadTitle(e.target.value)} placeholder="영상 제목" maxLength={100}/></FormField><FormField label="설명"><TextArea rows={3} value={uploadDescription} onChange={e=>setUploadDescription(e.target.value)} placeholder="설명"/></FormField><div className="grid grid-cols-2 gap-3"><FormField label="공개"><SelectInput value={uploadPrivacy} onChange={e=>setUploadPrivacy(e.target.value)}><option value="private">비공개</option><option value="unlisted">일부 공개</option><option value="public">공개</option></SelectInput></FormField><FormField label="태그"><TextInput value={uploadTags} onChange={e=>setUploadTags(e.target.value)} placeholder="쉼표 구분"/></FormField></div><Action disabled={uploading||!uploadFile.trim()||!uploadTitle.trim()} onClick={()=>void startUpload()} className="w-full">{uploading?<><LoaderCircle size={15} className="animate-spin"/> 요청 중</>:<><Upload size={15}/> YouTube 업로드</>}</Action>{uploadJob&&<div className="rounded-lg border border-[#E7E0D5] bg-[#FAF7F1] p-3 text-[11px]"><div className="flex justify-between"><span>{uploadJob.status}</span><span>{uploadJob.progress}%</span></div>{uploadJob.result?.url&&<a className="mt-1 inline-flex items-center gap-1 font-semibold text-[#C96442]" href={uploadJob.result.url} target="_blank" rel="noreferrer">YouTube에서 열기 <ExternalLink size={12}/></a>}{uploadJob.error&&<p className="mt-1 text-rose-600">{uploadJob.error}</p>}</div>}</div></Panel>
   </div>
  </div>
 </div>
}
