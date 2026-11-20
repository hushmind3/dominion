import {Check,ExternalLink,MessageCircle,Settings2,UsersRound,X} from 'lucide-react'
import {DialogBackdrop,DialogPanel} from './Dialog'
import {Button,TextInput,SelectInput} from './Primitives'
import {FormField,Notice} from './UI'
import {t,useLocale} from '../i18n'
import {channelDraftMissing,type ChannelEntry} from '../utils/channelView'

type ChannelEditorState={entry:ChannelEntry;record:Record<string,any>}
type Props={
 editor:ChannelEditorState;rooms:any[];agents:any[];providers:any[];
 saving:boolean;error:string;onChange:(record:Record<string,any>)=>void;
 onClose:()=>void;onSave:()=>void
}
export function ChannelEditor({editor,rooms,agents,providers,saving,error,onChange,onClose,onSave}:Props){
 useLocale()
 const {entry,record}=editor
 const missing=channelDraftMissing(entry,record)
 const change=(key:string,value:unknown)=>onChange({...record,[key]:value})
 const isOutbound=entry.mode==='outbound'
 const title=record.id?t('conn.edit'):t('conn.new')
 const roomCount=rooms.find(room=>room.id===record.roomId)?.agentIds?.length||0
 return <DialogBackdrop onDismiss={()=>{if(!saving)onClose()}}>
  <DialogPanel size="wide" aria-label={entry.name+' '+title}>
   <header className="sticky top-0 z-10 flex items-start justify-between gap-3 border-b border-[#EEE7DC] bg-white px-5 py-4">
    <div className="min-w-0"><p className="text-[11px] font-semibold uppercase tracking-wide text-[#A77762]">{entry.name}</p>
     <h2 className="mt-1 text-lg font-bold text-[#2E2923]">{title}</h2>
     <p className="mt-1 text-xs leading-5 text-[#7A7065]">{entry.description}</p></div>
    <Button size="icon" variant="ghost" aria-label={t('conn.cancel')} disabled={saving} onClick={onClose}><X size={18}/></Button>
   </header>
   <div className="space-y-6 p-5">
    <section className="space-y-3">
     <div className="flex items-center gap-2">
      <span className="grid size-6 place-items-center rounded-full bg-[#F7E8E1] text-xs font-bold text-[#A94E30]">1</span>
      <h3 className="text-sm font-bold text-[#3C342A]">{t('conn.editorCredentials')}</h3>
     </div>
     <FormField label={t('conn.name')}>
      <TextInput value={record.name||''} autoComplete="off" onChange={event=>change('name',event.target.value)}/>
     </FormField>
     <div className="grid gap-3 sm:grid-cols-2">
      {entry.fields.map(field=><FormField key={field.key} label={field.label+(field.required?' *':'')}
       hint={field.type==='secret'&&record.credentialsPresent?.[field.key]?t('conn.dlg.secretHint'):undefined}>
       <TextInput value={record[field.key]||''} autoComplete={field.type==='secret'?'new-password':'off'}
        type={field.type==='secret'?'password':field.type==='url'?'url':'text'} placeholder={field.placeholder||''}
        onChange={event=>change(field.key,event.target.value)}/>
      </FormField>)}
     </div>
    </section>
    <section className="space-y-3 border-t border-[#EFE9E0] pt-5">
     <div className="flex items-center gap-2">
      <span className="grid size-6 place-items-center rounded-full bg-[#F7E8E1] text-xs font-bold text-[#A94E30]">2</span>
      <h3 className="text-sm font-bold text-[#3C342A]">{t('conn.editorRouting')}</h3>
     </div>
     <FormField label={entry.requiresRoom?t('conn.roomOrAgent'):t('conn.replyMode')}>
      <SelectInput value={record.roomId||''} onChange={event=>change('roomId',event.target.value)}>
       <option value="">{entry.requiresRoom?t('conn.pickRoom'):t('conn.singleAgent')}</option>
       {rooms.map(room=><option key={room.id} value={room.id}
        disabled={!!entry.maxRoomParticipants&&(room.agentIds?.length||0)>entry.maxRoomParticipants}>
        {t('conn.dlg.roomOption',{name:room.name,count:room.agentIds?.length||0})}</option>)}
      </SelectInput>
     </FormField>
     {record.roomId&&<p className="rounded-lg bg-[#F9F4EE] px-3 py-2 text-[12px] leading-5 text-[#73695E]">
      <UsersRound size={14} className="mr-1 inline"/>
      {entry.maxRoomParticipants?t('conn.dlg.maxParticipants',{count:entry.maxRoomParticipants})
       :isOutbound?t('conn.dlg.outboundNote'):t('conn.dlg.roomPolicyNote')}
      {roomCount>0?' · '+roomCount:''}
     </p>}
     <label className="flex cursor-pointer items-center justify-between gap-4 rounded-xl border border-[#E8DCCE] bg-[#FFF9F3] p-3">
      <div><span className="block text-[13px] font-bold text-[#382F27]">{t('conn.enable')}</span>
       <span className="mt-1 block text-[11px] leading-5 text-[#776D61]">{t('conn.enableHint')}</span></div>
      <TextInput type="checkbox" checked={Boolean(record.enabled)}
       className="size-5 shrink-0 accent-[#C96442]" onChange={event=>change('enabled',event.target.checked)}/>
     </label>
    </section>
    <details className="group rounded-xl border border-[#E8E0D5] bg-[#FAF8F5]">
     <summary className="flex cursor-pointer select-none items-center gap-2 px-4 py-3 text-sm font-semibold text-[#62594F]">
      <Settings2 size={16}/>{t('conn.advanced')}<span className="ml-auto text-xs font-normal text-[#958779]">{t('conn.advancedHint')}</span>
     </summary>
     <div className="space-y-4 border-t border-[#E8E0D5] bg-white px-4 py-4">
      {record.roomId&&!isOutbound&&<FormField label={t('conn.policy')}>
       <SelectInput value={record.roomPolicy||'all_or_named'} onChange={event=>change('roomPolicy',event.target.value)}>
        <option value="all_or_named">{t('conn.dlg.policyAll')}</option>
        <option value="named_only">{t('conn.dlg.policyNamed')}</option>
        <option value="round_robin">{t('conn.dlg.policyRound')}</option>
       </SelectInput>
      </FormField>}
      {!isOutbound&&<>
       <div className="grid gap-3 sm:grid-cols-2">
        <FormField label={t('conn.agent')}><SelectInput value={record.agentId||''}
         onChange={event=>change('agentId',event.target.value)}>
         <option value="">{t('conn.dlg.pickAgent')}</option>
         {agents.map(agent=><option key={agent.id} value={agent.id}>{agent.name}</option>)}
        </SelectInput></FormField>
        <FormField label={t('conn.provider')}><SelectInput value={record.providerId||''}
         onChange={event=>change('providerId',event.target.value)}>
         <option value="">{t('conn.dlg.agentDefault')}</option>
         {providers.map(provider=><option key={provider.id} value={provider.id}>{provider.name}</option>)}
        </SelectInput></FormField>
       </div>
       <FormField label={t('conn.model')}><TextInput value={record.model||''}
        onChange={event=>change('model',event.target.value)} placeholder={t('conn.dlg.agentDefaultModel')}/></FormField>
       <label className="flex items-center gap-2 text-xs text-[#544A3E]"><TextInput type="checkbox"
        checked={record.mentionOnly!==false} onChange={event=>change('mentionOnly',event.target.checked)}/>{t('conn.mentionOnly')}</label>
      </>}
      {entry.id==='discord'&&<>
       <label className="flex items-center gap-2 text-xs text-[#544A3E]"><TextInput type="checkbox"
        checked={Boolean(record.replyMentionBots)} onChange={event=>change('replyMentionBots',event.target.checked)}/>{t('conn.dlg.discordMention')}</label>
       <label className="flex items-center gap-2 text-xs text-[#544A3E]"><TextInput type="checkbox"
        checked={Boolean(record.toolsEnabled)} onChange={event=>change('toolsEnabled',event.target.checked)}/>{t('conn.dlg.discordTools')}</label>
      </>}
      {record.roomId&&<label className="flex items-center gap-2 text-xs text-[#544A3E]"><TextInput type="checkbox"
       checked={Boolean(record.syncRoomMessages)} onChange={event=>change('syncRoomMessages',event.target.checked)}/>{t('conn.dlg.syncRoom')}</label>}
      {record.webhookPath&&<p className="break-all rounded-lg bg-[#F7F2EB] px-3 py-2 text-xs"><strong>{t('conn.webhook')}: </strong><code>{record.webhookPath}</code></p>}
     </div>
    </details>
    {entry.instructions?.length?<details className="rounded-xl border border-[#E8E0D5] px-4 py-3">
     <summary className="cursor-pointer text-xs font-semibold text-[#665A4D]">{t('conn.instructions')}</summary>
     <ol className="mt-3 list-decimal space-y-2 pl-5 text-xs leading-5 text-[#71675C]">
      {entry.instructions.map((item,i)=><li key={i}>{item}</li>)}</ol>
    </details>:null}
    {entry.docs&&<a className="inline-flex items-center gap-1.5 text-xs font-semibold text-[#8E5B43]"
     href={entry.docs} target="_blank" rel="noopener noreferrer">{t('conn.docs')}<ExternalLink size={13}/></a>}
    {missing.length>0&&<p role="status" className="rounded-xl border border-amber-200 bg-amber-50 px-3 py-3 text-xs leading-5 text-amber-900">
     {t('conn.fieldsRequired',{fields:missing.map(item=>item==='roomId'?t('conn.roomOrAgent'):item==='name'?t('conn.name'):item).join(', ')})}
    </p>}
    {error&&<Notice message={error}/>}
   </div>
   <footer className="sticky bottom-0 z-10 flex items-center justify-end gap-2 border-t border-[#EEE7DC] bg-white px-5 py-4">
    <Button variant="secondary" disabled={saving} onClick={onClose}>{t('conn.cancel')}</Button>
    <Button variant="primary" disabled={saving||missing.length>0} onClick={onSave}>
     <Check size={16}/>{saving?t('common.processing'):t('conn.save')}</Button>
   </footer>
  </DialogPanel>
 </DialogBackdrop>
}
