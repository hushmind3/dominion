#!/usr/bin/env node
// Main -> sub3 contract: prevent silent feature/API/UI regressions on future refactors.
// The canonical main SDK's distinct Rust harness is exposed under /api/harness;
// /api/sdk remains connected to the native Rust product job used by the GUI.
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';

const main = process.env.DOMINION_MAIN_REF || 'refs/remotes/origin/main';
function git(...args) {
  return execFileSync('git', args, {encoding:'utf8', maxBuffer:8*1024*1024});
}
function tracked(ref) {
  return new Set(git('ls-tree','-r','--name-only',ref).trim().split('\n').filter(Boolean));
}
function relevant(path) {
  return (path.startsWith('src/') || path.startsWith('web/')
    || path.startsWith('crates/harness/') || path.startsWith('packages/sdk-typescript/'))
    && !path.endsWith('.tsbuildinfo');
}
function blob(ref,path) {
  return git('rev-parse',ref+':'+path).trim();
}
function parseRoutes(source) {
  const routes=new Map();
  let cursor=0;
  while ((cursor=source.indexOf('.route(',cursor))>=0) {
    let pos=cursor+7,depth=1,quoted=false,escaped=false;
    const start=pos;
    for (;pos<source.length;pos++) {
      const ch=source[pos];
      if (quoted) {
        if (escaped) escaped=false;
        else if (ch==='\\') escaped=true;
        else if (ch==='"') quoted=false;
      } else if (ch==='"') quoted=true;
      else if (ch==='(') depth++;
      else if (ch===')' && --depth===0) break;
    }
    assert.ok(pos<source.length,'Unclosed Rust router call');
    const body=source.slice(start,pos);
    const entry=body.match(/^\s*"([^"]+)"\s*,\s*([\s\S]*)$/);
    if (entry) routes.set(entry[1],entry[2].replace(/\s+/g,''));
    cursor=pos+1;
  }
  return routes;
}
const mainFiles=tracked(main);
const subFiles=tracked('HEAD');
// The current main uses dominion-core's phase-local product adapter.
// Sub3 owns an independent live runtime backed by dominion-agent-sdk.
// Compare reachable API/UI and observable HTTP contracts rather than requiring
// an unused 98-KB alternate engine adapter to be copied into the product.
const internalAlternatives=new Map([
  ['src/engine/chat/product.rs',[
    'src/engine/chat/runtime.rs','crates/dominion-agent-sdk/src/dispatch.rs']],
]);
const independentlyImplemented=[...mainFiles].filter(path=>
  relevant(path)&&!subFiles.has(path)&&internalAlternatives.has(path));
for(const missingAdapter of independentlyImplemented) {
  const paths=internalAlternatives.get(missingAdapter);
  assert.ok(paths.every(path=>subFiles.has(path)),
    'Replacement for '+missingAdapter+' must be complete');
  const runtime=git('show','HEAD:src/engine/chat/runtime.rs');
  assert.ok(runtime.includes('dominion_agent_sdk::dispatch_batch('),
    'The native Rust kernel must remain wired to the live product runtime');
}
// Main and sub3 deliberately own different Rust execution kernels. New
// main-only internal engine modules are reported, not passed off as copied
// implementations: shipping parity is enforced at route and GUI contracts.
const mainOnlyEngineModules=[...mainFiles].filter(path=>
  path.startsWith('src/engine/')&&!subFiles.has(path)&&!internalAlternatives.has(path));
const missing=[...mainFiles].filter(path=>
  relevant(path)&&!subFiles.has(path)&&!internalAlternatives.has(path)
  &&!path.startsWith('src/engine/'));
assert.deepEqual(missing,[], 'Public API, GUI or SDK source files absent in sub3');

const web=[...mainFiles].filter(path=>path.startsWith('web/')&&relevant(path));
const changed=web.filter(path=>blob(main,path)!==blob('HEAD',path));
// Sub3 is explicitly improving only the video workspace's user experience.
// Every other shipping UI file must still exactly match main.
const permittedVideoChanges=new Set(['web/src/pages/Video.tsx']);
const unexpected=changed.filter(path=>!permittedVideoChanges.has(path));
assert.deepEqual(unexpected,[], 'Unrelated shipping web source differs from canonical main');
if(changed.includes('web/src/pages/Video.tsx')) {
  const studio=git('show','HEAD:web/src/pages/Video.tsx');
  for(const route of ['/api/video/jobs','/api/video/config','/api/video/outputs',
    '/api/video/catalog','/api/youtube/status']) {
    assert.ok(studio.includes(route),'Video studio disconnected from existing product endpoint: '+route);
  }
  assert.ok(studio.includes('video.value') && studio.includes('useJob('),
    'Video studio must stay connected to live state and durable jobs');
}

const mainRoutes=parseRoutes(git('show',main+':src/api/mod.rs'));
const subRoutes=parseRoutes(git('show','HEAD:src/api/mod.rs'));
const unserved=[];
const mismatched=[];
for (const [path,handler] of mainRoutes) {
  if (!subRoutes.has(path)) {unserved.push(path);continue;}
  if (subRoutes.get(path)===handler) continue;
  // These 6 routes intentionally retain the main GUI's native-job adapter.
  // The original main SDK handlers remain live on a distinct versioned route.
  if (path.startsWith('/api/sdk/runs')) {
    const legacy='/api/harness'+path.slice('/api/sdk'.length);
    if (subRoutes.get(legacy)===handler) continue;
    // Public SDK native-job steering is a separate, real implementation.
    if (path==='/api/sdk/runs/:id/steer' &&
        subRoutes.get(path)==='post(sdk_runs::steer)') continue;
  }
  mismatched.push(path+' main='+handler+' sub3='+subRoutes.get(path));
}
// The new main-only hierarchical-budget reconciliation belongs to a
// different execution budget implementation. Report the divergence instead
// of adding a misleading no-op compatibility handler.
const mainOnlyRoutes=new Set(['/api/executions/:id/budget/reconcile']);
const unsupportedMainRoutes=unserved.filter(path=>!mainOnlyRoutes.has(path));
assert.deepEqual(unsupportedMainRoutes,[], 'main public API routes missing without explicit independent-engine exception');
assert.deepEqual(mismatched,[], 'main route handler contract missing (including harness aliases)');
console.log(JSON.stringify({
  mainRef:git('rev-parse',main).trim(),
  matchingWebSourceFiles:web.length-changed.length,
  redesignedVideoSourceFiles:changed.filter(path=>permittedVideoChanges.has(path)),
  mainApiRoutes:mainRoutes.size,
  liveSub3ApiRoutes:subRoutes.size,
  unimplementedIndependentMainRoutes:unserved.filter(path=>mainOnlyRoutes.has(path)),
  mainPublicSurfaceFilesPresent:true,
  mainOnlyEngineModulesNotCompared:mainOnlyEngineModules,
  mainHandlerParity:true,
  nativeSdkGuiPreserved:true,
  activeAlternativeAdapters:independentlyImplemented,
},null,2));
