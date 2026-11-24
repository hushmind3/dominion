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
  'src/conversation/chat/runtime/resume_safety_tests.rs',
  'src/conversation/chat/runtime/input_contract.rs',
  'src/storage/sqlite/remote_tasks.rs',
  'src/features/cognition/algorithms/catalogue.rs',
  'src/features/collaboration/submission.rs',
  'src/features/collaboration/prompt_settings.rs',
  'src/features/collaboration/room_config.rs',
  'src/features/providers/connections.rs',
  'src/features/providers/probe.rs',
  'src/host/execution/job_control.rs',
  'src/host/execution/native_agent.rs',
  'src/conversation/chat/runtime/turn.rs',
  'src/features/collaboration/room_cursor.rs',
  'src/features/collaboration/task_team.rs',
  'extensions/mcp-connector/src/mcp/modern.rs',
  'extensions/mcp-connector/src/mcp/tests.rs',
  'scripts/mcp-echo-modern-server.mjs',
  'src/integrations/agent_bridge.rs',
  'src/api/remote_workers.rs',
  'src/host/execution/remote_worker.rs',
  'src/storage/sqlite.rs',
  'src/api/rooms.rs',
  'sdk/typescript/src/index.ts',
  'sdk/rust/src/lib.rs'
])assert.ok(existsSync(path.join(root,p)),p+' must exist');
assert.ok(!existsSync(path.join(root,'src/engine')),
  'The retired mixed engine directory must not return');
has('crates/dominion-agent-runtime/src/contracts.rs',
  ['pub trait AgentLoop','pub struct AgentPorts','dispatch_fence']);
has('crates/dominion-agent-runtime/src/run_contract.rs',
  ['pub enum RunSurface','pub enum RunPhase','pub struct RoomEvent','pub struct RunEvent']);
has('crates/dominion-agent-algorithms/src/agent_loop.rs',
  ['impl AgentLoop for ReActLoop','runtime::dispatch_model_turn']);
has('crates/dominion-agent-sdk/src/lib.rs',['pub use dominion_agent_algorithms']);
has('src/host/execution/native_agent.rs',['AgentLoop','agent_loop.execute(']);
has('src/conversation/chat/runtime/turn.rs',
  ['dominion_agent_sdk::dispatch_model_turn','dominion_agent_sdk::dispatch_batch']);
has('crates/dominion-agent-runtime/src/drive.rs',
  ['pub async fn dispatch_model_turn','pub enum ModelAttempt',
   'pub fn settle_model_turn','pub fn settle_rejected_model_turn']);
has('src/conversation/chat/protocol.rs',
  ['native_stream::SseDecoder','Packet::Data(value)','Packet::Done']);
has('src/conversation/chat/runtime/turn.rs',
  ['settle_rejected_chat_turn','settle_rejected_model_turn']);
has('src/conversation/chat/runtime/turn.rs',
  ['let lightweight_chat =','(!lightweight_chat || discovery["ownerActivated"]==true)']);
has('build.rs',['DOMINION_BUILD_COMMIT','DOMINION_BUILD_IDENTITY_SOURCE',
  'watch_git_path("HEAD")']);
has('src/build_identity.rs',['embedded-build','embedded-release','unavailable']);
assert.ok(!text('src/build_identity.rs').includes('source:"source-checkout"'),
  'health may not label moving checkout HEAD as a binary revision');
has('src/api/system.rs',['"binaryBuildSha": crate::build_identity::running().commit']);

assert.ok(!text('src/conversation/chat/runtime/turn.rs').includes('pure_chat_greeting'),
  'obsolete pure_chat_greeting reference prevents product Rust compilation');
has('src/conversation/chat/provider.rs',
  ['malformed Anthropic tool frame must fail']);
assert.ok(!text('src/conversation/chat/provider.rs').includes(
  '&messages,&[],&request,"tool-test",&tx,false).await.unwrap_err()'),
  'opaque Completion must not require Debug just to assert a provider error');

has('crates/dominion-agent-algorithms/src/agent_loop.rs',
  ['runtime::settle_rejected_model_turn','runtime::settle_model_turn']);
has('src/api/sdk_runs.rs',['"phase":phase','"needs_review"=>"needs_review"']);
has('src/conversation/chat/runtime.rs',
  ['ensure_task_resume_safe','run_has_uncertain_operation','LeaseGuard::acquire']);
has('src/conversation/chat/runtime.rs',['mod input_contract;']);
has('src/conversation/chat/runtime/turn.rs',['input_contract::prepare_input_and_acceptance']);
has('src/conversation/chat/runtime/input_contract.rs',
  ['pub(super) fn prepare_input_and_acceptance',
   'acceptanceSourceMessageId','retitle_if_placeholder']);
has('src/conversation/chat/runtime/resume_safety_tests.rs',
  ['paid_model_unknown_is_not_relaunched','unknown_tool_operation_blocks_resume']);
