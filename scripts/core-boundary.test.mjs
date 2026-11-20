import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync,existsSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
import path from 'node:path';
const root=fileURLToPath(new URL('..',import.meta.url));
const read=p=>readFileSync(path.join(root,p),'utf8');
const engine=read('src/engine/mod.rs');
const coreNames=[...engine.matchAll(/^pub(?:\(crate\))? mod ([a-z_]+);$/gm)].map(x=>x[1]);
const groups={
 cognition:['adaptation_loop','distill','agent_growth','algorithms','compact','decompose','episode','experience','procedure','reflect','improve','learning','memory','plan_quality','principle','adaptation'],
 collaboration:['debate','personas','rooms','room_knowledge','room_state'],
 providers:['native_model','native_codex_ws','native_cache','native_routing','native_stream','gateway','cli_model','llm','models','codex_oauth','router','reasoning'],
 platform:['attachments','backup_keys','dev','code_index','remote_backup','maintenance','policy','preflight_fixture','vault_rotation','wake'],
 forensics:['native_replay','native_adjudication']
};
test('only the language-to-action execution spine is declared by engine',()=>{
 assert.deepEqual(coreNames.sort(),['goal_control','native_agent','native_evidence','effect','chat','program','verification','recovery','jobs','lease','run_state','tools'].sort());
 assert.ok(existsSync(path.join(root,'crates/dominion-agent-sdk/src/lib.rs')));
 assert.match(read('src/main.rs'),/mod features;/);
});
test('optional module implementations cannot be reintroduced under engine',()=>{
 for(const [group,names] of Object.entries(groups)){
  for(const name of names){
   const nested=name==='adaptation';
   const from='src/engine/'+name+(nested?'/mod.rs':'.rs');
   const to='src/features/'+group+'/'+name+(nested?'/mod.rs':'.rs');
   assert.equal(existsSync(path.join(root,from)),false,from+' belongs outside core');
   assert.ok(existsSync(path.join(root,to)),to+' must be maintained as a feature');
  }
 }
});
test('non-core jobs and tools are dispatched through feature registries',()=>{
 const jobs=read('src/engine/jobs.rs');
 assert.match(jobs,/features::dispatch_job/);
 assert.match(jobs,/integrations::registry::dispatch_job/);
 for(const kind of ['room','meeting','learn','reflect','discord']){
  assert.doesNotMatch(jobs,new RegExp('"'+kind+'"\\s*=>'));
 }
 const tools=read('src/engine/tools.rs');
 assert.match(tools,/features::registry::definitions/);
 assert.match(tools,/integrations::registry::definitions/);
 assert.doesNotMatch(tools,/tool\("(?:memory_store|memory_search|web_search|http_fetch|code_search)"/);
 assert.doesNotMatch(tools,/async fn (?:http_fetch|web_search)\(/);
 assert.match(read('src/integrations/network.rs'),/pub\(crate\) async fn http_fetch/);
 assert.match(read('src/features/cognition/tools.rs'),/pub fn definitions/);
});
