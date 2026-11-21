// Runtime UX ↔ API contract guards. Browser behavior and native permission
// checks still need end-to-end tests; these protect structural regressions.
import {test} from 'node:test'
import assert from 'node:assert/strict'
import {readFileSync} from 'node:fs'
import {fileURLToPath} from 'node:url'
const base=fileURLToPath(new URL('../',import.meta.url))
const source=path=>readFileSync(base+path,'utf8')
const remote=source('src/components/RemoteWorkPanel.tsx')
const jobs=source('src/components/TeamWorkflowPanel.tsx')
const timeline=source('src/components/RunEventTimeline.tsx')
const sdk=source('src/components/NativeAgentPanel.tsx')
const usage=source('src/components/ExecutionInsights.tsx')
const operations=source('src/pages/Operations.tsx')
const rooms=source('src/pages/Rooms.tsx')

test('remote patch evidence cannot survive a selected-job switch',()=>{
 assert.match(remote,/<PatchReview key=\{selectedJob\.id\} jobId=\{selectedJob\.id\}/)
 assert.match(remote,/evidence\.manifest\.jobId!==jobId/)
 assert.match(remote,/setPatch\(evidence\)/)
 assert.match(remote,/status==='quarantined'/)
 assert.match(remote,/jobStatus==='owner_accepted'/)
 assert.match(remote,/does NOT apply or merge|파일 적용·병합/)
})
test('SDK usage preserves missing token data instead of crashing the panel',()=>{
 assert.match(sdk,/prompt_tokens\?:number\|null/)
 assert.match(sdk,/cached_prompt_tokens\?:number\|null/)
 assert.match(sdk,/fmtTokens\(run\.checkpoint\.usage\.cached_prompt_tokens\)/)
 assert.doesNotMatch(sdk,/cached_prompt_tokens\.toLocaleString\(\)/)
 assert.match(sdk,/selectionGeneration\.current/)
 assert.match(sdk,/selected===selectionGeneration\.current/)
})
test('run timeline ignores earlier execution fetches and retains truncation warnings',()=>{
 assert.match(timeline,/new AbortController\(\)/)
 assert.match(timeline,/inFlight\.current\?\.abort\(\)/)
 assert.match(timeline,/token!==epoch\.current/)
 assert.match(timeline,/controller\.signal/)
 assert.match(timeline,/previous\|\|page\.historyTruncated/)
 assert.match(timeline,/page\.nextAfter/)
})
test('team DAG and external resumption are backed by persisted job identities',()=>{
 assert.match(jobs,/j\.input\?\.plan/)
 assert.match(jobs,/jobMap\.get\(step\.jobId\)/)
 assert.match(jobs,/dependsOnJobIds/)
 assert.match(jobs,/\/'\+encodeURIComponent\(job\.id\)\+'\/resume-external'/)
 assert.match(jobs,/externalRunId/)
})
test('original screens use native APIs with measured and estimated token evidence',()=>{
 assert.match(operations,/<RemoteWorkPanel\/>/)
 assert.match(operations,/<TeamWorkflowPanel jobs=\{jobs\.value\}\/>/)
 assert.match(operations,/<RunEventTimeline executionId=\{selectedExecution\}\/>/)
 assert.match(rooms,/<TeamWorkflowPanel filterSessionId=\{sessionId\}\/>/)
 assert.match(usage,/usageExact/)
 assert.match(usage,/estimatedCalls/)
 assert.match(usage,/auxiliaryObservedTokens/)
})

test('global permission toggles serialize writes and reflect only confirmed backend policy',()=>{
 assert.match(operations,/policyWriteInFlight=useRef\(false\)/)
 assert.match(operations,/if\(policyWriteInFlight\.current\|\|policy\.loading\|\|policy\.error\)return/)
 assert.match(operations,/const saved=await post<any>\('\/api\/policy',value\)/)
 assert.match(operations,/policy\.setValue\(saved\)/)
 assert.match(operations,/refresh\('\/api\/policy'\)/)
 assert.match(operations,/await policy\.reload\(\)/)
 assert.match(operations,/disabled=\{policySaving\|\|policy\.loading\|\|Boolean\(policy\.error\)/)
 assert.match(operations,/\(error\|\|policy\.error\)&&<Notice/)
})

test('global policy and extension switches track server policy updates across screens',()=>{
 const control=source('src/components/ExecutionPermissions.tsx')
 const ext=source('src/components/ExtensionRuntimeControls.tsx')
 assert.match(control,/matchesRefreshTarget\('\/api\/policy',targets\)/)
 assert.match(control,/addEventListener\('dominion-data-change',onPolicyChanged\)/)
 assert.match(control,/request===latest/)
 assert.match(control,/removeEventListener\('dominion-data-change',onPolicyChanged\)/)
 assert.match(ext,/useRemote<RuntimePolicy>\('\/api\/extensions\/runtime',\{enabled:\{\},configured:\{\}\}\)/)
 assert.doesNotMatch(ext,/configured:\{\}\},false\)/)
})

test('backup, restore and key rotation commands prevent duplicate requests before React rerenders',()=>{
 const backup=source('src/pages/operations/Backup.tsx')
 assert.match(backup,/operationInFlight = useRef\(false\)/)
 assert.match(backup,/if \(operationInFlight\.current\) return/)
 assert.match(backup,/operationInFlight\.current = true/)
 assert.match(backup,/finally \{\s*operationInFlight\.current = false\s*setBusy\(''\)/)
 assert.match(backup,/await post\('\/api\/system\/restore'/)
 assert.match(backup,/await post<\{activeVersion: number/)
})
