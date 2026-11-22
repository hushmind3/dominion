import type {Agent, ChatSession, Provider} from '../../api'

type Override = {providerId: string; model: string}

/** Readiness comes from saved connection facts, never a preferred vendor. */
export function providerConfigured(provider: Provider) {
 return provider.enabled !== false && Boolean(provider.hasKey || provider.accountId || provider.local ||
  provider.kind === 'cli' || provider.kind === 'ollama' || provider.auth === 'none' ||
  provider.requiresKey === false || provider.integration === 'freellmapi')
}

export function resolveConversationRoute({providers, sessions, session, agent, manual, restoring}: {
 providers: Provider[]; sessions: ChatSession[]; session?: ChatSession; agent?: Agent; manual?: Override; restoring: boolean
}) {
 const enabled = providers.filter(provider => provider.enabled !== false)
 const recent = sessions.find(item => !item.roomId && !item.agentId && item.model &&
  enabled.some(provider => provider.id === item.providerId && providerConfigured(provider)))
 const preferred = enabled.find(provider => provider.id === recent?.providerId) ||
  enabled.find(provider => provider.model && providerConfigured(provider)) || enabled.find(providerConfigured)
 // A saved route that is unavailable stays explicit. Never silently send the
 // owner's next message to a different service while restoring a conversation.
 const providerId = manual?.providerId ?? (agent?.providerId || session?.providerId || (!restoring ? preferred?.id : '') || '')
 const provider = enabled.find(item => item.id === providerId)
 const model = manual?.model || (agent?.providerId === providerId ? agent.model : '') ||
  (session?.providerId === providerId ? session.model : '') ||
  (!session && recent?.providerId === providerId ? recent.model : '') || provider?.model || ''
 return {providerId, provider, model, providers: enabled, configured: Boolean(provider && providerConfigured(provider))}
}
