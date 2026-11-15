import test from 'node:test'
import assert from 'node:assert/strict'
import {mkdtempSync,mkdirSync,writeFileSync,rmSync} from 'node:fs'
import {tmpdir} from 'node:os'
import path from 'node:path'
import {auditSourceHygiene,RETIRED_ENTRYPOINTS} from './source-hygiene.mjs'

function fixture(changes={},remove=[]) {
  const root=mkdtempSync(path.join(tmpdir(),'dominion-hygiene-'))
  const contents={
    'src/engine/goals/mod.rs':'mod runner;\n',
    'src/engine/goals/runner.rs':'pub fn run() {}\n',
    'src/api/goals/mod.rs':'mod lifecycle;\n',
    'src/api/goals/lifecycle.rs':'pub fn create() {}\n',
    'src/api/providers.rs':'pub fn providers() {}\n',
    ...changes,
  }
  for (const deleted of remove) delete contents[deleted]
  for (const [p,body] of Object.entries(contents)){
    mkdirSync(path.dirname(path.join(root,p)),{recursive:true})
    writeFileSync(path.join(root,p),body)
  }
  return {root,files:Object.keys(contents),close:()=>rmSync(root,{recursive:true,force:true})}
}
function check(added={},removed=[]) {
  const f=fixture(added,removed)
  try {return auditSourceHygiene(f.root,f.files)}
  finally {f.close()}
}

test('modular goal entrypoints with valid files pass the cleanup gate',()=>{
  assert.deepEqual(check(),[])
})
test('old root goals.rs cannot be accidentally reintroduced alongside mod.rs',()=>{
  const errors=check({[RETIRED_ENTRYPOINTS[0]]:'pub fn obsolete() {}\n'})
  assert.ok(errors.some(e=>e.includes('obsolete goals.rs restored')))
  assert.ok(errors.some(e=>e.includes('duplicate Rust module entrypoint')))
})
test('static source-contract tests fail before the obsolete file reaches cargo',()=>{
  const errors=check({'scripts/probe.mjs':"const x=source('src/api/goals.rs')\n"})
  assert.ok(errors.some(e=>e.includes('stale reference to deleted source')))
  assert.ok(errors.some(e=>e.includes('static repository source loader targets missing file')))
})
test('old synthetic SDK plan endpoint is blocked, even with a passing mock',()=>{
  const errors=check({'sdk/typescript/src/client.ts':
    'const p="/api/goals/" + id + "/plan";'});
  assert.ok(errors.some(e=>e.includes('deleted /api/goals/:id/plan')));
})
test('broken Rust explicit path attribute is reported, not ignored',()=>{
  const errors=check({'src/engine/goals/mod.rs':'#[path="missing.rs"]\nmod runner;\n'})
  assert.ok(errors.some(e=>e.includes('#[path] module target missing')))
})
test('generated artifacts and editor backups cannot enter tracked source',()=>{
  const errors=check({'src/engine/recovered.rs.bak':'orphaned','web/dist/ghost.js':'built'});
  assert.equal(errors.filter(e=>e.includes('tracked build or abandoned')).length,2)
})
test('provider monolith may not silently grow back after modularization',()=>{
  const errors=check({'src/api/providers.rs':'//'.padEnd(46000,'x')})
  assert.ok(errors.some(e=>e.includes('entrypoint exceeds 45000 bytes')))
})
test('public entrypoint expansion beyond 4KB demands a real module split',()=>{
  const errors=check({'src/engine/goals/mod.rs':'//'.padEnd(4200,'x')})
  assert.ok(errors.some(e=>e.includes('entrypoint exceeds 4096 bytes')))
})
test('missing public module entrypoint cannot be masked by stale root file',()=>{
  const errors=check({},['src/api/goals/mod.rs'])
  assert.ok(errors.some(e=>e.includes('missing public module entrypoint')))
})
