import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {spawnSync} from 'node:child_process';
import {parseCargoTestArtifacts,parseTestInventory} from './rsi-result-format.mjs';
test('exact libtest discovery tolerates Windows CRLF without changing test identity',()=>{
 const lines=['cases::zero_boundary: test','cases::negative_one: test',
   'cases::positive_one: test','3 tests, 0 benchmarks'];
 for(const delimiter of ['\\n','\\r\\n']){
  const output=lines.join(delimiter).replaceAll('\\r','\r').replaceAll('\\n','\n');
  assert.deepEqual(parseTestInventory(output),lines.slice(0,3).map(x=>x.slice(0,-6)));
 }
 assert.deepEqual(parseTestInventory('cases::zero_boundary: test  \\r\\n'.replaceAll('\\r','\r').replaceAll('\\n','\n')),
  ['cases::zero_boundary']);
});
test('cargo executable artifacts are recovered from Windows and Unix JSON lines',()=>{
 const rows=[
  JSON.stringify({reason:'compiler-artifact',profile:{test:true},executable:'C:\\\\temp\\\\cases.exe'}),
  JSON.stringify({reason:'compiler-artifact',profile:{test:false},executable:'ignored'}),
  JSON.stringify({reason:'compiler-artifact',profile:{test:true},executable:'/tmp/tests'}),
  JSON.stringify({reason:'compiler-artifact',profile:{test:true},executable:'/tmp/tests'}),
 ];
 assert.deepEqual(parseCargoTestArtifacts(rows.join('\r\n')),
  ['C:\\\\temp\\\\cases.exe','/tmp/tests']);
});

const runner=fileURLToPath(new URL('./rsi-lab.mjs',import.meta.url));
test('portable runner applies exact patches in isolated worktrees and cleans up paths with spaces',()=>{
 const root=fs.mkdtempSync(path.join(os.tmpdir(),'rsi portable 한글 '));
 const git=(...args)=>{const p=spawnSync('git',args,{cwd:root,encoding:'utf8'});assert.equal(p.status,0,p.stderr);return p.stdout.trim()};
 try{
  git('init','-q');git('config','core.autocrlf','false');
  fs.writeFileSync(path.join(root,'subject.txt'),'before\n');git('add','.');
  git('-c','user.name=Fixture','-c','user.email=fixture@localhost','commit','-qm','baseline');
  const base=git('rev-parse','HEAD');
  const fixtures=path.join(root,'evaluation files');fs.mkdirSync(fixtures);
  fs.copyFileSync(runner,path.join(fixtures,'rsi-lab.mjs'));
  // A fixture grader inspects actual trees; this is not an RSI performance claim.
  fs.writeFileSync(path.join(fixtures,'rsi-measure.mjs'),`import fs from 'node:fs';import path from 'node:path';fs.writeFileSync(process.env.RSI_REPORT,JSON.stringify({baseline:fs.readFileSync(path.join(process.env.RSI_TEMP,'baseline','subject.txt'),'utf8'),candidate:fs.readFileSync(path.join(process.env.RSI_TEMP,'candidate','subject.txt'),'utf8'),temp:process.env.RSI_TEMP,baselineTreeSha:process.env.RSI_BASE_TREE_SHA,candidateTreeSha:process.env.RSI_CANDIDATE_TREE_SHA}));`);
  fs.writeFileSync(path.join(fixtures,'candidate.patch'),'diff --git a/subject.txt b/subject.txt\n--- a/subject.txt\n+++ b/subject.txt\n@@ -1 +1 @@\n-before\n+after\n');
  fs.writeFileSync(path.join(fixtures,'plan.json'),JSON.stringify({oracleVersion:'portable-test',cases:[{id:'a',domain:'tools',filter:'fixture::a'},{id:'b',domain:'memory',filter:'fixture::b'}]}));
  const report=path.join(fixtures,'report.json');
  const result=spawnSync(process.execPath,[path.join(fixtures,'rsi-lab.mjs'),base,path.join(fixtures,'candidate.patch'),path.join(fixtures,'plan.json'),report],{cwd:root,encoding:'utf8'});
  assert.equal(result.status,0,result.stderr);
  const measured=JSON.parse(fs.readFileSync(report));
  assert.equal(measured.baseline,'before\n');assert.equal(measured.candidate,'after\n');
  assert.equal(measured.baselineTreeSha,git('rev-parse',base+'^{tree}'));
  assert.match(measured.candidateTreeSha,/^[a-f0-9]{40}$/);
  assert.notEqual(measured.candidateTreeSha,measured.baselineTreeSha,
   'candidate code must have a distinct SHA-attested Git tree');
  assert.equal(fs.readFileSync(path.join(root,'subject.txt'),'utf8'),'before\n');
  assert.equal(fs.existsSync(measured.temp),false);assert.equal(git('worktree','list','--porcelain').split('worktree ').length-1,1);
  // A failed candidate patch still removes BOTH disposable worktrees and
  // leaves neither files nor a hidden Git worktree registration.
  fs.writeFileSync(path.join(fixtures,'bad.patch'),
   'diff --git a/subject.txt b/subject.txt\n--- a/subject.txt\n+++ b/subject.txt\n@@ -1 +1 @@\n-NOT_THE_BASE\n+after\n');
  const rejected=spawnSync(process.execPath,[path.join(fixtures,'rsi-lab.mjs'),base,
   path.join(fixtures,'bad.patch'),path.join(fixtures,'plan.json'),report],
   {cwd:root,encoding:'utf8'});
  assert.notEqual(rejected.status,0,'invalid source revision must not be evaluated');
  assert.equal(git('worktree','list','--porcelain').split('worktree ').length-1,1,
   'failure cleanup must not leave registered Git worktrees');
  assert.equal(fs.readFileSync(path.join(root,'subject.txt'),'utf8'),'before\n');
 }finally{fs.rmSync(root,{recursive:true,force:true})}
});

