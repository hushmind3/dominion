import {useEffect,useState} from 'react'
import {createPortal} from 'react-dom'
import {Download,LoaderCircle,Paperclip,X} from 'lucide-react'
import {fetchAttachmentBlob} from '../api'

export type AttachmentFile={id:string;name:string;isImage?:boolean}

/** A saved attachment must be requested through the same authenticated API as
 * the transcript. A bare /api/attachments/:id/content URL omits the app token.
 * Media is displayed through a local Blob URL, never a public file URL.
 */
export function AttachmentLink({file}:{file:AttachmentFile}){
 const [busy,setBusy]=useState(false)
 const [preview,setPreview]=useState<string|null>(null)
 const [error,setError]=useState('')
 useEffect(()=>()=>{if(preview)URL.revokeObjectURL(preview)},[preview])
 useEffect(()=>{
  if(!preview)return
  const escape=(event:KeyboardEvent)=>{if(event.key==='Escape')setPreview(null)}
  window.addEventListener('keydown',escape)
  return()=>window.removeEventListener('keydown',escape)
 },[preview])
 const open=async()=>{
  if(busy)return
  setBusy(true);setError('')
  try{
   const body=await fetchAttachmentBlob(file.id)
   const objectUrl=URL.createObjectURL(body)
   if(file.isImage)setPreview(objectUrl)
   else{
    const link=document.createElement('a')
    link.href=objectUrl;link.download=file.name||'attachment'
    document.body.appendChild(link);link.click();link.remove()
    window.setTimeout(()=>URL.revokeObjectURL(objectUrl),30000)
   }
  }catch(e){setError((e as Error).message)}
  finally{setBusy(false)}
 }
 return <>
  <button type="button" aria-label={file.name+' 첨부 열기'} title={file.name} disabled={busy} onClick={()=>void open()}>
   {busy?<LoaderCircle size={13} className="animate-spin"/>:<Paperclip size={13}/>}
   <span>{file.name}</span>{!file.isImage&&<Download size={12}/>}
  </button>
  {error&&<small className="attachment-open-error" role="alert">{error}</small>}
  {preview&&createPortal(<div className="attachment-preview-overlay" role="dialog" aria-modal="true" aria-label="첨부 이미지 미리보기" onMouseDown={event=>{if(event.target===event.currentTarget)setPreview(null)}}>
   <div className="attachment-preview-panel">
    <div className="attachment-preview-header"><span>{file.name}</span><button type="button" onClick={()=>setPreview(null)} aria-label="미리보기 닫기"><X size={19}/></button></div>
    <img src={preview} alt={file.name}/>
   </div>
  </div>,document.body)}
 </>
}
