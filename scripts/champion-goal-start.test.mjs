import test from 'node:test'
import assert from 'node:assert/strict'
import {readFileSync} from 'node:fs'
import {fileURLToPath} from 'node:url'
import path from 'node:path'
import {DEFAULT_GOAL_BUDGET,createGoalPayload} from '../web/src/pages/home/goalStart.mjs'

const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..')
const source=(p)=>readFileSync(path.join(root,p),'utf8')

test('everyday request is passed unchanged to actual durable-goal fields',()=>{
 assert.deepEqual(createGoalPayload('  보고서 조사하고 결과를 정리해 주세요\n'),{
  goal:'보고서 조사하고 결과를 정리해 주세요',
  isolateWorktree:false,requireTests:false,
  tokenBudget:DEFAULT_GOAL_BUDGET,maxIterations:16
 })
})
test('coding task requests isolated Git and actual post-edit test receipts',()=>{
 assert.deepEqual(createGoalPayload('Fix regression','code',32000),{
  goal:'Fix regression',isolateWorktree:true,requireTests:true,
  tokenBudget:32000,maxIterations:16
 })
})
test('goal payload rejects invalid tasks or bypassed owner budgets before POST',()=>{
 for(const input of ['','  ','\n'])assert.throws(()=>createGoalPayload(input))
 for(const value of [NaN,2.5,999,2000001,Infinity])
  assert.throws(()=>createGoalPayload('test','general',value))
 assert.throws(()=>createGoalPayload('test','video'))
 assert.throws(()=>createGoalPayload('x'.repeat(16001)))
})
test('beginner homepage submits to the real Rust goal queue, not a mock',()=>{
 const home=source('web/src/pages/Home.tsx')
 const launcher=source('web/src/pages/home/TaskLauncher.tsx')
 const router=source('src/api/mod.rs')
 const admission=source('src/api/goals/lifecycle.rs')
 const app=source('web/src/App.tsx')
 assert.match(home,/<TaskLauncher navigate=\{navigate\}/)
 assert.match(launcher,/createGoalPayload\(prompt,kind,tokenBudget\)/)
 assert.match(launcher,/post<\{id:string\}>\('\/api\/goals',payload\)/)
 assert.match(router,/\/api\/goals", get\(goals::list_goals\)\.post\(goals::create_goal\)/)
 assert.match(admission,/state\.store\.transaction\(&\[/)
 assert.match(admission,/requireTests/)
 assert.match(admission,/tokenBudget/)
 assert.match(admission,/isolateWorktree/)
 assert.match(app,/valid\?name:'home'/)
})
test('homepage owner correction uses the native idempotent steer contract',()=>{
 const launcher=source('web/src/pages/home/GoalActivity.tsx')
 const router=source('src/api/mod.rs')
 const recovery=source('src/api/goals/recovery.rs')
 const core=source('src/engine/goal_steering.rs')
 assert.match(launcher,/requestId\.current\?\?crypto\.randomUUID\(\)/)
 assert.match(launcher,/text,mode:'append',requestId:id/)
 assert.match(router,/\/api\/goals\/:id\/steer/)
 assert.match(recovery,/submit_instruction\(&state,&id,text,mode,request_id\)/)
 assert.match(core,/ownerSteerReceipts/)
 assert.match(core,/state\.store\.patch_if\("goal"/)
})
test('the other product routes stay visible to non-programmers',()=>{
 const launcher=source('web/src/pages/home/TaskLauncher.tsx')
 const nav=source('web/src/components/WorkspaceShell.tsx')
 for(const target of ["page:'chat'","page:'video'","page:'rooms'"])
  assert.ok(launcher.includes(target),target)
 const primary=nav.match(/title:'nav\.group\.workspace',items:\[([^\n]+)\]/)?.[1]??''
 for(const target of ["id:'home'","id:'chat'","id:'goals'","id:'video'","id:'rooms'"])
  assert.ok(primary.includes(target),target)
})
