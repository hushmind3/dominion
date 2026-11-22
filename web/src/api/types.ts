/** Public wire records. Product operations and browser state do not live here. */
export type RecordItem = {
  id: string
  name?: string
  createdAt?: string
  updatedAt?: string
  [key: string]: any
}

export type Provider = RecordItem & {
  name: string
  baseUrl: string
  kind: string
  enabled: boolean
  hasKey?: boolean
  model?: string
  learning?: boolean
  reflection?: boolean
  reflectionModel?: string
  accountId?: string
}

export type Agent = RecordItem & {
  name: string
  description?: string
  systemPrompt: string
  providerId: string
  model: string
  reasoningEffort?: string
  thinking?: boolean
  color?: string
  aliases?: string[]
  avatarUrl?: string
  personaSource?: string
  sourceProfile?: string
  role?: string
}

export type MessageUsage = {
  promptTokens?: number
  cachedPromptTokens?: number | null
  uncachedPromptTokens?: number | null
  completionTokens?: number
  totalTokens?: number
  estimated?: boolean
  model?: string
  providerId?: string
}

export type ChatMessage = {
  id: string
  session_id: string
  role: string
  content: string
  reasoning?: string
  agent_id?: string
  tool_calls?: any[]
  tool_call_id?: string
  metadata?: {
    authorKind?: string
    roomId?: string
    replyToMessageId?: string
    executionId?: string
    usage?: MessageUsage
    [key: string]: unknown
  }
  created_at: string
}

export type ChatSession = RecordItem & {
  title: string
  agentId: string
  chatMode?: 'chat' | 'agent'
  providerId?: string
  model?: string
  roomId?: string
}

export type Attachment = {
  id: string
  name: string
  mimeType: string
  size: number
  isImage: boolean
  url: string
  sessionId: string
  roomId?: string | null
  createdAt?: string
}

export type Room = RecordItem & {
  name: string
  agentIds: string[]
  rounds: number
  maxTurns?: number
  sessionId?: string
}

export type ModelOption = {
  id: string
  isDefault?: boolean
  name?: string
  free?: boolean | null
  freeStatus?: 'free' | 'paid' | 'unknown'
  freeSource?: string
  reasoningEfforts?: string[]
  defaultReasoningEffort?: string | null
  catalogStale?: boolean
}

export type Approval = {id: string; tool: string; args: unknown; status: string; sessionId: string}
export type StreamEvent = {type: string; [key: string]: any}

export type ChatRequest = {
  sessionId: string
  clientTurnId?: string
  requestId?: string
  conversationMode?: 'chat' | 'agent'
  agentId?: string
  directChat?: boolean
  providerId?: string
  model?: string
  input: string
  toolsEnabled: boolean
  toolsExecutionEnabled: boolean
  autoLearning: boolean
  reasoningLanguage?: string
  reasoningEffort?: string
  thinking?: boolean
  attachmentIds?: string[]
}
