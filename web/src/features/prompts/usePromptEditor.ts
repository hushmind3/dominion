import {useEffect, useState} from 'react'
import {post, koreanReasoningEnabled, type Agent, type ChatSession} from '../../api'
import {useRemote} from '../../hooks/useRemote'
import {refresh} from '../../dataRefresh'
import {useLocale} from '../../i18n'
import {usePromptDraft} from './usePromptDraft'

type Owner = {instructions: string; enabled: boolean}
export type PromptPreview = {
 systemPrompt: string; dynamicContext: string; systemChars: number; systemTokensEstimated: number
 contextChars: number; contextTokensEstimated: number; ownOverrideActive: boolean
 ownOverrideText?: string | null; effectiveOverrideScope?: 'agent' | 'global' | null
}

/** Owns prompt reads, scoped drafts and explicit writes; rendering owns no API calls. */
export function usePromptEditor() {
 const en = useLocale() === 'en'
 const tr = (ko: string, english: string) => en ? english : ko
 const agents = useRemote<Agent[]>('/api/agents', [])
 const sessions = useRemote<ChatSession[]>('/api/sessions', [])
 const owner = useRemote<Owner>('/api/settings/assistant', {instructions: '', enabled: true})
 const [agentId, setAgentId] = useState('')
 const [sessionId, setSessionId] = useState('')
 const [query, setQuery] = useState(tr('일반적인 질문', 'A general question'))
 const [previewQuery, setPreviewQuery] = useState(query)
 const [tools, setTools] = useState(true)
 const [korean, setKorean] = useState(koreanReasoningEnabled)
 const params = new URLSearchParams({input: previewQuery, toolsEnabled: String(tools), koreanReasoning: String(korean)})
 if (agentId) params.set('agentId', agentId)
 if (sessionId) params.set('sessionId', sessionId)
 const preview = useRemote<PromptPreview | null>('/api/settings/system-prompt/preview?' + params, null)
 const selectedAgent = agents.value.find(agent => agent.id === agentId)
 const ownerDraft = usePromptDraft('prompt:owner', {instructions: owner.value.instructions, enabled: owner.value.enabled !== false}, !owner.loading && !owner.error)
 const persona = usePromptDraft('prompt:persona:' + agentId, selectedAgent?.systemPrompt || '', Boolean(selectedAgent))
 const full = usePromptDraft('prompt:system:' + agentId, preview.value?.ownOverrideText ?? preview.value?.systemPrompt ?? '', Boolean(preview.value) && !preview.loading)
 const [busy, setBusy] = useState<'owner' | 'persona' | 'full' | null>(null)
 const [error, setError] = useState('')
 const [notice, setNotice] = useState('')
 useEffect(() => {setError(''); setNotice('')}, [agentId])
 const saveOwner = async () => {
  if (busy || !ownerDraft.dirty || ownerDraft.conflict) return
  setBusy('owner'); setError(''); setNotice('')
  try {
   const saved = await post<Owner>('/api/settings/assistant', {...ownerDraft.value, expectedInstructions: ownerDraft.baseline.instructions, expectedEnabled: ownerDraft.baseline.enabled})
   ownerDraft.accept({instructions: saved.instructions, enabled: saved.enabled !== false})
   owner.setValue(saved); refresh('/api/settings/assistant'); await preview.reload()
   setNotice(tr('공통 프롬프트를 저장했습니다. 다음 대화부터 적용됩니다.', 'Shared instructions saved for subsequent turns.'))
  } catch (e) {setError((e as Error).message)} finally {setBusy(null)}
 }
 const savePersona = async () => {
  if (busy || !selectedAgent || !persona.dirty || persona.conflict) return
  setBusy('persona'); setError(''); setNotice('')
  try {
   const saved = await post<Agent>('/api/agents/'+encodeURIComponent(agentId)+'/persona', {prompt:persona.value,expectedPrompt:persona.baseline})
   persona.accept(saved.systemPrompt || ''); agents.setValue(rows => rows.map(agent => agent.id === saved.id ? saved : agent))
   refresh('/api/agents'); await preview.reload()
   setNotice(tr('페르소나를 저장했습니다. 다음 대화부터 적용됩니다.', 'Persona saved for subsequent turns.'))
  } catch (e) {setError((e as Error).message)} finally {setBusy(null)}
 }
 const saveFull = async (reset = false) => {
  if (busy || !preview.value || (!reset && (!full.dirty || full.conflict || full.value.length > 32000))) return
  if (reset && !window.confirm(tr('직접 지정한 시스템 프롬프트를 해제하고 자동 생성 지침으로 복원할까요?', 'Remove the override and restore generated instructions?'))) return
  setBusy('full'); setError(''); setNotice('')
  try {
   await post('/api/settings/system-prompt/override', {...(agentId ? {agentId} : {}), prompt: reset ? null : full.value,
    expectedPrompt: preview.value.ownOverrideText ?? null})
   full.discard(); await preview.reload()
   refresh(agentId ? '/api/agents' : '/api/settings/assistant')
   setNotice(tr(reset ? '자동 생성 시스템 프롬프트로 복원했습니다.' : '시스템 프롬프트를 저장했습니다. 다음 일반 대화부터 적용됩니다.',
    reset ? 'Generated instructions restored.' : 'System prompt saved for subsequent private-chat turns.'))
  } catch (e) {setError((e as Error).message)} finally {setBusy(null)}
 }
 const reload = async () => {await Promise.all([owner.reload(), agents.reload(), sessions.reload(), preview.reload()])}
 return {en, tr, agents, sessions, owner, agentId, setAgentId, sessionId, setSessionId, selectedAgent, query, setQuery,
  tools, setTools, korean, setKorean, preview, ownerDraft, persona, full, busy, error, notice,
  saveOwner, savePersona, saveFull, reload, refreshPreview: () => {
   const next = query.trim()
   if (next === previewQuery) void preview.reload()
   else setPreviewQuery(next)
  }}
}
