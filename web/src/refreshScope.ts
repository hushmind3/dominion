/** Scoped SSE query invalidations, separate from manual full refresh.
 * Unknown future event kinds intentionally fall back to the legacy global
 * refresh. Only known, stable record contracts receive narrow updates.
 */
export type BusRefreshEvent={type:string;kind?:string}
const recordRoots:Record<string,readonly string[]>={
 session:['sessions','rooms'],
 message:['sessions','rooms'],
 room:['rooms','sessions'],
 room_state:['rooms'],
 agent:['agents','rooms'],
 provider:['providers','model-library'],
 model:['providers','model-library'],
 job:['jobs','rooms'],
 execution:['executions','jobs','rooms'],
 execution_operation:['executions','jobs','rooms'],
 approval:['approvals','jobs','rooms'],
 policy:['policy','tools'],
 mcp_server:['mcp','tools'],
 mcp:['mcp','tools'],
 oauth_config:['oauth'],
 oauth_connection:['oauth'],
 channel:['channels','rooms'],
 channel_status:['channels'],
 skill:['skills','extensions','capabilities','tools'],
 capability:['capabilities','tools'],
 memory:['memory','memories'],
 memory_entry:['memory','memories'],
 model_install:['model-library','providers'],
 browser_session:['browser'],
 goal:['goals','jobs'],
 audit:['audit'],
}
const eventRoots:Record<string,readonly string[]>={
 message:['sessions','rooms'],
 job_event:['jobs','rooms'],
 audit:['audit'],
 approval:['approvals','jobs','rooms'],
 channel_status:['channels'],
 goal_event:['goals','jobs'],
 execution_event:['executions','jobs','rooms'],
}
export function refreshScopesForEvent(event:BusRefreshEvent):readonly string[]|null{
 if(event.type==='record'){
  return typeof event.kind==='string'
   ?recordRoots[event.kind]??null:null
 }
 return eventRoots[event.type]??null
}
export function endpointAffected(endpoint:string,scopes:readonly string[]):boolean{
 const root=/^\/api\/([^/?]+)/.exec(endpoint)?.[1]
 // Unrecognized endpoints are intentionally conservative: refresh normally.
 if(!root)return true
 return scopes.includes(root)||scopes.includes('*')
}
