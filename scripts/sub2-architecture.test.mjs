import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
const file=path=>readFileSync(new URL('../'+path,import.meta.url),'utf8');
test('runtime delegates tool projection, terminal decisions and verification once',()=>{
 const runtime=file('src/engine/chat/runtime.rs');
 const projection=file('src/engine/chat/tool_projection.rs');
 const outcomes=file('src/engine/chat/outcome.rs');
 const module=file('src/engine/chat/mod.rs');
 for(const forbidden of ['fn select_room_tools(', 'fn select_tool_definitions(',
   'fn finalize_execution(', 'fn determine_task_outcome(']){
   assert.equal(runtime.includes(forbidden),false,forbidden+' must be owned by a single module');
 }
 assert.ok(projection.includes('fn select_room_tools('));
 assert.ok(outcomes.includes('fn finalize_execution('));
 assert.ok(outcomes.includes('fn determine_task_outcome('));
 assert.ok(module.includes('mod tool_projection;') && module.includes('mod outcome;'));
});
test('pricing, evidence, locks, and QA remain available with the imported main UI',()=>{
 const files=['src/engine/model_cost.rs','src/engine/route_calibration.rs',
   'src/engine/evaluation.rs','src/engine/resource_lock.rs'];
 for(const path of files)assert.ok(file(path).length>400,path);
 assert.ok(file('src/api/mod.rs').includes('/api/evaluation/summary'));
 assert.ok(file('src/api/system.rs').includes('engine::evaluation::summary(&state)'));
 assert.ok(file('sdk/typescript/src/index.ts').includes('evaluation = {'));
 assert.ok(file('web/src/App.tsx').includes('SettingsHeader'));
 assert.ok(file('web/src/pages/Operations.tsx').includes('export function OperationsPage'));
 assert.ok(file('src/engine/tools.rs').includes('resource_lock::acquire'));
 assert.doesNotMatch(file('src/engine/mod.rs'),/pub mod video;/);
 assert.doesNotMatch(file('src/engine/mod.rs'),/pub mod rsi(?:_\w+)?;/);
});