has('src/features/providers/native_stream.rs',
  ['native_chat_supports_final_message_only_without_duplication',
   'native_chat_does_not_replay_message_when_delta_already_arrived',
   'pub(crate) fn validate_anthropic_stop_reason']);
has('src/conversation/chat/provider.rs',['validate_anthropic_stop_reason(reason)?']);
// The retired browser fixture is not a product architecture dependency.
// Owner commands can be called by HTTP and transport adapters independently.
assert.ok(!text('src/integrations/channels/bridge.rs').includes('crate::api::'),
  'Channel ingestion may not call an HTTP handler');
for(const p of ['src/features/collaboration/submission.rs',
  'src/features/collaboration/prompt_settings.rs',
  'src/features/collaboration/room_config.rs',
  'src/features/providers/connections.rs',
  'src/features/providers/probe.rs',
  'src/host/execution/job_control.rs']){
  assert.ok(!text(p).includes('axum::'),p+' must not depend on HTTP handlers');
}
has('crates/dominion-agent-algorithms/src/harness.rs',
  ['macro_rules! harness_steps','both_loop_styles_share_one_control_flow_source']);
has('crates/dominion-agent-algorithms/src/agent_loop.rs',
  ['crate::harness_steps!','runtime::dispatch_model_turn']);
has('src/conversation/chat/runtime/turn.rs',
  ['dominion_agent_sdk::harness_steps!',
   'dominion_agent_sdk::dispatch_model_turn']);
has('src/features/collaboration/room_cursor.rs',
  ['through_message_id','MAX_UNREAD_PER_TURN']);
has('src/storage/sqlite.rs',
  ['CREATE TABLE IF NOT EXISTS room_events','CREATE TRIGGER IF NOT EXISTS room_events_message_commit',
   'write_room_answer_with_cursor','"throughSeq"','pub fn patch_if_all']);
has('src/features/collaboration/task_team.rs',['depends_on','WorkerRuntime','cancel_children']);
has('extensions/mcp-connector/src/mcp.rs',
  ['mod modern;','server/discover','modern::decorate',
   'modern::legacy_http_probe_rejection','modern::advertised_legacy_version',
   'session.request_tool','verify_pinned_tool']);
has('extensions/mcp-connector/src/mcp/modern.rs',
  ['2026-07-28','io.modelcontextprotocol/protocolVersion',
   'io.modelcontextprotocol/clientCapabilities','Mcp-Method',
   'Mcp-Name','Mcp-Param-','validate_tool_schema',
   'pub(super) fn envelope','advertised_legacy_version']);
has('extensions/mcp-connector/src/mcp.rs',
  ['modern::envelope(&plain)','session.request_tool','protocolMode']);
has('scripts/mcp-echo-modern-server.mjs',
  ['server/discover','2026-07-28','tools/list','tools/call']);
const mcpRoot=text('extensions/mcp-connector/src/mcp.rs').split('\n').length;
assert.ok(mcpRoot<900,'MCP transport must keep protocol and tests in separate modules');
has('src/integrations/agent_bridge.rs',['run_codex','run_claude','run_qwen']);
has('src/api/remote_workers.rs',['gateway_router','worker_authorized',
  'worker_claim','worker_complete','owner_create']);
has('src/host/execution/remote_worker.rs',['run_cli','run_team_read']);
has('src/storage/sqlite/remote_tasks.rs',
  ['pub fn claim_remote_task','pub fn finish_remote_task',
   'pub fn heartbeat_remote_task','pub fn cancel_remote_task',
   'pub fn review_expired_remote_tasks']);
has('src/storage/sqlite.rs',['mod remote_tasks;']);
has('src/features/cognition/algorithms.rs',['mod catalogue;','catalogue::builtins()']);
has('src/features/cognition/algorithms/catalogue.rs',
  ['pub(super) fn builtins()','builtin(']);
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

for(const p of ['src/host/execution/native_agent.rs','src/host/execution/effect.rs','src/conversation/chat/runtime/dispatch.rs','src/conversation/chat/runtime/capability.rs','src/conversation/chat/runtime/turn.rs']){
 const production=text(p).split('#[cfg(test)]')[0];
 for(const forbidden of ['mcp::','integrations::mcp','starts_with("mcp__")','personal_voice','recipe::voice_tag'])assert.ok(!production.includes(forbidden),p+' leaked connector identity '+forbidden);
}
assert.ok(!existsSync(path.join(root,'plugins/dominion-voice')),'Independent applications must not be embedded');
assert.ok(!existsSync(path.join(root,'src/api/plugins/personal_voice.rs')),'Independent app host handler must not return');

assert.ok(!existsSync(path.join(root,'src/integrations/mcp.rs')),'MCP transport must not live in the product integration tree');
has('Cargo.toml',['default = []','legacy-mcp = []']);
has('src/integrations/mod.rs',['#[cfg(feature = "legacy-mcp")]','../../extensions/mcp-connector/src/mcp.rs']);
