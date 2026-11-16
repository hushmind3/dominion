import {useEffect,useState} from 'react'
import {ArrowDownToLine,CheckCircle2,ExternalLink,HardDrive,LoaderCircle,RefreshCw,Search} from 'lucide-react'
import {get,post} from '../api'
import {Button,TextInput} from './Primitives'
import {Notice,useRemote} from './UI'

type HubModel={id:string;name:string;family:string;task:string;downloads?:number;gated?:boolean;url:string}
type HubWeight={path:string;size:number;format:string;quantization:string;sha256?:string}
type RepoFiles={repo:string;files:HubWeight[];gated?:boolean|string;license?:string}
type Transfer={id:string;repo?:string;path?:string;status:string;progress?:number;bytes?:number;total?:number;error?:string}
const FEATURED:HubModel[]=[
 {id:'Wan-AI/Wan2.2-TI2V-5B',name:'Wan 2.2 · TI2V 5B',family:'Wan',task:'text-to-video',url:'https://huggingface.co/Wan-AI/Wan2.2-TI2V-5B'},
 {id:'Lightricks/LTX-Video',name:'LTX-Video',family:'LTX',task:'text-to-video',url:'https://huggingface.co/Lightricks/LTX-Video'},
 {id:'Lightricks/LTX-2.3',name:'LTX-2.3',family:'LTX',task:'text-to-video',url:'https://huggingface.co/Lightricks/LTX-2.3'},
 {id:'tencent/HunyuanVideo',name:'HunyuanVideo',family:'Hunyuan',task:'text-to-video',url:'https://huggingface.co/tencent/HunyuanVideo'},
 {id:'THUDM/CogVideoX-5b',name:'CogVideoX 5B',family:'CogVideoX',task:'text-to-video',url:'https://huggingface.co/THUDM/CogVideoX-5b'},
 {id:'genmo/mochi-1-preview',name:'Mochi 1',family:'Mochi',task:'text-to-video',url:'https://huggingface.co/genmo/mochi-1-preview'}
]
const fmt=(n=0)=>n>=1e9?(n/1e9).toFixed(2)+' GB':(n/1e6).toFixed(0)+' MB'
const finished=(status?:string)=>status==='completed'||status==='failed'||status==='interrupted'

