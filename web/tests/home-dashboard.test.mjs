import test from 'node:test'
import assert from 'node:assert/strict'
import {readFileSync} from 'node:fs'
import {fileURLToPath} from 'node:url'
const root=fileURLToPath(new URL('../',import.meta.url))
const home=readFileSync(root+'src/pages/Home.tsx','utf8')
test('home stays a single task entry, not a telemetry dashboard',()=>{
 assert.match(home,/const privateSessions=sessions\.value\.filter/)
 assert.match(home,/!s\.roomId&&!s\.channelContext/)
 assert.match(home,/s\.kind!=='room'&&s\.kind!=='channel'&&!s\.archived/)
 assert.doesNotMatch(home,/data-dashboard-metric/)
 assert.doesNotMatch(home,/dashboard\.usage/)
 assert.match(home,/<NativeAgentPanel providers=\{providers\.value\}\/>/)
})
test('home exposes real owner-actionable pending approvals and uncertain jobs',()=>{
 assert.match(home,/\['awaiting_approval','needs_review'\]\.includes\(j\.status\)/)
 assert.match(home,/attention\.length>0&&<section role="status"/)
 assert.match(home,/onClick=\{\(\)=>navigate\('activity'\)\}/)
})

test('home keeps one plain greeting and one recent-work list',()=>{
 assert.match(home,/무엇을 도와드릴까요\?/)
 assert.match(home,/최근 작업/)
 assert.doesNotMatch(home,/token/i)
 assert.doesNotMatch(home,/api\/model-usage/)
})
