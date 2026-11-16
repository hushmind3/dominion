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
test('pricing, evidence, locks, QA and UI share real native routes',()=>{
 const files=['src/engine/model_cost.rs','src/engine/route_calibration.rs',
   'src/engine/evaluation.rs','src/engine/resource_lock.rs'];
 for(const path of files)assert.ok(file(path).length>400,path);
 assert.ok(file('src/api/mod.rs').includes('/api/evaluation/summary'));
 assert.ok(file('src/api/system.rs').includes('engine::evaluation::summary(&state)'));
 assert.ok(file('sdk/typescript/src/index.ts').includes('evaluation = {'));
 assert.ok(file('web/src/pages/Operations.tsx').includes('/api/evaluation/summary'));
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
 const state=file('src/state.rs');
 assert.match(kernel,/pub struct BudgetLedger/);
 assert.match(kernel,/self\.rounds >= self\.max_rounds \|\| self\.expired\(\)/);
 assert.match(adapter,/pub use dominion_agent_kernel::BudgetLedger;/);
 assert.match(runtime,/cancel_notify\.notified\(\)/);
 assert.match(runtime,/MissedTickBehavior::Skip/);
 assert.match(state,/cancel_notify\.notify_waiters\(\)/);
 assert.doesNotMatch(runtime,/tokio::time::sleep\(Duration::from_millis\(50\)\)/);
});
