export type GoalStartKind='general'|'code'
export interface GoalStartPayload{
 goal:string
 isolateWorktree:boolean
 requireTests:boolean
 tokenBudget:number
 maxIterations:number
}
export declare const DEFAULT_GOAL_BUDGET:number
export declare function createGoalPayload(input:string,kind?:GoalStartKind,tokenBudget?:number):GoalStartPayload
