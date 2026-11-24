import test from 'node:test'
import assert from 'node:assert/strict'
import {readFileSync} from 'node:fs'
const read=path=>readFileSync(new URL(path,import.meta.url),'utf8')

test('Sub2 owns an editable registry and every registered model instruction is wired',()=>{
 const root=read('../../src/engine/services/conversation/chat/harness_prompts.rs')
 const sources=[
  read('../../src/engine/services/conversation/chat_context.rs'),
  read('../../src/engine/services/conversation/chat_prompt_sources.rs'),
  read('../../src/engine/services/conversation/chat/runtime/turn_body.rs'),
  read('../../src/engine/services/conversation/chat/protocol.rs'),
  read('../../src/engine/services/decision/runtime_action_review.rs'),
  read('../../src/engine/services/decision/learning.rs'),
  read('../../src/engine/services/decision/reflect.rs'),
  read('../../src/engine/services/decision/runtime_program.rs'),
 ].join('\n')
 const ids=[...root.matchAll(/PromptSpec\{id:"([^"]+)"/g)].map(match=>match[1])
 assert.equal(ids.length,32)
 assert.equal(new Set(ids).size,ids.length)
 for(const id of ids)assert.ok(sources.includes('"'+id+'"'),'Unwired model-facing stage: '+id)
 assert.match(root,/harness_prompt_override/)
 assert.match(root,/pub\(crate\) fn render/)
 assert.match(root,/pub\(crate\) fn validate/)
 assert.match(root,/filter\(\|item\|!legacy_slot\(item\.id\)\|\|stored\(state,item\.id\)\.is_some\(\)\)/)
})

test('Sub2 Memory presents the real harness templates and saves with conflict checks',()=>{
 const memory=read('../src/pages/Memory.tsx')
 const editor=read('../src/pages/memory/HarnessPromptsPanel.tsx')
 const api=read('../../src/api/services/management/settings.rs')
 const routes=read('../../src/api/surfaces/management_routes.rs')
 assert.match(memory,/하네스 지침/)
 assert.match(memory,/#\/memory\?view=harness/)
 assert.match(memory,/<HarnessPromptsPanel\/>/)
 assert.match(editor,/\/api\/settings\/harness-prompts/)
 assert.match(editor,/expectedRevision:baseRevision/)
 assert.match(editor,/status===409/)
 assert.match(editor,/current\.required/)
 assert.match(editor,/reset:true/)
 assert.match(editor,/current\.default/)
 assert.match(routes,/\/api\/settings\/harness-prompts/)
 assert.match(api,/begin_session\(&key\)/)
 assert.match(api,/validate\(spec,content\)/)
})

test('plain conversation uses one shared base and memory rather than agent work manuals',()=>{
 const context=read('../../src/engine/services/conversation/chat_context.rs')
 assert.match(context,/let mut prompt=conversation_base\(state,request,true\)/)
 assert.match(context,/conversation_base\(state,request,false\)/)
 assert.match(context,/if request\.direct_chat \{\s*if web_available\(request\)/)
 assert.match(context,/if request\.direct_chat\{\s*conversation_memory\(/)
 assert.doesNotMatch(context,/effective\(state,"chat\.general"\)/)
 assert.doesNotMatch(context,/effective\(state,"agent\.general"\)/)
})
