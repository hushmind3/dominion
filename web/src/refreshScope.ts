// Explicit SSE invalidation boundaries. Unknown records remain conservative:
// refresh all consumers rather than risking stale owner-facing data.
// Manual refresh() still emits an unscoped event and refreshes everything.
const routes: Record<string, readonly string[]> = {
  message: ['/api/sessions','/api/rooms','/api/synergy','/api/model-usage'],
  'record:execution_operation': [
    '/api/executions','/api/jobs','/api/rooms','/api/synergy',
    '/api/agents','/api/status'
  ],
  'record:execution': [
    '/api/executions','/api/jobs','/api/rooms','/api/synergy',
    '/api/agents','/api/sessions','/api/status','/api/model-usage'
  ],
  'record:job': [
    '/api/jobs','/api/rooms','/api/synergy','/api/sessions',
    '/api/agents','/api/status'
  ],
  job_event: [
    '/api/jobs','/api/rooms','/api/synergy','/api/sessions',
    '/api/agents','/api/status'
  ],
  audit: ['/api/audit'],
  approval: ['/api/approvals','/api/policy','/api/tools'],
  channel_status: ['/api/channels','/api/rooms','/api/status'],
}
export function refreshScopeForEvent(event: {type?: unknown; kind?: unknown}): string {
  const type=typeof event.type==='string'?event.type:''
  return type==='record' && typeof event.kind==='string' && event.kind.length>0
    ? 'record:'+event.kind : type
}
export function shouldRefreshEndpoint(endpoint: string, scopes: readonly string[]): boolean {
  if (!scopes.length) return true
  const path=endpoint.split('?')[0]
  return scopes.some(scope=>{
    const prefixes=routes[scope]
    if (!prefixes) return true
    return prefixes.some(prefix=>path===prefix || path.startsWith(prefix+'/'))
  })
}
