import test from 'node:test'
import assert from 'node:assert/strict'
import {readFileSync} from 'node:fs'

const source=(path)=>readFileSync(new URL(path,import.meta.url),'utf8')

test('chat keeps live session-wide usage visible in its own compact top-right strip',()=>{
 const chat=source('../src/pages/Chat.tsx')
 const css=source('../src/theme.css')
 assert.match(chat,/chat-usage-bar/)
 assert.match(chat,/chat-usage-details/)
 assert.match(chat,/totalUsedTokens\.toLocaleString\(\)/)
 assert.match(chat,/usageSession===current/)
 assert.match(chat,/Math\.max\(serverUsage,messageUsage\)/)
 assert.match(chat,/\/api\/sessions\/\'+encodeURIComponent\(current\)\+'\/usage/)
 assert.match(css,/\.chat-usage-popover/)
})

test('empty chat stays uncluttered without deleting AI, agent and file controls',()=>{
 const chat=source('../src/pages/Chat.tsx')
 const css=source('../src/theme.css')
 const ko=source('../src/i18n/ko.ts')
 assert.match(chat,/chat-welcome-mark/)
 assert.match(chat,/chat-welcome-title/)
 assert.doesNotMatch(chat,/t\('chat\.examples'\)/)
 assert.match(chat,/ConversationTargetPicker/)
 assert.match(chat,/ExecutionPermissions/)
 assert.match(chat,/chat-file-add/)
 assert.match(ko,/새 대화를 시작하세요/)
 assert.match(css,/\.page-chat \.chat-execution-mode-hint\{display:none\}/)
 assert.match(css,/\.page-chat \.chat-turn-byline \.token-usage-breakdown\{display:none\}/)
})
