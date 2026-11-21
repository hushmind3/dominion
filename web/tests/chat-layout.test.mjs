import test from 'node:test'
import assert from 'node:assert/strict'
import {readFileSync} from 'node:fs'
import {fileURLToPath} from 'node:url'
const root=fileURLToPath(new URL('../',import.meta.url))
const chat=readFileSync(root+'src/pages/Chat.tsx','utf8')
const css=readFileSync(root+'src/theme.css','utf8')

test('basic chat aligns owner messages to the right and agent messages to the left',()=>{
 assert.match(chat,/chat-message--user justify-end/)
 assert.match(chat,/chat-message--assistant/)
 assert.match(chat,/data-role=\{human\?'user':'assistant'\}/)
 assert.match(css,/\.chat-transcript \.chat-message--user\{justify-content:flex-end\}/)
 assert.match(css,/\.chat-transcript \.chat-message--user \.chat-message-inner/)
 assert.match(css,/\.chat-transcript \.chat-message--assistant \.chat-message-inner/)
 assert.doesNotMatch(chat,/human\?<OwnerAvatar/)
})
test('conversation text preserves authored line breaks and uses readable typography on desktop and mobile',()=>{
 assert.match(chat,/chat-message-text min-w-0 whitespace-pre-wrap break-words/)
 assert.match(chat,/chat-message-text--user/)
 assert.match(chat,/chat-message-text--assistant/)
 assert.match(css,/font-size:16px/)
 assert.match(css,/line-height:1\.8/)
 assert.match(css,/@media\(max-width:767px\)/)
 assert.match(css,/max-width:90%/)
})
test('live agent output retains left-anchored layout and the same text style',()=>{
 assert.match(chat,/chat-message chat-message--assistant flex min-w-0 items-start/)
 assert.match(chat,/chat-message-text chat-message-text--assistant min-w-0 whitespace-pre-wrap/)
})
