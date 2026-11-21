import test from 'node:test'
import assert from 'node:assert/strict'
import {readFileSync} from 'node:fs'
import {fileURLToPath} from 'node:url'
const root=fileURLToPath(new URL('../',import.meta.url))
const src=readFileSync(root+'src/pages/Agents.tsx','utf8')
test('Agent editor restores only a tab-local unsaved draft and clears on save/cancel',()=>{
 assert.match(src,/AGENT_DRAFT_KEY='dominion\.agent\.unsavedEditor'/)
 assert.match(src,/useState<Partial<Agent> \| null>\(readAgentDraft\)/)
 assert.match(src,/sessionStorage\.getItem\(AGENT_DRAFT_KEY\)/)
 assert.match(src,/sessionStorage\.setItem\(AGENT_DRAFT_KEY,encoded\)/)
 assert.match(src,/sessionStorage\.removeItem\(AGENT_DRAFT_KEY\)/)
 assert.match(src,/setEdit\(null\)/)
})
