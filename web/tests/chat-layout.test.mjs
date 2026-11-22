import test from 'node:test'
import assert from 'node:assert/strict'
import {readFileSync} from 'node:fs'
import {fileURLToPath} from 'node:url'
const root=fileURLToPath(new URL('../',import.meta.url))
const chat=readFileSync(root+'src/pages/Chat.tsx','utf8')
const row=readFileSync(root+'src/components/ChatMessageRow.tsx','utf8')
// Transport lives in ChatPage; persisted message semantics live in ChatMessageRow.
assert.match(chat,/<ChatMessageRow /)
const css=readFileSync(root+'src/theme.css','utf8')

test('basic chat aligns owner messages to the right and agent messages to the left',()=>{
 assert.match(row,/chat-message--user justify-end/)
 assert.match(row,/chat-message--assistant/)
 assert.match(row,/data-role=\{human\?'user':'assistant'\}/)
 assert.match(css,/\.chat-transcript \.chat-message--user\{justify-content:flex-end\}/)
 assert.match(css,/\.chat-transcript \.chat-message--user \.chat-message-inner/)
 assert.match(css,/\.chat-transcript \.chat-message--assistant \.chat-message-inner/)
 assert.doesNotMatch(chat,/human\?<OwnerAvatar/)
})
test('conversation text preserves authored line breaks and uses readable typography on desktop and mobile',()=>{
 assert.match(row,/chat-message-text min-w-0 whitespace-pre-wrap break-words/)
 assert.match(row,/chat-message-text--user/)
 assert.match(row,/chat-message-text--assistant/)
 assert.match(css,/font-size:15px/)
 assert.match(css,/line-height:1\.75/)
 assert.match(css,/\.chat-transcript \.chat-message-text--user\{[^}]*background:var\(--color-brand\)/)
 assert.match(css,/@media\(max-width:767px\)/)
 assert.match(css,/max-width:90%/)
})
test('live agent output retains left-anchored layout and the same text style',()=>{
 assert.match(chat,/chat-message chat-message--assistant flex min-w-0 items-start/)
 assert.match(chat,/chat-message-text chat-message-text--assistant min-w-0/)
})

test('assistant live and saved messages render rich text, while owner message stays literal',()=>{
 assert.match(chat,/import \{ChatRichText\} from '\.\.\/components\/ChatRichText'/)
 assert.match(chat,/<ChatRichText content=\{live\}\/>/)
 assert.match(row,/human\?message\.content:\(message\.content\?<ChatRichText content=\{message\.content\}\/>/)
})

test('ordinary chat defaults to direct mode with no agent, tools or autonomous learning',()=>{
 assert.match(chat,/const mode:ChatMode=/)
 assert.match(chat,/\?'agent':'chat'\)/)
 assert.match(chat,/aria-label="대화 실행 모드"/)
 assert.match(chat,/option value="chat">일반 채팅/)
 assert.match(chat,/agentMode&&<ExecutionPermissions/)
 assert.match(chat,/agentMode&&<SelectInput aria-label=\{t\('chat.agentAria'\)\}/)
 assert.match(chat,/toolsEnabled:agentMode&&toolsEnabled,toolsExecutionEnabled:agentMode&&toolsEnabled,autoLearning:agentMode&&autoLearn/)
})

test('owner messages appear immediately and are deduplicated against the stored transcript',()=>{
 assert.match(chat,/setPendingOwner\(\{scope:current,text,priorUserId:/)
 assert.match(chat,/data-pending="true"/)
 assert.match(chat,/lastOwner\.id===pendingOwner\.priorUserId/)
 assert.match(chat,/pendingVisible\?pendingOwner\?\.text/)
 assert.match(chat,/setPendingOwner\(previous=>previous&&previous\.scope===current\?\{\.\.\.previous,scope:id\}:previous\)/)
})

test('partial model text stays visible after an incomplete unsaved response',()=>{
 assert.match(chat,/streamInterrupted\?'미완료 · 서버 저장 확인 필요'/)
 assert.match(chat,/if\(!saved&&!controller\.signal\.aborted\)throw Error/)
 assert.match(chat,/if\(!saved&&!unsavedFailure\)\{setLive\(''\);setThought\(''\);\}/)
 assert.match(chat,/if\(unsavedFailure\)setStreamInterrupted\(true\)/)
})

test('chat drafts are tab-local and scoped to the current conversation',()=>{
 const hook=readFileSync(root+'src/hooks/useChatDraft.ts','utf8')
 assert.match(chat,/useChatDraft\(current\)/)
 assert.match(hook,/const keyOf=\(sessionId\?:string\)=>PREFIX\+\(sessionId\|\|'new'\)/)
 assert.match(hook,/sessionStorage\.getItem/)
 assert.match(hook,/sessionStorage\.setItem/)
 assert.match(hook,/current\.key===key\?current\.text:read\(key\)/)
 assert.match(chat,/if\(!current\)setInput\(''\)/)
})

test('Room message drafts are per room, restored on remount and cleared only on committed send',()=>{
 const rooms=readFileSync(root+'src/pages/Rooms.tsx','utf8')
 assert.match(rooms,/useChatDraft\('room:'\+room\.id\)/)
 assert.match(rooms,/setInput\(''\);if\(inputRef\.current\)inputRef\.current\.style\.height=''\;setTargets\(\[\]\)/)
 assert.match(rooms,/if\(!creating\|\|saving\)return/)
})

test('chat history is searchable without modifying or deleting stored conversations',()=>{
 assert.match(chat,/const \[historyQuery,setHistoryQuery\]=useState\(''\)/)
 assert.match(chat,/historyEntries=useMemo/)
 assert.match(chat,/session\.title\.toLocaleLowerCase\(\)\.includes\(query\)/)
 assert.match(chat,/className=\{'chat-history-entry/)
 assert.match(chat,/historyQuery\.trim\(\)\?t\('chat.historyNoResults'\)/)
})

test('assistant task checklists render as explicit read-only checked/unchecked boxes',()=>{
 const rich=readFileSync(root+'src/components/ChatRichText.tsx','utf8')
 assert.match(rich,/chat-task-item/)
 assert.match(rich,/type="checkbox" disabled checked=/)
 assert.match(css,/\.chat-richtext \.chat-task-item/)
})

test('basic chat attaches local files to the real session and model request',()=>{
 const attachments=readFileSync(root+'src/utils/chatAttachments.ts','utf8')
 const api=readFileSync(root+'src/api.ts','utf8')
 assert.match(chat,/mergeChatFiles\(draftFiles,Array\.from\(picked\)\)/)
 assert.match(chat,/post<\{id:string\}>\('\/api\/attachments'/)
 assert.match(chat,/attachmentIds,/)
 assert.match(api,/attachmentIds\?:string\[\]/)
 assert.match(api,/async function attachmentBlob/)
 assert.match(attachments,/MAX_CHAT_FILE_BYTES=8\*1024\*1024/)
 assert.match(attachments,/MAX_CHAT_TOTAL_BYTES=24\*1024\*1024/)
 assert.match(attachments,/MAX_CHAT_FILES=8/)
})

test('stored images have authenticated byte-backed previews with URL cleanup',()=>{
 const src=readFileSync(root+'src/components/ChatAttachment.tsx','utf8')
 assert.match(row,/<ChatAttachment key=\{file\.id\} file=\{file\}\/>/)
 assert.match(src,/attachmentBlob\(file\.id\)/)
 assert.match(src,/DialogBackdrop onDismiss=\{close\}/)
 assert.match(src,/URL\.revokeObjectURL/)
 assert.doesNotMatch(src,/window\.alert/)
})
