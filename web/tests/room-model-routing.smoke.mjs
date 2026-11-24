// Chromium end-to-end UI contract for real Room model settings.
// API responses are mocked; native model dispatch is covered by Rust E2E.
import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {fileURLToPath} from 'node:url';
import {dirname,resolve} from 'node:path';
import {chromium} from 'playwright';

const root=resolve(dirname(fileURLToPath(import.meta.url)),'..');
const port=5188;
const address='http://127.0.0.1:'+port;
const server=spawn(process.execPath,[resolve(root,'node_modules/vite/bin/vite.js'),
 '--host','127.0.0.1','--port',String(port),'--strictPort'],{
 cwd:root,stdio:['ignore','pipe','pipe']
});
let browser;
let selectedModelRequest=false;
const submittedMessages=[];
const uploadedRoomAttachments=[];
let room={
 id:'room-model-fixture',name:'Model mapping room',
 agentIds:['lead','researcher'],routingMode:'discussion',rounds:1,maxTurns:2,
 autoModels:false,models:[],toolsEnabled:false,autoLearning:false,
 sessionId:'room:room-model-fixture',status:'idle'
};
let saved=null;
let releaseProviders;
const providerGate=new Promise(resolve=>{releaseProviders=resolve});
async function ready(){
 for(let i=0;i<60;i++){
  if(server.exitCode!==null)throw Error('Vite closed before Room smoke');
  try{if((await fetch(address)).ok)return}catch{}
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
  const {pathname}=new URL(route.request().url());
  let response=[];
  if(pathname==='/api/providers'){
   // Initial RoomModelPicker mount occurs BEFORE provider discovery resolves.
   await providerGate;
   response=[{id:'fixture',name:'Fixture Provider',model:'agent-default',
    enabled:true,kind:'openai',baseUrl:'http://127.0.0.1/v1'},
    {id:'offline',name:'Offline Catalog',model:'configured-only-model',
     enabled:true,kind:'openai',baseUrl:'http://127.0.0.1/unavailable'}];
  }
  if(pathname==='/api/agents')response=[
   {id:'lead',name:'Lead A',providerId:'fixture',model:'agent-default',enabled:true},
   {id:'researcher',name:'Researcher B',providerId:'fixture',model:'agent-default',enabled:true}
  ];
  if(pathname==='/api/providers/fixture/models'){
   selectedModelRequest=true;
   response=[{id:'owner-picked-model',name:'Owner-picked model'}];
  }
  if(pathname==='/api/providers/offline/models'){
   await route.fulfill({status:502,contentType:'application/json',
    body:JSON.stringify({error:'catalog offline'})});
   return;
  }
  if(pathname==='/api/attachments'&&route.request().method()==='POST'){
   uploadedRoomAttachments.push(route.request().postDataJSON());
   const uploaded=uploadedRoomAttachments.at(-1);
   response={id:uploaded.name==='notes.txt'?'room-uploaded-note':'room-uploaded-'+uploaded.name,
    name:uploaded.name,size:Buffer.from(uploaded.data,'base64').length,
    sessionId:room.sessionId,supportedForModel:'text'};
  }
  if(pathname==='/api/rooms'){
   if(route.request().method()==='POST'){
    saved=route.request().postDataJSON();
    room={...room,...saved};
    response=room;
   }else response=[room];
  }
  if(pathname==='/api/rooms/room-model-fixture/reset'){
   room={...room,sessionId:'room:reset-session',status:'idle'};
   response={sessionId:room.sessionId};
  }
  if(pathname==='/api/rooms/room-model-fixture/state')response={
   room,sessionId:room.sessionId,jobs:[],executions:[],learningJobs:[],
   operations:[],approvals:[],status:'idle'};
  if(pathname==='/api/rooms/room-model-fixture/messages'
    &&route.request().method()==='POST'){
   submittedMessages.push(route.request().postDataJSON());
   await route.fulfill({status:503,contentType:'application/json',
    body:JSON.stringify({error:'temporary-room-unavailable'})});
   return;
  }
  if(pathname==='/api/rooms/room-model-fixture/messages')response=[{
   id:'owner-layout',session_id:room.sessionId,role:'user',
   content:'방에서 내가 쓴 질문은 오른쪽에 표시합니다.',
   metadata:{attachments:[{id:'previous-room-file',name:'old-notes.txt',isImage:false}]},
   created_at:'2026-11-22 00:00:00'
  },{
   id:'model-reply',session_id:room.sessionId,role:'assistant',
   content:'Recorded real model',agent_id:'lead',
   created_at:'2026-11-21 00:00:00',
   metadata:{usage:{providerId:'fixture',model:'recorded-model',totalTokens:4}}
  }];
  if(pathname==='/api/status')response={jobs:0};
  if(pathname==='/api/health')response={branch:'dominion-sub2',commit:'fixture'};
  if(pathname==='/api/events'){
   await route.fulfill({status:200,contentType:'text/event-stream',body:': room smoke\n\n'});
   return;
  }
  await route.fulfill({status:200,contentType:'application/json',body:JSON.stringify(response)});
 });
 await page.goto(address+'/#/rooms',{waitUntil:'domcontentloaded'});
 await page.getByRole('button',{name:'그룹 설정',exact:true}).waitFor({timeout:10000});
 await page.getByRole('button',{name:'그룹 설정',exact:true}).click();
 await page.getByLabel('모델 제공자 선택').waitFor({timeout:10000});
 // A configured fallback option can render before the asynchronous catalog
 // request finishes. Observe the actual network response, not the checkbox.
 const modelsFetched=page.waitForResponse(response=>
  new URL(response.url()).pathname==='/api/providers/fixture/models',
  {timeout:10000});
 releaseProviders();
 await modelsFetched;
 await page.locator('.room-model-check input[type="checkbox"]').first()
  .waitFor({state:'visible',timeout:10000});
 assert.equal(selectedModelRequest,true,
  'room picker must fetch provider models even when providers load after mount');
 await page.locator('.room-model-check input[type="checkbox"]').first().check();
 await page.getByText('owner-picked-model',{exact:false}).first()
  .waitFor({state:'visible',timeout:10000});
 await page.getByRole('button',{name:'변경 저장',exact:true}).click();
 await page.waitForFunction(()=>Boolean(document.querySelector('.room-route-bar')
   ?.textContent?.includes('owner-picked-model')),{timeout:10000});
 assert.equal(saved.models[0].providerId,'fixture');
 assert.equal(saved.models[0].model,'owner-picked-model');
 assert.equal(saved.models.length,1);
 assert.equal(await page.locator('.room-route-details').getAttribute('open'),null,'routing details are initially collapsed');
 await page.locator('.room-route-details > summary').click();
 const routes=await page.locator('.room-route-bar-items > span').allTextContents();
 await page.locator('.room-route-details > summary').click();
 assert.equal(routes.length,2);
 assert.ok(routes.every(text=>text.includes('owner-picked-model')),JSON.stringify(routes));
 await page.locator('.room-message-owner .room-message-text').waitFor({timeout:10000});
 await page.getByRole('button',{name:'old-notes.txt 첨부 열기'}).waitFor({timeout:10000});
 const messageLayout=await page.locator('.room-message').evaluateAll(rows=>rows.map(row=>{
  const text=row.querySelector('.room-message-text');
  const rect=text?.getBoundingClientRect();
  return {owner:row.classList.contains('room-message-owner'),left:rect?.left||0,
   right:rect?.right||0,fontSize:parseFloat(text?getComputedStyle(text).fontSize:'0')};
 }));
 const own=messageLayout.find(row=>row.owner),reply=messageLayout.find(row=>!row.owner);
 assert.ok(own&&reply,JSON.stringify(messageLayout));
 assert.ok(own.left>reply.left+40,
  'Room owner must appear on right and AI reply on left: '+JSON.stringify(messageLayout));
 assert.ok(own.right<=390,'Room bubble must fit mobile: '+JSON.stringify(messageLayout));
 assert.ok(own.fontSize>=15&&reply.fontSize>=15,
  'Room transcript text must remain readable on mobile: '+JSON.stringify(messageLayout));
 assert.ok(await page.getByText('recorded-model',{exact:true}).first().isVisible(),
  'each saved reply must expose its ACTUAL model, not just the configured route');
 // The configured model remains selectable when /models is not available.
 // The settings panel remains open after Save; close it before exercising
 // the visible per-Room model-change entry point.
 await page.getByRole('button',{name:'설정 닫기',exact:true}).click();
 await page.getByRole('button',{name:'모델 변경',exact:true}).click();
 await page.getByLabel('모델 제공자 선택').selectOption('offline');
 await page.getByText('configured-only-model',{exact:true}).first()
  .waitFor({state:'visible',timeout:10000});
 await page.locator('.room-model-check input[type="checkbox"]').first().check();
 await page.getByRole('button',{name:'변경 저장',exact:true}).click();
 await page.waitForFunction(()=>Boolean(document.querySelector('.room-route-bar')
  ?.textContent?.includes('configured-only-model')),{timeout:10000});
 assert.equal(saved.models.length,2);
 const changedRoutes=await page.locator('.room-route-bar-items > span').allTextContents();
 assert.ok(changedRoutes[0].includes('owner-picked-model'),JSON.stringify(changedRoutes));
 assert.ok(changedRoutes[1].includes('configured-only-model'),JSON.stringify(changedRoutes));
 // Room drafts survive navigation/reload and retain exact recipient choices.
 await page.getByRole('button',{name:'설정 닫기',exact:true}).click();
 const composer=page.locator('.conversation-composer-input');
 await composer.fill('Room only: 검토 후 요약해 주세요');
 await page.locator('.conversation-recipients').click();
 await page.locator('[role="dialog"] button').filter({hasText:'Lead A'}).first().click();
 const persisted=await page.evaluate(()=>JSON.parse(sessionStorage.getItem(
  'dominion.room.composer.v1:room-model-fixture')||'null'));
 assert.equal(persisted.text,'Room only: 검토 후 요약해 주세요');
 assert.deepEqual(persisted.targets,['lead']);
 assert.ok(await page.locator('.room-target-chips').getByText('Lead A',{exact:false}).isVisible(),'explicit target is visible without opening the picker');
 await page.reload({waitUntil:'domcontentloaded'});
 await composer.waitFor({timeout:10000});
 assert.equal(await composer.inputValue(),'Room only: 검토 후 요약해 주세요');
 await page.locator('.conversation-recipients').click();
 assert.equal(await page.locator('[role="dialog"] button').filter({hasText:'Lead A'})
  .first().getAttribute('aria-pressed'),'true');
 // Deleted workers cannot be dropped to auto-routing without an owner choice.
 await page.evaluate(()=>sessionStorage.setItem('dominion.room.composer.v1:room-model-fixture',
  JSON.stringify({text:'Do not broadcast this',targets:['removed-agent']})));
 await page.reload({waitUntil:'domcontentloaded'});
 await composer.waitFor({timeout:10000});
 assert.equal(await composer.inputValue(),'Do not broadcast this');
 await page.getByText('이전에 지정한 참가자',{exact:false}).waitFor({timeout:10000});
 assert.equal(await page.getByRole('button',{name:'메시지 보내기'}).isDisabled(),true);
 await page.getByRole('button',{name:'사용할 수 없는 대상 제외'}).click();
 assert.equal(await composer.inputValue(),'Do not broadcast this');
 assert.equal(await page.getByRole('button',{name:'메시지 보내기'}).isEnabled(),true);
 // An uncertain HTTP result must not lose the committed-operation nonce.
 // Rust E2E covers server deduplication; Chromium verifies the real form
 // resends the SAME nonce after reopening the tab.
 await composer.fill('Retry the same Room task');
 await page.locator('input[aria-label="그룹 첨부 파일 선택"]').setInputFiles({
  name:'notes.txt',mimeType:'text/plain',buffer:Buffer.from('room file bytes')
 });
 await page.getByText('notes.txt',{exact:true}).waitFor({timeout:10000});
 assert.equal(uploadedRoomAttachments.length,1);
 assert.equal(uploadedRoomAttachments[0].roomId,'room-model-fixture');
 assert.equal(uploadedRoomAttachments[0].name,'notes.txt');
 assert.equal(uploadedRoomAttachments[0].data,Buffer.from('room file bytes').toString('base64'));
 const submit=page.getByRole('button',{name:'메시지 보내기'});
 const firstResponse=page.waitForResponse(response=>
  response.url().endsWith('/api/rooms/room-model-fixture/messages')
  &&response.request().method()==='POST',{timeout:10000});
 await submit.click();
 await firstResponse;
 await page.getByText('temporary-room-unavailable',{exact:false})
  .waitFor({timeout:10000});
 assert.equal(submittedMessages.length,1);
 assert.deepEqual(submittedMessages[0].attachmentIds,['room-uploaded-note']);
 const firstId=submittedMessages[0].clientMessageId;
 assert.ok(firstId,'the first request must carry an idempotency key');
 await page.reload({waitUntil:'domcontentloaded'});
 await composer.waitFor({timeout:10000});
 assert.equal(await composer.inputValue(),'Retry the same Room task');
 await page.getByText('notes.txt',{exact:true}).waitFor({timeout:10000});
 const retryResponse=page.waitForResponse(response=>
  response.url().endsWith('/api/rooms/room-model-fixture/messages')
  &&response.request().method()==='POST',{timeout:10000});
 await submit.click();
 await retryResponse;
 assert.equal(submittedMessages.length,2);
 assert.deepEqual(submittedMessages[1].attachmentIds,['room-uploaded-note']);
 assert.equal(uploadedRoomAttachments.length,1,'retries must reuse uploaded bytes by ID');
 assert.equal(submittedMessages[1].clientMessageId,firstId,
  'reload plus retry cannot spawn a second server-owned Room job');
 // Room paste/drop is handled by the same existing Rust room attachment API.
 await composer.fill('그룹에서 작성 중인 원문은 유지');
 await composer.evaluate(input=>{
  const dt=new DataTransfer()
  dt.items.add(new File(['pasted content'],'pasted.txt',{type:'text/plain'}))
  input.dispatchEvent(new ClipboardEvent('paste',{bubbles:true,cancelable:true,clipboardData:dt}))
 });
 await page.getByText('pasted.txt',{exact:true}).waitFor({timeout:10000});
 assert.equal(await composer.inputValue(),'그룹에서 작성 중인 원문은 유지');
 await page.getByRole('button',{name:'pasted.txt 첨부 취소'}).click();
 await page.locator('form.conversation-composer').evaluate(form=>{
  const dt=new DataTransfer()
  dt.items.add(new File(['dropped content'],'dropped.txt',{type:'text/plain'}))
  form.dispatchEvent(new DragEvent('drop',{bubbles:true,cancelable:true,dataTransfer:dt}))
 });
 await page.getByText('dropped.txt',{exact:true}).waitFor({timeout:10000});
 assert.equal(await composer.inputValue(),'그룹에서 작성 중인 원문은 유지');
 assert.ok(uploadedRoomAttachments.some(file=>file.name==='pasted.txt'));
 assert.ok(uploadedRoomAttachments.some(file=>file.name==='dropped.txt'));
 // Reset keeps the owner text/recipient draft but cannot reuse an upload
 // scoped to the previous Room transcript, or its stale idempotency nonce.
 page.once('dialog',dialog=>dialog.accept());
 const roomReset=page.waitForResponse(response=>
  new URL(response.url()).pathname==='/api/rooms/room-model-fixture/reset');
 const roomMenu=page.getByRole('button',{name:'그룹 관리',exact:true});
 await roomMenu.click();
 await page.getByRole('dialog',{name:'그룹 관리',exact:true}).waitFor();
 await page.keyboard.press('Escape');
 assert.equal(await roomMenu.getAttribute('aria-expanded'),'false');
 await roomMenu.click();
 await page.getByRole('button',{name:'대화 초기화'}).click();
 await roomReset;
 await page.getByText('dropped.txt',{exact:true}).waitFor({state:'detached',timeout:10000});
 assert.equal(await composer.inputValue(),'그룹에서 작성 중인 원문은 유지');
 const afterReset=await page.evaluate(()=>JSON.parse(sessionStorage.getItem(
  'dominion.room.composer.v1:room-model-fixture')||'null'));
 assert.deepEqual(afterReset.attachments,[]);
 assert.equal(afterReset.attempt,undefined);
 const sizes=await page.evaluate(()=>({screen:innerWidth,body:document.documentElement.scrollWidth}));
 assert.ok(sizes.body<=sizes.screen+2,'Room model UI overflows mobile: '+JSON.stringify(sizes));
 const layout=await page.locator('.room-main').evaluate(main=>Object.fromEntries(['.room-header','.conversation-composer-controls'].map(selector=>{const el=main.querySelector(selector);return [selector,{width:el.clientWidth,scroll:el.scrollWidth}]})));
 for(const [name,box] of Object.entries(layout))assert.ok(box.scroll<=box.width+2,name+' horizontally overflows: '+JSON.stringify(box));
 assert.deepEqual(errors,[],'Room runtime errors');
 console.log('PASS Room provider hydration, persisted model routing, per-reply model and mobile layout');
}finally{
 releaseProviders?.();
 await browser?.close();
 server.kill('SIGTERM');
}
