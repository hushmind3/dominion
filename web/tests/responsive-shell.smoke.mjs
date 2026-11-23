// Browser-level responsive navigation checks using a running Vite/React UI.
// Mock responses isolate layout behavior from external model credentials.
import assert from 'node:assert/strict'
import {spawn} from 'node:child_process'
import {fileURLToPath} from 'node:url'
import {dirname,resolve} from 'node:path'
import {chromium} from 'playwright'

const root=resolve(dirname(fileURLToPath(import.meta.url)),'..')
const port=5195
const origin='http://127.0.0.1:'+port
const server=spawn(process.execPath,[resolve(root,'node_modules/vite/bin/vite.js'),
 '--host','127.0.0.1','--port',String(port),'--strictPort'],{
 cwd:root,stdio:['ignore','pipe','pipe']
})
let browser
let logs=''
for(const stream of [server.stdout,server.stderr])stream.on('data',data=>{
 logs=(logs+data.toString()).slice(-3600)
})
async function ready(){
 for(let i=0;i<75;i++){
  if(server.exitCode!==null)throw Error('Vite quit: '+logs)
  try{if((await fetch(origin)).ok)return}catch{}
  await new Promise(done=>setTimeout(done,200))
 }
 throw Error('Vite did not start: '+logs)
}
try{
 await ready()
 browser=await chromium.launch({headless:true})
 const page=await browser.newPage({viewport:{width:1280,height:800},locale:'ko-KR'})
 const pageErrors=[]
 page.on('pageerror',error=>pageErrors.push(error.message))
 await page.route(url=>new URL(url).pathname.startsWith('/api/'),async route=>{
  const path=new URL(route.request().url()).pathname
  if(path==='/api/events'){
   await route.fulfill({status:200,contentType:'text/event-stream',body:': fixture\n\n'})
   return
  }
  let response=[]
  if(path==='/api/health')response={branch:'dominion-sub2',commit:'nav-fixture'}
  else if(path==='/api/status')response={jobs:0,providers:0,agents:0}
  else if(path==='/api/settings/owner-profile')response={id:'owner',name:'나',avatar:''}
  else if(path==='/api/policy'||path.startsWith('/api/settings/'))response={}
  else if(path==='/api/model-library/runtime')response={status:'idle'}
  else if(path==='/api/activity/teams')response={teams:[],leadJobs:[],remoteWorkers:[],remoteConfigured:false}
  await route.fulfill({status:200,contentType:'application/json',body:JSON.stringify(response)})
 })
 await page.goto(origin+'/#/chat',{waitUntil:'domcontentloaded'})
 const history=page.locator('#conversation-sidebar')
 await history.waitFor({state:'visible',timeout:12000})
 await history.getByRole('button',{name:'대화 목록 접기'}).click()
 const rail=page.locator('.page-chat .conversation-rail')
 await rail.waitFor({state:'visible',timeout:5000})
 assert.equal(await history.isVisible(),false,'desktop collapse must hide the full history list')
 assert.equal(await rail.getByRole('button',{name:'채팅'}).isVisible(),true)
 assert.equal(await rail.getByRole('button',{name:'그룹'}).isVisible(),true)
 await rail.getByRole('button',{name:'설정'}).click()
 const sidebar=page.locator('#workspace-settings-sidebar')
 await sidebar.waitFor({state:'visible',timeout:12000})
 assert.equal(await sidebar.getAttribute('data-collapsed'),'false','settings should begin expanded on desktop')
 await sidebar.getByRole('button',{name:'메뉴 접기'}).click()
 assert.equal(await sidebar.getAttribute('data-collapsed'),'true','settings collapse should retain a rail')
 assert.equal(await sidebar.getByRole('button',{name:'모델 관리'}).isVisible(),true,
  'collapsed settings must retain named icon navigation')
 await sidebar.getByRole('button',{name:'모델 관리'}).click()
 assert.match(page.url(),/#\/models/,'icon navigation must change route')
 await page.setViewportSize({width:390,height:844})
 const mobileToggle=page.locator('.workspace-responsive-mobile-toggle')
 await mobileToggle.waitFor({state:'visible',timeout:5000})
 await mobileToggle.click()
 assert.equal(await sidebar.getAttribute('data-collapsed'),'false','mobile hamburger opens settings drawer')
 const rect=await sidebar.boundingBox()
 assert.ok(rect && rect.x>=-1 && rect.width>200 && rect.width<=390,
  'mobile drawer must fit viewport: '+JSON.stringify(rect))
 await page.locator('.workspace-responsive-scrim').click({position:{x:380,y:440}})
 assert.equal(await sidebar.getAttribute('data-collapsed'),'true','scrim dismisses mobile drawer')
 await page.getByRole('button',{name:'닫기'}).last().click()
 assert.equal(await rail.isVisible(),false,'collapsed icon rail must not occupy mobile viewport')
 assert.deepEqual(pageErrors,[], 'responsive React navigation runtime errors')
 console.log('PASS: responsive Chat rail, settings icon nav, desktop/mobile drawer and route selection')
}finally{
 await browser?.close()
 server.kill('SIGTERM')
}