test('real Rust A/B runner compiles both frozen revisions and measures actual behavior',()=>{
 const root=fs.mkdtempSync(path.join(os.tmpdir(),'rsi paired rust '));
 const git=(...args)=>{
  const p=spawnSync('git',args,{cwd:root,encoding:'utf8',timeout:30000});
  assert.equal(p.status,0,p.stderr);return p.stdout.trim()
 };
 try{
  git('init','-q');git('config','core.autocrlf','false');
  fs.mkdirSync(path.join(root,'src'));
  fs.writeFileSync(path.join(root,'Cargo.toml'),
   '[package]\nname = "rsi_pair_real"\nversion = "0.1.0"\nedition = "2021"\n');
  fs.writeFileSync(path.join(root,'src/lib.rs'),
   'pub fn fixed() -> bool { false }\n#[cfg(test)] mod tests {\n #[test] fn regression() { assert!(super::fixed()); }\n #[test] fn guard() { assert_eq!(2 + 2, 4); }\n}\n');
  git('add','.');git('-c','user.name=Fixture','-c','user.email=test@localhost',
   'commit','-qm','immutable baseline');
  const base=git('rev-parse','HEAD');
  const evidence=path.join(root,'paired evidence');fs.mkdirSync(evidence);
  fs.writeFileSync(path.join(evidence,'patch.diff'),
   'diff --git a/src/lib.rs b/src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1,2 +1,2 @@\n-pub fn fixed() -> bool { false }\n+pub fn fixed() -> bool { true }\n #[cfg(test)] mod tests {\n');
  fs.writeFileSync(path.join(evidence,'plan.json'),JSON.stringify({
   oracleVersion:'real-rust-paired-v1',cases:[
    {id:'regression',domain:'tools',holdout:true,filter:'tests::regression'},
    {id:'guard',domain:'memory',holdout:true,filter:'tests::guard'}
   ]}));
  const reportFile=path.join(evidence,'report.json');
  const result=spawnSync(process.execPath,[runner,base,
   path.join(evidence,'patch.diff'),path.join(evidence,'plan.json'),reportFile],
   {cwd:root,encoding:'utf8',timeout:240000,env:{...process.env,
    RSI_BUILD_CACHE:path.join(root,'rsi-build-cache')}});
  assert.equal(result.status,0,result.stderr);
  const report=JSON.parse(fs.readFileSync(reportFile,'utf8'));
  assert.equal(report.runner,'balanced-frozen-rsi-lab-v3');
  assert.equal(report.baselineBuildOk,true);
  assert.equal(report.candidateBuildOk,true);
  assert.equal(report.baselineTreeSha,git('rev-parse',base+'^{tree}'));
  assert.match(report.candidateTreeSha,/^[a-f0-9]{40}$/);
  assert.notEqual(report.baselineTreeSha,report.candidateTreeSha);
  assert.equal(report.cases.length,2);
  assert.equal(report.cases[0].baselinePass,false,
   'unfixed source must fail the actual Rust regression test');
  assert.equal(report.cases[0].candidatePass,true,
   'real patch must improve the same frozen case');
  assert.equal(report.cases[1].baselinePass,true);
  assert.equal(report.cases[1].candidatePass,true);
  assert.equal(fs.readFileSync(path.join(root,'src/lib.rs'),'utf8')
   .includes('fixed() -> bool { false }'),true,
   'both actual compilations must leave the source checkout untouched');
  assert.equal(git('worktree','list','--porcelain').split('worktree ').length-1,1,
   'successful real A/B worktrees must be cleaned from Git registration');
 }finally{fs.rmSync(root,{recursive:true,force:true})}
});
