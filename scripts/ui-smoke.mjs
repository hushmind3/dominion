import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {mkdtemp,mkdir,rm} from 'node:fs/promises';
import {createServer} from 'node:net';
import os from 'node:os';
import path from 'node:path';
import {chromium} from 'playwright';

const temp=await mkdtemp(path.join(os.tmpdir(),'dominion-ui-'));
const shots=path.resolve(process.env.DOMINION_SCREENSHOT_DIR||path.join(temp,'screenshots'));
await mkdir(shots,{recursive:true});
const available=()=>new Promise((resolve,reject)=>{
 const server=createServer();server.once('error',reject);
 server.listen(0,'127.0.0.1',()=>{const port=server.address().port;server.close(()=>resolve(port));});
});
const port=await available(),origin='http://127.0.0.1:'+port;
const child=spawn(process.execPath,['server/index.mjs'],{
 cwd:process.cwd(),env:{...process.env,DOMINION_PORT:String(port),
 DOMINION_DATA:path.join(temp,'db'),DOMINION_WORKSPACE:path.join(temp,'workspace')},
 stdio:['ignore','pipe','pipe']
});
let logs='';
for(const stream of [child.stdout,child.stderr])stream.on('data',chunk=>logs=(logs+chunk.toString()).slice(-12000));
let browser;
try{
 let connected=false;
 for(let attempt=0;attempt<100;attempt++){
  if(child.exitCode!==null)throw Error('DOMINION server exited: '+logs);
  try{const response=await fetch(origin+'/api/health');if(response.ok){connected=true;break;}}catch{}
  await new Promise(resolve=>setTimeout(resolve,100));
 }
 assert.ok(connected,'Server never became healthy: '+logs);
 browser=await chromium.launch({headless:true,args:['--no-sandbox']});
 const page=await browser.newPage({viewport:{width:1440,height:900},deviceScaleFactor:1});
 const errors=[];
 page.on('pageerror',error=>errors.push(error.message));
 await page.goto(origin,{waitUntil:'domcontentloaded'});
 await page.getByText('DOMINION',{exact:true}).first().waitFor();
 const firstNav=page.locator('aside nav button').first();
 assert.equal(await firstNav.evaluate(el=>getComputedStyle(el).borderTopWidth),'0px','Sidebar items should not render as bordered pills');
 const links=[
  ['홈','home'],['대화','chat'],['모델 허브','models'],['에이전트','agents'],
  ['그룹 · 회의실','rooms'],['워크플로','workflows'],['기억 라이브러리','memory'],
  ['확장 허브','capabilities'],['MCP · 스킬','extensions'],['연결 · 채널','connections'],
  ['자동화','automations'],['실행 기록','activity'],['설정','settings']
 ];
 for(const [label,id] of links){
  await page.locator('aside nav button').filter({hasText:label}).first().click();
  await page.locator('main').waitFor({state:'visible'});
  assert.ok((await page.locator('main').innerText()).length>20,'Empty page: '+id);
  if(id==='workflows'){
   await page.getByText('자율 목표 실행',{exact:true}).waitFor();
   await page.locator('#new-goal').fill('Create a small verification note.');
   assert.equal(await page.locator('#new-goal').inputValue(),'Create a small verification note.');
  }
  await page.screenshot({path:path.join(shots,id+'.png'),fullPage:false});
 }
 assert.deepEqual(errors,[], 'Browser runtime exceptions across desktop routes');
 // The mobile navigation must remain usable after the CSS migration.
 await page.setViewportSize({width:390,height:844});
 await page.reload({waitUntil:'domcontentloaded'});
 const menu=page.getByTitle('모바일 메뉴');
 if(await menu.count())await menu.first().click();
 else await page.locator('header button').first().click();
 await page.locator('aside').waitFor({state:'visible'});
 await page.screenshot({path:path.join(shots,'mobile-sidebar-open.png'),fullPage:false});
 await page.locator('aside nav button').filter({hasText:'워크플로'}).first().click();
 await page.getByText('자율 목표 실행',{exact:true}).waitFor();
 await page.waitForFunction(()=>document.querySelector('aside')?.getBoundingClientRect().right<=2,null,{timeout:3000});
 await page.screenshot({path:path.join(shots,'mobile-workflows.png'),fullPage:false});
 assert.deepEqual(errors,[],'Browser runtime exceptions on mobile');
 console.log('UI smoke passed: '+links.length+' desktop routes + mobile navigation. Screenshots: '+shots);
}finally{
 if(browser)await browser.close();
 child.kill('SIGTERM');
 await new Promise(resolve=>{if(child.exitCode!==null||child.signalCode!==null)resolve();
  else{child.once('exit',resolve);setTimeout(()=>{child.kill('SIGKILL');resolve();},2000).unref();}});
 await rm(temp,{recursive:true,force:true});
}
