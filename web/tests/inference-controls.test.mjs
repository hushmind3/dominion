import test from 'node:test'
import assert from 'node:assert/strict'
import {selectableEfforts,commonEfforts,roomInferenceRoutes} from '../src/inferenceControls.ts'
const a={providerId:'a',model:'m'},b={providerId:'b',model:'m'}
test('distinct providers sharing a model ID retain distinct controls',()=>{
 const rows=[{...a,thinkingToggle:true,reasoningEfforts:[]},{...b,thinkingToggle:false,reasoningEfforts:['low','high']}]
 assert.deepEqual(selectableEfforts(rows[0]),['none'])
 assert.deepEqual(commonEfforts([a,b],rows),[])
 assert.deepEqual(commonEfforts([a],rows),['none'])
 assert.deepEqual(commonEfforts([a,b],[rows[0]]),[])
})
test('explicit model pool wins; unconfigured room routes cannot claim common support',()=>{
 const providers=[{id:'a',enabled:true,model:'m'},{id:'b',enabled:true,model:'m'}]
 const agents=[{id:'1',providerId:'a',model:'m'},{id:'2',providerId:'b',model:''}]
 assert.deepEqual(roomInferenceRoutes({agentIds:['1','2'],models:[b]},agents,providers),[b])
 assert.deepEqual(roomInferenceRoutes({agentIds:['1','2'],autoModels:true},agents,providers),[])
 assert.deepEqual(roomInferenceRoutes({agentIds:['1','2'],autoModels:false},agents,providers),[a,b])
})
