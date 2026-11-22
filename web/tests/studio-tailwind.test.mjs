import test from 'node:test'
import assert from 'node:assert/strict'
import {readFileSync,existsSync} from 'node:fs'
import {fileURLToPath} from 'node:url'

const root=fileURLToPath(new URL('../',import.meta.url))
const read=path=>readFileSync(root+path,'utf8')
const app=read('src/App.tsx')
const shell=read('src/components/WorkspaceShell.tsx')
const chat=read('src/pages/Chat.tsx')
const theme=read('src/theme.css')
const native=read('src/components/NativeAgentPanel.tsx')
const rooms=read('src/pages/Rooms.css')
const entry=read('src/main.tsx')

test('shared tokens are declared once in Tailwind CSS v4 theme',()=>{
 assert.match(theme,/@import "tailwindcss"/)
 for(const token of ['--color-canvas','--color-surface','--color-ink','--color-divider','--color-brand'])
  assert.ok(theme.includes(token),'Missing reusable Tailwind token: '+token)
})
test('React shell is one light top bar with a command palette, not a sidebar',()=>{
 assert.match(app,/grid-rows-\[3\.5rem_minmax\(0,1fr\)\]/)
 assert.match(app,/WorkspaceTopBar/)
 assert.match(app,/metaKey\|\|event\.ctrlKey/)
 assert.match(shell,/export function WorkspaceTopBar/)
 assert.match(shell,/h-14/)
 assert.match(shell,/bg-surface/)
 assert.match(shell,/const primary/)
})
test('chat and work modes own their responsive layout in React components',()=>{
 assert.match(chat,/grid-rows-\[2\.5rem_2\.25rem\]/)
 assert.match(chat,/chat-welcome my-auto flex/)
 assert.match(native,/native-command-grid grid min-w-0/)
 assert.match(native,/historyOpen\?'has-history /)
})
test('injected green theme is gone and cannot override Tailwind utilities',()=>{
 assert.doesNotMatch(entry,/studio\.css/)
 assert.equal(existsSync(root+'src/styles/studio.css'),false)
 assert.doesNotMatch(theme,/Sub3 workbench redesign/)
 assert.doesNotMatch(rooms,/Sub3 Room workbench/)
 assert.doesNotMatch(theme,/\.workspace-settings\{position:fixed/)
})
