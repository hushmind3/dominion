#!/usr/bin/env node
// Architectural regression contract for DOMINION's deliberately lean Rust
// autonomous-agent product. This replaces legacy "main media/RSI parity":
// older main intentionally exposes features this product no longer carries.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import {execFileSync} from 'node:child_process';
const read=p=>fs.readFileSync(p,'utf8');
const exists=p=>fs.existsSync(p);
const absent=[
 'src/api/video.rs','src/api/video_studio.rs','src/api/rsi.rs',
 'src/engine/video.rs','src/engine/video_hub.rs','src/engine/video_studio.rs',
 'web/src/pages/Video.tsx','web/src/pages/SelfImprovement.tsx',
 'web/src/components/VideoAutopilot.tsx','web/src/components/VideoHubBrowser.tsx',
 'web/src/components/VideoModelInstall.tsx','web/scripts/video-studio-smoke.mjs',
 'src/api/benchmark.rs','src/api/experiments.rs','src/engine/scheduler.rs',
 'scripts/rsi-lab.mjs','scripts/rsi-measure.mjs',
 'crates/harness', 'packages/sdk-typescript'
];
// Prevent retired-domain code, clients and docs from silently returning.
// Assemble the forbidden identifiers, rather than introducing them into
// the source that the scanner itself inspects.
const retiredTerms=[['mo','ney'].join(''),['머','니'].join(''),['돈','벌'].join('')];
const tracked=execFileSync('git',['ls-files','-z'],{
 encoding:'utf8',maxBuffer:4*1024*1024
}).split('\0').filter(Boolean);
for(const file of tracked){
 const lowered=file.toLocaleLowerCase();
 assert.ok(!retiredTerms.some(term=>lowered.includes(term)),
  'Retired domain filename reintroduced: '+file);
 // Scan all tracked, human-readable product/configuration formats, not just
 // Rust/TS. Legacy features can survive in SQL, styles, HTML or env examples.
 if(!/\.(rs|ts|tsx|js|mjs|cjs|json|md|toml|yml|yaml|sh|sql|py|html|css|scss|svg|xml|txt|ini|conf|properties|graphql|gql|env)$/i.test(file)
  && !/(^|\/)\.env(?:\.[^/]+)?$/i.test(file))continue;
 if(!exists(file))continue;
 const content=read(file).toLocaleLowerCase();
 assert.ok(!retiredTerms.some(term=>content.includes(term)),
  'Retired domain reference reintroduced: '+file);
}
for(const p of absent)assert.ok(!exists(p),'Retired or duplicate product path still exists: '+p);
// Reject duplicate local module imports in the existing TypeScript UI. Most
// imports are single-line; this zero-dependency gate catches accidental
// double wiring without mutating or rebuilding any UI.
for(const path of tracked.filter(p=>/^web\/src\/.*\.tsx?$/.test(p)&&exists(p))){
 const modules=new Set();
 for(const match of read(path).matchAll(/^\s*import\b[^\n]*\bfrom\s*['"]([^'"]+)['"]/gm)){
  const target=match[1];
  if(!target.startsWith('.'))continue;
  assert.ok(!modules.has(target),'Duplicate frontend module import: '+path+' -> '+target);
  modules.add(target);
 }
}
// The old campaign provider policy must not silently return to the live chat
// routing path. Historical job records are handled by a separate archive guard.
const liveProvider=read('src/engine/chat/provider.rs');
for(const stale of ['rsiRun','rsiLocalOnly','rsiApprovedProviderId',
 'rsiApprovedModel','rsiExplicitFocus','rsiAutoFocus','rsi_edit_requires_tool'])
 assert.ok(!liveProvider.includes(stale),'Retired model-policy branch returned: '+stale);
for(const p of fs.readdirSync('src/engine').filter(p=>/^rsi.*\.rs$/.test(p))){
 assert.fail('Live Rust RSI engine module survived retirement: '+p);
}
const app=read('web/src/App.tsx'),nav=read('web/src/components/WorkspaceShell.tsx');
for(const p of ['ChatPage','MemoryPage','ConnectionsPage','RoomsPage','HomePage'])
 assert.ok(app.includes(p),'Retained feature disconnected from app: '+p);
