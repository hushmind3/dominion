import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import net from 'node:net';
import {spawn} from 'node:child_process';
import {chromium} from 'playwright-core';

const wait=ms=>new Promise(resolve=>setTimeout(resolve,ms));
const port=()=>new Promise((resolve,reject)=>{
 const sock=net.createServer();
 sock.once('error',reject);
 sock.listen(0,'127.0.0.1',()=>{const n=sock.address().port;sock.close(()=>resolve(n));});
});
const browserExecutable=[
 process.env.CHROME_BIN,
 '/usr/bin/google-chrome',
 '/usr/bin/google-chrome-stable',
 '/opt/google/chrome/chrome',
 '/usr/bin/chromium'
].find(x=>x&&fs.existsSync(x));
if(!browserExecutable)throw Error('System Chromium/Chrome is required for UI smoke tests');
const base=fs.mkdtempSync(path.join(os.tmpdir(),'dominion-ui-'));
const artifacts=path.resolve('artifacts');
fs.mkdirSync(artifacts,{recursive:true});
const p=await port();
const url='http://127.0.0.1:'+p;
const server=spawn(process.execPath,['server/index.mjs'],{
 cwd:process.cwd(),
 env:{...process.env,DOMINION_PORT:String(p),DOMINION_HOST:'127.0.0.1',DOMINION_DATA:path.join(base,'data'),DOMINION_WORKSPACE:path.join(base,'workspace')},
 stdio:'pipe'
});
let log='';
server.stdout.on('data',v=>log+=v.toString());
server.stderr.on('data',v=>log+=v.toString());
let browser;
try{
 let ready=false;
 for(let i=0;i<100;i++){
  try{if((await fetch(url+'/api/health')).ok){ready=true;break}}catch{}
  await wait(140);
 }
 assert.ok(ready,'Backend failed: '+log);
 browser=await chromium.launch({headless:true,executablePath:browserExecutable,args:['--no-sandbox','--disable-dev-shm-usage']});
 const page=await browser.newPage({viewport:{width:1440,height:900},deviceScaleFactor:1});
 const failures=[];
 page.on('pageerror',e=>failures.push(e.message));
 await page.route('**/api/model-library/catalog?*',route=>route.fulfill({status:200,contentType:'application/json',body:JSON.stringify([
  {id:'demo/Qwen-9B-GGUF',downloads:123456,likes:81,updatedAt:'2026-11-01',task:'text-generation',tags:['gguf','reasoning'],gated:false,description:'테스트용 공개 모델 상세정보',capabilities:{vision:false,tools:false,reasoning:true}}
 ])}));
 await page.route('**/api/model-library/details?*',route=>route.fulfill({status:200,contentType:'application/json',body:JSON.stringify({
  id:'demo/Qwen-9B-GGUF',downloads:123456,likes:81,updatedAt:'2026-11-01',task:'text-generation',tags:['gguf','reasoning'],gated:false,description:'테스트용 공개 모델 상세정보',capabilities:{vision:false,tools:false,reasoning:true},
  revision:'a'.repeat(40),architecture:'qwen',license:'apache-2.0',readme:'# Qwen 9B\nSmoke test model',
  options:[{id:'gguf:model.Q4_K_M.gguf',format:'gguf',label:'Q4_K_M',quantization:'Q4_K_M',bytes:4823456000,files:['model.Q4_K_M.gguf'],shards:1}]
 })}));
 await page.goto(url,{waitUntil:'domcontentloaded'});
 await page.getByRole('heading',{name:'모든 지능을 한곳에서.'}).waitFor({timeout:10000});
 const nav=page.locator('aside[aria-label="데스크톱 메뉴"]');
 await nav.waitFor();
 let wide=await nav.evaluate(e=>e.getBoundingClientRect().width);
 assert.ok(wide>200,'Default sidebar should be expanded: '+wide);
 await page.getByRole('button',{name:'사이드바 접기 또는 펼치기'}).click();
 await page.waitForTimeout(250);
 let narrow=await nav.evaluate(e=>e.getBoundingClientRect().width);
 assert.ok(narrow<100,'Hamburger did not collapse desktop sidebar: '+narrow);
 await page.getByRole('button',{name:'사이드바 접기 또는 펼치기'}).click();
 await page.waitForTimeout(250);
 wide=await nav.evaluate(e=>e.getBoundingClientRect().width);
 assert.ok(wide>200,'Hamburger did not expand desktop sidebar');
 await page.getByRole('button',{name:'모델 라이브러리',exact:true}).click();
 await page.getByRole('heading',{name:'모델 라이브러리',exact:true}).waitFor();
 await page.getByText('Qwen-9B-GGUF',{exact:true}).first().waitFor({timeout:10000});
 await page.getByText('Q4_K_M',{exact:true}).first().waitFor();
 assert.ok((await page.getByText('4.82 GB').count())>=1,'Real download option not displayed');
 await page.screenshot({path:path.join(artifacts,'models-desktop.png'),fullPage:true});
 await page.getByRole('button',{name:'대화',exact:true}).first().click();
 await page.getByLabel('메시지 입력').waitFor({timeout:10000});
 await page.getByRole('button',{name:'에이전트',exact:true}).first().click();
 await page.getByRole('button',{name:'에이전트 만들기'}).click();
 await page.getByRole('dialog',{name:'에이전트 만들기'}).waitFor();
 const dialog=page.getByRole('dialog',{name:'에이전트 만들기'});
 await dialog.getByLabel('이름').fill('UI Smoke Agent');
 await dialog.getByRole('button',{name:'저장'}).click();
 await page.getByText('UI Smoke Agent').first().waitFor({timeout:10000});
 await page.setViewportSize({width:390,height:844});
 await page.getByRole('button',{name:'사이드바 접기 또는 펼치기'}).click();
 const mobileNav=page.locator('aside[aria-label="모바일 메뉴"]');
 await mobileNav.getByRole('button',{name:'모델 라이브러리',exact:true}).click();
 await page.getByRole('heading',{name:'모델 라이브러리',exact:true}).waitFor();
 await page.screenshot({path:path.join(artifacts,'models-mobile.png'),fullPage:true});
 assert.deepEqual(failures,[],'Unexpected browser runtime errors: '+failures.join(' | '));
 process.stdout.write(JSON.stringify({ok:true,sidebar:{wide,narrow},screenshots:['models-desktop.png','models-mobile.png'],agentCreated:true})+'\n');
}finally{
 await browser?.close();
 server.kill('SIGTERM');
 fs.rmSync(base,{recursive:true,force:true});
}
