import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync,existsSync} from 'node:fs';
const read=p=>readFileSync(new URL('../'+p,import.meta.url),'utf8');
test('runtime is standalone and never imports the planning policies',()=>{
 const manifest=read('crates/dominion-agent-runtime/Cargo.toml');
 assert.doesNotMatch(manifest,/dominion-agent-sdk|dominion-agent-algorithms|rusqlite|reqwest/);
 const src=read('crates/dominion-agent-runtime/src/lib.rs');
 assert.match(src,/pub struct BudgetLedger/);
 assert.match(src,/pub struct Action/);
 assert.match(src,/pub struct ToolSpec/);
 assert.match(src,/pub struct Receipt/);
 assert.match(src,/pub mod scheduler/);
 assert.match(src,/pub mod schema/);
 assert.match(src,/pub struct ModelTokenBudget/);
 assert.doesNotMatch(src,/AgentAlgorithm|StandardAlgorithm|crate::state|crate::db/);
 assert.equal(existsSync(new URL('../crates/dominion-agent-sdk/src/runtime.rs',import.meta.url)),false);
});
test('intelligence owns policy and tool scoring; SDK receives it by injection',()=>{
 const algorithms=read('crates/dominion-agent-algorithms/src/lib.rs');
 const ranking=read('crates/dominion-agent-algorithms/src/ranking.rs');
 const sdk=read('crates/dominion-agent-sdk/src/lib.rs');
 const tools=read('crates/dominion-agent-algorithms/src/tool_discovery.rs');
 assert.match(algorithms,/pub trait AgentAlgorithm/);
 assert.match(algorithms,/impl AgentAlgorithm for StandardAlgorithm/);
 assert.match(ranking,/pub fn tool_relevance_with_schema/);
 assert.match(sdk,/pub async fn run_with_algorithm/);
 assert.match(sdk,/pub async fn resume_with_algorithm/);
 assert.match(sdk,/algorithm\.planner_instructions/);
 assert.match(sdk,/algorithm\.select_tools/);
 assert.match(sdk,/algorithm\.project_context/);
 assert.match(sdk,/algorithm\.failure_guidance/);
 assert.match(sdk,/Algorithm attempted to expose an unauthorized or altered tool/);
 assert.match(sdk,/algorithm\.validate_plan/);
 assert.match(sdk,/StandardAlgorithm\.classify\(&request\.instruction\)/);
 assert.match(sdk,/\.union\(algorithm\.classify\(&request\.instruction\)\)/);
 assert.match(tools,/use crate::ranking::\{tool_relevance,tool_relevance_with_schema\}/);
 assert.doesNotMatch(sdk,/fn words\(s:&str\)/);
 for(const name of ['context','tool_discovery','failure_guidance']){
  assert.equal(existsSync(new URL('../crates/dominion-agent-sdk/src/'+name+'.rs',import.meta.url)),false);
  assert.equal(existsSync(new URL('../crates/dominion-agent-algorithms/src/'+name+'.rs',import.meta.url)),true);
 }
 assert.match(sdk,/use dominion_agent_algorithms::\{context,tool_discovery,failure_guidance\}/);
});
