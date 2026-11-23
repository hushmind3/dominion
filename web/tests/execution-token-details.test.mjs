import test from 'node:test'
import assert from 'node:assert/strict'
import {executionTokenDetails} from '../src/utils/executionTokenEvidence.ts'

test('Sub2 per-call provider receipts power the exact detailed panel',()=>{
 const record={
  id:'exec-test',modelCalls:2,tokens:195,status:'completed',
  providerRounds:[
   {round:1,promptTokens:100,completionTokens:20,cachedPromptTokens:0,usageExact:true,toolsAdvertised:0},
   {round:2,promptTokens:60,completionTokens:15,cachedPromptTokens:40,usageExact:true,toolsAdvertised:2},
  ],
  inputContext:{policy:'native-work-context',preAdapterWireProfile:{
    totalEstimatedTokens:210,toolDefinitions:0,
    components:{systemAndPersona:65,ownerAndHistory:100,toolSchemas:0,retrievedEvidence:20}
  }}
 }
 const d=executionTokenDetails(record)
 assert.equal(d.modelCalls,2)
 assert.equal(d.firstInput,100)
 assert.equal(d.firstInputExact,true)
 assert.equal(d.tools,0)
 assert.equal(d.system,65)
 assert.equal(d.history,100)
 assert.equal(d.retrieval,20)
 assert.equal(d.input,160)
 assert.equal(d.cached,40)
 assert.equal(d.newInput,120)
 assert.equal(d.output,35)
 assert.equal(d.exactCalls,2)
 assert.equal(d.allExact,true)
 assert.equal(d.recordedTotal,195)
 assert.equal(d.calls[1].index,2)
})

test('No provider cache receipt must remain unknown instead of a fabricated zero',()=>{
 const d=executionTokenDetails({id:'mixed',providerRounds:[
  {round:1,promptTokens:100,completionTokens:10,usageExact:true,cachedPromptTokens:20},
  {round:2,promptTokens:60,completionTokens:15,usageExact:false,cachedPromptTokens:null},
 ]})
 assert.equal(d.input,160)
 assert.equal(d.output,25)
 assert.equal(d.cached,null)
 assert.equal(d.newInput,null)
 assert.equal(d.estimatedCalls,1)
 assert.equal(d.allExact,false)
 assert.equal(d.firstInputExact,true)
})

test('Legacy Sub4 modelCallUsage and missing evidence remain honest',()=>{
 const d=executionTokenDetails({id:'old',modelCalls:1,inputContext:{
  modelCallUsage:[{completedCallIndex:1,estimatedPromptTokens:55,estimatedOutputTokens:8,usageExact:false}],
  providerPromptTokens:55,providerCachedPromptTokens:null
 }})
 assert.equal(d.firstInput,55)
 assert.equal(d.firstInputExact,false)
 assert.equal(d.cached,null)
 assert.equal(d.calls[0].estimated,true)
 const fallback=executionTokenDetails(null,{promptTokens:15,completionTokens:3,totalTokens:18,estimated:true})
 assert.equal(fallback.recordedTotal,18)
 assert.equal(fallback.cached,null)
 assert.equal(fallback.system,null)
 assert.equal(fallback.modelCalls,null)
})
