import {useEffect,useState} from 'react'
import {Check,Clapperboard,ExternalLink,LoaderCircle,Upload} from 'lucide-react'
import {Button,TextInput,TextArea,SelectInput} from '../components/Primitives'
import {Action,FormField,Headline,Notice,Panel,StateTag,Subheading,useRemote} from '../components/UI'
import {get,post} from '../api'

type UploadJob={id:string;status:string;progress:number;sent:number;total:number;title:string;privacyStatus:string;error?:string;result?:{id:string;url:string;title:string;status:string;privacyStatus:string}}
type YoutubeStatus={connected:boolean;uploadAllowed:boolean;account?:{displayName:string;scopes:string}|null;uploads:UploadJob[]}

export function VideoPage({navigate}:{navigate:(page:any)=>void}){
 const status=useRemote<YoutubeStatus>('/api/youtube/status',{connected:false,uploadAllowed:false,account:null,uploads:[]})
 const [file,setFile]=useState(''),[title,setTitle]=useState(''),[description,setDescription]=useState(''),[privacy,setPrivacy]=useState('private'),[tags,setTags]=useState('')
 const [job,setJob]=useState<UploadJob|null>(null),[error,setError]=useState(''),[busy,setBusy]=useState(false)
 const pick=async()=>{
  try{
   const picked=await (window as any).dominionDesktop?.pickVideo?.()
   if(picked){setFile(picked);if(!title)setTitle(String(picked).split(/[\\/]/).pop()?.replace(/\.[^.]+$/,'')||'')}
  }catch(e){setError((e as Error).message)}
 }
 const start=async()=>{
  if(!status.value.uploadAllowed){setError('Google 계정을 YouTube 업로드 권한으로 먼저 연결하세요.');return}
  if(!file.trim()||!title.trim()){setError('영상 파일 경로와 제목을 입력하세요.');return}
  setBusy(true);setError('');
  try{setJob(await post<UploadJob>('/api/youtube/upload',{path:file,title,description,privacyStatus:privacy,tags}));void status.reload()}
  catch(e){setError((e as Error).message)}finally{setBusy(false)}
 }
 useEffect(()=>{
  if(!job||job.status!=='uploading')return
  const timer=setInterval(async()=>{
   try{
    const next=await get<UploadJob>('/api/youtube/uploads/'+job.id)
    setJob(next)
    if(next.status!=='uploading'){void status.reload();clearInterval(timer)}
   }catch{}
  },1500)
  return()=>clearInterval(timer)
 },[job?.id,job?.status])
 return <div className="mx-auto w-full max-w-[1200px] px-5 py-6 sm:px-8 lg:px-6 lg:pb-20">
  <Headline eyebrow="VIDEO WORKSPACE" title="영상" description="로컬 영상을 YouTube에 업로드하고, 연결된 Google 계정으로 채널과 영상을 관리합니다." actions={<Button variant="secondary" onClick={()=>void status.reload()}><Clapperboard size={16}/> 상태 새로고침</Button>}/>
  {status.error&&<Notice message={status.error}/>}
  <Panel className={'mb-5 p-5 '+(status.value.uploadAllowed?'border-emerald-200 bg-emerald-50/60':'border-amber-200 bg-amber-50/60')}>
   <div className="flex flex-wrap items-start justify-between gap-4"><div className="flex min-w-0 items-start gap-3"><span className={'grid size-11 shrink-0 place-items-center rounded-xl '+(status.value.uploadAllowed?'bg-emerald-100 text-emerald-700':'bg-amber-100 text-amber-700')}><Clapperboard size={22}/></span><div><div className="flex flex-wrap items-center gap-2"><strong className="text-[15px] text-[#24211D]">YouTube 업로드</strong><StateTag online={Boolean(status.value.uploadAllowed)}>{status.value.uploadAllowed?'업로드 가능':status.value.connected?'업로드 권한 필요':'Google 연결 필요'}</StateTag></div><p className="mt-1 text-[12px] leading-5 text-[#5C544A]">{status.value.uploadAllowed?'Google 계정 '+status.value.account?.displayName+'의 업로드 권한이 확인되었습니다.':status.value.connected?'업로드 권한이 없는 기존 연결입니다. Google에서 다시 로그인해 주세요.':'연결 · 채널에서 Google OAuth 앱을 설정하고 계정을 로그인하세요.'}</p></div></div><Action variant="secondary" onClick={()=>navigate('capabilities')}>계정 연동 설정 <ExternalLink size={14}/></Action></div>
  </Panel>
  <div className="grid grid-cols-1 items-start gap-5 xl:grid-cols-[minmax(0,1fr)_380px]">
   <Panel className="p-5">
    <Subheading title="새 영상 업로드" extra={<span className="text-[11px] text-[#8A8176]">MP4 · MOV · WEBM · MKV</span>}/>
    <div className="grid gap-4">
     <FormField label="영상 파일" hint="데스크톱 앱에서는 파일 선택 버튼을 사용하세요. 브라우저에서는 게이트웨이 컴퓨터의 절대경로를 입력합니다.">
      <div className="flex flex-col gap-2 sm:flex-row"><TextInput value={file} onChange={e=>setFile(e.target.value)} placeholder="C:\Videos\clip.mp4"/><Button variant="secondary" onClick={()=>void pick()}>파일 선택</Button></div>
     </FormField>
     <FormField label="제목"><TextInput value={title} onChange={e=>setTitle(e.target.value)} placeholder="영상 제목" maxLength={100}/></FormField>
     <FormField label="설명"><TextArea rows={5} value={description} onChange={e=>setDescription(e.target.value)} placeholder="영상 설명" className="min-h-28"/></FormField>
     <div className="grid grid-cols-1 gap-4 sm:grid-cols-2"><FormField label="공개 범위"><SelectInput value={privacy} onChange={e=>setPrivacy(e.target.value)}><option value="private">비공개</option><option value="unlisted">일부 공개</option><option value="public">공개</option></SelectInput></FormField><FormField label="태그"><TextInput value={tags} onChange={e=>setTags(e.target.value)} placeholder="쉼표로 구분"/></FormField></div>
     {error&&<Notice message={error}/>}
     <div className="flex items-center justify-between gap-3"><p className="text-[11px] text-[#8A8176]">업로드 후 YouTube 처리 시간이 걸릴 수 있습니다.</p><Action disabled={busy||!file.trim()||!title.trim()} onClick={()=>void start()}>{busy?<><LoaderCircle size={15} className="animate-spin"/> 요청 중</>:<><Upload size={15}/> 업로드 시작</>}</Action></div>
    </div>
   </Panel>
   <div className="grid gap-5">
    {job&&<Panel className="p-5"><Subheading title="업로드 상태" extra={<StateTag online={job.status==='completed'}>{job.status}</StateTag>}/>{job.status==='uploading'&&<><div className="mb-2 flex justify-between text-[11px] text-[#7C7469]"><span>{job.title}</span><span>{job.progress}%</span></div><div className="h-2 overflow-hidden rounded-full bg-[#F3EEE5]"><div className="h-full bg-[#C96442] transition-all" style={{width:Math.min(100,job.progress)+'%'}}/></div><p className="mt-2 text-[10px] text-[#8A8176]">{Math.round(job.sent/1048576)}MB / {Math.round(job.total/1048576)}MB</p></>}{job.status==='completed'&&job.result&&<div className="grid gap-2 text-[12px] text-[#3A342D]"><p className="flex items-center gap-2 text-emerald-700"><Check size={15}/> 업로드 완료</p><strong>{job.result.title}</strong><a className="inline-flex items-center gap-1 font-semibold text-[#C96442] hover:underline" href={job.result.url} target="_blank" rel="noreferrer">YouTube에서 열기 <ExternalLink size={13}/></a></div>}{job.status==='failed'&&<Notice message={job.error||'업로드에 실패했습니다.'}/>}</Panel>}
    <Panel className="p-5"><Subheading title="최근 업로드" extra={<span className="text-[11px] text-[#8A8176]">{status.value.uploads.length}개</span>}/>{status.value.uploads.length?<div className="grid gap-2">{status.value.uploads.slice().reverse().map(item=><div key={item.id} className="rounded-xl border border-[#E7E0D5] bg-[#FAF7F1] px-3 py-2.5"><div className="flex items-center justify-between gap-2"><strong className="min-w-0 truncate text-[12px] text-[#3A342D]">{item.title||item.id.slice(0,8)}</strong><StateTag online={item.status==='completed'}>{item.status}</StateTag></div>{item.result?.url&&<a className="mt-1 inline-flex items-center gap-1 text-[11px] font-semibold text-[#C96442] hover:underline" href={item.result.url} target="_blank" rel="noreferrer">YouTube <ExternalLink size={12}/></a>}{item.error&&<p className="mt-1 text-[10px] text-rose-600">{item.error}</p>}</div>)}</div>:<p className="text-[12px] text-[#8A8176]">아직 업로드한 영상이 없습니다.</p>}</Panel>
   </div>
  </div>
 </div>
}
