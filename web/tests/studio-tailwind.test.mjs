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
 for(const token of ['--color-workspace-sidebar','--color-workspace-sidebar-text','--color-workspace-selected','--color-workspace-accent'])
  assert.ok(theme.includes(token),'Missing reusable Tailwind token: '+token)
})
test('React layout is responsive and does not depend on an absolute sidebar stylesheet',()=>{
 assert.match(app,/grid-rows-\[minmax\(0,1fr\)_4rem\]/)
 assert.match(app,/md:grid-cols-\[14rem_minmax\(0,1fr\)\]/)
 assert.match(app,/dominion-studio-stage col-start-1 row-start-1/)
 assert.match(shell,/md:row-start-1 md:h-full md:border-t-0/)
 assert.match(shell,/onToggleNav/)
 assert.match(shell,/bg-workspace-selected/)
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
