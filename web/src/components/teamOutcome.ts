/** Native Team status is not the same as integrated deliverables.
 * The saved pendingIntegrationJobs array is a historical integration manifest:
 * it remains populated after team_integrate completes. Check integratedAt
 * before showing anything as requiring approval.
 */
export type TeamReviewSource={
 status?:string
 integratedAt?:string
 result?:Record<string,unknown>|null
}
export type TeamReviewState={kind:'none'|'integrated'|'pending'|'gaps'|'review';count:number}
export function classifyTeamReview(team:TeamReviewSource):TeamReviewState{
 const gaps=Array.isArray(team.result?.gaps)?team.result.gaps.length:0
 const selected=Array.isArray(team.result?.pendingIntegrationJobs)
  ?team.result.pendingIntegrationJobs.length:0
 if(team.status==='completed'&&typeof team.integratedAt==='string'&&team.integratedAt.length>0)
  return {kind:'integrated',count:selected}
 if(team.status!=='needs_review')return {kind:'none',count:0}
 if(gaps>0)return {kind:'gaps',count:gaps}
 if(selected>0)return {kind:'pending',count:selected}
 return {kind:'review',count:0}
}
