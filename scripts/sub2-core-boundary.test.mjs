import test from 'node:test';
import assert from 'node:assert/strict';
import {existsSync,readFileSync,readdirSync} from 'node:fs';
const source=path=>readFileSync(new URL('../'+path,import.meta.url),'utf8');
const exists=path=>existsSync(new URL('../'+path,import.meta.url));
const engine=source('src/engine/mod.rs');
const chat=source('src/engine/services/conversation/chat/mod.rs');

test('core entrypoint owns natural language execution, not product services',()=>{
 const core=source('src/engine/core/mod.rs');
 assert.match(source('crates/dominion-turn-core/src/lib.rs'),/pub async fn run_with_events/);
 assert.match(source('crates/dominion-turn-core/src/lib.rs'),/pub trait ExecutionPort/);
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
 assert.match(s,/driver\.authorize_step/);
 assert.match(s,/driver\.admit_tool/);
 assert.match(s,/Driver::batch_advertised/);
 assert.ok(exists('crates/dominion-turn-core/src/driver.rs'));
});

test('provider and tool authority are exposed as host ports, not implemented in core',()=>{
 const core=source('src/engine/core/mod.rs');
 const driver=source('crates/dominion-turn-core/src/driver.rs');
 assert.doesNotMatch(core,/crate::engine::(memory|mcp|rooms|jobs|models|policy)/);
 assert.match(core,/pub use dominion_turn_core::\*/);
 assert.match(source('Cargo.toml'),/dominion-turn-core =/);
 assert.doesNotMatch(source('crates/dominion-turn-core/Cargo.toml'),/dominion-rust|mcp|reqwest|rusqlite|axum/);
 assert.doesNotMatch(driver,/crate::engine|AppState|reqwest|tokio::process/);
 assert.match(driver,/pub async fn request_model/);
 assert.match(source('crates/dominion-turn-core/src/lib.rs'),/pub trait ExecutionPort/);
});

test('product core is a one-file facade; standalone core has a stable source boundary',()=>{
 assert.deepEqual(readdirSync(new URL('../src/engine/core/',import.meta.url)).sort(),['mod.rs']);
 assert.deepEqual(readdirSync(new URL('../crates/dominion-turn-core/src/',import.meta.url)).sort(),['agent_loop.rs','driver.rs','lib.rs','port_tests.rs']);
});


test('policy algorithms are independently replaceable and never imported by execution core',()=>{
 const execution=source('src/engine/services/conversation/chat/runtime/turn_body.rs');
 const policy=source('src/engine/services/decision/turn_policy.rs');
 const strategies=source('crates/dominion-agent-kernel/src/decision_policy.rs');
 const driver=source('crates/dominion-turn-core/src/driver.rs');
 assert.doesNotMatch(execution,/crate::engine::algorithms::|crate::engine::policy::/);
 for(const name of ['budget','discovery','decomposition','order','combination',
                    'write_choice','retry_choice','signal','unrated','active']){
  assert.match(policy,new RegExp('pub fn '+name+'\\('));
 }
 for(const call of ['decision_policy.order()','decision_policy.combination()',
                    'decision_policy.retry_choice()','decision_policy.write_choice()']){
  assert.ok(execution.includes(call),'the running chat turn does not use policy hook '+call);
 }
 for(const name of ['ToolOrder','ToolCombination','WriteChoice','RetryChoice']){
  assert.ok(strategies.includes('pub enum '+name));
 }
 assert.match(strategies,/pub fn admit_plan/);
 assert.doesNotMatch(driver,/dominion_agent_kernel|turn_policy|crate::engine::algorithms/);
 assert.doesNotMatch(source('crates/dominion-turn-core/Cargo.toml'),/dominion-agent-kernel/);
 assert.match(source('src/engine/mod.rs'),/services\/decision\/turn_policy.rs/);
});

test('planning heuristics and decomposition flow only through the decision adapter',()=>{
 const runtime=source('src/engine/services/conversation/chat/runtime/turn_body.rs');
 const adapter=source('src/engine/services/decision/turn_policy.rs');
 const pure=source('crates/dominion-agent-kernel/src/decision_policy.rs');
 assert.doesNotMatch(runtime,/crate::engine::decompose::|crate::engine::plan_quality::/);
 assert.match(runtime,/decision_policy\.decompose_request\(/);
 assert.match(runtime,/TurnPolicy::plan_heuristic\(/);
 assert.match(runtime,/TurnPolicy::skip_extra_planner\(/);
 assert.match(runtime,/TurnPolicy::plan_scores\(/);
 assert.match(adapter,/pub fn decompose_request/);
 assert.match(adapter,/pub fn plan_scores/);
 assert.match(pure,/pub fn needs_plan/);
 assert.match(pure,/pub fn local_multifile_skips_extra_planner/);
});

test('swap-ready agent loops use the core trait without importing the gateway',()=>{
 const contract=source('crates/dominion-turn-core/src/agent_loop.rs');
 const algo=source('crates/dominion-agent-loop/src/lib.rs');
 const runtime=source('src/engine/services/conversation/chat/runtime/turn_body.rs');
 const policy=source('src/engine/services/decision/turn_policy.rs');
 assert.match(contract,/pub trait AgentLoop/);
 assert.match(contract,/pub trait ModelPort/);
 assert.match(contract,/pub trait ToolPort/);
 assert.match(contract,/fn authorize_step/);
 assert.match(algo,/impl AgentLoop for ReActLoop/);
 assert.match(algo,/impl AgentLoop for PlanExecuteLoop/);
 assert.match(algo,/pub async fn run</);
 assert.match(runtime,/driver\.authorize_step\(&mut agent_loop/);
 assert.match(runtime,/decision_policy\.workflow\(plan_requested\)/);
 assert.match(policy,/pub fn workflow\(/);
 assert.doesNotMatch(source('crates/dominion-agent-loop/Cargo.toml'),/dominion-rust|rusqlite|reqwest|axum/);
 assert.doesNotMatch(source('crates/dominion-turn-core/Cargo.toml'),/dominion-agent-loop/);
 assert.match(source('Cargo.toml'),/dominion-agent-loop =/);
});

test('active workflow revisions use existing policy storage without UI changes',()=>{
 const adapter=source('src/engine/services/decision/turn_policy.rs');
 const algorithms=source('crates/dominion-agent-loop/src/lib.rs');
 assert.match(adapter,/self\.active\("agent\.loop"\)/);
 assert.match(adapter,/dominion_agent_loop::select_named\(selected,plan_requested\)/);
 assert.match(algorithms,/pub fn select_named\(/);
 assert.match(algorithms,/Some\("react"\)/);
 assert.match(algorithms,/Some\("plan-execute"\)/);
 assert.doesNotMatch(source('crates/dominion-turn-core/src/agent_loop.rs'),/agent\.loop|select_named/);
});
