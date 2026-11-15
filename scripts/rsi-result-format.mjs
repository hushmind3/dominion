// Output normalization for the immutable Rust A/B evaluator.
// Windows libtest may emit CRLF even though Node splits stdout on LF.
// Treat test names and executable paths as opaque; do not normalize the
// names themselves, only the record delimiters.
export function parseCargoTestArtifacts(stdout) {
 const artifacts=new Set();
 for(const line of stdout.split(/\r?\n/)) {
  let record;
  try{record=JSON.parse(line)}catch{continue}
  if(record?.reason==='compiler-artifact'&&record.profile?.test&&
     typeof record.executable==='string'&&record.executable.length>0){
   artifacts.add(record.executable);
  }
 }
 return [...artifacts];
}

export function parseTestInventory(stdout) {
 const names=[];
 for(const raw of stdout.split(/\r?\n/)) {
  const line=raw.trimEnd();
  if(line.endsWith(': test'))names.push(line.slice(0,-6));
 }
 return names;
}

import crypto from 'node:crypto';
import os from 'node:os';
import path from 'node:path';

// Keep native paired Cargo caches separate by experiment arm. On Windows,
// Cargo's build-script intermediate paths exceed legacy toolchain limits when
// the target is nested under the application's deeply named data directory.
// A hashed, short directory under the system temp root preserves per-campaign
// cache identity without making the candidate share baseline build products.
export function pairedBuildTarget(cache,arm,platform=process.platform,tempRoot=os.tmpdir()) {
 if(arm!=='baseline'&&arm!=='candidate')throw Error('Unknown paired arm');
 const root=platform==='win32'
  ? path.join(tempRoot,'dr-'+crypto.createHash('sha256').update(path.resolve(cache)).digest('hex').slice(0,12))
  : cache;
 return path.join(root,arm);
}
