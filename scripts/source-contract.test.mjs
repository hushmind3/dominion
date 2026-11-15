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


test('all active main navigation uses real native product paths',()=>{
 const app=readFileSync(join(root,'web/src/App.tsx'),'utf8')
 const nav=readFileSync(join(root,'web/src/components/WorkspaceShell.tsx'),'utf8')
 const native=readFileSync(join(root,'web/src/pages/NativeWorkspace.tsx'),'utf8')
 const api=readFileSync(join(root,'src/api.rs'),'utf8')
 const required={
  'NativeAgents':['/api/agents'],
  'NativeWorkflows':['/api/workflows'],
  'NativeSystem':['/api/providers','/api/tools'],
  'NativeWorkspace':['/api/tasks','/api/groups','/api/videos','/api/improvements']
 }
 for(const [page,paths] of Object.entries(required)){
  assert.ok(app.includes(page),'not mounted: '+page)
  for(const route of paths)assert.ok(api.includes(route),'Rust endpoint missing: '+route)
 }
 assert.ok(nav.includes("id:'extensions'"),'extension navigation absent')
 assert.ok(native.includes('/resume'),'task continuation UI is missing')
 assert.ok(api.includes('/api/videos/:id/scenes/:index/image'),'video asset API is missing')
 assert.ok(api.includes('/api/tasks/:id/context'),'semantic evidence API is missing')
 const inspector=readFileSync(join(root,'web/src/pages/NativeSystem.tsx'),'utf8')
 assert.ok(inspector.includes('/context?q='),'main React evidence inspector is disconnected')
 const sdk=readFileSync(join(root,'sdk/typescript/src/index.ts'),'utf8')
 assert.ok(sdk.includes('contextEvidence('),'typed client lacks native evidence API')
})
test('native core owns recovery, spend and workflow execution',()=>{
 const runtime=readFileSync(join(root,'src/runtime.rs'),'utf8')
 const engine=readFileSync(join(root,'src/engine.rs'),'utf8')
 const workflow=readFileSync(join(root,'src/workflows.rs'),'utf8')
 const video=readFileSync(join(root,'src/video.rs'),'utf8')
 const migration=readFileSync(join(root,'src/legacy.rs'),'utf8')
 const provider=readFileSync(join(root,'src/provider.rs'),'utf8')
 assert.ok(runtime.includes('pub async fn resume('))
 assert.ok(engine.includes('context::project(&outbound,message_budget,&durable)'))
 assert.ok(engine.includes('"task_receipt"'))
 assert.ok(engine.includes('account_cost(&id,'))
 assert.ok(provider.includes('cached_input_krw_per_million'))
 assert.ok(workflow.includes('self.submit(input).await'))
 assert.ok(workflow.includes('self.wait(&result.id'))
 assert.ok(workflow.includes('workflow_recover'))
 assert.ok(video.includes('set_scene_image'))
 assert.ok(video.includes('zoompan'))
 assert.ok(migration.includes('OpenFlags::SQLITE_OPEN_READ_ONLY'))
 assert.ok(migration.includes('legacy_messages'))
 assert.ok(migration.includes('INSERT OR IGNORE'))
 assert.ok(engine.includes('semantic::refresh('))
 assert.ok(engine.includes('"context_search"'))
 const semantic=readFileSync(join(root,'src/engine/semantic.rs'),'utf8')
 const indexed=readFileSync(join(root,'src/context_memory.rs'),'utf8')
 assert.ok(semantic.includes('store.context_pin('))
 assert.ok(indexed.includes('context_fts MATCH'))
})


test('native coding source acceptance is the Rust completion path',()=>{
 const engine=readFileSync(join(root,'src/engine.rs'),'utf8')
 const checks=readFileSync(join(root,'src/engine/verification.rs'),'utf8')
 const files=readFileSync(join(root,'src/engine/files.rs'),'utf8')
 const tests=readFileSync(join(root,'src/engine/acceptance_tests.rs'),'utf8')
 const runtime=readFileSync(join(root,'src/runtime.rs'),'utf8')
 const ui=readFileSync(join(root,'web/src/pages/NativeWorkspace.tsx'),'utf8')
 const sdk=readFileSync(join(root,'sdk/typescript/src/index.ts'),'utf8')
 const api=readFileSync(join(root,'src/api.rs'),'utf8')
 assert.ok(engine.includes('verification::audit('))
 assert.ok(engine.includes('workspace_write_lock.lock().await'))
 assert.ok(engine.includes('proof.observe(tool_name,&arguments,&value)'))
 assert.ok(engine.includes('acceptance_complete'))
 assert.ok(checks.includes('pub fn acceptable('))
 assert.ok(checks.includes('pub(super) async fn audit('))
 assert.ok(checks.includes('current_failure('))
 assert.ok(files.includes('pub async fn replace_many('))
 assert.ok(files.includes('pub fn read_lines('))
 assert.ok(tests.includes('wrong_multifile_repair_receives_real_test_failure_and_fixes_it'))
 assert.ok(tests.includes('old_green_test_is_invalidated_by_newer_bad_write'))
 assert.ok(tests.includes('native_agent_can_change_two_files_then_pass_real_joint_acceptance'))
 assert.ok(runtime.includes('"acceptanceChecks": self.acceptance_checks'))
 assert.ok(ui.includes('acceptanceChecks,'))
 assert.ok(ui.includes("type==='acceptance_result'"))
 assert.ok(sdk.includes('acceptanceChecks?: AcceptanceCommand[]'))
 assert.ok(api.includes('workspace_replace_many'))
 assert.ok(api.includes('workspace_read_lines'))
});
