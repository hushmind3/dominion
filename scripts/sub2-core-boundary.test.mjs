import test from 'node:test';
import assert from 'node:assert/strict';
import {existsSync,readFileSync} from 'node:fs';
const source=path=>readFileSync(new URL('../'+path,import.meta.url),'utf8');
const exists=path=>existsSync(new URL('../'+path,import.meta.url));
const engine=source('src/engine/mod.rs');
const chat=source('src/engine/services/conversation/chat/mod.rs');

test('core entrypoint owns natural language execution, not product services',()=>{
 const core=source('src/engine/core/mod.rs');
 assert.match(core,/pub async fn run_with_events/);
 assert.match(core,/pub trait ExecutionPort/);
 const imports=core.split('\n').filter(line=>line.trimStart().startsWith('use ')).join('\n');
 assert.doesNotMatch(imports,/AppState|chat|mcp|memory|jobs|models/);
 assert.match(source('src/engine/services/automation/turn_quiet.rs'),/impl core::ExecutionPort for ChatPort/);
 assert.doesNotMatch(core,/pub async fn run_quiet_resume/);
 assert.doesNotMatch(core,/pub async fn run_room_quiet/);
 assert.match(source('src/engine/services/automation/turn_resume.rs'),/pub async fn run_quiet_resume/);
 assert.match(source('src/engine/services/conversation/room_worker.rs'),/pub async fn run_room_quiet/);
 assert.doesNotMatch(engine,/pub async fn run_quiet_outcome/);
 assert.match(engine,/pub use core::/);
 assert.match(engine,/#\[path = "services\/conversation\/chat\/mod.rs"\]/);
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
  assert.ok(chat.includes('#[path = "../../'+target+'"]'),'missing path boundary '+mod);
  assert.ok(exists('src/engine/services/'+target),'missing service '+target);
  assert.ok(!exists('src/engine/services/conversation/chat/'+mod+'.rs'),'core still owns '+mod);
 }
 for(const mod of ['protocol','runtime','budget','cancellation','approval','outcome']){
  assert.ok(exists('src/engine/services/conversation/chat/'+mod+'.rs'),'missing execution stage '+mod);
 }
});

const runtimeServices={
 program:'decision/runtime_program.rs',
 candidate_rollout:'decision/runtime_candidate_rollout.rs',
 action_review:'decision/runtime_action_review.rs',
 selection:'decision/runtime_selection.rs',
 session_state:'conversation/runtime_session_state.rs'
};
test('optional program and policy modules are no longer part of runtime core',()=>{
 const runtime=source('src/engine/services/conversation/chat/runtime.rs');
 for(const [mod,target] of Object.entries(runtimeServices)){
  assert.ok(runtime.includes('#[path = "../../'+target+'"]'),mod+' service declaration missing');
  assert.ok(exists('src/engine/services/'+target),'missing runtime service '+target);
  assert.ok(!exists('src/engine/services/conversation/chat/runtime/'+mod+'.rs'),'core still owns '+mod);
 }
});

test('no UI navigation changes or SDK API removal from core refactor',()=>{
 assert.match(source('src/lib.rs'),/pub mod sdk;/);
 assert.match(source('web/src/components/WorkspaceShell.tsx'),/id:'connections'/);
 assert.match(source('src/api/addons.rs'),/\/api\/mcp\/catalog/);
});
test('only language execution and approval endpoints remain in HTTP core',()=>{
 const api=source('src/api/mod.rs');
 const only=[...api.matchAll(/\.route\(\s*["'](\/api\/[^"']+)["']/g)].map(x=>x[1]).filter(x=>x!=='/api/*rest');
 const expected=['/api/health','/api/events','/api/chat','/api/sdk/runs',
 '/api/sdk/runs/:id','/api/sdk/runs/:id/cancel',
 '/api/sdk/runs/:id/approve','/api/sdk/runs/:id/resume',
 '/api/sdk/runs/:id/reconcile','/api/executions/:id/operations',
 '/api/approvals'];
 assert.deepEqual(only,expected);
 assert.match(api,/router=router\.merge\(surfaces::routes\(\)\)/);
 for(const name of ['provider_routes','conversation_routes','automation_routes','evaluation_routes','management_routes']){
  assert.ok(exists('src/api/surfaces/'+name+'.rs'),'missing HTTP surface '+name);
 }
});


const handlerGroups={
 providers:['providers','models','cli_oauth'],
 conversation:['agents','attachments','sessions','rooms','memory','personas','translation'],
 automation:['jobs','tasks','executions'],
 evaluation:['benchmark','experiments','improvements','experience'],
 management:['settings','system','dev','collections'],
 addons:['browser','channels','extensions','oauth','gateways','compat']
};
test('HTTP handler implementations are source-isolated from the minimal API core',()=>{
 const api=source('src/api/mod.rs');
 let moved=0;
 for(const [domain,names] of Object.entries(handlerGroups)){
  for(const name of names){
   const rel=domain==='addons'?'addons/'+name+'.rs':'services/'+domain+'/'+name+'.rs';
   assert.ok(api.includes('#[path = "'+rel+'"]'),name+' handler declaration missing');
   assert.ok(exists('src/api/'+rel),'missing handler '+name);
   assert.ok(!exists('src/api/'+name+'.rs'),'stale root handler '+name);
   moved++;
  }
 }
 assert.equal(moved,27);
 for(const mod of ['chat','sdk_bridge','approvals'])assert.ok(exists('src/api/'+mod+'.rs'));
});


test('real chat model loop consults pure execution driver and gates parallel tool batches',()=>{
 const s=source('src/engine/services/conversation/chat/runtime/turn_body.rs');
 assert.match(s,/driver\.request_model/);
 assert.match(s,/driver\.select_step/);
 assert.match(s,/driver\.admit_tool/);
 assert.match(s,/Driver::batch_advertised/);
 assert.ok(exists('src/engine/core/driver.rs'));
});

test('provider and tool authority are exposed as host ports, not implemented in core',()=>{
 const core=source('src/engine/core/mod.rs');
 const driver=source('src/engine/core/driver.rs');
 assert.doesNotMatch(core,/crate::engine::(memory|mcp|rooms|jobs|models|policy)/);
 assert.doesNotMatch(driver,/crate::engine|AppState|reqwest|tokio::process/);
 assert.match(driver,/pub async fn request_model/);
 assert.match(core,/pub trait ExecutionPort/);
});
