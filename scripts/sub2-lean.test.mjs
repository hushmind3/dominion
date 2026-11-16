import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
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
