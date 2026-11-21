// Source ownership contract from the owner-supplied 2026-11-20 architecture.
// Fast Node-only gate. Deliberately does NOT claim full streaming-loop migration.
import assert from 'node:assert/strict';
import {readFileSync,existsSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
import path from 'node:path';
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..');
const text=p=>readFileSync(path.join(root,p),'utf8');
const has=(p,need)=>{
  const source=text(p);
  for(const needle of need)assert.ok(source.includes(needle),
    p+': missing architecture contract '+needle);
};
for(const p of [
  'crates/dominion-agent-runtime/src/contracts.rs',
  'crates/dominion-agent-runtime/src/run_contract.rs',
  'crates/dominion-agent-algorithms/src/agent_loop.rs',
  'crates/dominion-agent-sdk/src/lib.rs',
  'src/host/execution/native_agent.rs',
  'src/conversation/chat/runtime/turn.rs',
  'src/features/collaboration/room_cursor.rs',
  'src/features/collaboration/task_team.rs',
  'src/integrations/agent_bridge.rs',
  'src/storage/sqlite.rs',
  'src/api/rooms.rs',
  'sdk/typescript/src/index.ts',
  'sdk/rust/src/lib.rs'
])assert.ok(existsSync(path.join(root,p)),p+' must exist');
assert.ok(!existsSync(path.join(root,'src/engine')),
  'The retired mixed engine directory must not return');
has('crates/dominion-agent-runtime/src/contracts.rs',
  ['pub trait AgentLoop','pub trait AgentPorts','dispatch_fence']);
has('crates/dominion-agent-runtime/src/run_contract.rs',
  ['pub enum RunSurface','pub enum RunPhase','pub struct RoomEvent']);
has('crates/dominion-agent-algorithms/src/agent_loop.rs',
  ['impl AgentLoop for ReActLoop','runtime::dispatch_fence']);
has('crates/dominion-agent-sdk/src/lib.rs',['pub use dominion_agent_algorithms']);
has('src/host/execution/native_agent.rs',['AgentLoop','agent_loop.execute(']);
has('src/conversation/chat/runtime/turn.rs',
  ['dominion_agent_sdk::dispatch_fence','dominion_agent_sdk::dispatch_batch']);
has('src/features/collaboration/room_cursor.rs',
  ['through_message_id','MAX_UNREAD_PER_TURN']);
has('src/storage/sqlite.rs',
  ['CREATE TABLE IF NOT EXISTS room_events','CREATE TRIGGER IF NOT EXISTS room_events_message_commit',
   'write_room_answer_with_cursor','"throughSeq"','pub fn patch_if_all']);
has('src/features/collaboration/task_team.rs',['depends_on','WorkerRuntime','cancel_children']);
has('src/integrations/agent_bridge.rs',['run_codex','run_claude','run_qwen']);
has('src/api/rooms.rs',['pub async fn events']);
has('sdk/typescript/src/index.ts',['readonly runs=','readonly taskTeams=','events:(roomId:string,after=0']);
has('sdk/rust/src/lib.rs',['pub async fn get_run','pub async fn create_task_team','pub async fn room_events']);
for(const p of ['crates/dominion-agent-runtime/src/contracts.rs',
  'crates/dominion-agent-algorithms/src/agent_loop.rs']){
  const source=text(p);
  for(const forbidden of ['crate::state::AppState','rusqlite::','axum::',
    'crate::db::Store'])assert.ok(!source.includes(forbidden),
      p+' illegally imports Host or storage: '+forbidden);
}
console.log('Architecture source contracts present (no build; Chat upper-loop still separate)');
