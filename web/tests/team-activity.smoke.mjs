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
    if(path==='/api/agents')response=[
      {id:'lead',name:'Lead',providerId:'fixture',model:'fixture',enabled:true},
      {id:'researcher',name:'Researcher',providerId:'fixture',model:'fixture',enabled:true},
      {id:'builder',name:'Builder',providerId:'fixture',model:'fixture',enabled:true}
    ];
    if(path==='/api/providers')response=[
      {id:'fixture',name:'Fixture',model:'fixture',enabled:true,kind:'openai'}
    ];
    if(path==='/api/sessions')response=[
      {id:'chat-one',title:'Conversation One',agentId:'lead',providerId:'fixture',model:'fixture'},
      {id:'chat-two',title:'Conversation Two',agentId:'lead',providerId:'fixture',model:'fixture'}
    ];
    if(path.endsWith('/messages'))response=[];
    if(path.endsWith('/usage'))response={totalTokens:0};
    if(path==='/api/model-library/runtime')response={status:'idle'};
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
  // A draft survives navigation and reload without submitting a job.
  await page.getByRole('button',{name:'팀 작업 시작',exact:true}).click();
  const editor=page.getByPlaceholder('목표, 필요한 산출물, 성공 조건을 입력하세요.');
  await editor.fill('동일한 기준으로 리포트를 작성하고 검증해 주세요');
  await page.getByRole('checkbox').first().check();
  await page.getByRole('button',{name:'팀 초안 이어쓰기',exact:true}).click();
  await page.reload({waitUntil:'domcontentloaded'});
  await page.getByRole('button',{name:'팀 초안 이어쓰기',exact:true}).waitFor({timeout:10000});
  await page.getByRole('button',{name:'팀 초안 이어쓰기',exact:true}).click();
  assert.equal(await editor.inputValue(),'동일한 기준으로 리포트를 작성하고 검증해 주세요');
  assert.equal(await page.getByRole('checkbox').first().isChecked(),true);
  // A previously valid Worker may have been deleted or disabled.
  await page.evaluate(()=>sessionStorage.setItem('dominion.team.launch.draft.v1',
    JSON.stringify({open:true,prompt:'review stale',leadId:'lead',
      workerIds:['deleted-agent'],remoteId:''})));
  await page.reload({waitUntil:'domcontentloaded'});
  await page.getByText('현재 사용할 수 없는 Worker:',{exact:false}).waitFor({timeout:10000});
  assert.equal(await page.getByRole('button',{name:'Lead 작업 시작'}).isDisabled(),true);
  await page.getByRole('button',{name:'사용 불가 Worker 제외'}).click();
  assert.equal(await editor.inputValue(),'review stale','removing a stale Worker must preserve the prompt');
  await page.getByRole('checkbox').first().check();
  assert.equal(await page.getByRole('button',{name:'Lead 작업 시작'}).isEnabled(),true);
  // A removed Lead or disconnected remote route must never silently fall back
  // to another model or local execution after the draft is reloaded.
  await page.evaluate(()=>sessionStorage.setItem('dominion.team.launch.draft.v1',
    JSON.stringify({open:true,prompt:'review target',leadId:'deleted-lead',
      workerIds:['lead'],remoteId:'worker-b'})));
  await page.reload({waitUntil:'domcontentloaded'});
  await page.getByText('이전에 선택한 Lead를 사용할 수 없습니다.',{exact:false})
    .waitFor({timeout:10000});
  assert.equal(await page.getByRole('button',{name:'Lead 작업 시작'}).isDisabled(),true);
  assert.ok(await page.getByText('선택한 원격 장치를 사용할 수 없습니다.',{exact:false}).isVisible());
  await page.getByRole('button',{name:'로컬 실행으로 변경'}).click();
  assert.equal(await page.getByRole('button',{name:'Lead 작업 시작'}).isDisabled(),true);
  await page.getByRole('button',{name:'초안 지우기'}).click();
  await page.reload({waitUntil:'domcontentloaded'});
  await page.getByRole('button',{name:'팀 작업 시작',exact:true}).click();
  assert.equal(await editor.inputValue(),'');
  assert.equal(await page.getByRole('checkbox').first().isChecked(),false);
  const widths=await page.evaluate(()=>({screen:innerWidth,body:document.documentElement.scrollWidth}));
  assert.ok(widths.body<=widths.screen+2,
    'Team UI is horizontally clipped on a mobile viewport: '+JSON.stringify(widths));
  assert.deepEqual(errors,[], 'React/JS runtime errors');
  // Chat drafts are scoped to the actual session, including fresh chats.
  const inputSelector='textarea[placeholder^="무엇을 도와드릴까요?"]';
  await page.goto(address+'/#/chat?session=chat-one',{waitUntil:'domcontentloaded'});
  const chatInput=page.locator(inputSelector);
  await chatInput.waitFor({timeout:10000});
  await chatInput.fill('첫 번째 대화에서만 보낼 미완성 지시');
  await page.evaluate(()=>{window.location.hash='#/chat?session=chat-two'});
  await page.waitForFunction(selector=>document.querySelector(selector)?.value==='',
    inputSelector,{timeout:10000});
  await chatInput.fill('두 번째 대화의 별도 초안');
  await page.reload({waitUntil:'domcontentloaded'});
  await page.waitForFunction(selector=>
      document.querySelector(selector)?.value==='두 번째 대화의 별도 초안',
    inputSelector,{timeout:10000});
  await page.evaluate(()=>{window.location.hash='#/chat?session=chat-one'});
  await page.waitForFunction(selector=>
      document.querySelector(selector)?.value==='첫 번째 대화에서만 보낼 미완성 지시',
    inputSelector,{timeout:10000});
  await page.evaluate(()=>{window.location.hash='#/chat'});
  await page.waitForFunction(selector=>document.querySelector(selector)?.value==='',
    inputSelector,{timeout:10000});
  await chatInput.fill('아직 세션을 만들지 않은 새 대화 초안');
  await page.evaluate(()=>{window.location.hash='#/chat?session=chat-two'});
  await page.waitForFunction(selector=>
      document.querySelector(selector)?.value==='두 번째 대화의 별도 초안',
    inputSelector,{timeout:10000});
  await page.evaluate(()=>{window.location.hash='#/chat'});
  await page.waitForFunction(selector=>
      document.querySelector(selector)?.value==='아직 세션을 만들지 않은 새 대화 초안',
    inputSelector,{timeout:10000});
  assert.deepEqual(errors,[], 'Chat/Team React runtime errors');
  console.log('PASS Chromium Team draft / stale route and per-session Chat draft recovery');
}finally{
  await browser?.close();
  server.kill('SIGTERM');
}
