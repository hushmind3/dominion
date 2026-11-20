import {useState} from 'react'
import {AlertTriangle,Check,RotateCcw} from 'lucide-react'
import {post} from '../api'
import {Button} from './Primitives'
import {useRemote} from './UI'
import {t,useLocale} from '../i18n'

type Delivery={id:string;messageId?:string;partIndex?:number;error?:string;errorAt?:string;updatedAt?:string;preview?:string}
type Response={total:number;items:Delivery[]}
type Resolution='confirmed_delivered'|'confirmed_not_delivered'

export function ChannelDeliveryReview({channelId,onChanged}:{channelId:string;onChanged:()=>Promise<void>}){
 useLocale()
 const {value,loading,error,reload}=useRemote<Response>(
  '/api/channels/'+encodeURIComponent(channelId)+'/delivery/reviews',{total:0,items:[]})
 const [busy,setBusy]=useState<string|null>(null)
 const [actionError,setActionError]=useState('')
 const [message,setMessage]=useState('')
 const resolve=async(item:Delivery,resolution:Resolution)=>{
  if(busy)return
  const delivered=resolution==='confirmed_delivered'
  if(!window.confirm(t(delivered?'conn.reviewConfirmDelivered':'conn.reviewConfirmNotDelivered')))return
  setBusy(item.id);setActionError('');setMessage('')
  try{
   await post('/api/channels/'+encodeURIComponent(channelId)+'/delivery/'+encodeURIComponent(item.id)+'/reconcile',
    {resolution})
   setMessage(t(delivered?'conn.reviewMarkedDelivered':'conn.reviewMarkedNotDelivered'))
   await Promise.all([reload(),onChanged()])
  }catch(error){setActionError((error as Error).message);await reload()}
  finally{setBusy(null)}
 }
 return <section className="space-y-3 border-t border-[#EFE8DD] pt-4" aria-label={t('conn.reviewTitle')}>
  <div className="flex flex-wrap items-start justify-between gap-3">
   <div><h4 className="flex items-center gap-2 text-xs font-bold text-amber-900"><AlertTriangle size={16}/>{t('conn.reviewTitle')}</h4>
    <p className="mt-1 max-w-xl text-xs leading-5 text-[#75695D]">{t('conn.reviewExplanation')}</p></div>
   <Button variant="ghost" size="sm" disabled={Boolean(busy)||loading} onClick={()=>void reload()}>
    <RotateCcw size={14}/>{t('conn.refresh')}</Button>
  </div>
  {(error||actionError)&&<p className="rounded-lg bg-rose-50 p-3 text-xs text-rose-700" role="alert">{actionError||error}</p>}
  {message&&<p className="rounded-lg bg-emerald-50 p-3 text-xs text-emerald-800" role="status">{message}</p>}
  {loading&&<p className="text-xs text-[#8A8074]" role="status">{t('conn.loading')}</p>}
  <div className="space-y-2">
   {value.items.map(item=><article key={item.id} className="space-y-2 rounded-xl border border-[#EADBC6] bg-[#FFFCF7] p-3">
    <p className="text-[11px] font-medium text-amber-900">{item.error||t('conn.reviewUnknown')}</p>
    {item.preview&&<p className="max-h-24 overflow-y-auto whitespace-pre-wrap break-words rounded-lg bg-white p-2 text-[11px] leading-5 text-[#54493E]">{item.preview}</p>}
    <div className="flex flex-wrap items-center justify-between gap-2 text-[11px] text-[#85786A]">
     <span>{item.errorAt||item.updatedAt?new Date(item.errorAt||item.updatedAt||'').toLocaleString():t('conn.reviewUnknownTime')}</span>
     {typeof item.partIndex==='number'&&<span>{t('conn.reviewPart',{count:item.partIndex+1})}</span>}
    </div>
    <div className="flex flex-wrap gap-2 border-t border-[#F0E3D2] pt-2">
     <Button variant="secondary" size="sm" disabled={Boolean(busy)} onClick={()=>void resolve(item,'confirmed_delivered')}>
      <Check size={14}/>{t('conn.reviewDelivered')}</Button>
     <Button variant="secondary" size="sm" disabled={Boolean(busy)} onClick={()=>void resolve(item,'confirmed_not_delivered')}>
      {t('conn.reviewNotDelivered')}</Button>
    </div>
   </article>)}
  </div>
  {value.total>value.items.length&&<p className="text-xs text-[#8B7E70]">{t('conn.reviewLimited',{count:value.total})}</p>}
  {!loading&&!value.items.length&&!error&&<p className="text-xs text-[#7B7167]">{t('conn.reviewEmpty')}</p>}
 </section>
}
