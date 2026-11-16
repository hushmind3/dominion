import {useEffect,useState} from 'react'
import {ArrowDown,ArrowRight,ArrowUp,CheckCircle2,Clapperboard,Copy,Film,LoaderCircle,Plus,RefreshCw,Save,ShieldCheck,Sparkles,Trash2,Upload,Wallet,WandSparkles} from 'lucide-react'
import {post} from '../api'
import {useRemote,Notice,Panel,StateTag,FormField} from './UI'
import {Button,SelectInput,TextArea,TextInput} from './Primitives'
import {useJob,jobFinished} from '../hooks/useJob'
import {VideoModelInstall} from './VideoModelInstall'

type Scene={id:string;script:string;visualPrompt:string;overlay:string;seconds:number}
type Project={id:string;title:string;brief:string;status:string;targetSeconds:number;aspectRatio:string;platform:string;engine:string;style:string;audience:string;revision:number;scenes:Scene[];useAI:boolean;planningSource?:string;autoRender:boolean;autoUpload:boolean;planJobId?:string;renderJobId?:string;uploadJobId?:string;planningError?:string;renderError?:string;planAttempts?:number}
type VideoJob={id:string;status:string;title?:string;brief?:string;outputUrl?:string;captionsUrl?:string;progress?:number;sceneIndex?:number;sceneTotal?:number;error?:string;result?:{bytes?:number;segmentsReused?:number;segmentsGenerated?:number;qa?:{containerVerified?:boolean;captionsReady?:boolean;segmentsReused?:number;segmentsGenerated?:number}}}
type Provider={id:string;name:string;model?:string;enabled?:boolean}
type Props={nativeReady:boolean;comfyReady:boolean;onSettings:()=>void;onModelInstalled:()=>void;onPublish:(job:VideoJob)=>void}

