// Chromium smoke of the real Vite/React Activity UI at a mobile viewport.
// HTTP responses are mocked; this tests rendering and control wiring, not a
// physical remote node or a model-backed Rust execution.
import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {fileURLToPath} from 'node:url';
import {dirname,resolve} from 'node:path';
import {chromium} from 'playwright';

const root=resolve(dirname(fileURLToPath(import.meta.url)),'..');
const port=5187;
const address='http://127.0.0.1:'+port;
const server=spawn(process.execPath,[resolve(root,'node_modules/vite/bin/vite.js'),
  '--host','127.0.0.1','--port',String(port),'--strictPort'],{
  cwd:root,stdio:['ignore','pipe','pipe']
});
let browser;
const data={
  leadJobs:[{id:'lead-job-fixture',status:'queued',createdAt:'2026-11-21T00:00:00Z',sessionId:'lead-session'}],
  teams:[{id:'managed-team-fixture',status:'running',leadAgentId:'lead',
    leadSessionId:'lead-session',verified:false,resumeAvailable:false,
    pendingRemoteArtifactTaskIds:[],tasks:[
      {id:'research',agentId:'researcher',status:'running',readOnly:true,
        verified:false,remoteWorkerId:'worker-b',workerOnline:false,
        displayState:'offline',dependsOn:[]},
      {id:'write',agentId:'builder',status:'assigned',readOnly:false,
        verified:false,workerOnline:true,displayState:'waiting_predecessors',
        waitingPredecessors:true,dependsOn:['research']},
      {id:'merged',agentId:'reviewer',status:'completed',readOnly:false,
        verified:true,remoteWorkerId:'worker-b',workerOnline:true,
        artifactApprovalState:'approved',artifactApplyStatus:'applied',
        artifactApplied:true,dependsOn:[]}
    ]}],remoteWorkers:[{id:'worker-b',online:false,allowWrites:true,
       agentCount:2,agents:['researcher','reviewer']}],remoteConfigured:true
};
async function ready(){
  for(let i=0;i<60;i++){
    if(server.exitCode!==null)throw Error('Vite exited before Chromium test');
    try{if((await fetch(address)).ok)return;}catch{}
    await new Promise(done=>setTimeout(done,200));
  }
  throw Error('Vite did not start');
}
try{
  await ready();
  browser=await chromium.launch({headless:true});
  const page=await browser.newPage({viewport:{width:390,height:844},locale:'ko-KR'});
  const errors=[];
  page.on('pageerror',error=>errors.push(error.message));
  await page.route('**/api/**',async route=>{
    const path=new URL(route.request().url()).pathname;
    let response=[];
    if(path==='/api/activity/teams')response=data;
    if(path==='/api/status')response={jobs:0};
    if(path==='/api/health')response={branch:'dominion-sub2',commit:'fixture'};
    if(path==='/api/events'){
      await route.fulfill({status:200,contentType:'text/event-stream',body:': smoke\n\n'});
      return;
    }
    await route.fulfill({status:200,contentType:'application/json',body:JSON.stringify(response)});
  });
  await page.goto(address+'/#/activity',{waitUntil:'domcontentloaded'});
  await page.getByRole('heading',{name:'멀티에이전트 팀 작업'}).waitFor({timeout:10000});
  await page.getByText('저장된 Lead 작업',{exact:false}).first()
    .waitFor({state:'visible',timeout:10000});
  await page.reload({waitUntil:'domcontentloaded'});
  await page.getByText('저장된 Lead 작업',{exact:false}).first().waitFor({timeout:10000});
  await page.locator('button[aria-expanded="false"]').filter({hasText:'m-fixture'}).click();
  for(const label of ['Worker 오프라인','선행 작업 대기','파일 변경 승인됨',
    '로컬 적용·해시 검증 완료']){
    assert.ok(await page.getByText(label).first().isVisible(),label+' not visible');
  }
  assert.ok(await page.locator('small').filter({hasText:'로컬 · 이 PC'}).first().isVisible());
  assert.ok(await page.locator('small').filter({hasText:'Writer · 변경 승인 필요'}).first().isVisible());
  assert.equal(await page.getByRole('button',{name:'작업 재개'}).isDisabled(),true);
  assert.equal(await page.getByRole('button',{name:'팀 중단'}).isEnabled(),true);
  const widths=await page.evaluate(()=>({screen:innerWidth,body:document.documentElement.scrollWidth}));
  assert.ok(widths.body<=widths.screen+2,
    'Team UI is horizontally clipped on a mobile viewport: '+JSON.stringify(widths));
  assert.deepEqual(errors,[], 'React/JS runtime errors');
  console.log('PASS Chromium mobile Team status / approval / local vs remote controls');
}finally{
  await browser?.close();
  server.kill('SIGTERM');
}
