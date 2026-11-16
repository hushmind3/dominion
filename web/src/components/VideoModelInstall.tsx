import {useEffect,useState} from 'react'
import {ArrowDownToLine,CheckCircle2,FolderOpen,HardDrive,LoaderCircle,RefreshCw,Server,Wifi,WifiOff} from 'lucide-react'
import {post} from '../api'
import {Button,SelectInput,TextInput} from './Primitives'
import {FormField,Notice,StateTag,useRemote} from './UI'

type VideoModel={id:string;name:string;kind:string;available:boolean;missingFiles:number;hint:string}
type Catalog={videoModels:VideoModel[];selectedVideoModel?:string;modelsDir?:string;modelsDirReady?:boolean;comfyReachable?:boolean;comfyUrl?:string}
type Download={id:string;model:string;status:string;progress:number;bytes:number;total:number;error?:string}
const DEFAULT_MODEL='wan21-t2v-1.3b-fp16'
const fmt=(n=0)=>n>=1024**3?(n/1024**3).toFixed(2)+' GB':n>=1024**2?(n/1024**2).toFixed(1)+' MB':Math.round(n/1024)+' KB'

export function VideoModelInstall({onChanged}:{onChanged:()=>void}){
 const catalog=useRemote<Catalog>('/api/video/catalog',{videoModels:[]},false)
 const downloads=useRemote<{downloads:Download[]}>('/api/video/models/downloads',{downloads:[]},false)
 const [chosen,setChosen]=useState(DEFAULT_MODEL)
 const [folder,setFolder]=useState('')
 const [folderDirty,setFolderDirty]=useState(false)
 const [url,setUrl]=useState('http://127.0.0.1:8188')
 const [busy,setBusy]=useState(false)
 const [error,setError]=useState('')
 const [notice,setNotice]=useState('')
 const model=catalog.value.videoModels.find(m=>m.id===chosen)
 const transfer=downloads.value.downloads.find(row=>row.model===chosen)
 const installed=Boolean(model?.available)
 const selected=installed&&catalog.value.selectedVideoModel===chosen
 const server=Boolean(catalog.value.comfyReachable)
 const ready=selected&&server
 useEffect(()=>{
  if(catalog.value.selectedVideoModel&&catalog.value.videoModels.some(m=>m.id===catalog.value.selectedVideoModel))
   setChosen(catalog.value.selectedVideoModel)
 },[catalog.value.selectedVideoModel,catalog.value.videoModels.length])
 useEffect(()=>{
  if(!folderDirty&&catalog.value.modelsDirReady&&catalog.value.modelsDir)
   setFolder(catalog.value.modelsDir)
 },[catalog.value.modelsDir,catalog.value.modelsDirReady,folderDirty])
 useEffect(()=>{
  if(catalog.value.comfyUrl)setUrl(catalog.value.comfyUrl)
 },[catalog.value.comfyUrl])
 useEffect(()=>{
  if(transfer?.status!=='downloading')return
  const timer=window.setInterval(()=>{
   void Promise.all([downloads.reload(),catalog.reload()])
  },2500)
  return()=>window.clearInterval(timer)
 },[transfer?.status,downloads.reload,catalog.reload])
 useEffect(()=>{
  if(transfer?.status==='completed'){
   void catalog.reload()
   onChanged()
  }
 },[transfer?.status,catalog.reload,onChanged])
 const chooseFolder=async()=>{
  setError('')
  try{
   const value=await post<{path?:string;cancelled?:boolean}>('/api/system/pick-folder',{})
   if(value.path){setFolder(value.path);setFolderDirty(true)}
  }catch(e){setError((e as Error).message)}
 }
 const setup=async()=>{
  if(busy||!chosen)return
  setBusy(true);setError('');setNotice('')
  try{
   const result=await post<{status:string;modelsDir?:string}>(
    '/api/video/models/'+encodeURIComponent(chosen)+'/setup',
    {modelsDir:folder.trim(),comfyUrl:url.trim()}
   )
   if(result.modelsDir){setFolder(result.modelsDir);setFolderDirty(false)}
   setNotice(result.status==='ready'
    ?'모델 파일이 준비됐습니다. ComfyUI 연결 상태를 확인하세요.'
    :'필요한 영상 모델 파일 다운로드를 시작했습니다. 중단 후 다시 누르면 이어받기를 시도합니다.')
   await Promise.all([catalog.reload(),downloads.reload()])
   onChanged()
  }catch(e){setError((e as Error).message)}
  finally{setBusy(false)}
 }
 const refresh=()=>void Promise.all([catalog.reload(),downloads.reload()]).then(onChanged)
 return <section className="mt-4 rounded-2xl border-2 border-[#D6A17E] bg-[#FFF8F0] p-4 sm:p-5" aria-label="AI 영상 모델 원클릭 설치">
  <div className="flex flex-wrap items-start justify-between gap-3">
   <div><p className="text-[10px] font-bold tracking-[.12em] text-[#B66846]">AI MODEL SETUP</p>
    <h4 className="mt-1 flex items-center gap-2 text-[15px] font-bold"><HardDrive size={18}/> 영상 모델 다운로드·설치</h4>
    <p className="mt-1 text-[11px] leading-5 text-[#7F7368]">내 컴퓨터의 ComfyUI 모델 폴더에 다운로드하고 자율 제작에 연결합니다.</p>
   </div>
   <StateTag online={ready}>{ready?'모델·서버 준비':'설치·연결 점검'}</StateTag>
  </div>
  <div className="mt-4 grid gap-3 sm:grid-cols-2">
   <FormField label="영상 모델">
    <SelectInput value={chosen} onChange={e=>setChosen(e.target.value)}>
     {catalog.value.videoModels.length===0&&<option value={DEFAULT_MODEL}>Wan2.1 1.3B FP16</option>}
     {catalog.value.videoModels.map(m=><option key={m.id} value={m.id}>{m.name}{m.available?' · 설치됨':' · 다운로드 가능'}</option>)}
    </SelectInput>
   </FormField>
   <div className="flex items-center justify-between rounded-xl border border-[#EBE1D6] bg-white px-3 py-2.5">
    <span><strong className="block text-[12px]">{model?.kind==='gguf'?'저메모리 4비트':'표준 모델 · 우선 추천'}</strong>
    <small className="mt-1 block text-[10px] text-[#918477]">{model?.kind==='gguf'?'ComfyUI-GGUF 확장 필요':'기본 ComfyUI 노드 사용'}</small></span>
    {installed?<CheckCircle2 size={19} className="text-emerald-700"/>:<ArrowDownToLine size={19} className="text-[#BE714B]"/>}
   </div>
  </div>
  <div className="mt-3 space-y-3">
   <FormField label="ComfyUI models 폴더" hint="ComfyUI를 실제 실행하는 설치 폴더 아래의 models를 선택하세요. 다른 폴더에 다운로드하면 ComfyUI가 모델을 읽지 못합니다.">
    <div className="flex gap-2"><TextInput value={folder} onChange={e=>{setFolder(e.target.value);setFolderDirty(true)}} placeholder="예: C:\ComfyUI_windows_portable\ComfyUI\models"/>
     <Button size="sm" variant="secondary" onClick={()=>void chooseFolder()}><FolderOpen size={15}/> 폴더 선택</Button></div>
   </FormField>
   <FormField label="ComfyUI 주소">
    <TextInput value={url} onChange={e=>setUrl(e.target.value)} placeholder="http://127.0.0.1:8188"/>
   </FormField>
  </div>
  {model?.kind==='gguf'&&<p className="mt-3 text-[11px] leading-5 text-amber-900">4비트 모델은 <a className="font-semibold underline" href="https://github.com/city96/ComfyUI-GGUF" target="_blank" rel="noreferrer">ComfyUI-GGUF 확장</a>이 설치되어야 실행됩니다. 확장 없이 시작하려면 FP16을 선택하세요.</p>}
  <div className="mt-4 flex flex-wrap items-center justify-between gap-3">
   <div className="flex items-center gap-2 text-[11px] text-[#85796B]">{server?<Wifi size={15} className="text-emerald-700"/>:<WifiOff size={15} className="text-amber-600"/>}{server?'ComfyUI 연결됨':'ComfyUI 실행·연결 필요'} <span className="text-[#AEA49A]">·</span> {model?.hint||'모델 조회 중'}</div>
   <div className="flex gap-2"><Button size="sm" variant="ghost" onClick={refresh}><RefreshCw size={14}/> 상태 확인</Button>
    <Button size="sm" variant="primary" disabled={busy||transfer?.status==='downloading'||!chosen} onClick={()=>void setup()}>
     {busy?<><LoaderCircle size={15} className="animate-spin"/> 연결 중</>:selected?'설정 다시 확인':installed?'모델 연결하기':<><ArrowDownToLine size={15}/> 선택·다운로드·설치</>}</Button></div>
  </div>
  {transfer&&<div className="mt-4 rounded-xl border border-[#E8DBCB] bg-white p-3" aria-live="polite">
   <div className="mb-2 flex justify-between gap-2 text-[11px]">
    <span className="font-semibold">{transfer.status==='downloading'?'모델 다운로드 진행 중':transfer.status==='completed'?'모델 파일 다운로드 완료':transfer.status==='failed'?'다운로드 실패':transfer.status==='interrupted'?'다운로드 중단 · 이어받기 가능':transfer.status}</span>
    <span className="tabular-nums text-[#8E8174]">{Math.round(transfer.progress||0)}% · {fmt(transfer.bytes)}{transfer.total?' / '+fmt(transfer.total):''}</span>
   </div>
   <div role="progressbar" aria-label="영상 모델 다운로드" aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.max(0,Math.min(100,transfer.progress||0))} className="h-2 overflow-hidden rounded-full bg-[#EEE8DF]"><div className="h-full rounded-full bg-[#C96442] transition-[width]" style={{width:Math.max(0,Math.min(100,transfer.progress||0))+'%'}}/></div>
   {transfer.error&&<p className="mt-2 text-[11px] text-rose-700">{transfer.error}</p>}
  </div>}
  {notice&&<p role="status" className="mt-3 flex items-center gap-2 text-[11px] text-emerald-800"><CheckCircle2 size={14}/>{notice}</p>}
  {error&&<Notice message={error}/>}
  {!server&&<p className="mt-3 flex items-center gap-2 text-[11px] text-[#8F7766]"><Server size={14}/> 모델 가중치 다운로드와 ComfyUI 서버 실행은 별개입니다. 먼저 <a href="https://github.com/Comfy-Org/ComfyUI" target="_blank" rel="noreferrer" className="underline">ComfyUI</a>를 실행하면 연결 상태를 다시 확인합니다.</p>}
 </section>
}
