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
  if(pathname==='/api/rooms'){
   if(route.request().method()==='POST'){
    saved=route.request().postDataJSON();
    room={...room,...saved};
    response=room;
   }else response=[room];
  }
  if(pathname==='/api/rooms/room-model-fixture/state')response={
   room,sessionId:room.sessionId,jobs:[],executions:[],learningJobs:[],
   operations:[],approvals:[],status:'idle'};
  if(pathname==='/api/rooms/room-model-fixture/messages')response=[{
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
 const routes=await page.locator('.room-route-bar-items > span').allTextContents();
 assert.equal(routes.length,2);
 assert.ok(routes.every(text=>text.includes('owner-picked-model')),JSON.stringify(routes));
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
 const sizes=await page.evaluate(()=>({screen:innerWidth,body:document.documentElement.scrollWidth}));
 assert.ok(sizes.body<=sizes.screen+2,'Room model UI overflows mobile: '+JSON.stringify(sizes));
 assert.deepEqual(errors,[],'Room runtime errors');
 console.log('PASS Room provider hydration, persisted model routing, per-reply model and mobile layout');
}finally{
 releaseProviders?.();
 await browser?.close();
 server.kill('SIGTERM');
}
