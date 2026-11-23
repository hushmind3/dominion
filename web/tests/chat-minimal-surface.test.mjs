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
 assert.ok(chat.includes("'/api/sessions/'+encodeURIComponent(current)+'/usage'"))
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

test('assistant messages expose copy beside a small options menu with on-demand reasoning and token breakdown',()=>{
 const chat=source('../src/pages/Chat.tsx')
 const css=source('../src/theme.css')
 assert.match(chat,/aria-label=\{copied\?'답변 복사됨':'답변 복사'\}/)
 assert.match(chat,/aria-label="답변 옵션"/)
 assert.match(chat,/aria-haspopup="menu"/)
 assert.match(chat,/추론 정보 보기/)
 assert.match(chat,/토큰 사용량 보기/)
 assert.match(chat,/navigator\.clipboard\.writeText\(message\.content\|\|''\)/)
 assert.match(chat,/detail==='reasoning'&&canShowReasoning/)
 assert.match(chat,/detail==='usage'&&canShowUsage/)
 assert.doesNotMatch(chat,/chat-turn-copy inline-flex/)
 assert.match(css,/\.chat-turn-action-popover/)
 assert.match(css,/\.chat-turn-detail/)
})
