/** SDK source metadata is provenance, never a verified plugin identity. */
export type SdkRun = {
 id:string;status:string;instruction?:string;providerId?:string;model?:string;sdkPluginId?:string;
 steeringGate?:{closed?:boolean};steeringRevision?:number;steering?:{revision:number;instruction:string;mode:string;at?:string}[];
 createdAt?:string;updatedAt?:string;sessionId?:string;readOnly?:boolean;
 source?:{kind?:string;id?:string;verified?:boolean}|null;
 answer?:string|null;error?:string|null;verified?:boolean;taskOutcome?:string|null;
 failureClass?:string|null;runRef?:{executionId?:string};
 checkpoint?:{rounds?:number;toolCalls?:number;pendingCall?:{id:string;name:string;arguments:unknown}}|null;
}
export const sdkActive=(run:SdkRun)=>['queued','running','awaiting_approval'].includes(run.status)
export const sdkNeedsReview=(run:SdkRun)=>['awaiting_approval','in_doubt','needs_review','interrupted'].includes(run.status)
export const sdkVerified=(run:SdkRun)=>run.status==='completed'&&run.verified===true&&run.taskOutcome==='succeeded'
export function sdkControls(run:SdkRun){
 const writable=Boolean(run.sessionId)&&!run.readOnly
 return {
  cancel:writable&&sdkActive(run),
  approve:writable&&run.status==='awaiting_approval'&&Boolean(run.checkpoint?.pendingCall?.id),
  resume:writable&&['failed','interrupted'].includes(run.status),
  recheck:writable&&run.status==='needs_review'&&run.failureClass==='completion_evidence_unavailable',
  reconcile:writable&&run.status==='in_doubt',
 }
}
export const sdkSourceKey=(run:SdkRun)=>run.sdkPluginId?JSON.stringify(['registered',run.sdkPluginId]):JSON.stringify(['source',run.source?.kind||'',run.source?.id||''])
export function sdkSources(runs:SdkRun[]){
 const groups=new Map<string,{key:string;kind:string;id:string;runs:SdkRun[];active:number;lastSeen:string}>()
 for(const run of runs){
  const kind=run.source?.kind||'',id=run.source?.id||''
  // JSON tuple avoids collisions for opaque IDs with slashes or colons.
  const key=sdkSourceKey(run)
  const group=groups.get(key)||{key,kind,id,runs:[],active:0,lastSeen:''}
  group.runs.push(run)
  if(sdkActive(run))group.active++
  const timestamp=run.updatedAt||run.createdAt||''
  if(timestamp>group.lastSeen)group.lastSeen=timestamp
  groups.set(key,group)
 }
 return [...groups.values()].sort((a,b)=>b.lastSeen.localeCompare(a.lastSeen)||a.key.localeCompare(b.key))
}
