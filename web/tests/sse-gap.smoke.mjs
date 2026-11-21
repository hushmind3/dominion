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
let browser,releaseGap;
const gapGate=new Promise(resolve=>{releaseGap=resolve});
let sessionsRead=0,sseConnections=0;
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
 const page=await browser.newPage({viewport:{width:390,height:844},locale:'ko-KR'});
 const errors=[];
 page.on('pageerror',error=>errors.push(error.message));
 await page.route('**/api/**',async route=>{
  const path=new URL(route.request().url()).pathname;
  if(path==='/api/events'){
   sseConnections++;
   if(sseConnections===1){
    await gapGate;
    await route.fulfill({status:200,contentType:'text/event-stream',
     body:'id: 0\\ndata: '+JSON.stringify({type:'event_gap',resyncRequired:true,
      reason:'cursor_ahead_of_journal'})+'\\n\\n'});
   }else{
    await route.fulfill({status:200,contentType:'text/event-stream',body:': idle\\n\\n'});
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
  if(path==='/api/status')result={jobs:0};
  if(path==='/api/model-library/runtime')result={status:'idle'};
  if(path.endsWith('/usage'))result={totalTokens:0};
  await route.fulfill({status:200,contentType:'application/json',
   body:JSON.stringify(result)});
 });
 await page.goto(origin+'/#/chat?session=gap-session',{waitUntil:'domcontentloaded'});
 await page.getByText('Before reconnect',{exact:true}).first().waitFor({timeout:10000});
 const before=sessionsRead;
 sessions=[{...sessions[0],title:'After reconnect'}];
 releaseGap();
 await page.getByText('After reconnect',{exact:true}).first().waitFor({timeout:10000});
 assert.ok(sessionsRead>before,'SSE gap must invalidate mounted /api/sessions');
 assert.deepEqual(errors,[],'React runtime errors during gap resync');
 console.log('PASS Chromium SSE gap refreshes mounted Chat without reload');
}finally{
 releaseGap?.();
 await browser?.close();
 vite.kill('SIGTERM');
}
