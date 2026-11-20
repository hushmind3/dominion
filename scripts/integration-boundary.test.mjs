// Native source boundary regression: no external adapter implementation in engine/.
import test from 'node:test';
import assert from 'node:assert/strict';
import {existsSync,readFileSync,readdirSync} from 'node:fs';
import {join,resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
const root=resolve(fileURLToPath(new URL('..',import.meta.url)));
const source=(p)=>readFileSync(join(root,p),'utf8');
const moved=[
 ['mcp.rs','mcp.rs'],
 ['channel_adapter.rs','channels/adapter.rs'],
 ['bridge.rs','channels/bridge.rs'],
 ['discord.rs','channels/discord.rs'],
 ['telegram.rs','channels/telegram.rs'],
 ['oauth.rs','accounts/oauth.rs'],
 ['github_files.rs','accounts/github_files.rs'],
 ['github_workflow.rs','accounts/github_workflow.rs'],
 ['google_workspace.rs','accounts/google_workspace.rs'],
 ['browser.rs','browser.rs'],
 ['skills.rs','skills/catalog.rs'],
 ['skill_discovery.rs','skills/discovery.rs'],
];
function rustFiles(dir){return readdirSync(join(root,dir),{withFileTypes:true}).flatMap(entry=>{
 const path=join(dir,entry.name);
 return entry.isDirectory()?rustFiles(path):entry.isFile()&&entry.name.endsWith('.rs')?[path]:[];
});}
test('external MCP, channel and account implementations live only in integrations',()=>{
 for(const [oldPath,newPath] of moved){
  assert.equal(existsSync(join(root,'src/engine',oldPath)),false,'engine retained '+oldPath);
  assert.equal(existsSync(join(root,'src/integrations',newPath)),true,'missing '+newPath);
 }
 const engine=source('src/engine/mod.rs');
 assert.doesNotMatch(engine,/pub (?:mod|use) (?:crate::integrations::)?(?:mcp|discord|telegram|channel_adapter|bridge|oauth|github_files|google_workspace|github_workflow)\b/);
 assert.match(source('src/main.rs'),/mod integrations;/);
});
test('connector ownership stays out of agent worker and host tool catalog',()=>{
 const state=source('src/state.rs');
 assert.match(state,/pub integrations: crate::integrations::IntegrationState/);
 assert.doesNotMatch(state,/pub\(crate\) mcp_sessions:/);
 assert.match(source('src/bootstrap.rs'),/integrations::start\(state.clone\(\)\)/);
 assert.doesNotMatch(source('src/engine/jobs.rs'),/channel_adapter::start/);
 const tools=source('src/integrations/local_tools.rs');
 assert.doesNotMatch(tools,/tool\("(?:github_|google_workspace|account_status)/);
 assert.doesNotMatch(tools,/async fn github_api/);
 assert.match(tools,/integrations::registry::execute/);
 assert.match(source('src/integrations/registry.rs'),/pub async fn execute/);
 assert.match(source('src/integrations/accounts/tools.rs'),/pub fn definitions\(\)/);
});
test('old engine connector paths cannot leak back into API or core source',()=>{
 const legacy=/\b(?:crate::)?engine::(?:mcp|bridge|channel_adapter|discord|telegram|oauth|github_files|github_workflow|google_workspace|browser|skills|skill_discovery)\b/;
 for(const file of rustFiles('src')){
  assert.doesNotMatch(source(file),legacy,'legacy connector import at '+file);
 }
});
