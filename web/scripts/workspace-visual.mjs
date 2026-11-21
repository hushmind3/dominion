import assert from 'node:assert/strict'
import {spawn} from 'node:child_process'
import {mkdir,stat,writeFile} from 'node:fs/promises'
import path from 'node:path'
import {fileURLToPath} from 'node:url'
import {chromium} from 'playwright'

// Actual Chromium interactions against React/Vite DEV (no release build).
// API fixtures isolate UX from paid inference and private credentials.
const root=fileURLToPath(new URL('../../',import.meta.url))
const out=path.join(root,'web','artifacts','workspace-visual')
const port=Number(process.env.DOMINION_VISUAL_PORT||4383)
const base='http://127.0.0.1:'+port
const server=spawn(process.execPath,['node_modules/vite/bin/vite.js',
 '--host','127.0.0.1','--port',String(port),'--strictPort'],{
 cwd:path.join(root,'web'),stdio:['ignore','pipe','pipe']
})
let log=''
server.stdout.on('data',b=>{log+=b.toString()})
server.stderr.on('data',b=>{log+=b.toString()})
const pages=['home','models','agents','connections','memory','activity','settings']
const viewports=[{width:1440,height:900},{width:768,height:1024},
 {width:390,height:844},{width:320,height:740}]
const pictures=[],errors=[],visits=[]
const requests={sdk:[],team:[],remote:[]}
let sdkRuns=[],teams=[],remoteTasks=[]
const send=(route,payload,status=200)=>route.fulfill({
 status,contentType:'application/json',body:JSON.stringify(payload)
})
async function mock(route){
 const req=route.request(),url=new URL(req.url()),n=url.pathname,m=req.method()
 const body=()=>JSON.parse(req.postData()||'{}')
 if(n==='/api/events')return route.fulfill({
  status:200,headers:{'content-type':'text/event-stream'},body:': visual-test\n\n'
 })
 if(n==='/api/health')return send(route,{branch:'dominion-sub3',commit:'visual-test'})
 if(n==='/api/status')return send(route,{jobs:0,agents:0,providers:0,executions:0})
 if(n==='/api/approvals')return send(route,[])
 if(n==='/api/agents'||n==='/api/providers'||n==='/api/providers/cli')return send(route,[])
 if(n==='/api/providers/catalog')return send(route,{providers:[]})
 if(n==='/api/providers/free-catalog')return send(route,{providers:[],counts:{free:0,paid:0,unknown:0}})
 if(n==='/api/personas/import/preview')return send(route,{profiles:[]})
 if(n==='/api/model-library/runtime')return send(route,{status:'idle'})
 if(n==='/api/model-library/installed')return send(route,[])
 if(n==='/api/capabilities')return send(route,{items:[]})
 if(n==='/api/browser/status')return send(route,{status:{available:false,running:false},screenshot:null})
 if(n==='/api/policy')return send(route,{id:'default',autoApprove:[],autoApproveMcp:false,autoApproveAll:false})
 if(n==='/api/skill-catalog')return send(route,{items:[],total:0})
 if(n==='/api/task-teams/bridges')return send(route,{bridges:{codex:{enabled:true,binaryAvailable:true},
  claude:{enabled:true,binaryAvailable:true},qwen:{enabled:true,binaryAvailable:true}}})
 if(n==='/api/task-teams'&&m==='GET')return send(route,teams)
 if(n==='/api/task-teams'&&m==='POST'){
  const data=body();requests.team.push(data)
  const item={id:'ux-team-'+requests.team.length,status:'queued',goal:data.goal,
   createdAt:new Date().toISOString(),teamState:{phase:'queued'},
   tasks:data.tasks.map(t=>({taskId:t.id,mode:t.mode,runtime:t.runtime,
    isolation:t.isolation,dependsOn:t.dependsOn,status:'queued'}))}
  teams=[item,...teams];return send(route,item,202)
 }
 if(n==='/api/remote/workers')return send(route,[{
  workerId:'test-machine',model:'local-27b',online:true,
  capabilities:['text_inference'],lastSeenAt:new Date().toISOString()
 }])
 if(n==='/api/remote/tasks'&&m==='GET')return send(route,remoteTasks)
 if(n==='/api/remote/tasks'&&m==='POST'){
  const data=body();requests.remote.push(data)
  const item={id:'remote:ux-'+requests.remote.length,status:'queued',
   queuedAt:new Date().toISOString(),evidenceGrade:'remote_unverified'}
  remoteTasks.push(item);return send(route,{id:item.id,status:item.status},202)
 }
 if(n.startsWith('/api/remote/tasks/')&&n.endsWith('/cancel')){
  const id=decodeURIComponent(n.slice('/api/remote/tasks/'.length,-'/cancel'.length))
  remoteTasks=remoteTasks.map(t=>t.id===id?{...t,status:'cancelled'}:t)
  return send(route,{id,status:'cancelled'})
 }
 if(n==='/api/sdk/runs'&&m==='GET')return send(route,sdkRuns)
 if(n==='/api/sdk/runs'&&m==='POST'){
  const data=body();requests.sdk.push(data)
  const item={id:'sdk-ux-'+requests.sdk.length,status:'queued',
   instruction:data.instruction,algorithmId:data.algorithmId,
   checkpoint:{rounds:0,toolCalls:0}}
  sdkRuns=[item,...sdkRuns];return send(route,item,202)
 }
 if(n.startsWith('/api/sdk/runs/')&&n.endsWith('/deny')&&m==='POST'){
  const id=n.split('/')[4],callId=body().callId
  const current=sdkRuns.find(item=>item.id===id)
  assert.equal(callId,current?.checkpoint?.pendingCall?.id,'Refusal must be bound to the real pending tool')
  sdkRuns=sdkRuns.map(item=>item.id===id?{...item,status:'running',
   checkpoint:{...item.checkpoint,pendingCall:null}}:item)
  return send(route,sdkRuns.find(item=>item.id===id))
 }
 if(n.startsWith('/api/sdk/runs/')&&m==='GET'){
  const id=n.slice('/api/sdk/runs/'.length)
  return send(route,sdkRuns.find(r=>r.id===id)||{})
 }
 if(n==='/api/memories'||n==='/api/rooms'||n==='/api/sessions'||
  n==='/api/jobs'||n==='/api/audit'||n==='/api/executions'||
  n==='/api/tools'||n==='/api/channels/catalog'||n==='/api/channels'||
  n==='/api/oauth/providers'||n==='/api/oauth/connections'||
  n==='/api/mcp'||n==='/api/mcp/catalog'||n==='/api/mcp/presets'||
  n==='/api/mcp/discover'||n==='/api/skills')return send(route,[])
 return send(route,[])
}
const sleep=ms=>new Promise(ok=>setTimeout(ok,ms))
async function waitServer(){
 for(let i=0;i<90;i++){
  if(server.exitCode!==null)throw Error('Vite exited: '+log)
  try{if((await fetch(base)).ok)return}catch{}
  await sleep(250)
 }
 throw Error('Vite did not start: '+log)
}
async function screenshot(page,name){
 const file=path.join(out,name)
 await page.screenshot({path:file,animations:'disabled'})
 const size=(await stat(file)).size
 assert.ok(size>3500,'Screenshot was too small: '+name)
 pictures.push({file:name,bytes:size})
}
async function overflow(page,route,width){
 const result=await page.evaluate(()=>{
  const content=document.querySelector('.workspace-settings-content')
  return {viewport:innerWidth,document:document.documentElement.scrollWidth,
   pane:content?.clientWidth||0,paneScroll:content?.scrollWidth||0}
 })
 assert.ok(result.document<=result.viewport+2,
  'Document overflows at '+route+' '+width+': '+JSON.stringify(result))
 if(result.pane)assert.ok(result.paneScroll<=result.pane+3,
  'Settings pane overflows at '+route+' '+width+': '+JSON.stringify(result))
}
async function waitScreen(page){
 // Sidebar appears before React.lazy resolves. A navigation-only screenshot
 // is not UX evidence: wait for the actual rendered page controls.
 await page.waitForFunction(()=>{
  const pane=document.querySelector('.workspace-settings-content')
  return Boolean(pane && !pane.textContent?.includes('화면 불러오는 중') &&
   pane.querySelectorAll('button,input,select,textarea').length>=2)
 },null,{timeout:20000})
 await page.waitForTimeout(160)
}
async function open(page,name){
 await page.goto(base+'/#/'+name,{waitUntil:'domcontentloaded'})
 const nav=page.locator('.workspace-settings>aside nav [aria-current="page"]')
 await nav.waitFor({timeout:15000})
 await waitScreen(page)
 await overflow(page,name,page.viewportSize().width)
 visits.push({name,width:page.viewportSize().width})
}
let browser
try{
 await mkdir(out,{recursive:true})
 await waitServer()
 browser=await chromium.launch({headless:true,args:['--no-sandbox']})
 const page=await browser.newPage({viewport:viewports[0],locale:'ko-KR',deviceScaleFactor:1})
 page.on('pageerror',e=>errors.push(e.message))
 await page.addInitScript(()=>localStorage.setItem('dominion.locale','ko'))
 await page.route('**/api/**',mock)
 await open(page,'home')
 await screenshot(page,'1440-home.png')
 // Click the genuine sidebar buttons, not only hash routes.
 for(let i=1;i<pages.length;i++){
  const buttons=page.locator('.workspace-settings>aside nav button')
  assert.equal(await buttons.count(),7,'The existing seven settings menu items stay intact')
  await buttons.nth(i).click()
  await page.waitForFunction(name=>location.hash==='#/'+name,pages[i])
  await waitScreen(page)
  await overflow(page,pages[i],1440)
  visits.push({name:pages[i],width:1440,interaction:'sidebar'})
  await screenshot(page,'1440-'+pages[i]+'.png')
 }
 await page.getByRole('button',{name:'고급 설정 보기'}).click()
 await page.getByRole('button',{name:'고급 설정 접기'}).waitFor()
 await screenshot(page,'1440-settings-expanded.png')
 await page.keyboard.press('Tab')
 assert.ok(await page.evaluate(()=>Boolean(document.activeElement?.closest('.workspace-settings'))),
  'Keyboard focus must stay inside settings dialog')
 // Native agent strategy and budget are part of the actual POST.
 await open(page,'home')
 await page.locator('#sdk-goal').fill('Inspect the project boundary')
 await page.locator('summary').filter({hasText:'고급 설정: 전략'}).click()
 await page.locator('#sdk-strategy').selectOption('focused-v1')
 await page.locator('#sdk-rounds').selectOption('12')
 await page.getByRole('button',{name:'작업 시작'}).click()
 assert.equal(requests.sdk.length,1)
 assert.equal(requests.sdk[0].algorithmId,'focused-v1')
 assert.equal(requests.sdk[0].maxRounds,12)
 await screenshot(page,'1440-agent-run.png')
 sdkRuns[0]={...sdkRuns[0],status:'awaiting_approval',checkpoint:{
  ...sdkRuns[0].checkpoint,pendingCall:{id:'tool-pending-1',
   name:'workspace_write',arguments:{path:'src/mod.rs'}}
 }}
 await page.getByRole('button',{name:'이 도구 실행 거부'}).waitFor({timeout:8000})
 await screenshot(page,'1440-tool-approval-review.png')
 await page.getByRole('button',{name:'이 도구 실행 거부'}).click()
 assert.equal(sdkRuns[0].checkpoint.pendingCall,null,
  'Owner rejection must settle the exact tool and clear the approval UI')
 // Team composer and remote worker use backend contracts, no fake completion.
 await open(page,'activity')
 const team=page.locator('[aria-label="작업 팀"]')
 await team.getByRole('button',{name:'팀 만들기'}).click()
 await team.getByLabel('팀 목표').fill('Review provider routing')
 await team.getByRole('button',{name:'팀 실행'}).click()
 assert.equal(requests.team[0].tasks[0].prompt,'Review provider routing')
 await screenshot(page,'1440-team-run.png')
 await page.getByRole('button',{name:'장치 · 작업 보기'}).click()
 await page.locator('#remote-task-prompt').fill('Summarize local device inference')
 await page.getByRole('button',{name:'원격 작업 요청'}).click()
 assert.equal(requests.remote.length,1)
 await screenshot(page,'1440-remote-run.png')
 // All settings pages in desktop, tablet, mobile and narrow mobile.
 for(const viewport of viewports.slice(1)){
  await page.setViewportSize(viewport)
  for(const route of pages){
   await open(page,route)
   await screenshot(page,String(viewport.width)+'-'+route+'.png')
  }
  const nav=page.locator('.workspace-settings>aside nav')
  const selected=nav.locator('[aria-current="page"]')
  const a=await nav.boundingBox(),b=await selected.boundingBox()
  assert.ok(a&&b&&b.x>=a.x-4&&b.x+b.width<=a.x+a.width+4,
   'Selected navigation must be visible at '+viewport.width)
 }
 // Main chat and Room pages are not settings overlays.
 for(const route of ['chat','rooms']){
  await page.goto(base+'/#/'+route,{waitUntil:'domcontentloaded'})
  await page.waitForTimeout(500)
  if(route==='chat'){
   assert.equal(await page.locator('.page-chat aside').first().isVisible(),false,
    'Desktop history drawer must collapse after resizing to mobile')
  }
  await overflow(page,route,320)
  await screenshot(page,'320-'+route+'.png')
  visits.push({name:route,width:320})
 }
 await page.keyboard.press('Control+,')
 await page.locator('.workspace-settings').waitFor({timeout:10000})
 await page.keyboard.press('Escape')
 await page.locator('.workspace-settings').waitFor({state:'detached',timeout:10000})
 await page.keyboard.press('Control+k')
 await page.locator('#dominion-menu-search').waitFor({timeout:10000})
 assert.equal(await page.locator('#dominion-menu-results [role="option"]').count(),9,
  'Palette must expose every existing destination without a new menu')
 await screenshot(page,'320-menu-search.png')
 await page.locator('#dominion-menu-search').fill('memory')
 await page.keyboard.press('Enter')
 await page.waitForFunction(()=>location.hash==='#/memory')
 await page.locator('.workspace-settings').waitFor({timeout:10000})
 await page.keyboard.press('Escape')
 await page.locator('.workspace-settings').waitFor({state:'detached',timeout:10000})
 assert.deepEqual(errors,[],'No uncaught React errors across the workspace')
 await writeFile(path.join(out,'summary.json'),JSON.stringify({
  ok:true,visits,screenshots:pictures,requests:{
   sdk:requests.sdk.length,team:requests.team.length,remote:requests.remote.length
  }
 },null,2))
 console.log(JSON.stringify({ok:true,visits:visits.length,
  screenshots:pictures.length,errors:errors.length}))
}finally{
 await browser?.close()
 server.kill('SIGTERM')
}
