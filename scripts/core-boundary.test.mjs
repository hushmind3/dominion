import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync,existsSync,readdirSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
import path from 'node:path';
const root=fileURLToPath(new URL('..',import.meta.url));
const file=p=>path.join(root,p);
const read=p=>readFileSync(file(p),'utf8');
const groups={
 cognition:['adaptation_loop','distill','agent_growth','algorithms','compact','decompose','episode','experience','procedure','reflect','improve','learning','memory','plan_quality','principle','adaptation'],
 collaboration:['debate','personas','rooms','room_knowledge','room_state'],
 providers:['native_model','native_codex_ws','native_cache','native_routing','native_stream','gateway','cli_model','llm','models','codex_oauth','router','reasoning'],
 platform:['attachments','backup_keys','dev','code_index','remote_backup','maintenance','policy','preflight_fixture','vault_rotation','wake'],
 forensics:['native_replay','native_adjudication']
};
test('portable agent kernel is the only core, with no SQLite or product AppState',()=>{
 const manifest=read('crates/dominion-agent-sdk/Cargo.toml');
 assert.doesNotMatch(manifest,/\brusqlite\b|\breqwest\b/);
 const srcDir=file('crates/dominion-agent-sdk/src');
 const sources=readdirSync(srcDir).filter(x=>x.endsWith('.rs'));
 for(const name of sources){
  assert.doesNotMatch(read('crates/dominion-agent-sdk/src/'+name),
    /\brusqlite\b|crate::db\b|crate::storage\b|crate::state\b|\bAppState\b/,name+' must stay portable');
 }
 const entry=read('crates/dominion-agent-sdk/src/lib.rs');
 assert.match(entry,/pub trait Model/);
 assert.match(entry,/pub trait Tools/);
 assert.match(entry,/pub trait Journal/);
});
test('DB, migrations, vault and shared host state cannot reappear in core',()=>{
 assert.equal(existsSync(file('src/engine')),false);
 assert.equal(existsSync(file('src/db.rs')),false);
 assert.equal(existsSync(file('src/migrations.rs')),false);
 assert.equal(existsSync(file('src/vault.rs')),false);
 assert.equal(existsSync(file('src/state.rs')),false);
 for(const p of ['storage/sqlite.rs','storage/migrations.rs','storage/vault.rs','host/state.rs',
                  'host/execution/native_agent.rs','host/execution/jobs.rs']){
  assert.ok(existsSync(file('src/'+p)),'missing product-only '+p);
 }
 const main=read('src/main.rs');
 assert.match(main,/use crate::storage::\{sqlite as db, migrations, vault\}/);
 assert.match(main,/use crate::compat::engine/);
 const host=read('src/host/execution/mod.rs');
 assert.doesNotMatch(host,/pub use crate::features::/);
 assert.doesNotMatch(host,/pub use crate::integrations::/);
 assert.ok(existsSync(file('src/compat/engine.rs')));
 assert.match(read('src/compat/engine.rs'),/pub\(crate\) use crate::host::execution::\*/);
 const declared=[...host.matchAll(/^pub(?:\(crate\))? mod ([a-z_]+);$/gm)].map(x=>x[1]).sort();
 assert.deepEqual(declared,['goal_control','native_agent','native_evidence','effect','program',
 'verification','recovery','jobs','lease','run_state'].sort());
});
test('product capabilities register outside the host execution adapter',()=>{
 for(const [group,names] of Object.entries(groups))for(const name of names){
  const nested=name==='adaptation';
  const path='src/features/'+group+'/'+name+(nested?'/mod.rs':'.rs');
  assert.ok(existsSync(file(path)),path+' must remain optional');
 }
 const jobs=read('src/host/execution/jobs.rs');
 assert.match(jobs,/features::dispatch_job/);
 assert.match(jobs,/integrations::registry::dispatch_job/);
 for(const kind of ['room','meeting','learn','reflect','discord'])
  assert.doesNotMatch(jobs,new RegExp('"'+kind+'"\\s*=>'));
 const native=read('src/host/execution/native_agent.rs');
 assert.doesNotMatch(native,/impl Journal for/);
 assert.match(read('src/host/journal.rs'),/impl Journal for DurableJournal/);
 assert.match(native,/use crate::host::journal::DurableJournal/);
 assert.doesNotMatch(native,/\bmcp::/);
 assert.match(native,/external::native_external_tools/);
 const tools=read('src/integrations/local_tools.rs');
 assert.match(tools,/features::registry::definitions/);
 assert.match(tools,/integrations::registry::definitions/);
 assert.doesNotMatch(tools,/tool\("(?:memory_store|memory_search|web_search|http_fetch|code_search)"/);
 assert.doesNotMatch(tools,/async fn (?:http_fetch|web_search)\(/);
 assert.match(read('src/integrations/network.rs'),/pub\(crate\) async fn http_fetch/);
 assert.match(read('src/features/cognition/tools.rs'),/pub fn definitions/);
});
