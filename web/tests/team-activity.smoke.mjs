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
  let cancelRequestPayload, cancelStatusReads=0, slowChatRelease;
  const deletedChatIds=new Set();
  page.on('pageerror',error=>errors.push(error.message));
  await page.route('**/api/**',async route=>{
    const path=new URL(route.request().url()).pathname;
    let response=[];
    if(path==='/api/activity/teams')response=data;
    if(path==='/api/status')response={jobs:0};
    if(path==='/api/agents')response=[
      {id:'lead',name:'Lead',providerId:'fixture',model:'fixture',enabled:true},
      {id:'researcher',name:'Researcher',providerId:'fixture',model:'fixture',enabled:true},
      {id:'builder',name:'Builder',providerId:'fixture',model:'fixture',enabled:true},
      {id:'separate',name:'Separate',providerId:'agent-route',model:'agent-model',enabled:true}
    ];
    if(path==='/api/providers')response=[
      {id:'fixture',name:'Fixture',model:'fixture',enabled:true,kind:'openai'},
      {id:'agent-route',name:'Agent provider',model:'agent-model',enabled:true,kind:'openai'}
    ];
    if(path==='/api/sessions')response=[
      {id:'chat-one',title:'Conversation One',agentId:'lead',providerId:'fixture',model:'fixture'},
      {id:'chat-two',title:'Conversation Two',agentId:'lead',providerId:'fixture',model:'fixture'},
      {id:'chat-route',title:'Independent route',agentId:'separate',providerId:'fixture',model:'fixture'},
      {id:'chat-persist',title:'Agent mode persisted server-side',agentId:'separate',providerId:'fixture',model:'fixture',conversationMode:'agent'}
    ].filter(session=>!deletedChatIds.has(session.id));
    if(path.startsWith('/api/sessions/')&&route.request().method()==='DELETE'){
      deletedChatIds.add(decodeURIComponent(path.split('/').at(-1)));
      response={ok:true};
    }
    if(path.endsWith('/messages'))response=path==='/api/sessions/chat-one/messages'?[
      {id:'owner-layout',session_id:'chat-one',role:'user',
       content:'내 질문은 오른쪽 말풍선에 표시되어야 합니다.',
       metadata:{attachments:[{id:'fixture-image',name:'photo.png',isImage:true}]},
       created_at:'2026-11-22 00:00:00'},
      {id:'assistant-layout',session_id:'chat-one',role:'assistant',
       content:'답변은 왼쪽에서 편하게 읽을 수 있어야 합니다.',
       agent_id:'lead',created_at:'2026-11-22 00:00:01'},
      {id:'assistant-markdown',session_id:'chat-one',role:'assistant',
       content:'## 결과 제목\n\n- 첫 번째 항목\n- 두 번째 항목\n\n```tsx\nconst count = 2\n```\n\n| 구분 | 결과 |\n| --- | --- |\n| A | B |\n\n[참조](https://example.com) <script>alert(1)</script>',
       agent_id:'lead',
       metadata:{sources:[
        {title:'2026 기술 자료',url:'https://example.com/report',published:'Sun, 22 Nov 2026 10:00:00 GMT'},
        {title:'Dangerous',url:'javascript:alert(1)'}]},
       created_at:'2026-11-22 00:00:02'}
    ]:[];
    if(path.endsWith('/usage'))response={totalTokens:0};
    if(path==='/api/model-library/runtime')response={status:'idle'};
    if(path==='/api/health')response={branch:'branch2',commit:'fixture'};
    if(path==='/api/events'){
      await route.fulfill({status:200,contentType:'text/event-stream',body:': smoke\n\n'});
      return;
    }
    if(path==='/api/attachments'){
      response={id:'attachment-fixture',name:'note.txt',size:15,supportedForModel:'text'};
    }
    if(path==='/api/sessions/chat-two/cancel'){
      cancelRequestPayload=route.request().postDataJSON();
      response={cancelled:true,requestRecorded:true,running:true};
    }
    if(path==='/api/sessions/chat-two/cancel-status'){
      cancelStatusReads++;
      response={stopped:cancelStatusReads>=2,
        active:cancelStatusReads<2,running:cancelStatusReads<2};
    }
    if(path==='/api/attachments/fixture-image/content'){
      await route.fulfill({status:200,contentType:'image/png',
       body:Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+j45QAAAAASUVORK5CYII=','base64')})
      return
    }
    if(path==='/api/chat'){
      if(route.request().postDataJSON()?.input==='중단 응답 확인'){
        // Keep the provider response pending while Stop must settle via the
        // separate durable cancel-status endpoint.
        await new Promise(resolve=>{slowChatRelease=resolve});
        try{await route.fulfill({status:200,contentType:'text/event-stream',
          body:'data: {"type":"done"}\n\n'});}catch{/* Request was aborted. */}
        return;
      }
      if(route.request().postDataJSON()?.input==='실패한 요청 복구'){
        await route.fulfill({status:503,contentType:'application/json',
         body:JSON.stringify({error:'provider temporarily unavailable'})});
      }else{
        await route.fulfill({status:200,contentType:'text/event-stream',
         body:'data: {"type":"done"}\n\n'});
      }
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
  const inputSelector='textarea.conversation-composer-input';
  await page.goto(address+'/#/chat?session=chat-one',{waitUntil:'domcontentloaded'});
  const chatInput=page.locator(inputSelector);
  await chatInput.waitFor({timeout:10000});
  await page.locator('[data-message][data-role="user"]').waitFor({timeout:10000});
  const chatLayout=await page.locator('[data-message]').evaluateAll(rows=>rows.map(row=>{
    const body=row.querySelector('.chat-turn-body')?.getBoundingClientRect();
    const text=row.querySelector('.chat-turn-text');
    return {role:row.getAttribute('data-role'),left:body?.left||0,
      right:body?.right||0,width:body?.width||0,
      fontSize:parseFloat(text?getComputedStyle(text).fontSize:'0')};
  }));
  const owner=chatLayout.find(item=>item.role==='user');
  const assistant=chatLayout.find(item=>item.role==='assistant');
  assert.ok(owner&&assistant,JSON.stringify(chatLayout));
  assert.ok(owner.left>assistant.left+40,
    'owner message must be on the right of assistant: '+JSON.stringify(chatLayout));
  assert.ok(owner.right<=390&&owner.width<390,
    'owner bubble must not overflow mobile: '+JSON.stringify(chatLayout));
  assert.ok(assistant.fontSize>=15&&owner.fontSize>=15,
    'chat content must be comfortably readable: '+JSON.stringify(chatLayout));
  const markdownTurn=page.locator('[data-message]').filter({hasText:'결과 제목'});
  assert.equal(await markdownTurn.locator('.md-message h2').count(),1);
  assert.equal(await markdownTurn.locator('.md-message li').count(),2);
  assert.equal(await markdownTurn.locator('.md-message pre code').innerText(),'const count = 2');
  assert.equal(await markdownTurn.locator('.md-message table tbody td').count(),2);
  assert.equal(await markdownTurn.locator('.md-message a[href="https://example.com"]').count(),1);
  assert.equal(await markdownTurn.locator('script').count(),0,
    'model text must never mount HTML scripts');
  await markdownTurn.getByText('웹 검색 결과 1개').click();
  assert.equal(await markdownTurn.locator('.chat-web-sources a').count(),1);
  assert.equal(await markdownTurn.locator('.chat-web-sources a').first().getAttribute('href'),
    'https://example.com/report');
  assert.equal(await markdownTurn.getByText('Sun, 22 Nov 2026 10:00:00 GMT').count(),1);
  // Saved files use the same credentialed request as Chat, even if the
  // server requires the DOMINION token rather than browser cookies.
  await page.evaluate(async()=>{(await import('/src/api.ts')).setAuthToken('attachment-test-token')})
  const imageRequest=page.waitForRequest(request=>
    new URL(request.url()).pathname==='/api/attachments/fixture-image/content')
  await page.getByRole('button',{name:'photo.png 첨부 열기'}).click()
  const imageFetch=await imageRequest
  assert.equal(imageFetch.headers()['x-dominion-token'],'attachment-test-token')
  await page.getByRole('dialog',{name:'첨부 이미지 미리보기'}).waitFor({timeout:10000})
  await page.getByRole('button',{name:'미리보기 닫기'}).click()
  await page.evaluate(async()=>{(await import('/src/api.ts')).setAuthToken('')})
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
  // Direct chat is the default, and must never inherit agent persona,
  // tools, execution, or learning from the workspace's default agent.
  await page.evaluate(()=>{window.location.hash='#/chat?session=chat-one'});
  await page.getByRole('button',{name:'일반 대화'}).waitFor({timeout:10000});
  assert.equal(await page.getByRole('button',{name:'일반 대화'}).getAttribute('aria-pressed'),'true');
  assert.equal(await page.locator('select[aria-label="에이전트"]').count(),0);
  await chatInput.fill('모델과 일반 대화');
  const directRequestPromise=page.waitForRequest(request=>new URL(request.url()).pathname==='/api/chat');
  await chatInput.press('Enter');
  const directRequest=(await directRequestPromise).postDataJSON();
  assert.equal(directRequest.input,'모델과 일반 대화');
  assert.ok(!directRequest.agentId,'direct chat must omit agent identity');
  assert.equal(directRequest.toolsEnabled,false);
  assert.equal(directRequest.toolsExecutionEnabled,false);
  assert.equal(directRequest.autoLearning,false);
  assert.equal(directRequest.webSearch,false);
  assert.equal(directRequest.providerId,'fixture');
  assert.equal(directRequest.model,'fixture');
  // Browser cannot grant arbitrary native tools, even when web lookup is on.
  await page.getByRole('button',{name:'웹 검색'}).click();
  assert.equal(await page.getByRole('button',{name:'웹 검색'}).getAttribute('aria-pressed'),'true');
  await chatInput.fill('웹 검색으로 최신 기술 뉴스를 찾아줘');
  const webRequestPromise=page.waitForRequest(request=>new URL(request.url()).pathname==='/api/chat');
  await chatInput.press('Enter');
  const webRequest=(await webRequestPromise).postDataJSON();
  assert.equal(webRequest.webSearch,true);
  assert.equal(webRequest.conversationMode,'chat');
  assert.equal(webRequest.toolsEnabled,false);
  assert.equal(webRequest.toolsExecutionEnabled,false);
  assert.equal(webRequest.agentId,undefined);
  await page.getByRole('button',{name:'에이전트 실행'}).click();
  assert.equal(await page.getByRole('button',{name:'에이전트 실행'}).getAttribute('aria-pressed'),'true');
  await chatInput.fill('에이전트 작업 요청');
  const agentRequestPromise=page.waitForRequest(request=>new URL(request.url()).pathname==='/api/chat');
  await chatInput.press('Enter');
  const agentRequest=(await agentRequestPromise).postDataJSON();
  assert.equal(agentRequest.input,'에이전트 작업 요청');
  assert.equal(agentRequest.agentId,'lead');
  assert.equal(agentRequest.toolsEnabled,true);
  assert.equal(agentRequest.toolsExecutionEnabled,true);
  assert.equal(agentRequest.webSearch,false);
  await page.reload({waitUntil:'domcontentloaded'});
  assert.equal(await page.getByRole('button',{name:'에이전트 실행'}).getAttribute('aria-pressed'),'true',
    'explicit agent mode should survive reload for the same session');
  await page.evaluate(()=>{window.location.hash='#/chat?session=chat-two'});
  assert.equal(await page.getByRole('button',{name:'일반 대화'}).getAttribute('aria-pressed'),'true',
    'unrelated conversation must not inherit another session agent mode');
  // A failed provider request must restore the user's unsent request in
  // the exact session, not discard it or transplant it to another session.
  await chatInput.fill('실패한 요청 복구');
  const failureResponse=page.waitForResponse(response=>
    new URL(response.url()).pathname==='/api/chat'&&response.status()===503);
  await chatInput.press('Enter');
  await failureResponse;
  await page.waitForFunction(selector=>
    document.querySelector(selector)?.value==='실패한 요청 복구',inputSelector,{timeout:10000});
  await page.reload({waitUntil:'domcontentloaded'});
  await page.waitForFunction(selector=>
    document.querySelector(selector)?.value==='실패한 요청 복구',inputSelector,{timeout:10000});
  // Switching an agent-backed session to ordinary chat must send the
  // persisted provider/model, never the hidden agent's different route.
  await page.evaluate(()=>{window.location.hash='#/chat?session=chat-route'});
  assert.equal(await page.getByRole('button',{name:'일반 대화'}).getAttribute('aria-pressed'),'true');
  await chatInput.fill('일반 대화의 독립적인 모델 경로');
  const directRoutePromise=page.waitForRequest(request=>
    new URL(request.url()).pathname==='/api/chat');
  await chatInput.press('Enter');
  const directRoute=(await directRoutePromise).postDataJSON();
  assert.equal(directRoute.conversationMode,'chat');
  assert.equal(directRoute.providerId,'fixture');
  assert.equal(directRoute.model,'fixture');
  assert.equal(directRoute.agentId,undefined);
  await page.getByRole('button',{name:'에이전트 실행'}).click();
  await chatInput.fill('에이전트 모델 경로');
  const agentRoutePromise=page.waitForRequest(request=>
    new URL(request.url()).pathname==='/api/chat');
  await chatInput.press('Enter');
  const agentRoute=(await agentRoutePromise).postDataJSON();
  assert.equal(agentRoute.conversationMode,'agent');
  assert.equal(agentRoute.agentId,'separate');
  assert.equal(agentRoute.providerId,undefined);
  assert.equal(agentRoute.model,undefined);
  await page.evaluate(()=>{window.location.hash='#/chat?session=chat-two'});
  await page.locator('input[aria-label="첨부 파일 선택"]').setInputFiles({
    name:'note.txt',mimeType:'text/plain',buffer:Buffer.from('hello from chat')
  });
  await page.getByText('note.txt',{exact:true}).waitFor({timeout:10000});
  await chatInput.fill('첨부 파일을 읽어줘');
  const uploadSeen=page.waitForRequest(request=>
    new URL(request.url()).pathname==='/api/attachments');
  const attachedChatSeen=page.waitForRequest(request=>
    new URL(request.url()).pathname==='/api/chat');
  await chatInput.press('Enter');
  const uploadBody=(await uploadSeen).postDataJSON();
  assert.equal(uploadBody.sessionId,'chat-two');
  assert.equal(uploadBody.name,'note.txt');
  assert.equal(uploadBody.data,Buffer.from('hello from chat').toString('base64'));
  const attachedChat=(await attachedChatSeen).postDataJSON();
  assert.deepEqual(attachedChat.attachmentIds,['attachment-fixture']);
  assert.equal(attachedChat.input,'첨부 파일을 읽어줘');
  // Cross-tab persisted mode fallback must not rely on sessionStorage.
  await page.evaluate(()=>{
    sessionStorage.removeItem('dominion.chat.execution-mode.v1:chat-persist')
    window.location.hash='#/chat?session=chat-persist'
  });
  await page.getByRole('button',{name:'에이전트 실행'}).waitFor({timeout:10000});
  await page.waitForFunction(()=>document.querySelector('button[aria-pressed="true"]')?.textContent?.includes('에이전트 실행'),null,{timeout:10000});
  await page.reload({waitUntil:'domcontentloaded'});
  await page.waitForFunction(()=>{
    const button=Array.from(document.querySelectorAll('button'))
      .find(item=>item.textContent?.includes('에이전트 실행'));
    return button?.getAttribute('aria-pressed')==='true';
  },null,{timeout:10000});
  assert.equal(await page.getByRole('button',{name:'에이전트 실행'}).getAttribute('aria-pressed'),'true');
  // Clipboard images/text files should become staged attachments without
  // accidentally submitting a message or replacing typed text.
  await chatInput.fill('대화 초안 유지')
  await chatInput.evaluate(input=>{
    const dt=new DataTransfer()
    dt.items.add(new File(['pasted-note'],'pasted.txt',{type:'text/plain'}))
    input.dispatchEvent(new ClipboardEvent('paste',{
      bubbles:true,cancelable:true,clipboardData:dt
    }))
  })
  await page.getByText('pasted.txt',{exact:true}).waitFor({timeout:10000})
  assert.equal(await chatInput.inputValue(),'대화 초안 유지')
  await page.getByRole('button',{name:'pasted.txt 첨부 취소'}).click()
  await page.locator('form.conversation-composer').evaluate(form=>{
    const dt=new DataTransfer()
    dt.items.add(new File(['dropped-note'],'dropped.txt',{type:'text/plain'}))
    form.dispatchEvent(new DragEvent('drop',{bubbles:true,cancelable:true,dataTransfer:dt}))
  })
  await page.getByText('dropped.txt',{exact:true}).waitFor({timeout:10000})
  assert.equal(await chatInput.inputValue(),'대화 초안 유지')
  await page.getByRole('button',{name:'dropped.txt 첨부 취소'}).click()
  await page.getByRole('button',{name:'대화 목록 펼치기'}).click();
  const historySearch=page.getByRole('searchbox',{name:'대화 목록 검색'});
  await historySearch.fill('independent');
  assert.equal(await page.locator('aside').getByRole('button',{name:'Independent route',exact:true}).count(),1);
  assert.equal(await page.locator('aside').getByRole('button',{name:'Conversation One',exact:true}).count(),0);
  await historySearch.fill('없는 대화 이름');
  await page.getByText('일치하는 대화가 없습니다.').waitFor({timeout:10000});
  await page.getByRole('button',{name:'검색 지우기'}).click();
  assert.equal(await page.locator('aside').getByRole('button',{name:'Conversation One',exact:true}).count(),1);
  // The backdrop sits beneath the mobile sidebar; tap outside its 232px panel.
  await page.mouse.click(380,420);
  await page.getByRole('button',{name:'대화 목록 펼치기'}).waitFor({timeout:10000});
  // Stop acknowledgement has no "stopped" field. Only the separate durable
  // cancel-status endpoint can confirm release of the pending provider turn.
  await page.evaluate(()=>{window.location.hash='#/chat?session=chat-two'});
  await chatInput.fill('중단 응답 확인');
  const pendingChat=page.waitForRequest(request=>
    new URL(request.url()).pathname==='/api/chat'
    && request.postDataJSON()?.input==='중단 응답 확인');
  await chatInput.press('Enter');
  const pendingBody=(await pendingChat).postDataJSON();
  assert.match(pendingBody.requestId,/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i);
  await page.getByRole('button',{name:'응답 중단'}).click();
  await page.locator('form.conversation-composer button[title="보내기"]').waitFor({timeout:10000});
  assert.equal(cancelRequestPayload.requestId,pendingBody.requestId,
    'server Stop must tombstone the exact delayed Chat request');
  assert.ok(cancelStatusReads>=2,'Stop must wait for durable session release');
  assert.equal(await page.getByText('서버가 아직 실행 종료를 확인하지 못했습니다. 다시 중단을 누르세요.').count(),0);
  slowChatRelease?.();
  await page.getByRole('button',{name:'대화 목록 펼치기'}).click();
  page.once('dialog',dialog=>dialog.accept());
  const deleting=page.waitForRequest(request=>
    new URL(request.url()).pathname==='/api/sessions/chat-one'
    &&request.method()==='DELETE');
  await page.getByRole('button',{name:'Conversation One 대화 삭제'}).click();
  await deleting;
  await page.locator('aside').getByRole('button',{name:'Conversation One',exact:true})
    .waitFor({state:'detached',timeout:10000});
  assert.ok(deletedChatIds.has('chat-one'),'private Chat delete reaches Rust endpoint');
  assert.deepEqual(errors,[], 'Chat/Team React runtime errors');
  console.log('PASS Chromium route isolation, Markdown, attachments, mode persistence and history search');
}finally{
  await browser?.close();
  server.kill('SIGTERM');
}
