import {ArrowDownToLine,ArrowRight,Check,LoaderCircle,Pause,Play,Square} from 'lucide-react'
import {Button,SelectInput} from '../../components/Primitives'
import {Notice} from '../../components/UI'
import {t,useLocale} from '../../i18n'
import {translateError} from '../../i18n/errors'

export type DownloadOption={id:string;format:'gguf'|'mlx';label:string;quantization:string;bytes:number;files:string[];shards:number}
type DownloadJob={id:string;status:string;downloaded:number;total:number;current:string;error:string}
const size=(bytes:number)=>bytes>=1073741824?(bytes/1073741824).toFixed(1)+' GB':bytes>=1048576?(bytes/1048576).toFixed(0)+' MB':bytes>0?Math.ceil(bytes/1024)+' KB':'—'

type Props={options:DownloadOption[];selected:string;onSelect:(id:string)=>void;gated:boolean;repository:string;job?:DownloadJob;installed?:{id:string};busy:boolean;loading:boolean;running:boolean;autoLaunch:boolean;error:string;onDownload:(launch:boolean)=>void;onLoad:()=>void;onChat?:()=>void;onJob:(action:'pause'|'resume'|'cancel')=>void}

/** File choice, download command and receipt stay together throughout the job. */
export function ModelDownloadPanel({options,selected,onSelect,gated,repository,job,installed,busy,loading,running,autoLaunch,error,onDownload,onLoad,onChat,onJob}:Props){
 const en=useLocale()==='en',tr=(ko:string,english:string)=>en?english:ko
 const option=options.find(option=>option.id===selected)
 const active=job&&['resolving','downloading'].includes(job.status)
 const resumable=job&&['paused','failed','cancelled'].includes(job.status)
 const percent=job?.total?Math.min(100,Math.floor(job.downloaded/job.total*100)):0
 const status=job?({resolving:tr('파일 확인 중','Resolving files'),downloading:tr('다운로드 중','Downloading'),paused:tr('일시정지','Paused'),failed:tr('다운로드 실패','Download failed'),cancelled:tr('취소됨','Cancelled'),completed:tr('다운로드 완료','Download complete')}[job.status]||job.status):''
 return <section aria-label={tr('모델 파일 다운로드','Download model files')} className="my-4 grid gap-3 rounded-2xl border border-divider bg-surface p-4">
  <label className="grid gap-2 text-sm font-semibold">{t('models.downloadFiles')}
   <SelectInput disabled={busy||!options.length} value={option?.id||''} onChange={event=>onSelect(event.target.value)}>
    {!options.length&&<option value="">{tr('다운로드 가능한 파일 없음','No downloadable files')}</option>}
    {options.map(option=><option key={option.id} value={option.id}>{option.label} · {option.format.toUpperCase()} · {size(option.bytes)}{option.shards>1?' · '+tr(option.shards+'개 파일',option.shards+' files'):''}</option>)}
   </SelectInput>
  </label>
  {option&&<div className="flex flex-wrap items-center justify-between gap-2 text-xs text-muted"><span className="min-w-0 break-all">{option.format.toUpperCase()}{option.quantization?' · '+option.quantization:''}</span><strong className="text-ink">{size(option.bytes)}</strong></div>}
  {!options.length&&<p className="text-sm leading-6 text-muted">{tr('이 형식으로 받을 수 있는 파일이 없습니다. 다른 형식이나 모델을 선택하세요.','No files are available in this format. Choose another format or model.')}</p>}
  <div className="flex flex-wrap items-center gap-2">
   {installed?<><Button variant="primary" disabled={busy||loading||running} onClick={onLoad}><Play size={16}/>{running?t('models.inUse'):loading?t('models.loadingModel'):t('models.run')}</Button>{onChat&&<Button onClick={onChat}><ArrowRight size={16}/>{t('models.chat')}</Button>}</>:<>
    <Button variant="primary" disabled={!option||busy||Boolean(active)||gated} onClick={()=>onDownload(false)}>{busy?<LoaderCircle size={16} className="animate-spin"/>:<ArrowDownToLine size={16}/>} {busy?t('models.requesting'):active?t('models.downloading'):resumable?t('models.resume'):t('models.downloadStart')}</Button>
    <Button variant="secondary" disabled={!option||busy||gated||autoLaunch} onClick={()=>onDownload(true)}><Play size={16}/>{autoLaunch?tr('다운로드 후 실행 대기','Waiting to run'):tr('받고 실행','Download & run')}</Button>
   </>}
  </div>
  {gated&&!installed&&<p className="text-sm leading-6 text-warning">{t('models.approval')} <a href={'https://huggingface.co/'+repository} target="_blank" rel="noreferrer" className="font-semibold underline">{tr('모델 접근 권한 확인','Check model access')}</a></p>}
  <Notice message={error}/>
  {job&&<div role="status" aria-live="polite" className="grid gap-2 border-t border-divider pt-3">
   <div className="flex flex-wrap justify-between gap-2 text-xs"><strong className="flex items-center gap-2">{job.status==='completed'&&<Check size={15} className="text-success"/>}{status}</strong><span className="text-muted">{size(job.downloaded)}{job.total?' / '+size(job.total)+' · '+percent+'%':''}</span></div>
   <div role="progressbar" aria-label={tr('다운로드 진행률','Download progress')} aria-valuemin={0} aria-valuemax={100} aria-valuenow={job.total?percent:undefined} className="h-2 overflow-hidden rounded-full bg-canvas"><div className={'h-full bg-brand '+(!job.total&&active?'w-1/3 animate-pulse':'')} style={job.total?{width:percent+'%'}:undefined}/></div>
   {job.current&&<p className="truncate text-xs text-muted">{job.current}</p>}
   {job.error&&<p role="alert" className="text-sm text-danger">{translateError(job.error)}</p>}
   {(active||job.status==='paused')&&<div className="flex flex-wrap gap-2">{job.status==='downloading'&&<Button variant="ghost" disabled={busy} onClick={()=>onJob('pause')}><Pause size={15}/>{t('models.pause')}</Button>}<Button variant="ghost" disabled={busy} onClick={()=>onJob('cancel')}><Square size={15}/>{t('models.download.cancel')}</Button></div>}
  </div>}
 </section>
}
