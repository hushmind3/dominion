// Opt-in, one primary model round; provider auxiliary calls may occur. Never selects a provider automatically,
// auto-approves a tool or changes global policy. Use a test account/workspace.
import {DominionAgentClient} from '../dist/index.js'
import assert from 'node:assert/strict'
const {DOMINION_LIVE_BASE:base,DOMINION_LIVE_PROVIDER:providerId,DOMINION_LIVE_MODEL:model,DOMINION_LIVE_TOKEN:token,DOMINION_LIVE_CONFIRM:confirm}=process.env
if(!base||!providerId||!model||confirm!=='one-primary-round'){
 console.error('Set DOMINION_LIVE_BASE, DOMINION_LIVE_PROVIDER, DOMINION_LIVE_MODEL and DOMINION_LIVE_CONFIRM=one-primary-round. Optional DOMINION_LIVE_TOKEN. No requests sent.')
 process.exit(2)
}
const sdk=new DominionAgentClient({baseUrl:base,adminToken:token})
const policyResponse=await fetch(base.replace(/\/$/,'')+'/api/policy',{headers:token?{'x-dominion-token':token}:{},redirect:'error'})
assert.ok(policyResponse.ok,'Cannot confirm test workspace policy')
const policy=await policyResponse.json()
assert.equal(Boolean(policy.autoApproveAll||policy.autoApproveMcp||(policy.autoApprove||[]).length),false,'Use an isolated test workspace without automatic tool approval')
const route=await sdk.inferenceSupport({providerId,model})
assert.equal(route.providerId,providerId)
assert.equal(route.model,model)
const run=await sdk.startRun({instruction:'Reply briefly with DOMINION_LIVE_OK. This is a connectivity test. Do not use tools, change files, or access private information.',providerId,model,maxRounds:1,idempotencyKey:'live-smoke-'+crypto.randomUUID(),source:{kind:'sdk_live_smoke',id:'manual'}})
const result=await sdk.waitForRun(run.id,{timeoutMs:90000})
assert.equal(result.status,'completed','Model smoke did not complete: '+result.status)
assert.match(result.answer||'',/DOMINION_LIVE_OK/)
assert.equal(result.providerId,providerId)
assert.equal(result.model,model)
const page=await sdk.getRunEvents(run.id,{limit:100})
assert.ok(page.events.length>0)
console.log(JSON.stringify({id:run.id,providerId,model,status:result.status,verified:result.verified,events:page.events.length}))
