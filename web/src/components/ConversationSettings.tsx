import type {ReactNode} from 'react'
import {BookOpenCheck,BrainCircuit,Eye,Languages,Wrench} from 'lucide-react'
import {Button,TextInput} from './Primitives'
import {t,useLocale} from '../i18n'

type Props={
 showReasoning:boolean;setShowReasoning:(value:boolean)=>void
 reasoningKo:boolean;setReasoningKo:(value:boolean)=>void
 thinkingEnabled?:boolean;setThinkingEnabled?:(value:boolean)=>void;canDisableThinking?:boolean;thinkingNotice?:string;hideThinking?:boolean
 reasoningEffort?:string;setReasoningEffort?:(value:string)=>void
 toolsEnabled:boolean;setToolsEnabled:(value:boolean)=>void
 autoLearn:boolean;setAutoLearn:(value:boolean)=>void
 learning:boolean;learnDisabled:boolean;onLearn:()=>void|Promise<void>
 optionsBusy?:boolean;optionSaving?:boolean;optionError?:string;notice?:string
 reasoningControl?:ReactNode;permissionControl?:ReactNode
}
/** Shared chat controls: readable labels, native checkboxes and unchanged
 * persistence callbacks for both single chat and nine-agent rooms. */
export function ConversationSettings({showReasoning,setShowReasoning,reasoningKo,setReasoningKo,thinkingEnabled=true,setThinkingEnabled=()=>{},canDisableThinking=true,thinkingNotice,hideThinking=false,reasoningEffort,setReasoningEffort=()=>{},toolsEnabled,setToolsEnabled,autoLearn,setAutoLearn,learning,learnDisabled,onLearn,optionsBusy=false,optionSaving=false,optionError,notice,reasoningControl,permissionControl}:Props){
 const locale=useLocale()
 const en=locale==='en'

 const row='flex min-h-12 cursor-pointer items-center justify-between gap-4 rounded-xl px-3 py-3 text-[#3A342D] transition-colors hover:bg-[#FAF7F1] focus-within:bg-[#FAF7F1]'
 const check='size-5 shrink-0 cursor-pointer accent-[#A4583E] focus-visible:outline-2 focus-visible:outline-offset-2 disabled:cursor-not-allowed'
 return (
   <div aria-label={t('chat.settings')}
    className="conversation-settings-inline rounded-2xl border border-divider bg-surface p-4">
    <div className="flex items-start justify-between gap-2 border-b border-[#EAE2D6] px-2 pb-3">
     <div><h3 className="text-[15px] font-bold text-[#3A342D]">{t('chat.settings')}</h3>
      <p className="mt-1 text-[12px] leading-5 text-[#82776B]">
       {en?'Adjust how replies are generated and displayed.':'응답 방식과 화면 표시를 조정합니다.'}
      </p>
     </div>

    </div>
    <p className="px-3 pb-1 pt-3 text-[11px] font-bold tracking-[.08em] text-[#98795C]">
     {en?'DISPLAY':'화면 표시'}
    </p>
    <label className={row}>
     <span className="flex min-w-0 items-start gap-2.5"><Eye size={18} className="mt-0.5 shrink-0 text-[#957E65]"/>
      <span><strong className="block text-[13px] font-semibold">{t('chat.showReasoning')}</strong>
       <small className="mt-1 block text-[12px] leading-5 text-[#81766A]">
        {en?'Show model reasoning when provided.':'모델이 제공하는 추론 내용을 화면에 표시합니다.'}
       </small>
      </span>
     </span>
     <TextInput type="checkbox" className={check} checked={showReasoning} onChange={e=>setShowReasoning(e.target.checked)}/>
    </label>
    <label className={row}>
     <span className="flex min-w-0 items-start gap-2.5"><Languages size={18} className="mt-0.5 shrink-0 text-[#957E65]"/>
      <span><strong className="block text-[13px] font-semibold">{t('chat.koreanReasoning')}</strong>
       <small className="mt-1 block text-[12px] leading-5 text-[#81766A]">
        {en?'Request Korean reasoning when supported.':'지원하는 모델에 한국어 추론을 요청합니다.'}
       </small>
      </span>
     </span>
     <TextInput type="checkbox" className={check} checked={reasoningKo} onChange={e=>setReasoningKo(e.target.checked)}/>
    </label>
    <div className="mt-2 border-t border-[#EAE2D6] pt-1">
     <p className="px-3 pb-1 pt-3 text-[11px] font-bold tracking-[.08em] text-[#98795C]">{en?'RESPONSE & TOOLS':'응답·도구'}</p>
    </div>
    {!hideThinking&&<label className={row}>
     <span className="flex min-w-0 items-start gap-2.5"><BrainCircuit size={18} className="mt-0.5 shrink-0 text-[#957E65]"/>
      <span><strong className="block text-[13px] font-semibold">{t('chat.useThinking')}</strong>
       <small className="mt-1 block text-[12px] leading-5 text-[#81766A]">
        {!canDisableThinking?(thinkingNotice||(en?'The selected model cannot turn reasoning off.':'현재 모델은 추론 끄기를 지원하지 않습니다.')):
         (en?'Enable reasoning on supported models.':'지원하는 모델에서 추론을 사용합니다.')}
       </small>
      </span>
     </span>
     {canDisableThinking?<TextInput type="checkbox" className={check} checked={thinkingEnabled}
      onChange={e=>{const on=e.target.checked;setThinkingEnabled(on);setReasoningEffort(on?'':'none')}}/>
      :<span className="shrink-0 rounded-lg bg-zinc-100 px-2 py-1 text-[11px] text-zinc-500">{en?'Provider default':'제공자 기본값'}</span>}
    </label>}
    {permissionControl&&<div className="px-3 py-3">{permissionControl}</div>}
    {reasoningControl&&<div className="px-3 pb-3">{reasoningControl}</div>}
    {optionSaving&&<p role="status" className="px-3 text-xs text-muted">{en?'Saving room settings…':'회의실 설정 저장 중…'}</p>}
    {optionError&&<p role="alert" className="px-3 text-xs text-rose-700">{optionError}</p>}
    <label className={row}>
     <span className="flex min-w-0 items-start gap-2.5"><Wrench size={18} className="mt-0.5 shrink-0 text-[#957E65]"/>
      <span><strong className="block text-[13px] font-semibold">{t('chat.useTools')}</strong>
       <small className="mt-1 block text-[12px] leading-5 text-[#81766A]">
        {en?'Available tools are loaded only when tasks need them.':'작업이 필요할 때만 관련 도구를 불러옵니다.'}
       </small>
      </span>
     </span>
     <TextInput type="checkbox" className={check} checked={toolsEnabled} disabled={optionsBusy}
      onChange={e=>setToolsEnabled(e.target.checked)}/>
    </label>
    <label className={row}>
     <span className="flex min-w-0 items-start gap-2.5"><BookOpenCheck size={18} className="mt-0.5 shrink-0 text-[#957E65]"/>
      <span><strong className="block text-[13px] font-semibold">{t('chat.autoLearn')}</strong>
       <small className="mt-1 block text-[12px] leading-5 text-[#81766A]">{t('chat.autoLearnDesc')}</small>
      </span>
     </span>
     <TextInput type="checkbox" className={check} checked={autoLearn} disabled={optionsBusy}
      onChange={e=>setAutoLearn(e.target.checked)}/>
    </label>
    <div className="mt-2 border-t border-[#EAE2D6] pt-3">
     <Button type="button" variant="secondary" disabled={learnDisabled||learning} onClick={()=>void onLearn()}
      className="flex w-full !min-h-10 !justify-center gap-2 !text-[13px]">
      <BookOpenCheck size={17}/>{learning?t('chat.learning'):t('chat.learnNow')}
     </Button>
     {notice&&<p role="status" className="px-2 py-2 text-[12px] leading-5 text-[#6A5C4E]">{notice}</p>}
    </div>
   </div>
 );
}
