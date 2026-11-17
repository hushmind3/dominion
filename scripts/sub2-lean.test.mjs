import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync,existsSync} from 'node:fs';
const src=path=>readFileSync(new URL('../'+path,import.meta.url),'utf8');
test('no default automatic learning or perpetual maintenance sweeps',()=>{
  const chat=src('src/engine/chat/mod.rs');
  const jobs=src('src/engine/jobs.rs');
  const rooms=src('src/api/rooms.rs');
  const uiChat=src('web/src/pages/Chat.tsx');
  const uiRoom=src('web/src/pages/Rooms.tsx');
  assert.match(chat,/auto_learning: value[\s\S]*?get\("autoLearning"\)[\s\S]*?unwrap_or\(false\)/);
  assert.match(rooms,/"autoLearning":bool_field\(body,previous,"autoLearning",false\)/);
  assert.match(uiChat,/\[autoLearn,setAutoLearn\]=useState\(false\)/);
  assert.match(uiRoom,/autoLearning:room\?\.autoLearning===true/);
  assert.doesNotMatch(jobs,/retry_failed_learning\(&maintenance_state\)/);
  assert.doesNotMatch(jobs,/learning_sweep\(&maintenance_state\)/);
});
test('waiting workers avoid reading historical job records every 400ms',()=>{
  const db=src('src/db.rs'),jobs=src('src/engine/jobs.rs');
  assert.match(db,/CREATE INDEX IF NOT EXISTS idx_job_status/);
  assert.match(db,/pub fn active_jobs\(&self\)/);
  assert.match(jobs,/state\.store\.active_jobs\(\)/);
  assert.match(jobs,/receiver\.recv\(\)/);
  assert.doesNotMatch(jobs,/sleep\(Duration::from_millis\(400\)\)/);
});
test('built-in video and RSI screens are absent from the primary navigation',()=>{
  const nav=src('web/src/components/WorkspaceShell.tsx');
  const app=src('web/src/App.tsx');
  assert.doesNotMatch(nav,/\{id:'video',label:'nav\.video'/);
  assert.doesNotMatch(nav,/\{id:'rsi',label:'nav\.rsi'/);
  assert.doesNotMatch(app,/<VideoPage/);
  assert.doesNotMatch(app,/<SelfImprovementPage/);
  assert.match(nav,/\{id:'connections',label:'nav\.connections'/);
  assert.match(nav,/\{id:'memory',label:'nav\.memory'/);
});

test('legacy generation and RSI effects cannot be started by worker or HTTP',()=>{
  const api=src('src/api/mod.rs');
  const jobs=src('src/engine/jobs.rs');
  assert.match(api,/async fn retired_capability\(/);
  assert.match(api,/get\(rsi::list\)\.post\(retired_capability\)/);
  assert.match(api,/get\(video::list_jobs\)\.post\(retired_capability\)/);
  assert.doesNotMatch(jobs,/"rsi_evolve"\s*=>\s*crate::engine::rsi_evolve::run/);
  assert.doesNotMatch(jobs,/"video"\s*=>\s*crate::engine::video::run/);
  assert.doesNotMatch(jobs,/start_rsi_install\(state.clone\(\)/);
  assert.match(api,/\/api\/video\/jobs\/:id\/content/);
  const media=file=>src(file);
  assert.match(media('src/api/video.rs'),/pub async fn content/);
  assert.doesNotMatch(media('src/engine/mod.rs'),/pub mod video;/);
});

test('no background improvement campaigns triggered by normal execution completion',()=>{
 const outcome=src('src/engine/chat/outcome.rs');
 assert.doesNotMatch(outcome,/improve::schedule_experiments\(state\)/);
 assert.doesNotMatch(outcome,/policy::meta_improve\(state\)/);
 assert.doesNotMatch(outcome,/policy::propose_candidates\(state\)/);
 assert.match(outcome,/route_calibration::observe_verified/);
});

test('old multimedia and RSI source/build dependencies are absent',()=>{
 const engine=src('src/engine/mod.rs');
 const cargo=src('Cargo.toml');
 assert.doesNotMatch(engine,/pub mod rsi(?:_\w+)?;/);
 assert.doesNotMatch(engine,/pub mod video;/);
 assert.doesNotMatch(cargo,/\bresvg\s*=/);
 const runtime=src('src/engine/chat/runtime.rs');
 assert.doesNotMatch(runtime,/crate::engine::rsi_evolve|rsiRun/);
 const dev=src('src/engine/dev.rs');
 assert.doesNotMatch(dev,/fn start_rsi_install|install_rsi_source/);
});

test('no hidden failed-learning retry loop or retired YouTube uploader claim',()=>{
 const jobs=src('src/engine/jobs.rs');
 const compat=src('src/api/compat.rs');
 assert.doesNotMatch(jobs,/pub fn retry_failed_learning\(/);
 assert.match(compat,/"uploadAllowed":false/);
 assert.match(compat,/"archiveOnly":true/);
});

test('SDK start does not enqueue autonomous policies or experiments after restart',()=>{
  const sdk=src('src/sdk.rs');
  assert.match(sdk,/engine::improve::recover_finalizations\(&self\.state\)/);
  assert.doesNotMatch(sdk,/engine::improve::schedule_experiments\(&self\.state\)/);
  assert.doesNotMatch(sdk,/engine::policy::meta_improve\(&self\.state\)/);
  assert.doesNotMatch(sdk,/engine::policy::propose_candidates\(&self\.state\)/);
});
test('retired RSI UI links and source-generation workflows are absent',()=>{
  const chat=src('web/src/pages/Chat.tsx');
  const nav=src('web/src/components/WorkspaceShell.tsx');
  const home=src('web/src/pages/Home.tsx');
  assert.doesNotMatch(home,/dashboard\.shortcut\.rsi|page:'rsi' as const/);
  assert.doesNotMatch(home,/type Destination=[^\n]*(?:'rsi'|'video')/);
  assert.match(home,/type Destination='chat'/);
  assert.doesNotMatch(chat,/openImprovementIssue|\/api\/rsi\/issues/);
  assert.doesNotMatch(nav,/'video'\|'models'|\|'developer'\|'rsi'/);
  for(const name of [
    'web/src/components/ImprovementNotice.tsx',
    'scripts/sub3-live-model-campaign.mjs',
    '.github/workflows/sub3-live-model-cycle.yml',
    '.github/workflows/sub3-first-gain.yml',
    'tests/fixtures/rsi-generation-worker.rs'
  ])assert.equal(existsSync(new URL('../'+name,import.meta.url)),false,name);
  // Historical job/MP4 access is intentionally preserved.
  assert.match(src('src/api/video.rs'),/pub async fn content/);
  assert.match(src('src/api/rsi.rs'),/Read-only archival RSI records/);
});

test('Home SDK policy is owned by a live Rust job, not a sticky chat session',()=>{
 const kernel=src('src/engine/execution_scope.rs');
 const jobs=src('src/engine/jobs.rs');
 const chat=src('src/engine/chat/runtime.rs');
 const tools=src('src/engine/tools.rs');
 assert.match(jobs,/sdkUiPolicyOwnerId/);
 assert.match(jobs,/sdkUiPolicyBaseline/);
 assert.match(kernel,/job\["status"\] != "running"/);
 assert.match(kernel,/job\["sessionId"\] != session\["id"\]/);
 assert.match(kernel,/usage_since_baseline/);
 assert.match(chat,/execution_scope::active_panel_policy/);
 assert.match(chat,/execution_scope::usage_since_baseline/);
 assert.match(tools,/execution_scope::active_panel_policy/);
});
