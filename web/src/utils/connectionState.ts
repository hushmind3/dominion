/** UI-only selectors for the existing channel catalog and stored bindings.
 * No secret values or connection side effects are persisted here. */
export type ChannelField = {
  key:string
  label:string
  type:'secret'|'text'|'url'
  required?:boolean
  placeholder?:string
}
export type ChannelEntry = {
  id:string
  name:string
  category:string
  mode:string
  description:string
  docs:string
  available:boolean
  status:string
  features:string[]
  fields:ChannelField[]
  instructions?:string[]
  requiresPublicUrl?:boolean
  requiresRoom?:boolean
  maxRoomParticipants?:number
}
export type ChannelBinding = {
  id:string
  name:string
  kind:string
  agentId:string
  providerId:string
  model:string
  enabled:boolean
  mentionOnly:boolean
  status:string
  configured:boolean
  roomId?:string
  roomPolicy?:string
  runtimeError?:string
  missing?:string[]
  credentialsPresent?:Record<string,boolean>
  channelId?:string
  chatId?:string
  delivery?:{pending?:number;sending?:number;sent?:number;needsReview?:number;lastError?:string}
  toolsEnabled?:boolean
  replyMentionBots?:boolean
  syncRoomMessages?:boolean
  webhookPath?:string
}
export type ChannelDraft = Partial<ChannelBinding> & Record<string,unknown>

export function channelsByKind(channels:ChannelBinding[]):Map<string,ChannelBinding[]> {
  const grouped=new Map<string,ChannelBinding[]>()
  for(const channel of channels){
    const key=channel.kind.toLowerCase()
    const group=grouped.get(key)||[]
    group.push(channel)
    grouped.set(key,group)
  }
  return grouped
}

/** Installed records first; within each group working drivers precede unavailable placeholders. */
export function visibleChannelCatalog(
  entries:ChannelEntry[],byKind:Map<string,ChannelBinding[]>,query:string,
  category:string,onlyConfigured:boolean,
):ChannelEntry[] {
  const term=query.trim().toLocaleLowerCase()
  const unique=new Map<string,ChannelEntry>()
  for(const entry of entries){
    const key=entry.id.toLowerCase()
    const bindings=byKind.get(key)||[]
    if(category!=='all'&&entry.category!==category)continue
    if(onlyConfigured&&!bindings.length)continue
    const haystack=[entry.name,entry.id,entry.description,...bindings.map(b=>b.name)].join(' ').toLocaleLowerCase()
    if(term&&!haystack.includes(term))continue
    if(!unique.has(key))unique.set(key,entry)
  }
  return [...unique.values()].sort((a,b)=>
    Number((byKind.get(b.id.toLowerCase())||[]).length>0)-
    Number((byKind.get(a.id.toLowerCase())||[]).length>0)||
    Number(b.available)-Number(a.available)||
    a.name.localeCompare(b.name)
  )
}

export function channelDraftMissing(entry:ChannelEntry,record:ChannelDraft):string[]{
  const missing:string[]=[]
  if(typeof record.name!=='string'||!record.name.trim())missing.push('name')
  for(const field of entry.fields){
    if(!field.required)continue
    const value=record[field.key]
    const supplied=typeof value==='string'&&Boolean(value.trim())
    const retained=field.type==='secret'&&Boolean(record.credentialsPresent?.[field.key])
    if(!supplied&&!retained)missing.push(field.key)
  }
  if(entry.requiresRoom&&record.enabled===true&&!record.roomId)missing.push('roomId')
  return missing
}

export function channelStatusTone(channel:ChannelBinding):'online'|'pending'|'error'|'disabled'{
  if(!channel.enabled||channel.status==='disabled')return 'disabled'
  if(channel.status==='error'||channel.runtimeError)return 'error'
  if(!channel.configured)return 'error'
  if(channel.status==='connected'||channel.status==='outbound_ready'||channel.status==='webhook_ready')return 'online'
  return 'pending'
}
