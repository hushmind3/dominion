/** Pure SSE invalidation map. Unknown events do not invalidate the whole UI. */
export type RefreshScope = {targets:string[];sessions:string[];rooms:string[]}
type BusRow = Record<string,unknown>
export type BusChange = {type?:string;kind?:string;record?:BusRow;message?:BusRow;approval?:BusRow;sessionId?:string;roomId?:string;jobId?:string}
const field=(row:BusRow|undefined,name:string):string|undefined=>{
 const value=row?.[name]
 return typeof value==='string'&&value.length>0?value:undefined
}
const child=(row:BusRow|undefined,name:string):BusRow|undefined=>{
 const value=row?.[name]
 return value&&typeof value==='object'&&!Array.isArray(value)?value as BusRow:undefined
}
const encoded=(value:string)=>encodeURIComponent(value)

export function classifyRefreshEvent(event:BusChange):RefreshScope {
 const targets=new Set<string>(), sessions=new Set<string>(), rooms=new Set<string>()
 const add=(...routes:string[])=>routes.forEach(route=>targets.add(route))
 const row=event.record
 const input=child(row,'input')
 const session=field(row,'sessionId')||field(input,'sessionId')||field(event.message,'session_id')||field(event.message,'sessionId')||field(event.approval,'sessionId')||event.sessionId
 const room=field(row,'roomId')||field(input,'roomId')||event.roomId
 if(session)sessions.add(session)
 if(room)rooms.add(room)
 if(event.type==='record'){
  const id=field(row,'id')
  switch(event.kind){
   case 'session':add('/api/sessions');break
   case 'message':add('/api/sessions');if(session)add('/api/sessions/'+encoded(session)+'/messages');break
   case 'room':add('/api/rooms');if(id){rooms.add(id);add('/api/rooms/'+encoded(id)+'/state')}break
   case 'room_state':if(room)add('/api/rooms/'+encoded(room)+'/state');break
   case 'room_knowledge':if(room)add('/api/rooms/'+encoded(room)+'/knowledge');break
   case 'agent':add('/api/agents');break
   case 'agent_growth':{
    add('/api/synergy')
    const agentId=field(row,'agentId')
    if(agentId)add('/api/agents/'+encoded(agentId)+'/growth')
    break
   }
   case 'provider':add('/api/providers');break
   case 'algorithm':case 'algorithm_stat':case 'algorithm_usage':
    add('/api/algorithms','/api/synergy');break
   case 'job':add('/api/jobs');if(id)add('/api/jobs/'+encoded(id));break
   case 'dev_job':if(id)add('/api/dev/jobs/'+encoded(id));break
   case 'remote_task':add('/api/remote/tasks');if(id)add('/api/remote/tasks/'+encoded(id));break
   case 'execution':add('/api/executions');break
   case 'execution_operation':{
    add('/api/executions')
    const runId=field(row,'runId')
    if(runId)add('/api/executions/'+encoded(runId)+'/operations')
    break
   }
   case 'approval':add('/api/approvals');break
   case 'mcp':add('/api/mcp','/api/mcp/discover');break
   case 'skill':add('/api/skills');break
   case 'channel':add('/api/channels');break
   case 'channel_outbox':{
    add('/api/channels')
    const bindingId=field(row,'bindingId')||field(row,'channelId')
    if(bindingId)add('/api/channels/'+encoded(bindingId)+'/delivery/reviews')
    break
   }
   case 'experience_principle':case 'experience_rule':
    add('/api/transfer','/api/synergy');break
   case 'policy':add('/api/policy');break
   case 'owner_profile':add('/api/settings/owner-profile');break
   case 'assistant_settings':add('/api/settings/assistant');break
   case 'oauth_config':case 'oauth_connection':add('/api/oauth/providers','/api/oauth/connections');break
   case 'sdk_run':add('/api/sdk/runs');if(id)add('/api/sdk/runs/'+encoded(id));break
   case 'memory':add('/api/memories','/api/memory/overview','/api/memory/*');break
   case 'experience':{
    add('/api/experience','/api/transfer','/api/synergy')
    const agentId=field(row,'agentId')
    if(agentId)add('/api/agents/'+encoded(agentId)+'/growth')
    break
   }
   default:
    if(event.kind?.startsWith('model_'))add('/api/model-library/*')
    break
  }
  if(event.kind==='job'&&field(row,'kind')?.startsWith('model_'))add('/api/model-library/*')
 }else if(event.type==='message'){
  add('/api/sessions')
  if(session)add('/api/sessions/'+encoded(session)+'/messages')
 }else if(event.type==='approval'){
  add('/api/approvals')
 }else if(event.type==='job_event'){
  add('/api/jobs')
  if(event.jobId)add('/api/jobs/'+encoded(event.jobId))
 }else if(event.type==='execution_event'){
  add('/api/executions')
 }else if(event.type==='goal_event'){
  add('/api/jobs')
 }else if(event.type==='channel_status'){
  add('/api/channels')
 }else if(event.type==='audit'){
  add('/api/audit/*','/api/audit')
 }
 return {targets:[...targets],sessions:[...sessions],rooms:[...rooms]}
}

/** A trailing /* matches a nested API family, never unrelated paths. */
export function matchesRefreshTarget(endpoint:string|undefined,targets:readonly string[]):boolean {
 if(!endpoint)return false
 const path=endpoint.split('?')[0]
 return targets.some(target=>target==='*'||target===path||
  (target.endsWith('/*')&&path.startsWith(target.slice(0,-1))))
}

export function matchesRoomRefresh(scope:RefreshScope,roomId:string,sessionId?:string):boolean {
 return scope.rooms.includes(roomId)||
  (Boolean(sessionId)&&scope.sessions.includes(sessionId!))
}

/** Combine bursts into one deterministic scope; no per-event network fetch. */
export function mergeRefreshScopes(base:RefreshScope,next:RefreshScope):RefreshScope {
 return {
  targets:[...new Set([...base.targets,...next.targets])],
  sessions:[...new Set([...base.sessions,...next.sessions])],
  rooms:[...new Set([...base.rooms,...next.rooms])]
 }
}
export const emptyRefreshScope=():RefreshScope=>({targets:[],sessions:[],rooms:[]})
