import {test} from 'node:test'
import assert from 'node:assert/strict'
import {readFileSync} from 'node:fs'
import {fileURLToPath} from 'node:url'
import {
 readConversationToggle,writeConversationToggle,readConversationEffort,writeConversationEffort,
 conversationToggleKey,conversationEffortKey,conversationPreferenceEvent,compatibleConversationEffort,
} from '../src/conversationPreferences.ts'

const root=fileURLToPath(new URL('../',import.meta.url))
const chat=readFileSync(root+'src/pages/Chat.tsx','utf8')
const rooms=readFileSync(root+'src/pages/Rooms.tsx','utf8')
const shared=readFileSync(root+'src/components/ConversationSettings.tsx','utf8')
const hook=readFileSync(root+'src/hooks/usePersistentConversationPreference.ts','utf8')
const app=readFileSync(root+'src/App.tsx','utf8')

function storageFixture(){
 const data=new Map()
 globalThis.localStorage={
  getItem:key=>data.has(key)?data.get(key):null,
  setItem:(key,value)=>{data.set(key,value)},
  removeItem:key=>data.delete(key),
 }
 globalThis.window=new EventTarget()
 return data
}

test('all private chat toggles survive read-back, including explicit false',()=>{
 storageFixture()
 const defaults={showReasoning:true,thinkingEnabled:true,toolsEnabled:true,autoLearn:false}
 for(const [name,fallback] of Object.entries(defaults)){
  assert.equal(readConversationToggle(name),fallback)
  assert.equal(writeConversationToggle(name,false),true)
  assert.equal(readConversationToggle(name),false)
  assert.equal(localStorage.getItem(conversationToggleKey(name)),'0')
  assert.equal(writeConversationToggle(name,true),true)
  assert.equal(readConversationToggle(name),true)
 }
})

test('bad storage and unsupported values fail safely without enabling learning',()=>{
 const data=storageFixture()
 data.set(conversationToggleKey('autoLearn'),'invalid')
 assert.equal(readConversationToggle('autoLearn'),false)
 data.set(conversationToggleKey('showReasoning'),'invalid')
 assert.equal(readConversationToggle('showReasoning'),true)
 data.set(conversationEffortKey,'<invalid>')
 assert.equal(readConversationEffort(),'')
 assert.equal(writeConversationEffort('<invalid>'),false)
 globalThis.localStorage={getItem:()=>{throw Error('unavailable')},setItem:()=>{throw Error('unavailable')}}
 assert.equal(readConversationToggle('autoLearn'),false)
 assert.equal(writeConversationToggle('autoLearn',true),false)
 assert.equal(readConversationEffort(),'')
})

test('reasoning effort preserves an off choice and deliberate model tiers',()=>{
 storageFixture()
 for(const effort of ['none','low','high','xhigh','']){
  assert.equal(writeConversationEffort(effort),true)
  assert.equal(readConversationEffort(),effort)
 }
})

test('persistence changes notify mounted views and native storage events are subscribed',()=>{
 storageFixture()
 let notices=0
 window.addEventListener(conversationPreferenceEvent,()=>notices++)
 writeConversationToggle('showReasoning',false)
 writeConversationEffort('none')
 assert.equal(notices,2)
 assert.match(hook,/addEventListener\('storage',sync\)/)
 assert.match(hook,/addEventListener\(conversationPreferenceEvent,sync\)/)
 assert.match(hook,/removeEventListener\('storage',sync\)/)
})

test('both screens bind persisted display/thinking controls and chat wire uses effective values',()=>{
 assert.match(chat,/usePersistentConversationToggle\('showReasoning'\)/)
 assert.match(rooms,/usePersistentConversationToggle\('showReasoning'\)/)
 assert.match(chat,/usePersistentConversationToggle\('toolsEnabled'\)/)
 assert.match(chat,/usePersistentConversationToggle\('autoLearn'\)/)
 assert.match(chat,/usePersistentReasoningEffort\(\)/)
 assert.match(rooms,/usePersistentReasoningEffort\(\)/)
 assert.match(chat,/reasoningEffort:effectiveReasoningEffort\|\|undefined/)
 assert.match(chat,/toolsEnabled,toolsExecutionEnabled:toolsEnabled,autoLearning:autoLearn/)
 assert.match(chat,/effectiveThinkingEnabled=!canDisableThinking\|\|thinkingEnabled/)
 assert.match(rooms,/changeRoomOption\('toolsEnabled',value\)/)
 assert.match(rooms,/changeRoomOption\('autoLearning',value\)/)
 assert.match(hook,/localStorage\.setItem\(koreanReasoningKey/)
 assert.match(shared,/onChange=\{e=>setReasoningKo\(e\.target\.checked\)\}/)
 assert.match(chat,/usePersistentKoreanReasoning\(\)/)
 assert.match(rooms,/usePersistentKoreanReasoning\(\)/)
 assert.match(app,/reloadUpdatedUi\(\)/)
 assert.doesNotMatch(chat,/\[toolsEnabled,setToolsEnabled\]=useState\(true\)/)
})

test('model changes suppress unsupported tiers on wire without erasing stored choice',()=>{
 storageFixture()
 writeConversationEffort('high')
 assert.equal(compatibleConversationEffort(readConversationEffort(),['none','low']), '')
 assert.equal(readConversationEffort(),'high')
 assert.equal(compatibleConversationEffort(readConversationEffort(),['none','low','high']), 'high')
 assert.equal(compatibleConversationEffort('none',['low','medium']), '')
 assert.equal(compatibleConversationEffort('none',['none','low']), 'none')
 assert.equal(compatibleConversationEffort('high',[]), '')
 assert.equal(compatibleConversationEffort('',[]), '')
 assert.match(chat,/compatibleConversationEffort\(/)
})
