import type {ReactNode} from 'react'
import {Blocks,BookOpenCheck,BrainCircuit,Eye,Languages,Wrench} from 'lucide-react'
import {Button,TextInput} from './Primitives'
import {t,useLocale} from '../i18n'

type Props={
 showReasoning:boolean;setShowReasoning:(value:boolean)=>void
 reasoningKo:boolean;setReasoningKo:(value:boolean)=>void
 thinkingEnabled:boolean;setThinkingEnabled:(value:boolean)=>void;canDisableThinking?:boolean
 reasoningEffort:string;setReasoningEffort:(value:string)=>void
 toolsEnabled:boolean;setToolsEnabled:(value:boolean)=>void
 autoLearn:boolean;setAutoLearn:(value:boolean)=>void;autoLearnUnavailable?:boolean
 learning:boolean;learnDisabled:boolean;onLearn:()=>void|Promise<void>
 optionsBusy?:boolean;notice?:string;onManageExtensions?:()=>void
 reasoningControl?:ReactNode;permissionControl?:ReactNode
}
/** Shared chat controls: readable labels, native checkboxes and unchanged
 * persistence callbacks for both single chat and nine-agent rooms. */
export function ConversationSettings({showReasoning,setShowReasoning,reasoningKo,setReasoningKo,thinkingEnabled,setThinkingEnabled,canDisableThinking=true,reasoningEffort,setReasoningEffort,toolsEnabled,setToolsEnabled,autoLearn,setAutoLearn,autoLearnUnavailable=false,learning,learnDisabled,onLearn,optionsBusy=false,notice,reasoningControl,permissionControl,onManageExtensions}:Props){
 const locale=useLocale()
 const en=locale==='en'

 const row='flex min-h-12 cursor-pointer items-center justify-between gap-4 rounded-xl px-3 py-3 text-ink transition-colors hover:bg-canvas focus-within:bg-canvas'
 const check='size-5 shrink-0 cursor-pointer accent-[var(--color-brand)] focus-visible:outline-2 focus-visible:outline-offset-2 disabled:cursor-not-allowed'
 return (
   <div aria-label={t('chat.settings')}
    className="conversation-settings-inline rounded-2xl border border-divider bg-surface p-4">
    <div className="flex items-start justify-between gap-2 border-b border-canvas px-2 pb-3">
     <div><h3 className="text-[15px] font-bold text-ink">{t('chat.settings')}</h3>
      <p className="mt-1 text-[12px] leading-5 text-muted">
       {en?'Adjust how replies are generated and displayed.':'응답 방식과 화면 표시를 조정합니다.'}
      </p>
     </div>

    </div>
    <p className="px-3 pb-1 pt-3 text-[11px] font-bold tracking-[.08em] text-muted">
     {en?'DISPLAY':'화면 표시'}
    </p>
    <label className={row}>
     <span className="flex min-w-0 items-start gap-2.5"><Eye size={18} className="mt-0.5 shrink-0 text-muted"/>
      <span><strong className="block text-[13px] font-semibold">{t('chat.showReasoning')}</strong>
       <small className="mt-1 block text-[12px] leading-5 text-muted">
        {en?'Show model reasoning when provided.':'모델이 제공하는 추론 내용을 화면에 표시합니다.'}
       </small>
      </span>
     </span>
     <TextInput type="checkbox" className={check} checked={showReasoning} onChange={e=>setShowReasoning(e.target.checked)}/>
    </label>
    <label className={row}>
     <span className="flex min-w-0 items-start gap-2.5"><Languages size={18} className="mt-0.5 shrink-0 text-muted"/>
      <span><strong className="block text-[13px] font-semibold">{t('chat.koreanReasoning')}</strong>
       <small className="mt-1 block text-[12px] leading-5 text-muted">
        {en?'Request Korean reasoning when supported.':'지원하는 모델에 한국어 추론을 요청합니다.'}
       </small>
      </span>
     </span>
     <TextInput type="checkbox" className={check} checked={reasoningKo} onChange={e=>setReasoningKo(e.target.checked)}/>
    </label>
    <div className="mt-2 border-t border-canvas pt-1">
     <p className="px-3 pb-1 pt-3 text-[11px] font-bold tracking-[.08em] text-muted">{en?'RESPONSE & TOOLS':'응답·도구'}</p>
    </div>
    <label className={row}>
     <span className="flex min-w-0 items-start gap-2.5"><BrainCircuit size={18} className="mt-0.5 shrink-0 text-muted"/>
      <span><strong className="block text-[13px] font-semibold">{t('chat.useThinking')}</strong>
       <small className="mt-1 block text-[12px] leading-5 text-muted">
        {!canDisableThinking?(en?'The selected model cannot turn reasoning off.':'현재 모델은 추론 끄기를 지원하지 않습니다.'):
         (en?'Enable reasoning on supported models.':'지원하는 모델에서 추론을 사용합니다.')}
       </small>
      </span>
     </span>
     <TextInput type="checkbox" className={check} checked={canDisableThinking?thinkingEnabled:true}
      disabled={!canDisableThinking} title={!canDisableThinking?'이 모델은 추론 끄기를 지원하지 않습니다. 낮은 강도를 선택하세요.':undefined}
      onChange={e=>{const on=e.target.checked;setThinkingEnabled(on);setReasoningEffort(on?'':'none')}}/>
    </label>
    {permissionControl&&<div className="px-3 py-3">{permissionControl}</div>}
    {reasoningControl&&<div className="px-3 pb-3">{reasoningControl}</div>}
    <label className={row}>
     <span className="flex min-w-0 items-start gap-2.5"><Wrench size={18} className="mt-0.5 shrink-0 text-muted"/>
      <span><strong className="block text-[13px] font-semibold">{t('chat.useTools')}</strong>
       <small className="mt-1 block text-[12px] leading-5 text-muted">
        {en?'Built-in search, files and code tools are available by default. MCP plugins are separate.':'검색·파일·코드는 기본 도구입니다. MCP 확장은 별도로 켜거나 끌 수 있습니다.'}
       </small>
      </span>
     </span>
     <TextInput type="checkbox" className={check} checked={toolsEnabled} disabled={optionsBusy}
      onChange={e=>setToolsEnabled(e.target.checked)}/>
    </label>
    {onManageExtensions&&<Button type="button" variant="ghost" onClick={onManageExtensions}
      className="mb-2 ml-2 !min-h-9 !gap-2 !px-2 !text-xs !text-muted hover:!text-ink">
      <Blocks size={16}/>{en?'Manage MCP plugins':'MCP 확장 관리'}
     </Button>}
    <label className={row}>
     <span className="flex min-w-0 items-start gap-2.5"><BookOpenCheck size={18} className="mt-0.5 shrink-0 text-muted"/>
      <span><strong className="block text-[13px] font-semibold">{t('chat.autoLearn')}</strong>
       <small className="mt-1 block text-[12px] leading-5 text-muted">{autoLearnUnavailable?(en?'Select an AI persona to enable automatic learning.':'AI 페르소나를 선택하면 자동 학습을 사용할 수 있습니다.'):t('chat.autoLearnDesc')}</small>
      </span>
     </span>
     <TextInput type="checkbox" className={check} checked={autoLearn} disabled={optionsBusy||autoLearnUnavailable}
      onChange={e=>setAutoLearn(e.target.checked)}/>
    </label>
    <div className="mt-2 border-t border-canvas pt-3">
     <Button type="button" variant="secondary" disabled={learnDisabled||learning} onClick={()=>void onLearn()}
      className="flex w-full !min-h-10 !justify-center gap-2 !text-[13px]">
      <BookOpenCheck size={17}/>{learning?t('chat.learning'):t('chat.learnNow')}
     </Button>
     {notice&&<p role="status" className="px-2 py-2 text-[12px] leading-5 text-success">{notice}</p>}
    </div>
   </div>
 );
}
