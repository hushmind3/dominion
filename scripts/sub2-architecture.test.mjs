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
   'src/engine/evaluation.rs','src/engine/resource_lock.rs',
   'src/engine/video_quality.rs'];
 for(const path of files)assert.ok(file(path).length>400,path);
 assert.ok(file('src/api/mod.rs').includes('/api/evaluation/summary'));
 assert.ok(file('src/api/system.rs').includes('engine::evaluation::summary(&state)'));
 assert.ok(file('sdk/typescript/src/index.ts').includes('evaluation = {'));
 assert.ok(file('web/src/pages/Operations.tsx').includes('/api/evaluation/summary'));
 assert.ok(file('src/engine/tools.rs').includes('resource_lock::acquire'));
 assert.ok(file('src/engine/video.rs').includes('video_quality::inspect'));
});
