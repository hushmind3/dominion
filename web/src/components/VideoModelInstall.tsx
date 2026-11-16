import {useEffect,useState} from 'react'
import {ArrowDownToLine,CheckCircle2,ChevronDown,FolderOpen,HardDrive,LoaderCircle,RefreshCw,Server,Wifi,WifiOff} from 'lucide-react'
import {post} from '../api'
import {Button,TextInput} from './Primitives'
import {Notice,StateTag,useRemote} from './UI'
import {VideoHubBrowser} from './VideoHubBrowser'

type VideoModel={
 id:string;name:string;family?:string;task?:string;kind:string;
 quantization?:string;weightBytes?:number;totalBytes?:number;
 available:boolean;missingFiles:number;hint:string;notes?:string
}
type Catalog={
 videoModels:VideoModel[];selectedVideoModel?:string;modelsDir?:string;
 comfyReachable?:boolean;comfyAvailable?:boolean;comfyUrl?:string;missingNodes?:string[]
}
type Download={id:string;model:string;status:string;progress:number;bytes:number;total:number;error?:string}
const DEFAULT_MODEL='wan21-t2v-1.3b-q4'
const gb=(bytes=0)=>bytes>=1e9?(bytes/1e9).toFixed(2)+' GB':(bytes/1e6).toFixed(0)+' MB'
const recommendation=(quant?:string)=>quant==='Q4_K_M'?'용량·품질 균형':quant==='Q3_K_M'?'공간 절약':quant==='Q5_K_M'?'고화질 양자화':quant==='Q8_0'?'대용량 양자화':'고정밀'

