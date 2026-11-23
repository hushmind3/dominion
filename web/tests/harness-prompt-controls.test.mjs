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
 ].join('\n')
 const ids=[...root.matchAll(/PromptSpec\{id:"([^"]+)"/g)].map(match=>match[1])
 assert.equal(ids.length,31)
 assert.equal(new Set(ids).size,ids.length)
 for(const id of ids)assert.ok(sources.includes('"'+id+'"'),'Unwired model-facing stage: '+id)
 assert.match(root,/harness_prompt_override/)
 assert.match(root,/pub\(crate\) fn render/)
 assert.match(root,/pub\(crate\) fn validate/)
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
