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
const missing=[...mainFiles].filter(path=>relevant(path)&&!subFiles.has(path));
assert.deepEqual(missing,[], 'main source files absent in sub3');

const web=[...mainFiles].filter(path=>path.startsWith('web/')&&relevant(path));
const changed=web.filter(path=>blob(main,path)!==blob('HEAD',path));
assert.deepEqual(changed,[], 'Shipping web source differs from canonical main');

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
  }
  mismatched.push(path+' main='+handler+' sub3='+subRoutes.get(path));
}
assert.deepEqual(unserved,[], 'main API routes missing');
assert.deepEqual(mismatched,[], 'main route handler contract missing (including harness aliases)');
console.log(JSON.stringify({
  mainRef:git('rev-parse',main).trim(),
  matchingWebSourceFiles:web.length,
  mainApiRoutes:mainRoutes.size,
  liveSub3ApiRoutes:subRoutes.size,
  mainFileParity:true,
  mainHandlerParity:true,
  nativeSdkGuiPreserved:true,
},null,2));
