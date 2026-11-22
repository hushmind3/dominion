import {PromptSection,PromptConflict} from '../../features/prompts/PromptSection'
import {FormActions} from '../../components/ui/WorkspaceUI'
import {Check, Copy, RefreshCw, RotateCcw, Save} from 'lucide-react'
import {Button, SelectInput, TextArea, TextInput} from '../../components/Primitives'
import {FormField, Notice} from '../../components/UI'
import {usePromptEditor} from '../../features/prompts/usePromptEditor'

export function SystemPromptPanel() {
 const editor = usePromptEditor()
 const {tr, agents, owner, selectedAgent, ownerDraft, full, preview, busy} = editor
 const disabled = Boolean(busy)
 const conflict = ownerDraft.conflict || full.conflict
 return <div className="grid gap-6">
  <Notice message={editor.error || owner.error || agents.error || preview.error} onRetry={() => void editor.reload()}/>
  {conflict && <Notice message={tr('원본이 다른 곳에서 변경됐습니다. 작성한 내용은 유지했습니다. 원본을 확인한 후 다시 편집해 주세요.', 'The original changed elsewhere. Your draft is retained; review the current version before saving.')}/>}
  {editor.notice && <p className="flex items-center gap-2 rounded-xl border border-success/20 bg-success-soft/25 p-4 text-sm text-success" role="status"><Check size={16}/>{editor.notice}</p>}
  <PromptSection id="prompt-shared" scope={tr('전체 AI에 공통 적용', 'Shared across AIs')} title={tr('공통 프롬프트', 'Shared instructions')} controls={<><label className="flex shrink-0 items-center gap-2 text-sm [&>input]:size-4.5 [&>input]:accent-brand"><input type="checkbox" checked={ownerDraft.value.enabled} disabled={owner.loading || disabled}
     onChange={event => ownerDraft.change({...ownerDraft.value, enabled: event.target.checked})}/>{tr('사용', 'Enabled')}</label></>}>
   <p>{tr('대답하는 방식, 말투, 지켜야 할 규칙을 직접 적어 주세요.', 'Set the response style, tone and rules that should apply across your AIs.')}</p>
   <FormField label={tr('공통 지침 내용', 'Shared instruction text')}><TextArea rows={8} value={ownerDraft.value.instructions} disabled={owner.loading || Boolean(owner.error) || disabled}
    onChange={event => ownerDraft.change({...ownerDraft.value, instructions: event.target.value})}/></FormField>
   {ownerDraft.conflict&&<PromptConflict current={ownerDraft.confirmed.instructions} onContinue={ownerDraft.rebase} onDiscard={ownerDraft.discard} english={editor.en}/>}
   <FormActions description={<>{ownerDraft.value.instructions.length.toLocaleString()} / 16,000 {ownerDraft.dirty && ' · ' + tr('저장하지 않음', 'Unsaved')}</>}>
    <Button variant="ghost" disabled={!ownerDraft.dirty || disabled} onClick={ownerDraft.discard}>{tr('변경 취소', 'Discard changes')}</Button>
    <Button variant="primary" disabled={!ownerDraft.dirty || ownerDraft.conflict || disabled || ownerDraft.value.instructions.length > 16000} onClick={() => void editor.saveOwner()}><Save size={15}/>{tr('공통 프롬프트 저장', 'Save shared instructions')}</Button>
    </FormActions>
  </PromptSection>
  <PromptSection id="prompt-system" scope={editor.agentId ? selectedAgent?.name : tr('모든 일반 대화', 'All private chats')} title={tr('전체 시스템 프롬프트', 'Full system prompt')} controls={<><Button variant="ghost" disabled={!preview.value} onClick={() => void navigator.clipboard.writeText(full.value)}><Copy size={15}/>{tr('복사', 'Copy')}</Button></>}>
   <p>{tr('실제 모델에 전달할 시스템 텍스트를 직접 수정할 수 있습니다. 직접 저장하면 이 범위의 기본 지침·공통 프롬프트·페르소나를 대체합니다. 그룹별 지침은 이 메뉴의 그룹 지침 탭에서 편집합니다.', 'Edit the exact system text sent to the model. Saving an override replaces generated identity, shared instructions and persona in this scope. Group instructions are edited in the Group instructions tab here.')}</p>
   <FormField label={tr('적용 범위', 'Override scope')}><SelectInput value={editor.agentId} disabled={disabled} onChange={event => editor.setAgentId(event.target.value)}>
    <option value="">{tr('공통 · 모든 일반 대화', 'Shared · all private chats')}</option>{agents.value.map(agent => <option key={agent.id} value={agent.id}>{agent.name}</option>)}
   </SelectInput></FormField>
   <FormField label={tr('모델에 전송하는 시스템 텍스트', 'Model-facing system text')}><TextArea rows={16} className="font-mono !text-sm !leading-7" value={full.value} disabled={disabled || !preview.value || preview.loading} onChange={event => full.change(event.target.value)}/></FormField>
   <p className="text-xs leading-6 text-muted">{tr('빈 내용으로 저장할 수도 있습니다. 실행 권한·도구 제한은 프롬프트와 별도로 적용됩니다.', 'You can save an empty prompt. Tool permissions and execution limits are enforced separately.')}</p>
   {full.conflict&&<PromptConflict current={full.confirmed} onContinue={full.rebase} onDiscard={full.discard} english={editor.en}/>}
   <FormActions description={<>{full.value.length.toLocaleString()} / 32,000 {full.dirty && ' · ' + tr('저장하지 않음', 'Unsaved')}</>}>
    <Button variant="ghost" disabled={!full.dirty || disabled} onClick={full.discard}>{tr('변경 취소', 'Discard changes')}</Button>
    <Button variant="secondary" disabled={disabled || !preview.value?.ownOverrideActive} onClick={() => void editor.saveFull(true)}><RotateCcw size={15}/>{tr('자동 지침으로 복원', 'Restore generated')}</Button>
    <Button variant="primary" disabled={disabled || !full.dirty || full.conflict || full.value.length > 32000} onClick={() => void editor.saveFull()}><Save size={15}/>{tr('시스템 프롬프트 저장', 'Save system prompt')}</Button>
    </FormActions>
   <details className="border-t border-divider pt-4 [&>summary]:cursor-pointer [&>summary]:text-sm"><summary>{tr('실제 입력 미리보기와 추가 문맥', 'Preview options and retrieved context')}</summary>
    <div className="grid gap-4 py-5 sm:grid-cols-2 [&>label]:flex [&>label]:gap-2">
     <FormField label={tr('대화 미리보기', 'Preview conversation')}><SelectInput value={editor.sessionId} onChange={event => editor.setSessionId(event.target.value)}><option value="">{tr('새 일반 대화', 'New private chat')}</option>{editor.sessions.value.filter(session => !session.roomId).map(session => <option key={session.id} value={session.id}>{session.title}</option>)}</SelectInput></FormField>
     <FormField label={tr('미리보기 질문', 'Sample question')}><TextInput maxLength={512} value={editor.query} onChange={event => editor.setQuery(event.target.value)}/></FormField>
     <label><input type="checkbox" checked={editor.tools} onChange={event => editor.setTools(event.target.checked)}/>{tr('도구 사용', 'Tools enabled')}</label>
     <label><input type="checkbox" checked={editor.korean} onChange={event => editor.setKorean(event.target.checked)}/>{tr('한국어 추론', 'Korean reasoning')}</label>
     <Button variant="secondary" disabled={!editor.query.trim()} onClick={editor.refreshPreview}><RefreshCw size={15}/>{tr('미리보기 갱신', 'Refresh preview')}</Button>
    </div>
    <pre className="max-h-80 overflow-auto whitespace-pre-wrap break-words rounded-xl bg-canvas p-4 text-xs leading-6">{preview.value?.dynamicContext || tr('추가 문맥 없음', 'No retrieved context')}</pre>
   </details>
  </PromptSection>
 </div>
}
