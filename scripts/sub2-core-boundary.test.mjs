import test from 'node:test';
import assert from 'node:assert/strict';
import {existsSync,readFileSync} from 'node:fs';
const source=path=>readFileSync(new URL('../'+path,import.meta.url),'utf8');
const exists=path=>existsSync(new URL('../'+path,import.meta.url));
const engine=source('src/engine/mod.rs');
const chat=source('src/engine/core/chat/mod.rs');

test('core entrypoint owns natural language execution, not product services',()=>{
 const core=source('src/engine/core/mod.rs');
 assert.match(core,/pub async fn run_quiet_outcome/);
 assert.match(core,/pub async fn run_quiet_resume/);
 assert.doesNotMatch(engine,/pub async fn run_quiet_outcome/);
 assert.match(engine,/pub use core::/);
 assert.match(engine,/#\[path = "core\/chat\/mod.rs"\]/);
 for(const name of ['models','memory','rooms','jobs','tools','router']){
  assert.ok(!exists('src/engine/'+name+'.rs'),'stale product implementation '+name);
 }
});

const chatServices={
 provider:'providers/chat_provider.rs',
 codex_ws:'providers/chat_codex_ws.rs',
 context:'conversation/chat_context.rs',
 context_window:'conversation/chat_context_window.rs',
 continuity:'conversation/chat_continuity.rs',
 room_context:'conversation/chat_room_context.rs',
 prompt_sources:'conversation/chat_prompt_sources.rs',
 trace_budget:'operations/chat_trace_budget.rs',
 evidence:'decision/chat_evidence.rs',
 tool_projection:'tools/chat_tool_projection.rs',
 capability:'tools/chat_capability.rs',
 transport:'providers/chat_transport.rs'
};
test('context, providers, catalogs and transport are outside execution core',()=>{
 for(const [mod,target] of Object.entries(chatServices)){
  assert.ok(chat.includes('#[path = "../../services/'+target+'"]'),'missing path boundary '+mod);
  assert.ok(exists('src/engine/services/'+target),'missing service '+target);
  assert.ok(!exists('src/engine/core/chat/'+mod+'.rs'),'core still owns '+mod);
 }
 for(const mod of ['protocol','runtime','budget','cancellation','approval','outcome']){
  assert.ok(exists('src/engine/core/chat/'+mod+'.rs'),'missing execution stage '+mod);
 }
});

test('no UI navigation changes or SDK API removal from core refactor',()=>{
 assert.match(source('src/lib.rs'),/pub mod sdk;/);
 assert.match(source('web/src/components/WorkspaceShell.tsx'),/id:'connections'/);
 assert.match(source('src/api/addons.rs'),/\/api\/mcp\/catalog/);
});
