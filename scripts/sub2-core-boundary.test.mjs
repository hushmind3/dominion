import test from 'node:test';
import assert from 'node:assert/strict';
import {existsSync,readFileSync,readdirSync} from 'node:fs';
const source=path=>readFileSync(new URL('../'+path,import.meta.url),'utf8');
const exists=path=>existsSync(new URL('../'+path,import.meta.url));
const engine=source('src/engine/mod.rs');
const chat=source('src/engine/services/conversation/chat/mod.rs');

test('core entrypoint owns natural language execution, not product services',()=>{
 const core=source('crates/dominion-turn-core/src/lib.rs');
 assert.match(engine,/pub use dominion_turn_core as core;/);
 assert.ok(!exists('src/engine/core/mod.rs'),'obsolete core shim reintroduced');
 assert.match(core,/pub async fn run_with_events/);
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
 assert.match(chat,/use dominion_agent_kernel as budget;/);
 assert.match(chat,/use dominion_context_policy as trace_budget;/);
 assert.ok(!exists('src/engine/services/conversation/chat/budget.rs'));
 assert.ok(!exists('src/engine/services/operations/chat_trace_budget.rs'));
 for(const mod of ['protocol','runtime','cancellation','approval','outcome']){
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
 assert.match(s,/Harness::new/);
 assert.match(s,/harness\.request_model/);
 assert.match(s,/harness\.authorize_step/);
 assert.match(s,/harness\.admit_batch/);
 assert.match(s,/budget\.charge_proposals\(round_calls\.len\(\)\)/);
 assert.doesNotMatch(s,/driver\.admit_tool/);
 assert.doesNotMatch(s,/Driver::batch_advertised/);
 assert.ok(exists('crates/dominion-turn-core/src/driver.rs'));
});

test('provider and tool authority are exposed as host ports, not implemented in core',()=>{
 const core=source('crates/dominion-turn-core/src/lib.rs');
 const driver=source('crates/dominion-turn-core/src/driver.rs');
 assert.doesNotMatch(core,/crate::engine::(memory|mcp|rooms|jobs|models|policy)/);
 assert.match(engine,/pub use dominion_turn_core as core;/);
 assert.match(engine,/pub use dominion_agent_kernel as agent_kernel;/);
 assert.ok(!exists('src/engine/services/decision/agent_kernel.rs'));
 assert.match(source('Cargo.toml'),/dominion-turn-core =/);
 assert.doesNotMatch(source('crates/dominion-turn-core/Cargo.toml'),/dominion-rust|mcp|reqwest|rusqlite|axum/);
 assert.doesNotMatch(driver,/crate::engine|AppState|reqwest|tokio::process/);
 assert.match(driver,/pub async fn request_model/);
 assert.match(driver,/pub fn admit_batch/);
 assert.doesNotMatch(driver,/pub fn admit_tool|pub fn batch_advertised/);
 assert.match(source('crates/dominion-turn-core/src/lib.rs'),/pub trait ExecutionPort/);
});

test('product imports the standalone core directly without facade files',()=>{
 assert.ok(!exists('src/engine/core/mod.rs'));
 assert.match(engine,/pub use dominion_turn_core as core;/);
 assert.deepEqual(readdirSync(new URL('../crates/dominion-turn-core/src/',import.meta.url)).sort(),['agent_loop.rs','driver.rs','lib.rs','port_tests.rs','run_state.rs']);
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
 assert.match(runtime,/Harness::new/);
 assert.match(runtime,/harness\.authorize_step/);
 assert.match(algo,/Harness::new\(driver,policy\)/);
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

test('group coordination persists indexed owner-scoped workboard and injects only compact committed evidence',()=>{
 const rooms=source('src/engine/services/conversation/rooms.rs');
 const compiler=source('src/engine/services/conversation/chat_room_context.rs');
 const board=source('src/engine/services/conversation/room_workboard.rs');
 const db=source('src/db.rs');
 assert.match(rooms,/room_workboard::record\(/);
 assert.match(compiler,/room_workboard::snapshot\(/);
 assert.match(compiler,/workboardTokens/);
 assert.match(board,/put_if_absent\("room_work_item"/);
 assert.match(board,/replyToMessageId/);
 assert.match(board,/completedToolCount/);
 assert.match(board,/visible_messages/);
 assert.match(db,/idx_room_work_scope/);
 assert.match(db,/pub fn room_work_items/);
 assert.match(engine,/services\/conversation\/room_workboard.rs/);
 assert.doesNotMatch(source('crates/dominion-turn-core/src/lib.rs'),/room_workboard/);
});

test('final answer persistence and continuity commit are isolated from the inference loop',()=>{
 const turn=source('src/engine/services/conversation/chat/runtime/turn_body.rs');
 const finish=source('src/engine/services/conversation/chat/runtime/finalize.rs');
 assert.match(turn,/super::finalize::finish\(/);
 assert.match(finish,/pub\(super\) fn finish\(/);
 assert.match(finish,/super::continuity::commit\(/);
 assert.match(finish,/super::continuity::commit_projection\(/);
 assert.match(finish,/cancellation_generation\(session_id\)/);
});

test('room workers use bounded Tokio JoinSet and remain private to the existing chat surface',()=>{
 const rooms=source('src/engine/services/conversation/rooms.rs');
 const workers=source('src/engine/services/conversation/room_parallel.rs');
 const roomApi=source('src/api/services/conversation/rooms.rs');
 const sessionApi=source('src/api/services/conversation/sessions.rs');
 assert.match(rooms,/parallel::run\(/);
 assert.match(workers,/JoinSet::<WorkerResult>/);
 assert.match(workers,/const MAX_CONCURRENT:usize=4/);
 assert.match(workers,/chat::run_turn_background/);
 assert.match(workers,/tools_enabled:false,tools_execution_enabled:false/);
 assert.match(workers,/room_workboard::record/);
 assert.match(roomApi,/executionMode/);
 assert.match(sessionApi,/room_worker/);
 assert.doesNotMatch(source('crates/dominion-turn-core/src/lib.rs'),/room_parallel/);
});

test('owner ingress and acceptance preflight are outside the model loop',()=>{
 const runtime=source('src/engine/services/conversation/chat/runtime/turn_body.rs');
 const setup=source('src/engine/services/conversation/chat/runtime/preflight.rs');
 assert.match(runtime,/super::preflight::prepare\(/);
 assert.match(setup,/pub\(super\) fn prepare\(/);
 assert.match(setup,/attachments::validate_ids/);
 assert.match(setup,/acceptance\.demoted/);
 assert.match(setup,/episode::attach_source/);
 assert.doesNotMatch(setup,/stream_completion|dispatch_tool_call|run_program/);
});

test('group work assignments require committed source-matching receipts',()=>{
 const board=source('src/engine/services/conversation/room_workboard.rs');
 const room=source('src/engine/services/conversation/rooms.rs');
 const parallel=source('src/engine/services/conversation/room_parallel.rs');
 assert.match(board,/pub fn assign\(/);
 assert.match(board,/pub fn complete\(/);
 assert.match(board,/pub fn skip\(/);
 assert.match(board,/ownerDigest/);
 assert.match(board,/status":"assigned"/);
 assert.match(board,/evidenceStatus/);
 assert.match(room,/room_workboard::assign\(/);
 assert.match(room,/room_workboard::complete\(/);
 assert.match(parallel,/room_workboard::complete\(/);
});

test('actual provider requests share bounded local/cloud gates independent of algorithm',()=>{
 const provider=source('src/engine/services/providers/chat_provider.rs');
 const state=source('src/state.rs');
 assert.match(provider,/provider_slot\(&gate_provider_id,&gate_model,allowed\)/);
 assert.match(provider,/candidate\.local\{1\}else\{4\}/);
 assert.match(provider,/state\.cancel_notify\.notified\(\)/);
 assert.match(state,/OwnedSemaphorePermit/);
 assert.match(state,/Arc<Semaphore>/);
});

test('catalog and MCP/skill/account visibility live outside the model-round loop',()=>{
 const turn=source('src/engine/services/conversation/chat/runtime/turn_body.rs');
 const catalog=source('src/engine/services/conversation/chat/runtime/tool_catalog.rs');
 assert.match(turn,/super::tool_catalog::prepare\(/);
 assert.match(catalog,/pub\(super\) async fn prepare\(/);
 assert.match(catalog,/retain_connected_account_tools/);
 assert.match(catalog,/canonical_tool_definitions/);
 assert.match(catalog,/capability::should_enable/);
 assert.match(catalog,/discovery_catalog/);
 assert.doesNotMatch(catalog,/stream_completion|dispatch_tool_call/);
 assert.doesNotMatch(turn,/addons::tool_definitions\(state\)\.await/);
});

test('provider tool history compaction is pure independently testable middleware',()=>{
 const pure=source('crates/dominion-context-policy/src/lib.rs');
 const dep=source('crates/dominion-context-policy/Cargo.toml');
 assert.match(pure,/pub fn bounded_tool_trace/);
 assert.match(pure,/pub fn compact_historical/);
 assert.match(pure,/completed_frame_end/);
 assert.match(pure,/sourceMessageId/);
 assert.match(chat,/use dominion_context_policy as trace_budget;/);
 assert.ok(!exists('src/engine/services/operations/chat_trace_budget.rs'));
 assert.doesNotMatch(dep,/dominion-rust|dominion-turn-core|reqwest|rusqlite|axum/);
 assert.doesNotMatch(source('crates/dominion-turn-core/Cargo.toml'),/dominion-context-policy/);
 assert.match(source('Cargo.toml'),/dominion-context-policy =/);
});

test('fallback model capabilities are model specific and fail closed on explicit incompatibility',()=>{
 const gateway=source('src/engine/services/providers/gateway.rs');
 const completion=source('src/engine/services/providers/chat_provider.rs');
 assert.match(gateway,/pub struct ModelRequirements/);
 assert.match(gateway,/pub fn supports_requirements\(/);
 assert.match(gateway,/modelCapabilities/);
 assert.match(completion,/gateway::supports_requirements\(/);
 assert.match(completion,/tools:!tools\.is_empty\(\)/);
});

test('chat, SDK jobs and room assignments share a durable typed run state projection',()=>{
 const core=source('crates/dominion-turn-core/src/run_state.rs');
 const bus=source('src/events.rs');
 const board=source('src/engine/services/conversation/room_workboard.rs');
 assert.match(core,/pub enum RunState/);
 assert.match(core,/pub struct RunEvent/);
 assert.match(core,/pub fn from_record/);
 assert.match(bus,/RunEvent::from_record\(kind,record\)/);
 assert.match(bus,/value\["eventSeq"\]=json!\(seq\)/);
 assert.match(bus,/self\.emit_many\(events\)/);
 assert.match(board,/RunState::parse/);
 assert.match(board,/memory::stable_id\(&\[/);
 assert.match(board,/state\.bus\.record\("room_work_assignment",&saved\)/);
 assert.doesNotMatch(board,/RunEvent\{/);
});

test('both agent loops share one tool transcript protocol and persisted feedback',()=>{
 const core=source('crates/dominion-turn-core/src/agent_loop.rs');
 const independent=source('crates/dominion-agent-loop/src/lib.rs');
 const chat=source('src/engine/services/conversation/chat/runtime/turn_body.rs');
 const dispatch=source('src/engine/services/conversation/chat/runtime/tool_calls.rs');
 assert.match(core,/pub struct ToolFeedback/);
 assert.match(core,/pub fn provider_message/);
 assert.match(independent,/ToolFeedback::outcome_unknown/);
 assert.match(independent,/ToolFeedback::not_dispatched/);
 assert.match(chat,/ToolFeedback::from_content/);
 assert.match(dispatch,/ToolFeedback::from_value/);
 assert.match(dispatch,/"content":feedback\.content/);
 assert.match(dispatch,/soft_denial_replay_is_byte_identical_to_provider_transcript/);
});

test('owner source binding is durable across Chat Room and SDK run state feeds',()=>{
 const preflight=source('src/engine/services/conversation/chat/runtime/preflight.rs');
 const lifecycle=source('src/engine/services/conversation/chat/runtime.rs');
 assert.match(preflight,/fn bind_execution_source/);
 assert.match(preflight,/patch_if\("execution",execution_id/);
 assert.match(preflight,/bind_execution_source\(state,execution_id,reply_to/);
 assert.match(lifecycle,/"sourceMessageId":source_message_id/);
 assert.match(source('src/events.rs'),/RunEvent::from_record\(kind,record\)/);
});

test('dependency aware team execution cannot parallelize delegated writers',()=>{
 const p=source('crates/dominion-agent-loop/src/task_team.rs');
 const runtime=source('src/engine/services/decision/runtime_program.rs');
 const ir=source('src/engine/services/decision/program.rs');
 assert.match(p,/pub fn schedule\(/);
 assert.match(p,/cyclic team dependencies/);
 assert.match(runtime,/schedule\.requires_serial\(\)/);
 assert.match(runtime,/for wave in &schedule\.waves/);
 assert.match(ir,/dominion_agent_loop::task_team::schedule/);
});

test('DAG ready read waves stay concurrent while writer and delegate steps remain fenced',()=>{
 const s=source('src/engine/services/decision/runtime_program.rs');
 assert.match(s,/for wave in &schedule\.waves/);
 assert.match(s,/parallel_mode!="sequential" && wave\.len\(\)>1/);
 assert.match(s,/dependency_waves/);
 assert.match(s,/budget\.parallel_shares\(wave\.len\(\)\)/);
});

test('model/provider duplicate call IDs cannot be silently rewritten by native Chat',()=>{
 const product=source('src/engine/services/conversation/chat/runtime/turn_body.rs');
 const driver=source('crates/dominion-turn-core/src/driver.rs');
 assert.match(product,/Driver::validate_call_ids/);
 assert.doesNotMatch(product,/!seen_ids\.insert\(call\.id\.clone\(\)\)/);
 assert.match(driver,/pub fn validate_call_ids/);
 assert.match(driver,/Self::validate_call_ids\(pending\.iter/);
});
test('DAG shares the native safe-read allowlist and persists writer fences before dispatch',()=>{
 const team=source('crates/dominion-agent-loop/src/task_team.rs');
 const program=source('src/engine/services/decision/runtime_program.rs');
 assert.match(team,/dominion_agent_kernel::scheduling::safe_to_overlap/);
 assert.match(team,/for previous in 0\.\.i/);
 assert.match(program,/fn commit_program_checkpoint/);
 assert.match(program,/commit_program_checkpoint\(state, execution_id, checkpoint\)\?/);
 assert.match(program,/depth\+1,persist,true/);
});

test('product tools share cross-process workspace writer lease without locking nested delegation',()=>{
 const resources=source('src/engine/services/operations/resource_lock.rs');
 const native=source('src/engine/services/tools/tools.rs');
 assert.match(resources,/pub async fn acquire_durable/);
 assert.match(resources,/LeaseGuard::acquire\(state,&scope\)/);
 assert.match(resources,/resource-writer:/);
 assert.match(native,/resource_lock::acquire_durable/);
 assert.match(resources,/"delegate_task"/);
});
