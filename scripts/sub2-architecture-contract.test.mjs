// Sub2 architecture invariants: executable source checks, no build/network.
// The Rust unit tests exercise transactional behavior; this gate prevents
// structural regressions even when refactors move code between packages.
import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync,existsSync} from 'node:fs';
const source=path=>readFileSync(new URL('../'+path,import.meta.url),'utf8');
const exists=path=>existsSync(new URL('../'+path,import.meta.url));

test('one Rust Harness owns provider/tool admission, independent strategies only select',()=>{
  const core=source('crates/dominion-turn-core/src/agent_loop.rs');
  const driver=source('crates/dominion-turn-core/src/driver.rs');
  const loops=source('crates/dominion-agent-loop/src/lib.rs').split('#[cfg(test)]')[0];
  const runtime=source('src/engine/services/conversation/chat/runtime/turn_body.rs');
  assert.match(core,/pub struct Harness</);
  assert.match(core,/pub async fn drive_ports</);
  assert.match(core,/pub fn prepare_response/);
  assert.match(core,/pub fn admit_prepared/);
  assert.match(core,/ToolFeedback::outcome_unknown/);
  assert.match(driver,/pub fn admit_batch/);
  assert.match(loops,/Harness::new\(driver,policy\)\.drive_ports\(/);
  assert.doesNotMatch(loops,/tools\.dispatch\(/,'standalone loop must not own tool dispatch');
  assert.doesNotMatch(loops,/model\.complete\(/,'standalone loop must not own provider cycles');
  assert.match(runtime,/Harness::new/);
  assert.match(runtime,/harness\.prepare_response/);
  assert.match(runtime,/harness\.admit_prepared/);
  assert.doesNotMatch(source('crates/dominion-turn-core/Cargo.toml'),
    /dominion-agent-loop|dominion-rust|rusqlite|reqwest|axum/);
  assert.doesNotMatch(source('crates/dominion-agent-loop/Cargo.toml'),
    /dominion-rust|rusqlite|reqwest|axum/);
  assert.ok(!exists('src/engine/core/mod.rs'),'shadow compatibility kernel reintroduced');
});

test('all model tools share one effect gateway and writer lock alias',()=>{
  const tools=source('src/engine/services/tools/tools.rs');
  const lease=source('src/engine/services/operations/resource_lock.rs');
  const bridge=source('src/engine/addons/bridge.rs');
  assert.match(tools,/resource_lock::acquire\(/);
  assert.match(tools,/resource_lock::acquire_durable\(/);
  assert.match(lease,/"file_write"=>/);
  assert.match(lease,/root\.starts_with\(&canonical\)/);
  assert.match(lease,/workspace:\{prefix\}/);
  assert.match(lease,/global:unresolved-file-write/);
  assert.match(lease,/absolute_file_write_inside_workspace_shares_writer_lease_with_shell/);
  assert.match(bridge,/external|External|harness|Harness/);
});

test('Task Team parent and worker claims share one durable SQLite transaction',()=>{
  const db=source('src/db.rs');
  const team=source('src/engine/services/decision/task_team_work.rs');
  const dag=source('crates/dominion-agent-loop/src/task_team.rs');
  assert.match(db,/pub fn patch_team_work_if_parent_running/);
  assert.match(db,/TransactionBehavior::Immediate/);
  assert.match(db,/parent\["status"\]!="running"/);
  assert.match(db,/parent\["sessionId"\]!=work\["sessionId"\]/);
  assert.match(team,/pub fn reassign_unstarted/);
  assert.match(team,/pub fn claim/);
  assert.match(team,/pub fn settle/);
  const callCount=(team.match(/patch_team_work_if_parent_running/g)||[]).length;
  assert.ok(callCount>=3,`expected reassign/claim/settle guards, got ${callCount}`);
  assert.match(team,/parent_cancellation_blocks_reassignment_claim_and_late_settlement/);
  assert.match(dag,/pub fn schedule/);
  assert.match(dag,/cyclic team dependencies/);
  assert.match(team,/tool_failed_observed/);
  assert.match(team,/member_verified=status=="tool_completed_observed"/);
});

test('Room Chat and Task Team are different coordinators over shared chat core',()=>{
  const rooms=source('src/engine/services/conversation/rooms.rs');
  const parallel=source('src/engine/services/conversation/room_parallel.rs');
  const cursor=source('src/engine/services/conversation/room_state.rs');
  const tasks=source('src/engine/services/decision/runtime_program.rs');
  assert.match(rooms,/room_workboard::record/);
  assert.match(parallel,/chat::run_turn_background/);
  assert.match(parallel,/tools_enabled:false,tools_execution_enabled:false/);
  assert.match(cursor,/pub fn acknowledge_read/);
  assert.match(tasks,/task_team_work::claim/);
  assert.match(tasks,/task_team_work::finish_team/);
  assert.doesNotMatch(source('crates/dominion-turn-core/src/lib.rs'),/room_workboard|task_team_work/);
});

test('Rust / event journal / TypeScript SDK agree on run lifecycle',()=>{
  const state=source('crates/dominion-turn-core/src/run_state.rs');
  const events=source('src/events.rs');
  const sdk=source('sdk/typescript/src/index.ts');
  for(const kind of ['execution','job','room_work_assignment','task_team_work','managed_team','managed_team_task','remote_task']){
    assert.ok(state.includes('"'+kind+'"'),`Rust missing ${kind}`);
    assert.ok(sdk.includes("'"+kind+"'"),`SDK missing ${kind}`);
  }
  assert.match(state,/pub fn transition/);
  assert.match(state,/invalid_phase_never_becomes_a_valid_terminal_receipt/);
  assert.match(events,/value\["eventSeq"\]=json!\(seq\)/);
  assert.match(events,/RunEvent::from_record/);
  assert.match(sdk,/event\.eventSeq===event\.seq/);
  assert.match(sdk,/async \*runEvents/);
  assert.match(source('src/engine/services/operations/run_state.rs'),/transition\(current,event\)/);
});

test('architecture is documented at code-level locations, not old core path',()=>{
  const design=source('docs/SUB2-ARCHITECTURE-2026-11-21.md');
  const engine=source('ENGINE.md');
  assert.match(engine,/docs\/SUB2-ARCHITECTURE-2026-11-21\.md/);

  for(const location of [
    'crates/dominion-turn-core','crates/dominion-agent-loop',
    'src/engine/services/providers/gateway.rs','src/engine/services/conversation/room_state.rs',
    'src/db.rs','src/events.rs','sdk/typescript/',
  ])assert.ok(design.includes(location),`missing documented boundary ${location}`);
});

test('new autonomous collaboration tools route into native Rust jobs and durable Room cursors',()=>{
  const toolGateway=source('src/engine/services/tools/tools.rs');
  const team=source('src/engine/services/tools/tools_team.rs');
  const mailbox=source('src/engine/services/tools/tools_room_peer.rs');
  const room=source('src/engine/services/conversation/room_state.rs');
  const events=source('src/events.rs');
  const sdk=source('sdk/typescript/src/index.ts');
  for(const tool of ['team_execute','team_status','room_peer_send','room_peer_inbox',
    'external_harness_sessions']){
    assert.ok(toolGateway.includes('"'+tool+'"'),'tool not reachable: '+tool);
  }
  assert.match(team,/task_team::schedule/);
  assert.match(team,/delegate\(ctx/);
  assert.match(team,/ctx\.state\.bus\.record\("managed_team_task"/);
  assert.match(mailbox,/ctx\.store\.put_if_absent\("room_peer_mail"/);
  assert.match(room,/event\["kind"\]=="room_peer_mail"/);
  assert.match(room,/acknowledge_peer_mail/);
  assert.match(events,/kind=="managed_team_task"/);
  assert.match(sdk,/'managed_team_task'/);
  assert.ok(!exists('src/engine/core/mod.rs'));
});

test('P3 remote Rust control plane and outbound native Worker reuse existing Harness and SSE',()=>{
  const cluster=source('src/remote.rs');
  const worker=source('src/remote_worker.rs');
  const boot=source('src/bootstrap.rs');
  const team=source('src/engine/services/tools/tools_team.rs');
  const sdk=source('sdk/typescript/src/index.ts');
  assert.match(cluster,/start_control_plane/);
  assert.match(cluster,/x-dominion-remote-token/);
  assert.match(cluster,/patch_if_all_merge/);
  assert.match(cluster,/needs_review/);
  assert.match(cluster,/remote_job_claim_is_atomic_and_old_epoch_cannot_settle/);
  assert.match(cluster,/real_http_remote_worker_registers_claims_renews_and_settles/);
  assert.match(worker,/Dominion::from_config/);
  assert.match(worker,/engine::jobs::enqueue_once/);
  assert.match(worker,/engine::jobs::start_now/);
  assert.match(worker,/parallelDelegationReadOnly/);
  assert.match(worker,/cancel_job/);
  assert.match(boot,/crate::remote::start_control_plane/);
  assert.match(boot,/crate::remote::run_worker/);
  assert.match(team,/crate::remote::submit/);
  assert.match(team,/crate::remote::task_status/);
  assert.match(team,/crate::remote::visible_workers/);
  assert.match(sdk,/'remote_task'/);
});