export function VideoHubBrowser(){
 const [search,setSearch]=useState('')
 const [models,setModels]=useState<HubModel[]>(FEATURED)
 const [repo,setRepo]=useState('')
 const [file,setFile]=useState('')
 const [listing,setListing]=useState<RepoFiles|null>(null)
 const [loading,setLoading]=useState(false)
 const [searching,setSearching]=useState(false)
 const [busy,setBusy]=useState(false)
 const [error,setError]=useState('')
 const [notice,setNotice]=useState('')
 const downloads=useRemote<{downloads:Transfer[]}>('/api/video/models/downloads',{downloads:[]},false)
 const transfer=downloads.value.downloads.find(row=>row.repo===repo&&row.path===file)
 const selectedFile=listing?.files.find(item=>item.path===file)
 const hasActive=downloads.value.downloads.some(item=>item.repo&&item.status==='downloading')
 useEffect(()=>{
  if(!hasActive)return
  const id=window.setInterval(()=>{void downloads.reload()},2500)
  return()=>window.clearInterval(id)
 },[hasActive,downloads.reload])
 const browse=async(query:string)=>{
  setSearching(true);setError('');setNotice('');setRepo('');setListing(null);setFile('')
  try{
   const result=await get<{models:HubModel[]}>('/api/video/hub/search?q='+encodeURIComponent(query.trim()))
   setModels(result.models)
  }catch(e){setError((e as Error).message)}
  finally{setSearching(false)}
 }
 const choose=async(id:string)=>{
  setRepo(id);setFile('');setListing(null);setLoading(true);setError('');setNotice('')
  try{
   const result=await get<RepoFiles>('/api/video/hub/files?repo='+encodeURIComponent(id))
   setListing(result)
   setFile(result.files[0]?.path||'')
   await downloads.reload()
  }catch(e){setError((e as Error).message)}
  finally{setLoading(false)}
 }
 const download=async()=>{
  if(!repo||!file||busy)return
  setBusy(true);setError('');setNotice('')
  try{
   const result=await post<{status:string}>('/api/video/hub/download',{repo,path:file})
   setNotice(result.status==='completed'?'이미 다운로드된 모델 파일입니다.':'선택한 영상 모델 파일의 다운로드를 시작했습니다.')
   await downloads.reload()
  }catch(e){setError((e as Error).message)}
  finally{setBusy(false)}
 }
 const gated=listing?.gated!==undefined&&listing.gated!==false
 return <section className="rounded-2xl border border-[#DCD2C7] bg-[#FFFDF9] p-4 sm:p-5" aria-label="다양한 영상 AI 모델 검색 및 다운로드">
  <div className="flex flex-wrap items-start justify-between gap-3">
   <div><p className="text-[10px] font-bold tracking-[.12em] text-[#B46A43]">VIDEO MODEL DISCOVERY</p>
    <h4 className="mt-1 text-[15px] font-bold">다른 영상 모델 찾기</h4>
    <p className="mt-1 text-[11px] text-[#87786C]">Wan뿐 아니라 LTX, Hunyuan, CogVideoX, Mochi 등 공개 모델을 직접 찾아 다운로드하세요.</p>
   </div>
   <span className="rounded-lg bg-[#F4EEE6] px-2 py-1 text-[10px] text-[#836D5C]">Hugging Face 실시간 검색</span>
  </div>
  <div className="mt-3 flex gap-2">
   <div className="min-w-0 flex-1"><TextInput aria-label="영상 모델 이름 검색" value={search} placeholder="예: LTX, Wan2.2, Hunyuan, CogVideoX"
    onChange={e=>setSearch(e.target.value)} onKeyDown={e=>{if(e.key==='Enter')void browse(search)}}/></div>
   <Button variant="primary" size="sm" disabled={searching} onClick={()=>void browse(search)}>
    {searching?<LoaderCircle size={14} className="animate-spin"/>:<Search size={14}/>} 검색</Button>
  </div>
  <div className="mt-3 grid max-h-52 grid-cols-2 gap-2 overflow-y-auto sm:grid-cols-3">
   {models.map(model=><button type="button" key={model.id} disabled={loading} onClick={()=>void choose(model.id)}
    className={'min-w-0 rounded-xl border p-3 text-left '+(repo===model.id?'border-[#B97755] bg-[#FFF1E5]':'border-[#E8E0D7] bg-white hover:border-[#C7AA92]')}>
    <strong className="block truncate text-[12px]">{model.name}</strong>
    <span className="mt-1 block truncate text-[10px] text-[#85796F]">{model.family} · {model.id.split('/')[0]}</span>
    <span className="mt-1 block text-[10px] text-[#8F8072]">{model.downloads?'다운로드 '+model.downloads.toLocaleString():'파일 목록 보기'}</span>
   </button>)}
  </div>
  {!searching&&!models.length&&<p className="mt-3 text-[11px] text-[#826F5D]">해당 검색어의 영상 모델이 없습니다. 다른 모델명으로 검색해 보세요.</p>}
  {loading&&<p className="mt-3 flex items-center gap-2 text-[11px]"><LoaderCircle size={14} className="animate-spin"/> Hugging Face에서 실제 파일과 용량을 조회하는 중입니다.</p>}
  {listing&&!loading&&<div className="mt-4 rounded-xl border border-[#E4D9CF] bg-[#FAF6F0] p-3">
   <div className="flex flex-wrap items-center justify-between gap-2">
    <strong className="max-w-full truncate text-[12px]">{listing.repo}</strong>
    <a href={'https://huggingface.co/'+listing.repo} target="_blank" rel="noreferrer" className="inline-flex items-center gap-1 text-[11px] text-[#925F43] underline">모델 원본 <ExternalLink size={12}/></a>
   </div>
   {listing.license&&<p className="mt-1 text-[10px] text-[#86766A]">라이선스: {listing.license}</p>}
   {gated&&<p className="mt-2 text-[11px] text-amber-800">접근 승인 모델입니다. 원본 저장소에서 이용 조건을 확인해야 합니다.</p>}
   <p className="mt-2 text-[11px] text-[#85776A]">실제 등록된 가중치 파일 중 하나를 선택하세요. 양자화와 파일 크기는 저장소 정보를 기준으로 표시합니다.</p>
   <div className="mt-2 max-h-52 space-y-1.5 overflow-y-auto">
    {listing.files.map(weight=><label key={weight.path} className={'flex cursor-pointer items-start gap-2 rounded-lg border px-2.5 py-2 '+(file===weight.path?'border-[#CD9D80] bg-white':'border-[#E5DCD2] bg-[#FFFEFC]')}>
     <input type="radio" name="hub-video-weight" value={weight.path} checked={file===weight.path}
      onChange={()=>{setFile(weight.path);setNotice('')}} className="mt-1 accent-[#C96442]"/>
     <span className="min-w-0 flex-1"><span className="block break-all text-[11px]">{weight.path}</span>
      <span className="mt-0.5 block text-[10px] font-semibold text-[#846B5C]">{weight.quantization} · {fmt(weight.size)}</span>
     </span>
    </label>)}
   </div>
   {!listing.files.length&&<p className="mt-2 text-[11px] text-amber-800">직접 다운로드 가능한 GGUF/Safetensors 가중치가 목록에 없습니다.</p>}
   <div className="mt-3 flex flex-wrap items-center justify-between gap-2">
    <span className="text-[11px] text-[#86796B]">{selectedFile?'선택 파일 '+fmt(selectedFile.size):'가중치를 선택하세요'}</span>
    <Button variant="primary" size="sm" disabled={!selectedFile||gated||busy||transfer?.status==='downloading'} onClick={()=>void download()}>
     {busy?<LoaderCircle size={14} className="animate-spin"/>:transfer?.status==='completed'?<CheckCircle2 size={14}/>:<ArrowDownToLine size={14}/>}
     {transfer?.status==='completed'?'다운로드 완료':transfer?.status==='interrupted'?'이어받기':'선택 파일 다운로드'}
    </Button>
   </div>
   {transfer&&<div className="mt-3" aria-live="polite">
    <div className="mb-1 flex justify-between text-[10px]"><span>{transfer.status==='downloading'?'다운로드 중':finished(transfer.status)?transfer.status:'대기'}</span>
     <span>{Math.round(transfer.progress||0)}% · {fmt(transfer.bytes||0)} / {fmt(transfer.total||selectedFile?.size||0)}</span></div>
    <div className="h-2 overflow-hidden rounded-full bg-[#E7DDD1]"><div className="h-full bg-[#B87452]" style={{width:Math.min(100,transfer.progress||0)+'%'}}/></div>
    {transfer.error&&<p className="mt-1 text-[11px] text-rose-700">{transfer.error}</p>}
   </div>}
  </div>}
  <p className="mt-3 flex items-start gap-1 text-[10px] leading-5 text-[#897C70]"><HardDrive size={13} className="mt-0.5 shrink-0"/>
   이 목록은 공개 영상 가중치를 찾아 저장하는 기능입니다. 다중 파일 모델은 추가 파일이 필요할 수 있고, 생성 실행에는 해당 모델용 워크플로·GPU 환경이 필요합니다. 다운로드만으로 즉시 생성 가능하다고 표시하지 않습니다.</p>
  {notice&&<p className="mt-2 text-[11px] text-emerald-800">{notice}</p>}
  {error&&<Notice message={error}/>}
  <div className="mt-2 flex justify-end"><Button variant="ghost" size="sm" onClick={()=>void downloads.reload()}><RefreshCw size={13}/> 다운로드 상태 새로고침</Button></div>
 </section>
}
