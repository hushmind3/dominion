import test from 'node:test'
import assert from 'node:assert/strict'
import {readFileSync} from 'node:fs'
import {fileURLToPath} from 'node:url'
import {resolve,dirname} from 'node:path'

const dir=dirname(fileURLToPath(import.meta.url))
const chat=readFileSync(resolve(dir,'../src/pages/Chat.tsx'),'utf8')
const rooms=readFileSync(resolve(dir,'../src/pages/Rooms.tsx'),'utf8')
const dialog=readFileSync(resolve(dir,'../src/features/models/ConnectionTools.tsx'),'utf8')
const modelHub=readFileSync(resolve(dir,'../src/features/models/useModelHub.ts'),'utf8')
const api=readFileSync(resolve(dir,'../src/api/attachments.ts'),'utf8')

test('private chat uploads selected images and forwards their ids',()=>{
 assert.match(chat,/uploadAttachment\(\{sessionId:/)
 assert.match(chat,/attachmentIds/)
 assert.match(chat,/type="file"/)
 assert.match(chat,/accept=\{ATTACH_ALLOWED\.join\(','\)\}/)
 assert.match(chat,/removeImage\(item\.key\)/)
})

test('room chat uploads by room and forwards attachment ids',()=>{
 assert.match(rooms,/uploadAttachment\(\{roomId:room\.id\}/)
 assert.match(rooms,/attachmentIds/)
 assert.match(rooms,/type="file"/)
 assert.match(rooms,/accept=\{ROOM_ATTACH_ALLOWED\.join\(','\)\}/)
})

test('the provider dialog persists the image declaration',()=>{
 assert.match(models,/'models\.dlg\.vision'/)
 assert.match(models,/capabilities:\{\.\.\.\(\(edit\?\.capabilities/)
 assert.match(models,/checked=\{vision\}/)
})

test('uploads always go through the verified attachment endpoint',()=>{
 assert.equal((api.match(/\/api\/attachments/g)||[]).length,1)
})
