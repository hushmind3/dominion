import test from 'node:test'
import assert from 'node:assert/strict'
import {readFileSync} from 'node:fs'
import {fileURLToPath} from 'node:url'
const root = fileURLToPath(new URL('../', import.meta.url))
const read = path => readFileSync(root + path, 'utf8')

// A machine token must never reach the owner. These tests lock the shared
// mapping and the call sites that use it.

test('status values resolve to human sentences in both languages', () => {
  const plain = read('src/i18n/plain.ts')
  assert.match(plain, /export function plainStatus/)
  for (const status of ["error: '확인 필요'", "running: '진행 중'", "starting: '시작 중'", "stopped: '중지됨'"])
    assert.ok(plain.includes(status), 'missing plain status ' + status)
  assert.match(plain, /enStatus/)
  assert.match(plain, /Needs attention/)
})

test('technical failures map to actionable Korean', () => {
  const plain = read('src/i18n/plain.ts')
  assert.match(plain, /export function plainError/)
  assert.match(plain, /failed to fetch/i)
  assert.match(plain, /http \?5\\d\\d/)
  assert.match(plain, /http \?401/)
  assert.match(plain, /인터넷 연결을 확인한 뒤 다시 시도해 주세요\./)
  assert.match(plain, /서버가 잠시 응답하지 않습니다/)
  assert.match(plain, /\[가-힣\]/)
})

test('every surface uses the shared plain-language mapping', () => {
  const api = read('src/api.ts')
  assert.match(api, /import \{plainError\} from '\.\/i18n\/plain'/)
  assert.match(api, /plainError\(translateError\(/)

  const pill = read('src/components/StatusPill.tsx')
  assert.match(pill, /import \{plainStatus\} from '\.\.\/i18n\/plain'/)
  assert.doesNotMatch(pill, /\?t\(labels\[status\]\):status\}/)

  const rooms = read('src/pages/Rooms.tsx')
  assert.match(rooms, /plainStatus\(status\)/)
  assert.doesNotMatch(rooms, /labels\[status\]\?t\(labels\[status\]\):status\}/)

  const channels = read('src/components/ChannelDirectory.tsx')
  assert.match(channels, /plainError\(selected\.runtimeError/)

  const models = read('src/pages/Models.tsx')
  assert.match(models, /plainError\(counts\[p\.id\]\.error\)/)
})
