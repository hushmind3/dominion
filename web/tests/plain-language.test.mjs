import test from 'node:test'
import assert from 'node:assert/strict'
import {readFileSync} from 'node:fs'
import {fileURLToPath} from 'node:url'
const root=fileURLToPath(new URL('../',import.meta.url))
const read=path=>readFileSync(root+path,'utf8')

// The product is used by the owner, not by its developers. These tests keep
// the first-run surfaces from drifting back to operator vocabulary.

test('the top bar exposes three task tabs and a more menu',()=>{
 const shell=read('src/components/WorkspaceShell.tsx')
 assert.match(shell,/const primary: PageId\[\] = \['home', 'chat', 'rooms'\]/)
 assert.match(shell,/const secondary: PageId\[\] = \['models','agents','connections','extensions','capabilities','memory','activity'\]/)
 assert.match(shell,/t\('shell\.more'\)/)
 assert.match(shell,/import \{ChevronDown,MessageSquare,Search,Settings,UsersRound\} from 'lucide-react'/)
})

test('the home screen is a greeting, a request box and recent work',()=>{
 const home=read('src/pages/Home.tsx')
 assert.match(home,/무엇을 도와드릴까요\?/)
 assert.match(home,/<NativeAgentPanel providers=\{providers\.value\}\/>/)
 assert.match(home,/최근 작업/)
 assert.doesNotMatch(home,/data-dashboard-metric/)
 assert.doesNotMatch(home,/model-usage|token/i)
})

test('model labels never repeat a model suffix already inside the name',()=>{
 const api=read('src/api.ts')
 assert.match(api,/export function providerLabel/)
 assert.match(api,/provider\.name\.endsWith\(suffix\)/)
 const panel=read('src/components/NativeAgentPanel.tsx')
 assert.match(panel,/providerLabel\(p\)/)
 assert.doesNotMatch(panel,/\{p\.name\}\{p\.model\?` · \$\{p\.model\}`:''\}/)
})

test('page labels use user words, not operator words',()=>{
 const ko=read('src/i18n/ko.ts')
 for (const label of ["'nav.models': 'AI 선택'","'nav.agents': '도우미'","'nav.activity': '최근 작업'","'nav.extensions': '도구'"])
  assert.ok(ko.includes(label),'expected plain label '+label)
 for (const stale of ["'nav.models': '모델 관리'","'nav.agents': '에이전트'","'nav.activity': '실행 기록'"])
  assert.ok(!ko.includes(stale),'operator label came back: '+stale)
})
