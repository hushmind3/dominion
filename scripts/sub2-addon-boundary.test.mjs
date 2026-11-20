// Optional integrations must not become independent runtimes or add menus.
import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync,existsSync} from 'node:fs';

const source=path=>readFileSync(new URL('../'+path,import.meta.url),'utf8');
const core=source('src/api/mod.rs');
const extra=source('src/api/addons.rs');
const engine=source('src/engine/mod.rs');
const port=source('src/engine/addons/mod.rs');
const optionalPaths=[
 '/api/tools','/api/browser/status','/api/browser/screenshot','/api/browser/action',
 '/api/gateways/freellmapi','/api/providers/:id/gateway',
 '/api/skills/:id/export','/api/mcp/presets','/api/mcp/catalog',
 '/api/mcp/discover','/api/skill-catalog','/api/skill-catalog/meta',
 '/api/skill-catalog/import','/api/skills/import-repo',
 '/api/oauth/providers','/api/oauth/connections','/api/oauth/providers/:id',
 '/api/oauth/:id/start','/api/oauth/:id/open','/api/oauth/:id/browser-open',
 '/api/oauth/:id/callback','/api/oauth/:id',
 '/api/channels/catalog','/api/channels','/api/channels/:id',
 '/api/channels/:id/diagnose','/api/webhooks/:id'
];
const routes=text=>[...text.matchAll(/\.route\(\s*["'](\/api\/[^"']+)["']/g)].map(x=>x[1]);

test('27 legacy optional API paths stay intact and outside the core router',()=>{
 assert.deepEqual(routes(extra),optionalPaths);
 for(const p of optionalPaths)assert.ok(!routes(core).includes(p),'core leaked '+p);
 assert.match(core,/router=router\.merge\(addons::routes\(\)\)/);
 assert.match(core,/middleware::from_fn_with_state\(state\.clone\(\), guard_writes\)/);
 assert.doesNotMatch(extra,/\.with_state\(|\.layer\(|TcpListener|fallback_service/);
});

test('all seven optional adapters are physically isolated and compatibility-exported',()=>{
 assert.match(engine,/pub mod addons;/);
 for(const name of ['browser','channel_adapter','discord','github_work','google_work','mcp','telegram']){
  assert.ok(existsSync(new URL('../src/engine/addons/'+name+'.rs',import.meta.url)),name);
  assert.ok(!existsSync(new URL('../src/engine/'+name+'.rs',import.meta.url)),'stale '+name);
  assert.ok(port.includes('pub mod '+name+';'),'unregistered '+name);
  assert.ok(!engine.includes('pub mod '+name+';'),'core module '+name);
 }
});

test('native model turn delegates all MCP tool operations through addon port',()=>{
 const turn=source('src/engine/services/conversation/chat/runtime/turn_body.rs');
 const dispatcher=source('src/engine/services/conversation/chat/runtime/tool_calls.rs');
 const discovery=source('src/engine/services/tools/chat_capability.rs');
 const catalog=source('src/engine/services/conversation/chat/runtime/tool_catalog.rs');
 assert.match(turn,/tool_catalog::prepare/);
 assert.match(catalog,/addons::tool_definitions/);
 assert.match(discovery,/addons::tool_definitions/);
 assert.match(dispatcher,/addons::requires_tool_approval/);
 assert.match(dispatcher,/addons::execute_tool/);
 assert.doesNotMatch(dispatcher,/mcp::(?:call|requires_approval)/);
 assert.match(port,/mcp::call/);
});
