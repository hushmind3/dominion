import {useState} from 'react'
import {ArrowLeft,ArrowRight,Check,CheckCircle2,ChevronDown,ExternalLink,KeyRound,Settings2,ShieldCheck,UsersRound,X} from 'lucide-react'
import {DialogBackdrop,DialogPanel} from './Dialog'
import {Button,TextInput,SelectInput} from './Primitives'
import {FormField,Notice} from './UI'
import {t,useLocale} from '../i18n'
import {channelDraftMissing,type ChannelEntry,type ChannelField} from '../utils/channelView'

type State={entry:ChannelEntry;record:Record<string,any>}
type Props={editor:State;rooms:any[];agents:any[];providers:any[];saving:boolean;error:string;
 onChange:(record:Record<string,any>)=>void;onClose:()=>void;onSave:()=>void}
const nextField=(field:ChannelField,record:Record<string,any>)=>{
 const value=record[field.key]
 return Array.isArray(value)?value.join(', '):typeof value==='string'?value:''
}
const advancedKeys=new Set(['allowedUserIds','allowedBotIds','apiBase','graphBase','topic'])
export function ChannelEditor({editor,rooms,agents,providers,saving,error,onChange,onClose,onSave}:Props){
 useLocale()
 const {entry,record}=editor
 const [step,setStep]=useState(0)
 const [showPassword,setShowPassword]=useState(false)
 const changing=(key:string,value:unknown)=>onChange({...record,[key]:value})
 const secrets=entry.fields.filter(f=>f.type==='secret')
 const destinations=entry.fields.filter(f=>f.type!=='secret'&&!advancedKeys.has(f.key)&&f.key!=='roomId')
 const optional=entry.fields.filter(f=>advancedKeys.has(f.key))
 const missing=channelDraftMissing(entry,record,rooms)
 const getSecret=(field:ChannelField)=>(nextField(field,record).trim()||record.credentialsPresent?.[field.key])===true||Boolean(nextField(field,record).trim())
 const credentialsReady=Boolean(String(record.name||'').trim())&&secrets.every(f=>!f.required||getSecret(f))
 const destinationsReady=destinations.every(f=>!f.required||String(record[f.key]||'').trim())&&
  (!entry.requiresRoom||!!record.roomId)&&!missing.includes('roomId')
 const canAdvance=step===0?credentialsReady:step===1?destinationsReady:missing.length===0
 const isOutbound=entry.mode==='outbound'
 const edit=Boolean(record.id)
 const roomCount=rooms.find(room=>room.id===record.roomId)?.agentIds?.length||0
 const updateField=(field:ChannelField)=><FormField key={field.key} label={field.label+(field.required?' *':'')}
  hint={field.type==='secret'&&record.credentialsPresent?.[field.key]?t('conn.dlg.secretHint'):undefined}>
  <TextInput type={field.type==='secret'&&!showPassword?'password':field.type==='url'?'url':'text'}
   autoComplete={field.type==='secret'?'new-password':'off'} value={nextField(field,record)}
   placeholder={field.placeholder||''} onChange={event=>changing(field.key,event.target.value)}/>
 </FormField>
 const choose=(index:number)=>{
  if(saving)return
  if(edit||index<=step||(index===1&&credentialsReady)||(index===2&&credentialsReady&&destinationsReady))setStep(index)
 }
 return <DialogBackdrop onDismiss={()=>{if(!saving)onClose()}}>
  <DialogPanel size="wide" aria-label={entry.name+' '+(edit?t('conn.edit'):t('conn.new'))} className="!max-w-[820px]">
   <header className="sticky top-0 z-10 flex items-center justify-between gap-4 border-b border-[#EBE1D5] bg-white px-5 py-4 sm:px-7">
    <div className="flex min-w-0 items-center gap-3">
     <span className="grid size-10 shrink-0 place-items-center rounded-xl bg-[#F5E9E0] text-[#AE6145]"><KeyRound size={19}/></span>
     <div className="min-w-0"><p className="text-[10px] font-bold uppercase tracking-[.13em] text-[#B37B62]">{entry.name} / SETUP</p>
      <h2 className="mt-0.5 truncate text-[18px] font-bold tracking-tight text-[#302820]">{edit?t('conn.edit'):t('conn.new')}</h2>
     </div>
    </div>
    <Button variant="ghost" size="icon" aria-label={t('conn.cancel')} disabled={saving} onClick={onClose}><X size={18}/></Button>
   </header>
   <div className="border-b border-[#EEE6DB] bg-[#FCF9F5] px-5 py-4 sm:px-7">
    <ol className="grid grid-cols-3 gap-2">
     {[
      {label:t('conn.wizardCredentials'),number:'01'},
      {label:t('conn.wizardDestination'),number:'02'},
      {label:t('conn.wizardReview'),number:'03'}
     ].map((stage,index)=><li key={stage.number}>
      <button type="button" onClick={()=>choose(index)} disabled={saving||(!edit&&index>step&&!((index===1&&credentialsReady)||(index===2&&credentialsReady&&destinationsReady)))}
       aria-current={step===index?'step':undefined}
       className={'flex w-full items-center gap-2 rounded-lg px-2 py-2 text-left transition-colors '+(step===index?'bg-[#F2E2D6] text-[#9B543B]':index<step?'text-[#6F7766]':'text-[#9A8D80]')+' disabled:cursor-not-allowed disabled:opacity-60'}>
       <span className={'grid size-7 shrink-0 place-items-center rounded-full text-[11px] font-bold '+(step===index?'bg-brand text-white':index<step?'bg-[#E1F0E1] text-[#51815C]':'bg-[#EFE9E1] text-[#897E72]')}>{index<step?<Check size={14}/>:stage.number}</span>
       <span className="hidden min-w-0 truncate text-[11px] font-semibold sm:inline">{stage.label}</span>
      </button>
     </li>)}
    </ol>
   </div>
   <div className="min-h-[340px] space-y-5 px-5 py-6 sm:px-7">
    {step===0&&<section className="space-y-5" aria-label={t('conn.wizardCredentials')}>
     <div><p className="mb-1 text-[11px] font-bold uppercase tracking-[.14em] text-[#BE7E63]">{t('conn.stepCount',{current:1})}</p>
      <h3 className="text-xl font-bold tracking-tight text-[#332A22]">{t('conn.wizardCredentials')}</h3>
      <p className="mt-1.5 text-[12px] leading-5 text-[#827668]">{t('conn.wizardCredentialsDesc')}</p></div>
     <FormField label={t('conn.name')+' *'}>
      <TextInput autoComplete="off" value={record.name||''} onChange={event=>changing('name',event.target.value)}/>
     </FormField>
     {secrets.length>0&&<div className="grid gap-4 sm:grid-cols-2">
      {secrets.map(updateField)}
     </div>}
     {secrets.length>0&&<div className="flex items-start gap-2.5 rounded-xl border border-[#DCE7E0] bg-[#F5FAF6] px-3 py-3 text-[12px] leading-5 text-[#54715C]">
      <ShieldCheck size={17} className="mt-0.5 shrink-0"/>
      <p>{t('conn.editorCredentialsHint')}</p>
     </div>}
     {secrets.length>0&&<Button variant="ghost" size="sm" onClick={()=>setShowPassword(v=>!v)}>
      {showPassword?t('conn.hideToken'):t('conn.showToken')}</Button>}
     {entry.instructions?.length?<details className="rounded-xl border border-[#E9E0D5] bg-white px-4 py-3">
      <summary className="flex cursor-pointer items-center justify-between gap-2 text-[12px] font-semibold text-[#695849]">{t('conn.instructions')}<ChevronDown size={15}/></summary>
      <ol className="mt-3 list-decimal space-y-2 border-t border-[#F0E9DF] pt-3 pl-4 text-xs leading-5 text-[#827567]">
       {entry.instructions.map((item,index)=><li key={index}>{item}</li>)}</ol>
     </details>:null}
     {entry.docs&&<a className="inline-flex items-center gap-1.5 text-[12px] font-semibold text-[#A55B3F]" href={entry.docs} target="_blank" rel="noopener noreferrer">
      {t('conn.docs')}<ExternalLink size={13}/></a>}
    </section>}
    {step===1&&<section className="space-y-5" aria-label={t('conn.wizardDestination')}>
     <div><p className="mb-1 text-[11px] font-bold uppercase tracking-[.14em] text-[#BE7E63]">{t('conn.stepCount',{current:2})}</p>
      <h3 className="text-xl font-bold tracking-tight text-[#332A22]">{t('conn.wizardDestination')}</h3>
      <p className="mt-1.5 text-[12px] leading-5 text-[#827668]">{t('conn.wizardDestinationDesc')}</p></div>
     <div className="grid gap-4 sm:grid-cols-2">{destinations.map(updateField)}</div>
     <FormField label={entry.requiresRoom?t('conn.roomOrAgent')+' *':t('conn.replyMode')}>
      <SelectInput value={record.roomId||''} onChange={event=>changing('roomId',event.target.value)}>
       <option value="">{entry.requiresRoom?t('conn.pickRoom'):t('conn.singleAgent')}</option>
       {rooms.map(room=><option key={room.id} value={room.id}
        disabled={!!entry.maxRoomParticipants&&(room.agentIds?.length||0)>entry.maxRoomParticipants}>
        {t('conn.dlg.roomOption',{name:room.name,count:room.agentIds?.length||0})}</option>)}
      </SelectInput>
     </FormField>
     {rooms.length===0&&entry.requiresRoom&&<div className="rounded-xl border border-[#EADBC7] bg-[#FFF9F1] px-3 py-3 text-xs leading-5 text-[#805B3D]">
      {t('conn.wizardNoRooms')}
     </div>}
     {record.roomId&&<div className="flex items-center gap-3 rounded-xl bg-[#F8F4EE] p-3 text-[12px] text-[#75685B]">
      <UsersRound size={18} className="shrink-0 text-[#B0795A]"/>
      <span>{entry.maxRoomParticipants?t('conn.dlg.maxParticipants',{count:entry.maxRoomParticipants}):isOutbound?t('conn.dlg.outboundNote'):t('conn.dlg.roomPolicyNote')}{roomCount>0?' · '+t('conn.wizardParticipants',{count:roomCount}):''}</span>
     </div>}
     <p className="text-[11px] leading-5 text-[#A09488]">{t('conn.wizardTargetHelp')}</p>
    </section>}
    {step===2&&<section className="space-y-5" aria-label={t('conn.wizardReview')}>
     <div><p className="mb-1 text-[11px] font-bold uppercase tracking-[.14em] text-[#BE7E63]">{t('conn.stepCount',{current:3})}</p>
      <h3 className="text-xl font-bold tracking-tight text-[#332A22]">{t('conn.wizardReview')}</h3>
      <p className="mt-1.5 text-[12px] leading-5 text-[#827668]">{t('conn.wizardReviewDesc')}</p></div>
     <div className="grid gap-2 rounded-xl border border-[#E8DFD3] bg-[#FAF8F4] p-4 sm:grid-cols-2">
      <div className="text-[12px]"><span className="text-[#9C8D7B]">{t('conn.name')}</span><strong className="mt-1 block break-words text-[#3E362D]">{record.name||'-'}</strong></div>
      <div className="text-[12px]"><span className="text-[#9C8D7B]">{t('conn.room')}</span>
       <strong className="mt-1 block break-words text-[#3E362D]">{rooms.find(room=>room.id===record.roomId)?.name||t('conn.singleAgent')}</strong></div>
      {destinations.filter(field=>field.required).map(field=><div key={field.key} className="text-[12px]">
       <span className="text-[#9C8D7B]">{field.label}</span><strong className="mt-1 block break-all text-[#3E362D]">{nextField(field,record)||'-'}</strong></div>)}
     </div>
     <label className="flex cursor-pointer items-center justify-between gap-4 rounded-xl border border-[#E3D7C7] bg-[#FFF9F2] px-4 py-3">
      <span><strong className="block text-[13px] text-[#3B3026]">{t('conn.enable')}</strong>
       <span className="mt-1 block text-[11px] leading-5 text-[#8F7C68]">{t('conn.enableHint')}</span></span>
      <TextInput type="checkbox" checked={Boolean(record.enabled)} onChange={event=>changing('enabled',event.target.checked)}
       className="size-5 shrink-0 accent-[#C96442]"/>
     </label>
     <details className="rounded-xl border border-[#E9E0D5] bg-white px-4 py-3">
      <summary className="flex cursor-pointer items-center justify-between gap-2 text-[12px] font-semibold text-[#695849]"><span className="inline-flex items-center gap-2"><Settings2 size={15}/>{t('conn.advanced')}</span><ChevronDown size={15}/></summary>
      <div className="mt-4 space-y-4 border-t border-[#F1E9DF] pt-4">
       {optional.length>0&&<div className="grid gap-4 sm:grid-cols-2">{optional.map(updateField)}</div>}
       {!isOutbound&&<div className="grid gap-4 sm:grid-cols-2">
        <FormField label={t('conn.agent')}><SelectInput value={record.agentId||''} onChange={event=>changing('agentId',event.target.value)}>
         <option value="">{t('conn.dlg.pickAgent')}</option>{agents.map(agent=><option key={agent.id} value={agent.id}>{agent.name}</option>)}
        </SelectInput></FormField>
        <FormField label={t('conn.provider')}><SelectInput value={record.providerId||''} onChange={event=>changing('providerId',event.target.value)}>
         <option value="">{t('conn.dlg.agentDefault')}</option>{providers.map(provider=><option key={provider.id} value={provider.id}>{provider.name}</option>)}
        </SelectInput></FormField>
        <FormField label={t('conn.model')}><TextInput value={record.model||''} onChange={event=>changing('model',event.target.value)} placeholder={t('conn.dlg.agentDefaultModel')}/></FormField>
       </div>}
       {record.roomId&&!isOutbound&&<FormField label={t('conn.policy')}>
        <SelectInput value={record.roomPolicy||'all_or_named'} onChange={event=>changing('roomPolicy',event.target.value)}>
         <option value="all_or_named">{t('conn.dlg.policyAll')}</option>
         <option value="named_only">{t('conn.dlg.policyNamed')}</option>
         <option value="round_robin">{t('conn.dlg.policyRound')}</option>
        </SelectInput>
       </FormField>}
       {!isOutbound&&<label className="flex items-center gap-2 text-xs text-muted"><TextInput type="checkbox"
        checked={record.mentionOnly!==false} onChange={event=>changing('mentionOnly',event.target.checked)}/>{t('conn.mentionOnly')}</label>}
       {entry.id==='discord'&&<>
        <label className="flex items-center gap-2 text-xs text-muted"><TextInput type="checkbox" checked={Boolean(record.replyMentionBots)} onChange={event=>changing('replyMentionBots',event.target.checked)}/>{t('conn.dlg.discordMention')}</label>
        <label className="flex items-center gap-2 text-xs text-muted"><TextInput type="checkbox" checked={Boolean(record.toolsEnabled)} onChange={event=>changing('toolsEnabled',event.target.checked)}/>{t('conn.dlg.discordTools')}</label>
       </>}
       {record.roomId&&<label className="flex items-center gap-2 text-xs text-muted"><TextInput type="checkbox" checked={Boolean(record.syncRoomMessages)} onChange={event=>changing('syncRoomMessages',event.target.checked)}/>{t('conn.dlg.syncRoom')}</label>}
       {record.webhookPath&&<code className="block break-all rounded-lg bg-[#F7F3EC] p-3 text-xs text-[#756553]">{record.webhookPath}</code>}
      </div>
     </details>
     <p className="flex items-start gap-2 text-[11px] leading-5 text-[#8A8175]"><CheckCircle2 size={15} className="mt-0.5 shrink-0 text-[#69826B]"/>{t('conn.wizardPostSave')}</p>
    </section>}
    {error&&<Notice message={error}/>}
    {step===2&&missing.length>0&&<p className="rounded-lg bg-amber-50 px-3 py-2 text-xs text-amber-900" role="status">
     {t('conn.fieldsRequired',{fields:missing.map(item=>item==='roomId'?t('conn.roomOrAgent'):item==='name'?t('conn.name'):item).join(', ')})}
    </p>}
   </div>
   <footer className="sticky bottom-0 z-10 flex flex-wrap items-center justify-between gap-3 border-t border-[#EBE1D5] bg-white px-5 py-4 sm:px-7">
    <span className="text-[11px] font-semibold tabular-nums text-[#9B8C7B]">{step+1} / 3</span>
    <div className="flex items-center gap-2">
     <Button variant="ghost" disabled={saving} onClick={()=>step===0?onClose():setStep(n=>n-1)}>
      {step===0?t('conn.cancel'):<><ArrowLeft size={15}/>{t('conn.previous')}</>}
     </Button>
     {step<2
      ?<Button variant="primary" disabled={saving||!canAdvance} onClick={()=>setStep(n=>n+1)}>{t('conn.next')}<ArrowRight size={15}/></Button>
      :<Button variant="primary" disabled={saving||missing.length>0} onClick={onSave}>
       {saving?<span className="size-4 animate-spin rounded-full border-2 border-white/50 border-t-white"/>:<Check size={16}/>}
       {saving?t('common.processing'):t('conn.save')}
      </Button>}
    </div>
   </footer>
  </DialogPanel>
 </DialogBackdrop>
}