export function VideoModelInstall({onChanged}:{onChanged:()=>void}){
 const catalog=useRemote<Catalog>('/api/video/catalog',{videoModels:[]},false)
 const downloads=useRemote<{downloads:Download[]}>('/api/video/models/downloads',{downloads:[]},false)
 const [chosen,setChosen]=useState(DEFAULT_MODEL)
 const [busy,setBusy]=useState(false)
 const [starting,setStarting]=useState(false)
 const [folder,setFolder]=useState('')
 const [url,setUrl]=useState('http://127.0.0.1:8188')
 const [error,setError]=useState('')
 const [notice,setNotice]=useState('')
 const model=catalog.value.videoModels.find(item=>item.id===chosen)
 const transfer=downloads.value.downloads.find(row=>row.model===chosen)
 const installed=Boolean(model?.available)
 const selected=installed&&catalog.value.selectedVideoModel===chosen
 const server=Boolean(catalog.value.comfyReachable)
 const ready=selected&&Boolean(catalog.value.comfyAvailable)
 useEffect(()=>{
  const active=catalog.value.selectedVideoModel
  if(active&&catalog.value.videoModels.some(item=>item.id===active))setChosen(active)
 },[catalog.value.selectedVideoModel,catalog.value.videoModels.length])
 useEffect(()=>{if(catalog.value.modelsDir)setFolder(catalog.value.modelsDir)},[catalog.value.modelsDir])
 useEffect(()=>{if(catalog.value.comfyUrl)setUrl(catalog.value.comfyUrl)},[catalog.value.comfyUrl])
 useEffect(()=>{
  if(transfer?.status!=='downloading')return
  const timer=window.setInterval(()=>{void Promise.all([downloads.reload(),catalog.reload()])},2500)
  return()=>window.clearInterval(timer)
 },[transfer?.status,downloads.reload,catalog.reload])
 useEffect(()=>{
  if(transfer?.status==='completed'){void catalog.reload();onChanged()}
 },[transfer?.status,catalog.reload,onChanged])
 useEffect(()=>{
  if(!starting)return
  let count=0
  const timer=window.setInterval(()=>{
   count+=1
   void catalog.reload()
   onChanged()
   if(count>=40)setStarting(false)
  },2500)
  return()=>window.clearInterval(timer)
 },[starting,catalog.reload,onChanged])
 useEffect(()=>{if(starting&&server)setStarting(false)},[server,starting])
 const refresh=()=>void Promise.all([catalog.reload(),downloads.reload()]).then(onChanged)
 const setup=async()=>{
  if(busy||!chosen)return
  setBusy(true);setError('');setNotice('')
  try{
   // Download only must never overwrite a running video engine or a user's
   // imported ComfyUI workflow. Activation is a separate explicit choice.
   const endpoint='/api/video/models/'+encodeURIComponent(chosen)
    +(installed?'/setup':'/download')
   const result=await post<{status:string;modelsDir?:string}>(endpoint,{})
   setNotice(installed?'선택한 영상 모델을 생성 엔진에 연결했습니다.':'선택한 양자화 모델과 필수 구성 파일 다운로드를 시작했습니다.')
   await Promise.all([catalog.reload(),downloads.reload()])
   onChanged()
  }catch(e){setError((e as Error).message)}
  finally{setBusy(false)}
 }
 const chooseFolder=async()=>{
  setError('')
  try{
   const result=await post<{path?:string;cancelled?:boolean}>('/api/system/pick-folder',{})
   if(result.path)setFolder(result.path)
  }catch(e){setError((e as Error).message)}
 }
 const advancedSetup=async()=>{
  if(busy)return
  setBusy(true);setError('')
  try{
   await post('/api/video/models/'+encodeURIComponent(chosen)+'/setup',
    {modelsDir:folder.trim(),comfyUrl:url.trim()})
   await Promise.all([catalog.reload(),downloads.reload()])
   onChanged()
  }catch(e){setError((e as Error).message)}
  finally{setBusy(false)}
 }
 const launch=async()=>{
  if(starting)return
  setStarting(true);setError('')
  try{
   await post('/api/video/comfy/start',{})
   setNotice('연결된 ComfyUI 실행 상태를 확인 중입니다.')
   await catalog.reload()
   onChanged()
  }catch(e){setError((e as Error).message);setStarting(false)}
 }
 return <div className="space-y-4">
 <VideoHubBrowser/>
 <section className="rounded-2xl border border-[#E0D6CA] bg-[#FFFDF9] p-4 sm:p-5" aria-label="AI 영상 모델 원클릭 설치">
  <div className="flex flex-wrap items-start justify-between gap-3">
   <div><p className="text-[10px] font-bold uppercase tracking-[.12em] text-[#AF7657]">VIDEO MODEL LIBRARY</p>
    <h3 className="mt-1 flex items-center gap-2 text-[15px] font-bold"><HardDrive size={18}/> 바로 연결 가능한 기본 모델 · Wan2.1</h3>
    <p className="mt-1 text-[11px] text-[#84786D]">위에서 다양한 모델을 검색할 수 있습니다. 이 영역은 도미니온 내장 워크플로가 있는 Wan2.1 계열의 양자화 선택입니다.</p>
   </div>
   <StateTag online={ready}>{ready?'생성 준비 완료':installed?'모델 다운로드 완료':'모델 선택'}</StateTag>
  </div>
  <div className="mt-4 flex items-center justify-between gap-2 border-b border-[#EEE6DE] pb-2">
   <strong className="text-[12px]">Wan2.1 · Text-to-Video · 1.3B</strong>
   <span className="text-[10px] text-[#87796D]">양자화별 용량 비교</span>
  </div>
  <div className="mt-2 grid gap-2 sm:grid-cols-2" role="radiogroup" aria-label="영상 모델 양자화 선택">
   {(catalog.value.videoModels.length?catalog.value.videoModels:[]).filter(item=>item.task==='text-to-video').map(item=>{
    const active=item.id===chosen
    return <label key={item.id} className={'flex cursor-pointer items-start gap-2.5 rounded-xl border px-3 py-3 '+(active?'border-[#C88763] bg-[#FFF4E8]':'border-[#E7DFD5] bg-white hover:border-[#D2BCA9]')}>
     <input type="radio" name="video-quantization" value={item.id} checked={active}
      onChange={()=>{setChosen(item.id);setNotice('');setError('')}} className="mt-1 accent-[#C96442]"/>
     <span className="min-w-0 flex-1"><span className="flex flex-wrap items-center gap-2">
      <strong className="text-[12px]">{item.quantization||item.name}</strong>
      {item.available&&<CheckCircle2 size={14} className="text-emerald-700"/>}
      {item.quantization==='Q4_K_M'&&<span className="rounded bg-[#F5E1D2] px-1.5 py-0.5 text-[9px] text-[#91573F]">추천</span>}
     </span><small className="mt-1 block text-[10px] text-[#81776C]">{recommendation(item.quantization)} · 본체 {gb(item.weightBytes)}</small>
     <small className="mt-0.5 block text-[10px] font-semibold text-[#514B43]">필수 파일 전체 약 {gb(item.totalBytes)}</small>
     </span>
    </label>
   })}
  </div>
  {!catalog.value.videoModels.length&&<p className="mt-3 text-[11px] text-[#8B796C]">영상 전용 모델 목록을 불러오는 중입니다.</p>}
  <p className="mt-3 text-[10px] leading-5 text-[#887A6E]">전체 용량에는 영상 모델 본체·텍스트 인코더·VAE가 포함됩니다. 이미 받은 공통 파일은 다시 받지 않습니다. 디스크 용량과 실행 VRAM은 다릅니다.</p>
  <div className="mt-3 flex flex-wrap items-center justify-between gap-3">
   <span className="text-[11px] text-[#82776D]">{model?.hint||'모델을 선택하세요'}</span>
   <div className="flex gap-2">
    <Button size="sm" variant="ghost" onClick={refresh}><RefreshCw size={14}/> 갱신</Button>
    <Button size="sm" variant="primary" onClick={()=>void setup()}
     disabled={busy||transfer?.status==='downloading'||!model}>
     {busy?<><LoaderCircle size={14} className="animate-spin"/> 처리 중</>:
      selected?'다운로드 완료':installed?'이 모델 사용':<><ArrowDownToLine size={14}/> {transfer?.status==='interrupted'?'이어받기':'다운로드'}</>}
    </Button>
   </div>
  </div>
  {transfer&&<div className="mt-3 rounded-lg border border-[#E6DED5] bg-white p-3" aria-live="polite">
   <div className="mb-2 flex justify-between gap-2 text-[11px]"><strong>
    {transfer.status==='downloading'?'다운로드 중':transfer.status==='completed'?'다운로드 완료':transfer.status==='failed'?'다운로드 오류':transfer.status==='interrupted'?'중단됨 · 이어받기 가능':transfer.status}
   </strong><span className="tabular-nums">{Math.round(transfer.progress||0)}% · {gb(transfer.bytes)}{transfer.total?' / '+gb(transfer.total):''}</span></div>
   <div role="progressbar" aria-label="영상 모델 다운로드" aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.max(0,Math.min(100,transfer.progress||0))} className="h-2 overflow-hidden rounded-full bg-[#EDE6DC]">
    <div className="h-full rounded-full bg-[#C96442]" style={{width:Math.max(0,Math.min(100,transfer.progress||0))+'%'}}/></div>
   {transfer.error&&<p className="mt-2 text-[11px] text-rose-700">{transfer.error}</p>}
  </div>}
  {notice&&<p role="status" className="mt-3 text-[11px] text-emerald-800">{notice}</p>}
  {error&&<Notice message={error}/>}
  <details className="mt-4 border-t border-[#EEE5DA] pt-3">
   <summary className="flex cursor-pointer items-center gap-1 text-[11px] text-[#7E756A]"><ChevronDown size={13}/> 고급 설정 · 기존 ComfyUI 연결</summary>
   <p className="mt-3 text-[11px] text-[#897B6F]">모델 다운로드에는 이 설정이 필요하지 않습니다. 영상 생성 실행기가 따로 있는 경우에만 사용하세요.</p>
   <div className="mt-3 space-y-3">
    <label className="block text-[11px] font-semibold">모델 저장 경로 변경<div className="mt-1 flex gap-2"><TextInput value={folder} onChange={e=>setFolder(e.target.value)}/><Button size="sm" variant="secondary" onClick={()=>void chooseFolder()}><FolderOpen size={14}/> 폴더</Button></div></label>
    <label className="block text-[11px] font-semibold">ComfyUI 주소<TextInput value={url} onChange={e=>setUrl(e.target.value)} className="mt-1"/></label>
    <div className="flex flex-wrap gap-2"><Button size="sm" variant="secondary" onClick={()=>void advancedSetup()} disabled={busy}>연결 설정 저장</Button>
     <Button size="sm" variant="secondary" onClick={()=>void launch()} disabled={starting}>{starting?<LoaderCircle size={14} className="animate-spin"/>:<Server size={14}/>} ComfyUI 실행</Button></div>
    <p className="flex items-center gap-1 text-[11px] text-[#897C6D]">{server?<Wifi size={13}/>:<WifiOff size={13}/>}
     {server?'실행기 연결됨':'실행기 미연결'}{catalog.value.missingNodes?.length?' · 누락 노드: '+catalog.value.missingNodes.join(', '):''}</p>
   </div>
  </details>
 </section>
 </div>
}
