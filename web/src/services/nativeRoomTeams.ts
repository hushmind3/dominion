export type NativeRoomJob={
 id:string;kind?:string;status?:string;sessionId?:string;createdAt?:string;queuedAt?:string;error?:string;
 input?:{uiSource?:string;roomTeamLaunch?:boolean;roomId?:string;parentSessionId?:string;goal?:string;maxParallel?:number}
}
export type NativeTeamTask={
 id:string;agentId?:string;status?:string;displayState?:string;verified?:boolean;
 pendingApproval?:boolean;dependsOn?:string[];waitingPredecessors?:boolean;error?:string
}
export type NativeRoomTeam={
 id:string;leadSessionId?:string;status?:string;verified?:boolean;tasks?:NativeTeamTask[];
 resumeAvailable?:boolean
}
export type NativeTeamActivity={teams:NativeRoomTeam[];leadJobs?:unknown[];remoteWorkers?:unknown[]}
export type RoomTeamRun={lead:NativeRoomJob;teams:NativeRoomTeam[]}

/** A native Team belongs to a room only through its persisted Lead job
 * session. Never infer ownership from names, shared agent IDs or timestamps. */
export function selectNativeRoomTeamRuns(
 jobs:NativeRoomJob[],teams:NativeRoomTeam[],roomId:string,sessionId?:string,
):RoomTeamRun[]{
 return jobs.filter(job=>
  job.kind==='prompt'&&job.input?.roomTeamLaunch===true
  &&job.input?.roomId===roomId
  &&Boolean(sessionId)&&job.input?.parentSessionId===sessionId
 ).sort((a,b)=>(b.queuedAt||b.createdAt||'').localeCompare(a.queuedAt||a.createdAt||''))
 .slice(0,12).map(lead=>({
  lead,teams:teams.filter(team=>Boolean(lead.sessionId)&&team.leadSessionId===lead.sessionId)
 }))
}
