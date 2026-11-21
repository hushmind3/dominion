import assert from 'node:assert/strict'
import {spawn} from 'node:child_process'
import {chromium} from 'playwright'
import path from 'node:path'
import {fileURLToPath} from 'node:url'

const root=fileURLToPath(new URL('../../',import.meta.url))
const port=Number(process.env.DOMINION_HOME_VISUAL_PORT||4390)
const base='http://127.0.0.1:'+port
const server=spawn(process.execPath,['node_modules/vite/bin/vite.js',
 '--host','127.0.0.1','--port',String(port),'--strictPort'],{
 cwd:path.join(root,'web'),stdio:['ignore','pipe','pipe']
})
let logs=''
server.stdout.on('data',chunk=>{logs+=chunk})
server.stderr.on('data',chunk=>{logs+=chunk})
const sessions=[
 {id:'chat1',title:'Private',mode:'chat',agentId:null},
 {id:'room1',title:'Room',kind:'room',roomId:'room-1'},
 {id:'channel1',title:'Connected',kind:'channel',channelContext:{transport:'discord'}},
 {id:'old',title:'Archived private',kind:'chat',archived:true}
]
const jobs=[
 {id:'job1',kind:'chat',status:'running',createdAt:'2026-11-20T10:00:00Z'},
 {id:'job2',kind:'agent',status:'awaiting_approval',createdAt:'2026-11-20T11:00:00Z'},
 {id:'job3',kind:'agent',status:'needs_review',createdAt:'2026-11-20T12:00:00Z'}
]
const reply=(route,value)=>route.fulfill({status:200,contentType:'application/json',body:JSON.stringify(value)})
async function mock(route){
 const pathname=new URL(route.request().url()).pathname
 if(pathname==='/api/events')return route.fulfill({
  status:200,contentType:'text/event-stream',body:':ok\n\n'
 })
 if(pathname==='/api/sessions')return reply(route,sessions)
 if(pathname==='/api/jobs')return reply(route,jobs)
 if(pathname==='/api/providers')return reply(route,[{
  id:'provider-home',name:'Local mock',kind:'openai',enabled:true,hasKey:true,model:'model-home'
 }])
 if(pathname==='/api/agents'||pathname==='/api/channels'||pathname==='/api/sdk/runs'
  ||pathname==='/api/remote/workers'||pathname==='/api/remote/tasks'
  ||pathname==='/api/approvals')return reply(route,[])
 if(pathname==='/api/status')return reply(route,{jobs:3,agents:0,providers:1})
 if(pathname==='/api/model-usage')return reply(route,{calls:3,totalTokens:12345,estimatedCalls:1})
 if(pathname==='/api/model-library/runtime')return reply(route,{status:'idle'})
 return reply(route,[])
}
async function ready(){
 for(let n=0;n<90;n++){
  if(server.exitCode!==null)throw Error('Vite exited: '+logs)
  try{if((await fetch(base)).ok)return}catch{}
  await new Promise(resolve=>setTimeout(resolve,200))
 }
 throw Error('Vite not ready: '+logs)
}
let browser
try{
 await ready()
 browser=await chromium.launch({headless:true,args:['--no-sandbox']})
 const page=await browser.newPage({viewport:{width:1440,height:900},locale:'ko-KR'})
 const errors=[]
 page.on('pageerror',e=>errors.push(e.message))
 await page.addInitScript(()=>localStorage.setItem('dominion.locale','ko'))
 await page.route('**/api/**',mock)
 await page.goto(base+'/#/home',{waitUntil:'domcontentloaded'})
 const chatCount=page.locator('[data-dashboard-metric="chat"] strong')
 await chatCount.waitFor({timeout:15000})
 await page.waitForFunction(()=>document.querySelector('[data-dashboard-metric="chat"] strong')?.textContent?.trim()==='1',null,{timeout:15000})
 assert.equal((await chatCount.textContent()).trim(),'1',
  'Room/channel/archived sessions must not inflate private-chat counts')
 const jobCount=page.locator('[data-dashboard-metric="activity"] strong')
 await page.waitForFunction(()=>document.querySelector('[data-dashboard-metric="activity"] strong')?.textContent?.trim()==='1',null,{timeout:15000})
 assert.equal((await jobCount.textContent()).trim(),'1',
  'Only actual running jobs count as active work')
 const usage=page.locator('[data-home-usage]')
 await usage.waitFor({timeout:15000})
 await page.waitForFunction(()=>document.querySelector('[data-home-usage]')?.textContent?.includes('12,345'),null,{timeout:15000})
 assert.match(await usage.textContent(),/호출 3회 · 토큰 12,345개/)
 assert.match(await usage.textContent(),/이 중 1회는 사용량을 추정했습니다/)
 const attention=page.locator('[data-home-attention]')
 await attention.waitFor({timeout:15000})
 assert.match(await attention.textContent(),/승인 또는 수동 검토를 기다리는 작업 2개/)
 await attention.getByRole('button',{name:'실행 기록 확인'}).click()
 await page.waitForURL(/#\/activity/)
 assert.deepEqual(errors,[],'Home/Activity navigation threw React errors')
 console.log('Sub3 Home private sessions + owner attention + activity navigation: PASS')
}finally{
 await browser?.close().catch(()=>{})
 server.kill('SIGTERM')
}