test('the native owner task loop delegates decisions to a model-free execution kernel',()=>{
 const autonomy=file('src/engine/autonomy.rs');
 const kernel=file('crates/dominion-agent-kernel/src/lib.rs');
 const adapter=file('src/engine/agent_kernel.rs');
 assert.match(adapter,/pub use dominion_agent_kernel::\*/);
 assert.match(autonomy,/engine::agent_kernel::/);
 assert.match(autonomy,/run_quiet_outcome/);
 assert.doesNotMatch(autonomy,/fn side_effect_unknown\(/);
 assert.doesNotMatch(autonomy,/fn decide\(/);
 for(const signature of ['pub fn decide(', 'pub fn side_effect_unknown(',
   'pub fn owner_intent(', 'pub fn next_instruction('])
    assert.ok(kernel.includes(signature),signature);
 assert.match(kernel,/outcomeUnknown/);
});

test('a standalone Rust task kernel is consumed by the same native owner worker',()=>{
 const manifest=file('Cargo.toml');
 const crate=file('crates/dominion-agent-kernel/Cargo.toml');
 const lib=file('crates/dominion-agent-kernel/src/lib.rs');
 const adapter=file('src/engine/agent_kernel.rs');
 const worker=file('src/engine/autonomy.rs');
 assert.match(manifest,/dominion-agent-kernel = \{ path = "crates\/dominion-agent-kernel" \}/);
 assert.match(crate,/name = "dominion-agent-kernel"/);
 assert.match(lib,/pub fn decide\(/);
 assert.match(lib,/pub fn side_effect_unknown\(/);
 assert.match(adapter,/pub use dominion_agent_kernel::\*/);
 assert.match(worker,/agent_kernel::/);
});

test('chat budgets and cancellation share the real kernel instead of polling SQLite at 20Hz',()=>{
 const kernel=file('crates/dominion-agent-kernel/src/budget.rs');
 const adapter=file('src/engine/chat/budget.rs');
 const runtime=file('src/engine/chat/runtime.rs');
 const cancel=file('src/engine/chat/cancellation.rs');
 const state=file('src/state.rs');
 assert.match(kernel,/pub struct BudgetLedger/);
 assert.match(kernel,/self\.rounds >= self\.max_rounds \|\| self\.expired\(\)/);
 assert.match(adapter,/pub use dominion_agent_kernel::BudgetLedger;/);
 assert.match(cancel,/cancel_notify\.notified\(\)/);
 assert.match(cancel,/MissedTickBehavior::Skip/);
 assert.match(state,/cancel_notify\.notify_waiters\(\)/);
 assert.doesNotMatch(runtime,/tokio::time::sleep\(Duration::from_millis\(50\)\)/);
});

test('cancellation supervisor is separated from model/provider runtime and preserves durable review',()=>{
 const runtime=file('src/engine/chat/runtime.rs');
 const cancel=file('src/engine/chat/cancellation.rs');
 const module=file('src/engine/chat/mod.rs');
 assert.match(module,/mod cancellation;/);
 assert.match(runtime,/cancellation::supervise_tool_cancellation/);
 assert.doesNotMatch(runtime,/fn supervise_tool_cancellation</);
 assert.match(cancel,/pub\(super\) async fn supervise_tool_cancellation/);
 assert.match(cancel,/tool\.unsettled_after_cancel/);
 assert.match(cancel,/outcomeUnknown/);
 assert.match(cancel,/durable_cross_process_cancellation_works_without_local_notification/);
 assert.match(cancel,/local_cancel_notifies_without_waiting_for_the_persistence_backstop/);
});

test('Home SDK progress polls only active approval IDs and exact stored result messages',()=>{
 const bridge=file('src/api/sdk_bridge.rs');
 assert.doesNotMatch(bridge,/state\.store\.all\("approval"\)/);
 assert.match(bridge,/live_ids\.into_iter\(\)\.find_map/);
 assert.match(bridge,/state\.store\.message\(id\)/);
 assert.match(bridge,/panel_poll_uses_live_approval_ids_and_scoped_answer_receipts/);
});

test('verification and delegated work read indexed per-run receipts rather than all history',()=>{
 const evidence=file('src/engine/chat/evidence.rs');
 const tools=file('src/engine/tools.rs');
 assert.match(evidence,/execution_operations\(&run_id\)/);
 assert.doesNotMatch(evidence,/store\.all\("execution_operation"\)/);
 const start=tools.indexOf('fn task_payload(');
 const end=tools.indexOf('fn required_arg(',start);
 assert.ok(start>=0 && end>start);
 const payload=tools.slice(start,end);
 assert.match(payload,/session_executions\(session_id\)/);
 assert.match(payload,/execution_operations\(run_id\)/);
 assert.doesNotMatch(payload,/\.all\("execution_operation"\)/);
});

test('GitHub repo file workflow uses connected OAuth, optimistic SHA and owner approval',()=>{
 const work=file('src/engine/github_work.rs');
 const tools=file('src/engine/tools.rs');
 const oauth=file('src/engine/oauth.rs');
 const effect=file('src/engine/effect.rs');
 const projection=file('src/engine/chat/tool_projection.rs');
 assert.match(work,/fresh_access_token\(state,"github"\)/);
 assert.match(work,/expectedSha/);
 assert.match(work,/get_json\(&client,url,&token\)/);
 assert.match(work,/outcomeUnknown/);
 assert.match(tools,/github_file_read/);
 assert.match(tools,/github_file_commit/);
 assert.match(tools,/github_file_commit"\s*\|/);
 assert.match(effect,/github_file_commit/);
 assert.match(projection,/github_file_read/);
 assert.match(oauth,/updated\["refresh_token"\]/);
});

test('runtime scoped receipt lookups avoid global history scans',()=>{
 const runtime=file('src/engine/chat/runtime.rs');
 const db=file('src/db.rs');
 const parts=['turn_body','program','deterministic','tool_calls','action_review',
   'selection','session_state'].map(name=>file('src/engine/chat/runtime/'+name+'.rs'));
 const live=[runtime,...parts].join('\n');
 assert.doesNotMatch(live,/\.all\("execution_operation"\)/);
 assert.doesNotMatch(live,/\.all\("execution"\)/);
 assert.match(live,/session_operations\(session_id\)/);
 assert.match(live,/task_executions\(task_id\)/);
 assert.match(db,/idx_operation_session/);
});

test('GitHub discovery resolves selected repository and access before file changes',()=>{
 const work=file('src/engine/github_work.rs');
 const tools=file('src/engine/tools.rs');
 const projection=file('src/engine/chat/tool_projection.rs');
 assert.match(work,/pub async fn repositories\(/);
 assert.match(work,/repo_summaries\(/);
 assert.match(work,/fresh_access_token\(state,"github"\)/);
 assert.match(work,/client\(\)\?/);
 assert.match(work,/std::sync::OnceLock/);
 assert.match(work,/\["permissions"\]\["push"\]/);
 assert.match(tools,/github_repositories/);
 assert.match(projection,/github_repositories/);
 assert.match(work,/repository_discovery_uses_only_explicit_git_permissions/);
});

test('MCP HTTP sessions are reused per server and vault credential without blind retries',()=>{
 const mcp=file('src/engine/mcp.rs');
 assert.match(mcp,/MAX_HTTP_SESSIONS:usize=16/);
 assert.match(mcp,/http_request\(state,server,"tools\/list"/);
 assert.match(mcp,/http_request\(state,server,"tools\/call"/);
 assert.match(mcp,/http_identity\(state,record\)/);
 assert.match(mcp,/Arc::ptr_eq/);
 assert.match(mcp,/lease\.valid=result\.is_ok\(\)/);
 assert.doesNotMatch(mcp,/let mut session = HttpSession::open\(server, state\)/);
});

test('turn coordinator delegates substantial single-responsibility modules',()=>{
 const runtime=file('src/engine/chat/runtime.rs');
 const modules=['turn_body','program','deterministic','tool_calls',
    'action_review','selection','session_state'];
 assert.ok(runtime.split('\n').length<600,'coordinator must remain under 600 lines');
 assert.doesNotMatch(runtime,/fn run_turn_body\(/);
 assert.doesNotMatch(runtime,/fn execute_nodes</);
 assert.match(runtime,/pub async fn run_turn\(/);
 for(const name of modules){
    assert.match(runtime,new RegExp('mod '+name+';'));
    const source=file('src/engine/chat/runtime/'+name+'.rs');
    assert.ok(source.split('\n').length<2000,name+' oversized');
    assert.match(source,/use super::\*/);
 }
 const tests=['outcome_tests','cancellation_drain_tests','harness_tool_selection_tests'];
 for(const name of tests)assert.ok(file('src/engine/chat/runtime/'+name+'.rs').length>100);
 const body=file('src/engine/chat/runtime/turn_body.rs');
 const program=file('src/engine/chat/runtime/program.rs');
 const dispatch=file('src/engine/chat/runtime/tool_calls.rs');
 assert.match(body,/pub\(super\) async fn run_turn_body\(/);
 assert.match(program,/pub\(super\) fn restore_program_state\(/);
 assert.match(dispatch,/pub\(super\) async fn dispatch_tool_call\(/);
});

test('the real-binary e2e suite is partitioned without dropping scenarios',()=>{
 const root=file('tests/e2e.rs');
 const modules=['conversation_rooms','recovery_scheduler','backup_adaptation',
    'program_recovery','tool_preflight','api_jobs','room_context_benchmark',
    'codex_transports'];
 assert.ok(root.split('\n').length<1000);
 for(const name of modules){
   assert.match(root,new RegExp('mod '+name+';'));
   const source=file('tests/e2e/'+name+'.rs');
   assert.ok(source.split('\n').length<2000,name+' huge test suite');
   assert.match(source,/use super::\*/);
 }
 assert.match(root,/fn scripted_provider\(/);
 assert.match(root,/fn start_mock_provider\(/);
 assert.match(file('tests/e2e/program_recovery.rs'),/a_mid_adaptation_restart_restores/);
 assert.match(file('tests/e2e/codex_transports.rs'),/codex_websocket_executes/);
});