const stages=[{key:'planning',name:'기획·대본'},{key:'planned',name:'장면 구성'},{key:'rendering',name:'영상·음성 생성'},{key:'ready',name:'검수·배포'}] as const
const statusText:Record<string,string>={planning:'콘텐츠 기획 중',planned:'대본 편집 가능',rendering:'영상 제작 중',ready:'검수 준비 완료',failed:'작업 오류',queued:'실행 대기'}
const card='rounded-2xl border border-[#E5DED3] bg-[#FFFDF9] p-5 shadow-[0_6px_24px_#2D292306]'
export function VideoAutopilot({nativeReady,comfyReady,onSettings,onModelInstalled,onPublish}:Props){
 const projects=useRemote<Project[]>('/api/video/projects',[],false)
 const providers=useRemote<Provider[]>('/api/providers',[])
 const [chosen,setChosen]=useState('')
 const detail=useRemote<Project|null>(chosen?'/api/video/projects/'+encodeURIComponent(chosen):null,null,false)
 const project=detail.value??projects.value.find(row=>row.id===chosen)
 const render=useJob<VideoJob>(project?.renderJobId?'/api/video/jobs/'+encodeURIComponent(project.renderJobId):null,null)
 const planned=useJob<VideoJob>(project?.planJobId?'/api/jobs/'+encodeURIComponent(project.planJobId):null,null)
 const upload=useJob<VideoJob>(project?.uploadJobId?'/api/youtube/uploads/'+encodeURIComponent(project.uploadJobId):null,null)
 const [goal,setGoal]=useState('')
 const [title,setTitle]=useState('')
 const [audience,setAudience]=useState('일반 시청자')
 const [style,setStyle]=useState('친근한 정보 전달')
 const [platform,setPlatform]=useState('shorts')
 const [seconds,setSeconds]=useState(30)
 const [aspect,setAspect]=useState('9:16')
 const [engine,setEngine]=useState('native')
 const [ai,setAi]=useState(false)
 const [provider,setProvider]=useState('')
 const [autoRender,setAutoRender]=useState(true)
 const [autoUpload,setAutoUpload]=useState(false)
 const [privacy,setPrivacy]=useState('private')
 const [economy,setEconomy]=useState(true)
 const [working,setWorking]=useState(false)
 const [error,setError]=useState('')
 const [draft,setDraft]=useState<Scene[]>([])
 const [draftTitle,setDraftTitle]=useState('')
 const [dirty,setDirty]=useState(false)
 const ready=engine==='native'?nativeReady:comfyReady
 useEffect(()=>{
  if(!chosen&&projects.value.length)setChosen(projects.value[0].id)
 },[chosen,projects.value])
 useEffect(()=>{
  if(!project)return
  setDraft(project.scenes||[])
  setDraftTitle(project.title)
  setDirty(false)
 },[project?.id,project?.revision])
 useEffect(()=>{
  if(!chosen)return
  if(project?.status!=='planning'&&project?.status!=='rendering'&&
   !project?.uploadJobId)return
  const timer=setInterval(()=>{
   void detail.reload()
   void projects.reload()
  },2200)
  return()=>clearInterval(timer)
 },[chosen,project?.status,project?.uploadJobId,detail.reload,projects.reload])
 useEffect(()=>{
  if(render.value&&jobFinished(render.value.status)){
   void detail.reload()
   void projects.reload()
  }
 },[render.value?.status,detail.reload,projects.reload])
 const selectedProvider=providers.value.find(row=>row.id===provider)
 const changedDraft=(index:number,key:keyof Scene,value:string|number)=>{
  setDraft(previous=>previous.map((item,i)=>i===index?{...item,[key]:value}:item))
  setDirty(true)
 }
 const addScene=(after:number)=>{
  if(draft.length>=12)return
  const scene:Scene={id:'draft-'+Math.random().toString(36).slice(2),
   script:'',visualPrompt:'',overlay:'',seconds:5}
  setDraft(previous=>[...previous.slice(0,after+1),scene,...previous.slice(after+1)])
  setDirty(true)
 }
 const duplicateScene=(index:number)=>{
  if(draft.length>=12)return
  setDraft(previous=>{
   const scene=previous[index]
   if(!scene)return previous
   return [...previous.slice(0,index+1),
    {...scene,id:'draft-'+Math.random().toString(36).slice(2)},
    ...previous.slice(index+1)]
  })
  setDirty(true)
 }
 const deleteScene=(index:number)=>{
  if(draft.length<=1)return
  setDraft(previous=>previous.filter((_,i)=>i!==index))
  setDirty(true)
 }
 const move=(index:number,delta:number)=>{
  const next=index+delta
  if(next<0||next>=draft.length)return
  setDraft(previous=>{const copy=[...previous];[copy[index],copy[next]]=[copy[next],copy[index]];return copy})
  setDirty(true)
 }
 const start=async()=>{
  if(!goal.trim()||working)return
  setWorking(true);setError('')
  try{
   const project=await post<Project>('/api/video/projects',{
    brief:goal.trim(),title:title.trim(),audience,style,platform,
    targetSeconds:seconds,aspectRatio:aspect,engine,useAI:ai,
    providerId:ai?provider:'',model:ai?(selectedProvider?.model||''):'',
    autoRender,autoUpload:autoUpload&&autoRender,privacyStatus:privacy,economy
   })
   setChosen(project.id)
   detail.setValue(project)
   await projects.reload()
  }catch(e){setError((e as Error).message)}
  finally{setWorking(false)}
 }
 const save=async()=>{
  if(!project||working)return
  setWorking(true);setError('')
  try{
   const updated=await post<Project>('/api/video/projects/'+encodeURIComponent(project.id)+'/scenes',{
    revision:project.revision,title:draftTitle.trim(),scenes:draft
   })
   detail.setValue(updated);setDirty(false);await projects.reload()
  }catch(e){setError((e as Error).message)}
  finally{setWorking(false)}
 }
 const renderNow=async()=>{
  if(!project||working||dirty)return
  setWorking(true);setError('')
  try{
   const updated=await post<Project>('/api/video/projects/'+encodeURIComponent(project.id)+'/render',{revision:project.revision})
   detail.setValue(updated);await projects.reload()
  }catch(e){setError((e as Error).message)}
  finally{setWorking(false)}
 }
 const retryPlan=async()=>{
  if(!project||working||project.status!=='failed'||project.scenes?.length)return
  setWorking(true);setError('')
  try{
   const updated=await post<Project>('/api/video/projects/'+encodeURIComponent(project.id)+'/retry-plan',
    {revision:project.revision})
   detail.setValue(updated)
   await projects.reload()
  }catch(e){setError((e as Error).message)}
  finally{setWorking(false)}
 }
 const draftSeconds=draft.reduce((sum,scene)=>sum+Number(scene.seconds||0),0)
 const draftClips=project?.engine==='comfyui'?draft.reduce((sum,scene)=>sum+Math.ceil(Number(scene.seconds||0)/5),0):0
 const draftIssue=draft.length>12?'장면은 최대 12개입니다.':
  draft.some(scene=>!scene.script.trim()||!Number.isInteger(scene.seconds)||scene.seconds<1||scene.seconds>180)
   ?'각 장면에 내레이션과 1~180초 길이를 입력하세요.':
  draftSeconds<3||draftSeconds>180?'영상 총 길이는 3~180초여야 합니다.':
  project?.engine==='comfyui'&&draftClips>12?'AI 영상은 최대 12개 클립(각 5초)까지 생성할 수 있습니다.':''
 const active=project?.status==='planning'||project?.status==='rendering'
 return <section className="grid min-w-0 gap-5">
  <div className="overflow-hidden rounded-[26px] bg-[linear-gradient(125deg,#242E37,#354C50_58%,#345748)] p-6 text-white sm:p-8">
   <p className="mb-2 flex items-center gap-2 text-[11px] font-bold tracking-[.16em] text-[#F0B997]"><WandSparkles size={17}/> CONTENT AUTOPILOT</p>
   <div className="flex flex-wrap items-end justify-between gap-5"><div className="max-w-2xl">
    <h2 className="text-[24px] font-bold tracking-tight sm:text-[28px]">주제 하나로, 콘텐츠 제작 전체를</h2>
    <p className="mt-2 text-[12px] leading-6 text-white/75">대본 작성 → 장면별 구성 → 음성·영상 렌더링 → 품질 확인 → 게시. 단계마다 작업 기록이 저장됩니다.</p>
   </div><span className="rounded-full border border-white/20 bg-white/10 px-4 py-2 text-[11px]">Rust 자율 제작 파이프라인</span></div>
   <div className="mt-5 grid grid-cols-2 gap-2 sm:grid-cols-4">{stages.map((stage,index)=><div key={stage.key} className="flex items-center gap-2 rounded-xl border border-white/10 bg-white/5 px-3 py-3"><span className="flex size-6 shrink-0 items-center justify-center rounded-full bg-white/15 text-[10px] font-bold">{index+1}</span><span className="text-[11px]">{stage.name}</span></div>)}</div>
  </div>
  {(error||projects.error||detail.error)&&<Notice message={error||projects.error||detail.error}/>}
  <div className="grid items-start gap-5 xl:grid-cols-[minmax(0,1.1fr)_minmax(370px,1fr)]">
   <div className="space-y-5">
    <div className={card}>
     <div className="mb-4 flex flex-wrap items-center justify-between gap-3"><div><p className="text-[10px] font-bold tracking-[.12em] text-[#C96442]">NEW PROJECT</p><h3 className="mt-1 text-[17px] font-bold">새 콘텐츠 제작</h3></div><span className="rounded-lg bg-[#F6F0E6] px-3 py-1.5 text-[11px] text-[#87786B]">주제 입력 → 자동 기획</span></div>
     <div className="space-y-4">
      <FormField label="무엇을 제작할까요?"><TextArea rows={5} value={goal} onChange={e=>setGoal(e.target.value)} maxLength={16000} placeholder="예: 2026년 AI 로컬 모델의 변화와 실제 비용을 5장면으로 쉽게 설명하는 쇼츠 영상. 처음 3초에 후킹을 넣고 마지막에 요약해 주세요."/></FormField>
      <div className="grid gap-3 sm:grid-cols-2"><FormField label="제목 · 선택"><TextInput value={title} onChange={e=>setTitle(e.target.value)} maxLength={180} placeholder="자동으로 만들 수도 있습니다"/></FormField><FormField label="목표 시청자"><TextInput value={audience} maxLength={180} onChange={e=>setAudience(e.target.value)}/></FormField></div>
      <div className="grid gap-3 sm:grid-cols-2"><FormField label="연출 톤"><SelectInput value={style} onChange={e=>setStyle(e.target.value)}><option>친근한 정보 전달</option><option>뉴스 브리핑</option><option>여행 다큐멘터리</option><option>제품 리뷰</option><option>스토리텔링</option></SelectInput></FormField><FormField label="콘텐츠 목표"><SelectInput value={platform} onChange={e=>{const p=e.target.value;setPlatform(p);setAspect(p==='youtube'?'16:9':'9:16');if(p!=='youtube'&&p!=='shorts')setAutoUpload(false)}}><option value="shorts">YouTube 쇼츠</option><option value="youtube">YouTube 일반 영상</option><option value="reels">Instagram 릴스 (파일)</option><option value="tiktok">TikTok (파일)</option><option value="local">로컬 MP4</option></SelectInput></FormField></div>
      <div className="grid grid-cols-3 gap-3"><FormField label="길이 (초)"><TextInput type="number" min={3} max={engine==='comfyui'?60:180} value={seconds} onChange={e=>setSeconds(Number(e.target.value))}/></FormField><FormField label="화면 비율"><SelectInput value={aspect} onChange={e=>setAspect(e.target.value)}><option value="9:16">9:16 세로</option><option value="16:9">16:9 가로</option><option value="1:1">1:1 정사각</option></SelectInput></FormField><FormField label="렌더링"><SelectInput value={engine} onChange={e=>{const next=e.target.value;setEngine(next);if(next==='comfyui')setSeconds(current=>Math.min(current,60))}}><option value="native">FFmpeg 로컬</option><option value="comfyui">ComfyUI AI</option></SelectInput></FormField></div>
      <VideoModelInstall onChanged={onModelInstalled}/>
      {engine==='comfyui'&&<p className="rounded-xl border border-[#E6D6C8] bg-[#FBF2E9] px-3 py-2 text-[11px] text-[#836B57]">AI 영상은 최대 60초이며, 5초 이하 장면으로 자동 분할해 순서대로 생성·합성합니다. GPU 속도와 VRAM 사용량은 모델에 따라 달라집니다.</p>}
       <div className="rounded-xl border border-[#E8DFD3] bg-[#FBF7F0] p-4">
       <label className="flex cursor-pointer items-start gap-3"><input type="checkbox" checked={ai} onChange={e=>setAi(e.target.checked)} className="mt-1 accent-[#C96442]"/><span><strong className="block text-[12px]">연결된 AI로 기획·대본 생성</strong><small className="mt-1 block text-[11px] text-[#867B6F]">해제하면 로컬 규칙 기반으로 주제를 장면으로 나눕니다. AI 비용을 쓰지 않습니다.</small></span></label>
       {ai&&<FormField label="기획 모델 Provider"><SelectInput value={provider} onChange={e=>setProvider(e.target.value)} className="mt-3"><option value="">연결된 Provider 선택</option>{providers.value.filter(p=>p.enabled!==false).map(p=><option key={p.id} value={p.id}>{p.name||p.id} · {p.model||'기본 모델'}</option>)}</SelectInput></FormField>}
      </div>
      <div className="rounded-xl border border-[#E8DFD3] bg-[#FBF7F0] p-4">
       <label className="flex cursor-pointer items-start gap-3"><input type="checkbox" checked={autoRender} onChange={e=>{setAutoRender(e.target.checked);if(!e.target.checked)setAutoUpload(false)}} className="mt-1 accent-[#C96442]"/><span><strong className="block text-[12px]">기획이 끝나면 자동으로 영상까지 제작</strong><small className="mt-1 block text-[11px] text-[#867B6F]">끄면 장면별 대본을 먼저 검토·수정하고 생성할 수 있습니다.</small></span></label>
       <label className="mt-3 flex cursor-pointer items-start gap-3"><input type="checkbox" checked={economy} onChange={e=>setEconomy(e.target.checked)} className="mt-1 accent-[#C96442]"/><span><strong className="flex items-center gap-1.5 text-[12px]"><Wallet size={13}/> 절약 모드</strong><small className="block text-[11px] text-[#867B6F]">적용 가능한 엔진에서만 연산량을 줄입니다.</small></span></label>
       {(platform==='youtube'||platform==='shorts')&&<div className="mt-3 border-t border-[#E7DED2] pt-3"><label className="flex cursor-pointer items-start gap-3"><input type="checkbox" checked={autoUpload} disabled={!autoRender} onChange={e=>setAutoUpload(e.target.checked)} className="mt-1 accent-[#C96442]"/><span><strong className="block text-[12px]">완성 후 YouTube에 자동 업로드</strong><small className="block text-[11px] text-[#867B6F]">별도 동의가 있어야 실행합니다. 비공개·일부공개만 가능하며 전체 공개는 직접 확인합니다.</small></span></label>{autoUpload&&<FormField label="자동 업로드 공개 범위"><SelectInput className="mt-3" value={privacy} onChange={e=>setPrivacy(e.target.value)}><option value="private">비공개</option><option value="unlisted">일부공개</option></SelectInput></FormField>}</div>}
      </div>
      {autoRender&&!ready&&<div role="status" className="flex items-center justify-between gap-3 rounded-xl border border-amber-200 bg-amber-50 p-3 text-[11px] text-amber-800"><span>선택한 렌더링 엔진이 준비되지 않았습니다. 자동 제작 전에 연결을 확인하세요.</span><Button size="sm" variant="secondary" onClick={onSettings}>설정</Button></div>}
      <Button variant="primary" className="min-h-12 w-full !text-[14px]" disabled={working||!goal.trim()||seconds<3||seconds>(engine==='comfyui'?60:180)||(ai&&!provider)||(autoRender&&!ready)} onClick={()=>void start()}>{working?<><LoaderCircle size={16} className="animate-spin"/> 등록 중</>:<><Sparkles size={17}/> 자율 콘텐츠 제작 시작 <ArrowRight size={16}/></>}</Button>
     </div>
    </div>
    <div className={card}><div className="mb-3 flex items-center justify-between gap-2"><h3 className="text-[14px] font-bold">제작 프로젝트</h3><Button size="sm" variant="ghost" onClick={()=>void projects.reload()}><RefreshCw size={13}/> 갱신</Button></div>
     {projects.value.length===0?<p className="rounded-xl bg-[#F8F4ED] p-4 text-[11px] text-[#8C8174]">아직 프로젝트가 없습니다. 위에서 주제를 입력해 첫 제작을 시작하세요.</p>:
     <div className="max-h-80 space-y-2 overflow-y-auto">{projects.value.map(item=><button key={item.id} type="button" onClick={()=>setChosen(item.id)} className={'flex w-full items-center gap-3 rounded-xl border px-3 py-3 text-left '+(item.id===chosen?'border-[#C96442] bg-[#FFF2E9]':'border-[#ECE6DC] hover:bg-[#FCF8F2]')}><Clapperboard size={17} className="shrink-0 text-[#B46C4B]"/><span className="min-w-0 flex-1"><strong className="block truncate text-[12px]">{item.title}</strong><small className="block text-[10px] text-[#918478]">{statusText[item.status]||item.status} · {item.targetSeconds}초 · v{item.revision}</small></span><ArrowRight size={14} className="text-[#A69A8E]"/></button>)}</div>}
    </div>
   </div>
   <div className="space-y-5">
    {!project?<Panel className="p-8 text-center"><Film size={32} className="mx-auto text-[#B9A18F]"/><h3 className="mt-3 text-[15px] font-bold">제작 컨트롤룸</h3><p className="mt-2 text-[12px] leading-6 text-[#918478]">프로젝트를 시작하면 기획안·장면별 대본·음성/영상 작업·배포 상태가 여기에 표시됩니다.</p></Panel>:<>
     <div className={card}><div className="flex flex-wrap items-start justify-between gap-3"><div className="min-w-0"><p className="mb-1 text-[10px] font-bold text-[#C96442]">PRODUCTION BOARD</p><h3 className="truncate text-[17px] font-bold">{project.title}</h3><p className="mt-1 text-[11px] text-[#918478]">{project.platform} · {project.engine} · 수정 버전 {project.revision}</p></div><StateTag online={project.status==='ready'}>{statusText[project.status]||project.status}</StateTag></div>
      <div className="mt-4 grid grid-cols-4 gap-1.5">{stages.map((step,index)=><div key={step.key} className={'rounded-lg border px-1.5 py-3 text-center text-[10px] '+((project.status==='ready'||(project.status==='rendering'&&index<3)||(project.status==='planned'&&index<2)||(project.status==='planning'&&index===0))?'border-[#D7A387] bg-[#FFF2E9] text-[#A15E41]':'border-[#EDE6DC] bg-[#F9F7F2] text-[#A3998E]')}><span className="block text-[13px] font-bold">{index+1}</span>{step.name}</div>)}</div>
      {project.planningSource&&<p className="mt-3 flex items-center gap-2 text-[11px] text-[#82776B]"><CheckCircle2 size={14} className="text-emerald-700"/> 대본 작성 방식: {project.planningSource==='provider'?'연결된 AI 모델':'오프라인 규칙'}</p>}
      {project.status==='planning'&&<p className="mt-3 flex items-center gap-2 text-[11px] text-[#9B7556]"><LoaderCircle size={14} className="animate-spin"/> {planned.value?.status==='failed'?'기획 작업에서 오류가 발생했습니다.':'대본과 장면을 준비 중입니다.'}</p>}
      {(project.planningError||planned.value?.error||project.renderError)&&<Notice message={project.planningError||planned.value?.error||project.renderError||''}/>}
      {project.status==='failed'&&(!project.scenes?.length)&&<div className="mt-3 flex flex-wrap items-center justify-between gap-3 rounded-xl border border-[#E6D6C8] bg-[#FBF2E9] p-3">
       <p className="max-w-md text-[11px] leading-5 text-[#836B57]">기획이 완료되지 않았습니다. 이전 작업 기록은 보존하고 새 작업으로 다시 실행합니다.{project.useAI?' 연결된 AI를 다시 호출하면 별도 사용료가 발생할 수 있습니다.':''}</p>
       <Button size="sm" variant="primary" disabled={working} onClick={()=>void retryPlan()}>{working?<LoaderCircle size={14} className="animate-spin"/>:<RefreshCw size={14}/>} 기획 다시 실행</Button>
      </div>}
     </div>
     {draft.length>0&&<div className={card}><div className="mb-4 flex flex-wrap items-center justify-between gap-3"><div><h3 className="text-[15px] font-bold">장면별 대본 · 스토리보드</h3><p className="mt-1 text-[11px] text-[#92877A]">수정한 대본은 실제 렌더링과 내레이션에 반영됩니다.</p></div><div className="flex flex-wrap items-center gap-2"><span className="rounded-lg bg-[#F6F1E9] px-2.5 py-1.5 text-[11px]">{draft.length}장면 · {draftSeconds}초</span><Button size="sm" variant="secondary" disabled={active||working||draft.length>=12} onClick={()=>addScene(draft.length-1)}><Plus size={13}/> 장면 추가</Button></div></div>
      <FormField label="영상 제목"><TextInput value={draftTitle} disabled={active} onChange={e=>{setDraftTitle(e.target.value);setDirty(true)}}/></FormField>
      <div className="mt-4 max-h-[680px] space-y-3 overflow-y-auto pr-1">{draft.map((shot,i)=><article key={shot.id} className="rounded-xl border border-[#E8DFD5] bg-[#FDFBF7] p-4"><div className="mb-3 flex items-center justify-between gap-2"><strong className="text-[12px]">장면 {i+1}</strong><div className="flex items-center gap-1"><Button variant="ghost" size="sm" aria-label={'장면 '+(i+1)+' 위로 이동'} disabled={active||i===0} onClick={()=>move(i,-1)}><ArrowUp size={14}/></Button><Button variant="ghost" size="sm" aria-label={'장면 '+(i+1)+' 아래로 이동'} disabled={active||i===draft.length-1} onClick={()=>move(i,1)}><ArrowDown size={14}/></Button><Button variant="ghost" size="sm" aria-label={'장면 '+(i+1)+' 복제'} disabled={active||working||draft.length>=12} onClick={()=>duplicateScene(i)}><Copy size={14}/></Button><Button variant="ghost" size="sm" aria-label={'장면 '+(i+1)+' 삭제'} disabled={active||working||draft.length<=1} onClick={()=>deleteScene(i)}><Trash2 size={14}/></Button></div></div>
       <div className="space-y-3"><FormField label="내레이션 대본"><TextArea rows={3} maxLength={1800} disabled={active} value={shot.script} onChange={e=>changedDraft(i,'script',e.target.value)}/></FormField><FormField label="영상 생성/연출 지시"><TextArea rows={2} maxLength={1800} disabled={active} value={shot.visualPrompt} onChange={e=>changedDraft(i,'visualPrompt',e.target.value)}/></FormField><div className="grid grid-cols-[minmax(0,1fr)_100px] gap-2"><FormField label="화면 문구"><TextInput maxLength={180} disabled={active} value={shot.overlay} onChange={e=>changedDraft(i,'overlay',e.target.value)}/></FormField><FormField label="초"><TextInput type="number" min={1} max={180} disabled={active} value={shot.seconds} onChange={e=>changedDraft(i,'seconds',Number(e.target.value))}/></FormField></div></div>
      </article>)}</div>
      <div className="mt-4 flex flex-wrap gap-2"><Button variant="secondary" disabled={!dirty||working||active||Boolean(draftIssue)} onClick={()=>void save()}><Save size={14}/> 장면 수정 저장</Button><Button variant="primary" disabled={working||dirty||active} onClick={()=>void renderNow()}><Film size={15}/> {project.status==='ready'?'새 버전 렌더링':'이 대본으로 영상 렌더링'}</Button></div>
      {dirty&&<p className="mt-2 text-[11px] text-amber-700">{draftIssue||'변경 내용을 저장한 뒤 렌더링할 수 있습니다.'}</p>}
     </div>}
     {project.renderJobId&&<div className={card}><div className="mb-3 flex items-center justify-between"><h3 className="text-[14px] font-bold">영상 결과 · 품질 검수</h3><StateTag online={render.value?.status==='completed'}>{render.value?.status||'조회 중'}</StateTag></div>
      {render.value?.outputUrl?<div className="overflow-hidden rounded-xl bg-[#1A222A] p-2"><video key={render.value.id} className="mx-auto max-h-[440px] max-w-full rounded-lg" src={render.value.outputUrl} controls playsInline preload="metadata"/></div>:<div className="flex min-h-32 items-center justify-center rounded-xl bg-[#252D37] text-[12px] text-white/75">{render.value?.status==='failed'?'제작 오류':'장면 합성·음성 처리·인코딩 중'}</div>}
      {!jobFinished(render.value?.status)&&<div className="mt-3"><div className="mb-1 flex justify-between text-[11px]"><span>렌더링 진행 상황{render.value?.sceneIndex&&render.value?.sceneTotal?' · 장면 '+render.value.sceneIndex+'/'+render.value.sceneTotal:''}</span><span>{render.value?.progress||0}%</span></div><div className="h-2 rounded-full bg-[#EBE5DD]"><div className="h-full rounded-full bg-[#C96442]" style={{width:Math.max(0,Math.min(100,render.value?.progress||0))+'%'}}/></div></div>}
      {render.value?.result?.segmentsGenerated!==undefined&&<p className="mt-3 rounded-xl border border-[#DCE8D8] bg-[#F3FAF0] p-3 text-[11px] text-[#416D4D]">AI 영상 장면 재사용: <strong>{render.value.result.segmentsReused??0}개</strong> · 새 GPU 생성: <strong>{render.value.result.segmentsGenerated}개</strong>. 동일한 모델·프롬프트·시드·해상도·설정의 검증된 기존 클립만 재사용했습니다.</p>}
      {render.value?.error&&<Notice message={render.value.error}/>}
      {render.value?.status==='completed'&&<div className="mt-3 flex flex-wrap gap-2"><a download href={render.value.outputUrl} className="inline-flex items-center gap-2 rounded-xl bg-[#C96442] px-3.5 py-2 text-[12px] font-semibold text-white"><Upload size={14}/> MP4 저장</a>{render.value.captionsUrl&&<a href={render.value.captionsUrl} download className="inline-flex items-center gap-2 rounded-xl border border-[#E1D9CC] bg-[#FFFDF9] px-3.5 py-2 text-[12px] font-semibold text-[#3A342D]"><Save size={14}/> SRT 자막</a>}
       <a href={'/api/video/jobs/'+encodeURIComponent(render.value.id)+'/production'} download className="inline-flex items-center gap-2 rounded-xl border border-[#E1D9CC] bg-[#FFFDF9] px-3.5 py-2 text-[12px] font-semibold text-[#3A342D]"><Film size={14}/> 제작 기록</a>
       <Button variant="secondary" onClick={()=>onPublish(render.value!)}><ShieldCheck size={14}/> 게시 검토</Button></div>}
      {project.uploadJobId&&<p className="mt-3 text-[11px] text-[#81756A]">자동 업로드: {upload.value?.status||'대기'}{upload.value?.error?' · '+upload.value.error:''}</p>}
     </div>}
    </>}
   </div>
  </div>
 </section>
}
