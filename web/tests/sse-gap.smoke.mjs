// Real Chromium/Vite integration check: a gap receipt must refresh an
// already-mounted Chat transcript index. Network/endpoint fixtures are mocked;
// Rust's journal and cursor semantics are covered by the native regression.
import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {fileURLToPath} from 'node:url';
import {dirname,resolve} from 'node:path';
import {chromium} from 'playwright';

const root=resolve(dirname(fileURLToPath(import.meta.url)),'..');
const port=5189,origin='http://127.0.0.1:'+port;
const vite=spawn(process.execPath,[resolve(root,'node_modules/vite/bin/vite.js'),
 '--host','127.0.0.1','--port',String(port),'--strictPort'],{
 cwd:root,stdio:['ignore','pipe','pipe']
});
let browser;
let sessionsRead=0,sseConnections=0,gapArmed=false,gapSent=false;
let sessions=[{id:'gap-session',title:'Before reconnect',agentId:'agent-a',
 providerId:'provider-a',model:'fixture'}];
async function waitForServer(){
 for(let i=0;i<60;i++){
  if(vite.exitCode!==null)throw Error('Vite exited before SSE test');
  try{if((await fetch(origin)).ok)return}catch{}
  await new Promise(resolve=>setTimeout(resolve,200));
 }
 throw Error('Vite did not start');
}
try{
 await waitForServer();
 browser=await chromium.launch({headless:true});
 const page=await browser.newPage({viewport:{width:1280,height:800},locale:'ko-KR'});
 const errors=[];
 page.on('pageerror',error=>errors.push(error.message));
 await page.route('**/api/**',async route=>{
  const path=new URL(route.request().url()).pathname;
  if(path==='/api/events'){
   sseConnections++;
   // A pending route.fulfill can stall initial browser loading. Let the
   // first SSE complete, then send a gap on EventSource's native reconnect.
   if(gapArmed&&!gapSent){
    gapSent=true;
    await route.fulfill({status:200,contentType:'text/event-stream',
     body:'id: 0\ndata: '+JSON.stringify({type:'event_gap',resyncRequired:true,
      reason:'cursor_ahead_of_journal'})+'\n\n'});
   }else{
    await route.fulfill({status:200,contentType:'text/event-stream',body:': idle\n\n'});
   }
   return;
  }
  let result=[];
  if(path==='/api/sessions'){sessionsRead++;result=sessions}
  if(path==='/api/agents')result=[{id:'agent-a',name:'Agent A',
   providerId:'provider-a',model:'fixture',systemPrompt:''}];
  if(path==='/api/providers')result=[{id:'provider-a',name:'Fixture',
   kind:'openai',model:'fixture',enabled:true}];
  if(path==='/api/health')result={branch:'dominion-sub2',commit:'fixture'};
  if(path==='/api/activity/teams')result={leadJobs:[],teams:[],remoteWorkers:[],remoteConfigured:false};
  if(path==='/api/status')result={jobs:0};
  if(path==='/api/model-library/runtime')result={status:'idle'};
  if(path.endsWith('/usage'))result={totalTokens:0};
  await route.fulfill({status:200,contentType:'application/json',
   body:JSON.stringify(result)});
 });
 await page.goto(origin+'/#/chat?session=gap-session',{waitUntil:'domcontentloaded'});
 // The clean chat surface intentionally starts with a compact navigation rail.
 // Open the history explicitly to observe the refreshed title.
 await page.getByRole('button',{name:'대화 목록 펼치기'}).click();
 await page.locator('#conversation-sidebar').waitFor({state:'visible',timeout:10000});
 await page.getByText('Before reconnect',{exact:true}).first().waitFor({timeout:10000})
  .catch(async error=>{
   console.error('SSE fixture initial Chat diagnosis',JSON.stringify({
    sessionsRead,sseConnections,errors,
    text:(await page.locator('body').innerText()).slice(0,3500),
    activeUrl:page.url(),
    dom:(await page.locator('body').innerHTML()).slice(0,1800),
   }));
   throw error;
  });
 const before=sessionsRead;
 sessions=[{...sessions[0],title:'After reconnect'}];
 gapArmed=true;
 await page.getByText('After reconnect',{exact:true}).first().waitFor({timeout:15000});
 assert.ok(gapSent,'EventSource must reconnect and receive the journal gap');
 assert.ok(sessionsRead>before,'SSE gap must invalidate mounted /api/sessions');
 assert.deepEqual(errors,[],'React runtime errors during gap resync');
 console.log('PASS Chromium SSE gap refreshes mounted Chat without reload');
}finally{
 await browser?.close();
 vite.kill('SIGTERM');
}