assert.ok(!/page==='(?:video|rsi)'/.test(app),'Retired route remains in app');
for(const p of ['nav.chat','nav.memory','nav.connections','nav.rooms'])
 assert.ok(nav.includes(p),'Retained navigation missing: '+p);
assert.ok(!/id:'(?:video|rsi)'/.test(nav),'Retired navigation remains');

// A retired upload integration must not remain a live OAuth provider or
// silently regain a Google refresh grant through fallback credentials.
const oauth=read('src/engine/oauth.rs');
assert.ok(!/id:\s*["']youtube["']/.test(oauth),'Removed upload OAuth provider returned');
assert.ok(!oauth.includes('auth/youtube.'),'Removed upload permission returned');
assert.ok(!/id\s*==\s*["']youtube["']/.test(oauth),'Removed upload token fallback returned');
assert.ok(oauth.includes('purge_retired_grants'),'Obsolete OAuth grant cleanup missing');
const routes=read('src/api/mod.rs');
const providerEndpoints=[
 '/api/providers/catalog/:id/models',
 '/api/providers/openrouter/free-models'
];
for(const route of providerEndpoints){
 assert.ok(routes.includes('"'+route+'"'),'Latest main model UI route missing: '+route);
}
for(const path of ['web/src/components/ConnectedAccounts.tsx',
 'web/src/components/DominionCloudAccount.tsx','web/src/refreshEvents.ts',
 'web/src/utils/modelFree.ts','web/src/utils/modelHardware.ts']){
 assert.ok(exists(path),'Latest main frontend component missing: '+path);
}
for(const pattern of ['/api/video','/api/youtube','/api/rsi','/api/experiment-requests','/api/benchmark/policies','/api/algorithms/generate','/api/algorithms/evaluate','/api/improvements']){
 assert.ok(!routes.includes('"'+pattern),'Retired public endpoint remains: '+pattern);
}
for(const route of ['/api/chat','/api/jobs','/api/sdk/runs','/api/harness/runs',
 '/api/mcp/catalog','/api/algorithms','/api/attachments','/api/sessions','/api/events']){
 assert.ok(routes.includes(route),'Rust app missing existing product contract: '+route);
}
const native=read('src/engine/native_agent.rs'),
 kernel=read('crates/dominion-agent-sdk/src/lib.rs'),
 chatRoot=read('src/engine/chat/runtime.rs'),
 chatModuleFiles=fs.readdirSync('src/engine/chat/runtime')
  .filter(file=>file.endsWith('.rs')).map(file=>'src/engine/chat/runtime/'+file),
 chat=chatRoot+'\n'+chatModuleFiles.map(read).join('\n'),
 jobs=read('src/engine/jobs.rs'),
 approval=read('src/engine/chat/approval.rs'),
 tools=read('src/engine/tools.rs');
for(const required of ['turn.rs','program.rs','dispatch.rs','finalization.rs','selection.rs']){
 assert.ok(chatModuleFiles.some(file=>file.endsWith('/'+required)),
  'Missing extracted Rust chat responsibility: '+required);
}
for(const file of ['src/engine/chat/runtime.rs',...chatModuleFiles]){
 const lines=read(file).split('\n').length;
 assert.ok(lines<=2000,'Oversized chat file ('+lines+' lines): '+file);
}
for(const name of ['run_turn_body','run_program','dispatch_tool_call','finalize_execution']){
 assert.ok(chat.includes('fn '+name+'('),
  'Chat lifecycle function lost during module extraction: '+name);
}
// Enforce a maintainable Rust source layout, including integration tests.
// This is a structural regression gate, not a claim about execution speed.
function rustFiles(directory){
 return fs.readdirSync(directory,{withFileTypes:true}).flatMap(entry=>{
  const path=directory+'/'+entry.name;
  if(entry.isDirectory())return rustFiles(path);
  return entry.isFile()&&entry.name.endsWith('.rs')?[path]:[];
 });
}
const allRust=['src','tests','crates','sdk/rust'].flatMap(rustFiles);
for(const file of allRust){
 const lines=read(file).split('\n').length;
 const focused=file.startsWith('src/engine/chat/runtime/')||
  file==='src/engine/chat/runtime.rs' ||
  file.startsWith('tests/e2e/') || file==='tests/e2e.rs';
 const limit=focused?2000:4000;
 assert.ok(lines<=limit,
  'Rust source must be decomposed before it grows to '+lines+
  ' lines (limit '+limit+'): '+file);
}
for(const suite of ['rooms','durability','adaptation','web_api','continuity'])
 assert.ok(exists('tests/e2e/'+suite+'.rs'),
  'Real integration test suite missing after split: '+suite);
for(const text of ['dominion_agent_sdk::','dispatch_batch','acceptance'])
 assert.ok(chat.includes(text),'Real chat-to-Rust execution/verification disconnected: '+text);
assert.ok(!/rsiRun|rsi_evolve|rsi_worktree|rsi_patch_written/.test(chat),'Retired RSI logic remains in chat runtime');
assert.ok(!approval.includes('rsi_evolve::auto_write_allowed'),'Old RSI approval bypass survived');
assert.ok(!tools.includes('tool("rsi_')&&!tools.includes('tool("algorithm_propose"'),'Old RSI tools exposed to model');
for(const signature of ['retired_legacy_kind','active_jobs()','job_wakeup.notified()'])
 assert.ok(jobs.includes(signature),'Worker lost archival safety or indexed wakeup: '+signature);
assert.ok(!read('src/bootstrap.rs').includes('schedule_experiments'),'Auto-experiment boot hook survived');
assert.ok(native.includes('dominion_sdk::harness::'),'Native job stopped using owned Rust SDK');
assert.ok(kernel.includes('mod schema;')&&kernel.includes('pub mod replay;'),'Kernel lost proven argument/evidence operations');
const rustSdk=read('sdk/rust/src/lib.rs');
for(const old of ['pub async fn video_jobs','pub async fn create_video','pub async fn rsi_campaigns'])
 assert.ok(!rustSdk.includes(old),'Retired Rust SDK method survived: '+old);
for(const retained of ['pub async fn jobs','pub async fn chat','pub async fn events'])
 assert.ok(rustSdk.includes(retained),'Rust SDK lost normal work/chat continuity: '+retained);
const ts=read('sdk/typescript/src/index.ts');
assert.ok(!ts.includes('readonly video=')&&!ts.includes('readonly rsi='),'Retired TS SDK methods remain');
assert.ok(ts.includes('readonly nativeRuns=')&&ts.includes('readonly jobs='),
 'Native TypeScript client no longer routes to retained Rust agent');
const cargo=read('Cargo.toml');
assert.ok(!/^resvg\s*=/m.test(cargo),'Unused video raster renderer still in root Cargo dependencies');
assert.ok(!cargo.includes('dominion-harness')&&!cargo.includes('crates/harness'),
 'The retired parallel Rust harness returned to the product workspace');
const legacyApi=read('src/api/sdk.rs');
assert.ok(!legacyApi.includes('dominion_harness::')&&!legacyApi.includes('AgentSdk::run'),
 'The old SDK execution loop returned to the live compatibility API');
assert.ok(legacyApi.includes('sdk_runs::create')&&legacyApi.includes('archiveOnly'),
 'Legacy API must reuse the native runner and keep historical records inspectable');
const nativeSdkApi=read('src/api/sdk_runs.rs');
assert.ok(nativeSdkApi.includes('harnessCompat')&&nativeSdkApi.includes('allowedTools'),
 'Legacy API migration lost its narrower tool-authority boundary');
const capabilityBridge=read('src/engine/chat/runtime/capability.rs');
const chatTurn=read('src/engine/chat/runtime/turn.rs');
const chatDispatch=read('src/engine/chat/runtime/dispatch.rs');
assert.ok(capabilityBridge.includes('execution_operations(execution_id)')&&
 capabilityBridge.includes('schemaSha256')&&
 capabilityBridge.includes('dominion_agent_sdk::validate_arguments'),
 'Progressive discovery must keep same-run receipt, schema pinning and argument admission');
assert.ok(chatTurn.includes('capability::bind_model_call(')&&
 chatDispatch.includes('capability::search(state,session_id,&arguments).await'),
 'Chat discovery must route through the existing execution and tool journal');
console.log(JSON.stringify({leanProduct:true,legacyNoReplay:true,coreChat:true,
 rustSdk:true,toolsAndMcp:true,sourceFilesRemoved:absent.length},null,2));
