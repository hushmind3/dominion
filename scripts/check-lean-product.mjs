#!/usr/bin/env node
// Architectural regression contract for DOMINION's deliberately lean Rust
// autonomous-agent product. This replaces legacy "main media/RSI parity":
// older main intentionally exposes features this product no longer carries.
import assert from 'node:assert/strict';
import fs from 'node:fs';
const read=p=>fs.readFileSync(p,'utf8');
const exists=p=>fs.existsSync(p);
const absent=[
 'src/api/video.rs','src/api/video_studio.rs','src/api/rsi.rs',
 'src/engine/video.rs','src/engine/video_hub.rs','src/engine/video_studio.rs',
 'web/src/pages/Video.tsx','web/src/pages/SelfImprovement.tsx',
 'web/src/components/VideoAutopilot.tsx','web/src/components/VideoHubBrowser.tsx',
 'web/src/components/VideoModelInstall.tsx','web/scripts/video-studio-smoke.mjs',
 'src/api/benchmark.rs','src/api/experiments.rs','src/engine/scheduler.rs',
 'scripts/rsi-lab.mjs','scripts/rsi-measure.mjs'
];
for(const p of absent)assert.ok(!exists(p),'Retired built-in product feature still exists: '+p);
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
const routes=read('src/api/mod.rs');
for(const pattern of ['/api/video','/api/youtube','/api/rsi','/api/experiment-requests','/api/benchmark/','/api/algorithms/','/api/improvements']){
 assert.ok(!routes.includes('"'+pattern),'Retired public endpoint remains: '+pattern);
}
for(const route of ['/api/chat','/api/jobs','/api/sdk/runs','/api/harness/runs',
 '/api/mcp','/api/attachments','/api/sessions','/api/events']){
 assert.ok(routes.includes(route),'Rust app missing existing product contract: '+route);
}
const native=read('src/engine/native_agent.rs'),
 kernel=read('crates/dominion-agent-sdk/src/lib.rs'),
 chat=read('src/engine/chat/runtime.rs'),
 jobs=read('src/engine/jobs.rs'),
 approval=read('src/engine/chat/approval.rs'),
 tools=read('src/engine/tools.rs');
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
const ts=read('sdk/typescript/src/index.ts');
assert.ok(!ts.includes('readonly video=')&&!ts.includes('readonly rsi='),'Retired TS SDK methods remain');
assert.ok(ts.includes('readonly nativeRuns=')&&ts.includes('readonly jobs='),
 'Native TypeScript client no longer routes to retained Rust agent');
const cargo=read('Cargo.toml');
assert.ok(!/^resvg\s*=/m.test(cargo),'Unused video raster renderer still in root Cargo dependencies');
console.log(JSON.stringify({leanProduct:true,legacyNoReplay:true,coreChat:true,
 rustSdk:true,toolsAndMcp:true,sourceFilesRemoved:absent.length},null,2));
