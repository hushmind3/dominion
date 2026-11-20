export type ChannelField={key:string;label:string;type:'secret'|'text'|'url';required?:boolean;placeholder?:string}
export type ChannelEntry={
 id:string;name:string;category:string;mode:string;description:string;docs:string;
 available:boolean;status:string;features:string[];fields:ChannelField[];
 instructions?:string[];requiresPublicUrl?:boolean;requiresRoom?:boolean;maxRoomParticipants?:number
}
export type Channel={
 id:string;name:string;kind:string;agentId:string;providerId:string;model:string;
 enabled:boolean;mentionOnly:boolean;roomId?:string;roomPolicy?:string;status:string;
 runtimeError?:string;configured:boolean;missing?:string[];credentialsPresent?:Record<string,boolean>;
 channelId?:string;chatId?:string;delivery?:{
  pending?:number;sending?:number;sent?:number;needsReview?:number;lastError?:string
 };
 toolsEnabled?:boolean;replyMentionBots?:boolean;webhookPath?:string
}
export type ChannelHealth='setup'|'issue'|'connected'|'outbound'|'paused'|'disabled'|'starting'|'stopped'
export function channelHealth(c:Channel):ChannelHealth{
 if(!c.configured || (c.missing?.length??0)>0)return 'setup'
 if(c.status==='error'||Boolean(c.runtimeError))return 'issue'
 if((c.delivery?.needsReview??0)>0)return 'issue'
 if(!c.enabled||c.status==='disabled')return 'disabled'
 if(c.status==='connected')return 'connected'
 if(c.status==='outbound_ready')return 'outbound'
 if(c.status==='sync_disabled')return 'paused'
 if(c.status==='starting'||c.status==='reconnecting')return 'starting'
 return 'stopped'
}
export function sortChannelBindings(bindings:Channel[]):Channel[]{
 const priority:Record<ChannelHealth,number>={
  issue:0,setup:1,starting:2,connected:3,outbound:4,paused:5,disabled:6,stopped:7
 }
 return [...bindings].sort((a,b)=>priority[channelHealth(a)]-priority[channelHealth(b)]
  ||a.name.localeCompare(b.name))
}
export function matchingChannels(
 entries:ChannelEntry[],term:string,category:string,
):ChannelEntry[]{
 const needle=term.trim().toLocaleLowerCase()
 return entries.filter(entry=>(category==='all'||entry.category===category)
  &&[entry.name,entry.id,entry.description,...(entry.features||[])].join(' ').toLocaleLowerCase().includes(needle))
  .sort((a,b)=>Number(b.available)-Number(a.available)||a.name.localeCompare(b.name))
}
export function channelDraftMissing(entry:ChannelEntry,record:Record<string,unknown>):string[]{
 const missing=entry.fields.filter(field=>field.required
  &&!(typeof record[field.key]==='string' && (record[field.key] as string).trim())
  &&!(field.type==='secret' && (record.credentialsPresent as Record<string,boolean>|undefined)?.[field.key]))
  .map(field=>field.label)
 if(entry.requiresRoom&&!String(record.roomId||'').trim())missing.push('roomId')
 if(!String(record.name||'').trim())missing.push('name')
 return missing
}
