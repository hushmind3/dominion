import {test} from 'node:test'
import assert from 'node:assert/strict'
import {readFileSync,readdirSync,statSync,existsSync} from 'node:fs'
import {join,relative,resolve} from 'node:path'

const root=resolve(import.meta.dirname,'..')
const omitted=new Set(['.git','node_modules','target','dist','.dominion'])
const retired=String.fromCharCode(103,111,97,108)

function walk(dir){
 const found=[]
 for(const name of readdirSync(dir)){
  if(omitted.has(name))continue
  const full=join(dir,name)
  if(statSync(full).isDirectory())found.push(...walk(full))
  else found.push(full)
 }
 return found
}
test('native product source contains no retired feature marker',()=>{
 for(const file of walk(root)){
  const label=relative(root,file).toLowerCase()
  assert.ok(!label.includes(retired),'retired path: '+label)
  if(/\.(rs|tsx?|mjs|jsx?|json|toml|md|html|css)$/.test(file)){
   const text=readFileSync(file,'utf8').toLowerCase()
   assert.ok(!text.includes(retired),'retired source: '+label)
  }
 }
})
test('main React workspace delegates to native Rust endpoints',()=>{
 const required=['src/main.rs','src/api.rs','src/store.rs','src/engine.rs',
  'web/index.html','web/src/main.tsx','web/src/App.tsx','web/src/pages/NativeWorkspace.tsx',
  'sdk/typescript/src/index.ts','web/vite.config.ts']
 for(const file of required)assert.ok(existsSync(join(root,file)),file)
 const api=readFileSync(join(root,'src/api.rs'),'utf8')
 const ui=readFileSync(join(root,'web/src/pages/NativeWorkspace.tsx'),'utf8')
 const app=readFileSync(join(root,'web/src/App.tsx'),'utf8')
 const sdk=readFileSync(join(root,'sdk/typescript/src/index.ts'),'utf8')
 for(const path of ['/api/tasks','/api/groups','/api/videos','/api/improvements','/api/memory','/api/skills']){
  assert.ok(ui.includes(path),'missing native UI path '+path)
  assert.ok(api.includes(path),'missing native Rust route '+path)
 }
 assert.ok(sdk.includes('/api/tasks'))
 assert.ok(app.includes('WorkspaceSidebar'))
 assert.ok(api.includes('ServeDir::new(&root)'))
})
