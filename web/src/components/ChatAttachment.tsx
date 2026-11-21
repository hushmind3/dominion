import {useEffect,useRef,useState} from 'react'
import {createPortal} from 'react-dom'
import {FileText,Image as ImageIcon,LoaderCircle,X} from 'lucide-react'
import {attachmentBlob} from '../api'
import {DialogBackdrop,DialogPanel} from './Dialog'
import {Button} from './Primitives'
import {t} from '../i18n'

export type SavedChatFile={id:string;name:string;isImage?:boolean}

/** Fetches bytes through the authenticated API, never through an untrusted
 * third-party image URL. URLs are ephemeral and revoked after preview/leave.
 */
export function ChatAttachment({file}:{file:SavedChatFile}){
 const [preview,setPreview]=useState<string|null>(null)
 const [busy,setBusy]=useState(false)
 const [error,setError]=useState('')
 const urlRef=useRef<string|null>(null)
 useEffect(()=>()=>{if(urlRef.current)URL.revokeObjectURL(urlRef.current)},[])
 const close=()=>{
  setPreview(null)
  if(urlRef.current){URL.revokeObjectURL(urlRef.current);urlRef.current=null}
 }
 const open=async()=>{
  if(busy)return
  setBusy(true);setError('')
  try{
   const blob=await attachmentBlob(file.id)
   const image=file.isImage&&/^image\/(?:png|jpeg|webp|gif)(?:$|;)/i.test(blob.type)
   const objectUrl=URL.createObjectURL(blob)
   if(image){
    if(urlRef.current)URL.revokeObjectURL(urlRef.current)
    urlRef.current=objectUrl
    setPreview(objectUrl)
   }else{
    const link=document.createElement('a')
    link.href=objectUrl;link.download=file.name;link.click()
    window.setTimeout(()=>URL.revokeObjectURL(objectUrl),1000)
   }
  }catch{
   setError(t('chat.fileDownloadFailed'))
  }finally{setBusy(false)}
 }
 return <>
  <div className="inline-flex max-w-full flex-col gap-1">
   <button type="button" disabled={busy} onClick={()=>void open()}
    className="inline-flex min-w-0 max-w-full items-center gap-1.5 rounded-lg border border-[#D6C8B7] bg-white px-2 py-1 text-[11px] text-[#4A443C] hover:border-[#A94E30] disabled:opacity-50"
    title={t(file.isImage?'chat.previewImage':'chat.downloadAttachment',{name:file.name})}>
    {busy?<LoaderCircle size={12} className="shrink-0 animate-spin"/>:
      file.isImage?<ImageIcon size={12} className="shrink-0"/>:<FileText size={12} className="shrink-0"/>}
    <span className="max-w-[180px] truncate">{file.name}</span>
   </button>
   {error&&<span role="status" className="text-xs text-rose-700">{error}</span>}
  </div>
  {preview&&createPortal(<DialogBackdrop onDismiss={close}>
   <DialogPanel size="wide" aria-label={t('chat.previewImage',{name:file.name})}>
    <div className="flex items-center justify-between gap-3 border-b border-[#E7E0D5] px-4 py-3">
     <strong className="min-w-0 truncate text-sm">{file.name}</strong>
     <Button variant="ghost" type="button" size="icon" onClick={close} aria-label={t('chat.closePreview')}>
      <X size={18}/>
     </Button>
    </div>
    <div className="flex min-h-24 items-center justify-center overflow-auto bg-[#F7F2EA] p-3">
     <img src={preview} alt={file.name} className="max-h-[72dvh] max-w-full object-contain"
      onError={()=>{close();setError(t('chat.fileDownloadFailed'))}}/>
    </div>
   </DialogPanel>
  </DialogBackdrop>,document.body)}
 </>
}
