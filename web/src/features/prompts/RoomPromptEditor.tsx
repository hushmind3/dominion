import {Check, LoaderCircle, X} from 'lucide-react'
import type {Agent} from '../../api'
import {DialogBackdrop, DialogPanel} from '../../components/Dialog'
import {Button, SelectInput, TextArea} from '../../components/Primitives'
import {FormField, Notice} from '../../components/UI'
import {t, useLocale} from '../../i18n'
import {PromptSection} from './PromptSection'

export type RoomPromptPreview = {
 prompt:string; chars:number; tokens:number
 common:string; commonChars:number; commonTokens:number
 persona:string; personaChars:number; personaTokens:number
 commonOverride?:string|null; finalOverride?:string|null
}
type Props = {
 roster:Agent[]; agentId:string; preview:RoomPromptPreview|null
 loading:boolean; saving:boolean; dirty:boolean; error:string; notice:string
 common:string; persona:string; final:string
 onCommon:(text:string)=>void; onPersona:(text:string)=>void; onFinal:(text:string)=>void
 onAgent:(id:string)=>void; onSave:()=>void; onClose:()=>void
}

/** Group and private-chat prompts share visible editors and form hierarchy. */
export function RoomPromptEditor(props:Props) {
 const en=useLocale()==='en'
 const tr=(ko:string,english:string)=>en?english:ko
 const {roster,agentId,preview,loading,saving,dirty,error,notice,common,persona,final,onCommon,onPersona,onFinal,onAgent,onSave,onClose}=props
 const tooLong=common.length>32000||final.length>32000
 return <DialogBackdrop onDismiss={onClose}><DialogPanel size="wide" className="flex flex-col !overflow-hidden" aria-label={t('rooms.systemPrompt')}>
  <header className="flex shrink-0 items-start justify-between gap-4 border-b border-divider p-5 sm:p-6">
   <div><h1 className="text-xl font-semibold">{t('rooms.systemPrompt')}</h1><p className="mt-2 text-sm leading-7 text-muted">{tr('공통 지침, AI의 성격, 최종 시스템 프롬프트를 직접 편집합니다.','Edit shared instructions, persona and the full system prompt.')}</p></div>
   <Button variant="ghost" size="icon" aria-label={t('rooms.close')} disabled={saving} onClick={onClose}><X size={18}/></Button>
  </header>
  <div className="grid min-h-0 flex-1 gap-5 overflow-y-auto overscroll-contain p-4 sm:p-6">
   {roster.length>0&&<FormField label={t('rooms.systemPromptAgent')}><SelectInput disabled={saving||loading} value={agentId} onChange={event=>onAgent(event.target.value)}>{roster.map(agent=><option key={agent.id} value={agent.id}>{agent.name}</option>)}</SelectInput></FormField>}
   {loading?<p role="status" className="flex items-center gap-2 text-sm text-muted"><LoaderCircle size={16} className="animate-spin"/>{t('rooms.systemPromptLoading')}</p>:preview&&<>
    <PromptSection id="room-prompt-common" title={tr('그룹 공통 프롬프트','Shared group instructions')} scope={tr('이 그룹의 모든 AI','All AIs in this group')}>
     <p>{tr('공통 답변 규칙을 수정합니다. 직접 지정한 최종 프롬프트가 있으면 그 내용이 우선합니다.','Edit shared response rules. A full-prompt override takes precedence when present.')}</p>
     <TextArea aria-label={t('rooms.systemPromptCommon')} rows={8} disabled={saving} value={common} onChange={event=>onCommon(event.target.value)}/>
    </PromptSection>
    <PromptSection id="room-prompt-persona" title={tr('페르소나','Persona')} scope={roster.find(agent=>agent.id===agentId)?.name||tr('선택한 AI','Selected AI')}>
     <p>{tr('이 AI의 역할과 성격입니다. 저장하면 이 AI를 사용하는 다른 대화에도 적용됩니다.','This AI’s role and personality. Saving also affects other conversations using this AI.')}</p>
     <TextArea aria-label={t('rooms.systemPromptPersona')} rows={8} disabled={saving||!agentId} value={persona} onChange={event=>onPersona(event.target.value)}/>
    </PromptSection>
    <PromptSection id="room-prompt-final" title={t('rooms.systemPromptCombined')} scope={tr('이 그룹에 적용','Applied to this group')}>
     <p>{tr('직접 수정해 저장하면 공통 지침과 페르소나를 합치는 자동 생성 방식을 대체합니다.','Saving changes here replaces automatic composition of shared instructions and personas.')}</p>
     <TextArea aria-label={t('rooms.systemPromptCombined')} className="font-mono !text-sm !leading-7" rows={12} disabled={saving} value={final} onChange={event=>onFinal(event.target.value)}/>
    </PromptSection>
   </>}
   <Notice message={error||tooLong&&tr('공통·최종 프롬프트는 각각 32,000자 이내로 입력하세요.','Shared and final prompts must each be within 32,000 characters.')||''}/>
   {notice&&<p role="status" className="flex items-center gap-2 text-sm text-success"><Check size={16}/>{notice}</p>}
  </div>
  <footer className="flex shrink-0 flex-wrap items-center justify-between gap-3 border-t border-divider p-4 sm:px-6">
   <span className="text-xs text-muted">{dirty?tr('저장하지 않은 변경 사항','Unsaved changes'):tr('저장 후 다음 대화부터 적용됩니다.','Saved changes apply to subsequent turns.')}</span>
   <div className="flex gap-2"><Button disabled={saving} onClick={onClose}>{t('rooms.close')}</Button><Button variant="primary" disabled={saving||loading||!dirty||tooLong} onClick={onSave}>{saving?<LoaderCircle size={16} className="animate-spin"/>:<Check size={16}/>} {t('rooms.saveChanges')}</Button></div>
  </footer>
 </DialogPanel></DialogBackdrop>
}
