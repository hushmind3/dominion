import test from 'node:test'
import assert from 'node:assert/strict'
import {readFileSync} from 'node:fs'
import {fileURLToPath} from 'node:url'
const root=fileURLToPath(new URL('../',import.meta.url))
const home=readFileSync(root+'src/pages/Home.tsx','utf8')
test('home counts only private conversations, never Room or external channels',()=>{
 assert.match(home,/const privateSessions=sessions\.value\.filter/)
 assert.match(home,/!s\.roomId&&!s\.channelContext/)
 assert.match(home,/s\.kind!=='room'&&s\.kind!=='channel'&&!s\.archived/)
 assert.match(home,/dashboard\.metric\.chat',value:privateSessions\.length/)
})
test('home exposes real owner-actionable pending approvals and uncertain jobs',()=>{
 assert.match(home,/\['awaiting_approval','needs_review'\]\.includes\(j\.status\)/)
 assert.match(home,/attention\.length>0&&<section role="status"/)
 assert.match(home,/onClick=\{\(\)=>navigate\('activity'\)\}/)
})
