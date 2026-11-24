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
 let localRunning=false
 let forceMissingEngine=false,mlxInstalled=false,llamaInstalled=false,mlxRepairs=0
 page.on('pageerror',error=>pageErrors.push(error.message))
 page.on('dialog',dialog=>void dialog.accept())
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
  else if(path==='/api/model-library/runtime')
   response=localRunning?{status:'ready',id:'fixture-local',name:'Fixture 4B',model:'fixture-local-model'}:
    {status:'idle',capabilities:forceMissingEngine?{gguf:{available:llamaInstalled,path:llamaInstalled?'/fixture/llama-server':null},memory:{free:1024},gpu:{free:0}}:undefined}
  else if(path==='/api/model-library/runtimes')
   response={items:[
    {id:'mlx-lm',name:'MLX (mlx-lm)',formats:['mlx'],installMethod:'fixture',installed:mlxInstalled,installable:true},
    {id:'llama-cpp',name:'llama.cpp',formats:['gguf'],installMethod:'fixture',installed:llamaInstalled,installable:true}
   ]}
  else if(path==='/api/model-library/runtime/install'){
   const payload=route.request().postDataJSON()
   const engine=payload?.engine
   if(engine==='mlx-lm'&&payload?.repair===true)mlxRepairs++
   if(engine==='mlx-lm')mlxInstalled=true
   if(engine==='llama-cpp')llamaInstalled=true
   response={ok:true}
  }
  else if(path==='/api/model-library/runtime/load'){
   localRunning=true
   response={status:'ready',id:'fixture-local',name:'Fixture 4B',model:'fixture-local-model'}
  }
  else if(path==='/api/model-library/runtime/unload'){
   localRunning=false
   response={status:'idle'}
  }
  else if(path==='/api/model-library/installed')
   response=[{id:'fixture-local',repo:'fixture/local-4b',label:'Fixture 4B',format:'gguf',bytes:4294967296}]
  else if(path==='/api/providers')
   response=localRunning?[{id:'dominion-native',name:'Local model',model:'fixture-local-model',enabled:true}]:[]
  else if(path==='/api/activity/teams')response={teams:[],leadJobs:[],remoteWorkers:[],remoteConfigured:false}
  await route.fulfill({status:200,contentType:'application/json',body:JSON.stringify(response)})
 })
 await page.goto(origin+'/#/chat',{waitUntil:'domcontentloaded'})
 await page.getByRole('heading',{name:'새 대화를 시작하세요'}).waitFor({state:'visible',timeout:12000})
 assert.equal(await page.getByText('예시 명령').count(),0,'empty chat should not show redundant prompt cards')
 const tokenBadge=page.locator('.chat-usage-details > summary')
 await tokenBadge.waitFor({state:'visible',timeout:5000})
 assert.match((await tokenBadge.textContent())||'',/누적 0 토큰/)
 await tokenBadge.click()
 await page.getByRole('note').getByText('새 대화 · 아직 사용한 토큰이 없습니다.').waitFor()
 await tokenBadge.click()
 const railAtStart=page.locator('.page-chat .conversation-rail')
 await railAtStart.waitFor({state:'visible',timeout:5000})
 const history=page.locator('#conversation-sidebar')
 assert.equal(await history.isVisible(),false,'fresh chat starts with compact navigation')
 await railAtStart.getByRole('button',{name:'대화 목록 펼치기'}).click()
 await history.waitFor({state:'visible',timeout:5000})
 assert.equal(await page.locator('.page-chat .chat-header').count(),0,'Chat must not restore the old bulky header')
 // Both views share one sidebar preference and the same switch location.
 await history.getByRole('button',{name:'그룹',exact:true}).click()
 const roomHistory=page.locator('#room-sidebar')
 await roomHistory.waitFor({state:'visible',timeout:12000})
 await roomHistory.getByRole('button',{name:'채팅',exact:true}).click()
 await history.waitFor({state:'visible',timeout:12000})
 assert.equal(await history.isVisible(),true,'switching back must retain expanded history')
 assert.equal(await page.evaluate(()=>localStorage.getItem('dominion.conversation.sidebar')),'1')
 const chatTop=page.locator('.page-chat .conversation-topbar')
 assert.equal(await chatTop.locator('.conversation-switch').count(),0,'no duplicate conversation navigation')
 assert.equal(await chatTop.locator('.chat-usage-details').isVisible(),true,'keep cumulative tokens')
 assert.ok((await chatTop.boundingBox()).height<=32,'compact utility area')
 await history.getByRole('button',{name:'그룹',exact:true}).click()
 await roomHistory.waitFor({state:'visible'})
 assert.equal(await page.locator('.page-rooms .conversation-topbar .conversation-switch').count(),0)
 await roomHistory.getByRole('button',{name:'채팅',exact:true}).click()
 await history.waitFor({state:'visible'})
 await page.getByRole('button',{name:'대화 상대 선택'}).click()
 const picker=page.getByRole('dialog',{name:'대화 상대 선택'})
 await picker.waitFor({state:'visible',timeout:5000})
 await picker.getByRole('tab',{name:'로컬 모델'}).click()
 await picker.getByText('fixture/local-4b').waitFor({timeout:5000})
 await picker.getByRole('button',{name:/fixture\/local-4b/}).click()
 await picker.waitFor({state:'hidden',timeout:5000})
 assert.equal(localRunning,true,'native model load must run from AI picker')
 await page.getByRole('button',{name:'대화 상대 선택'}).click()
 await picker.getByRole('tab',{name:'로컬 모델'}).click()
 await picker.getByRole('button',{name:'로컬 모델 종료'}).click()
 assert.equal(localRunning,false,'native model unload must run in AI picker')
 // The local picker should open the EXISTING installation panel directly.
 await picker.getByRole('button',{name:'실행기 설치 · 로컬 모델 관리'}).click()
 await page.waitForURL('**/#/models?tab=local&view=import',{timeout:8000})
 await page.getByText('MLX (mlx-lm)',{exact:true}).waitFor({state:'visible',timeout:8000})
 const mlxCard=page.getByText('MLX (mlx-lm)',{exact:true})
  .locator('xpath=ancestor::div[contains(@class,"rounded-xl")][1]')
 await mlxCard.getByRole('button',{name:'설치',exact:true}).click()
 assert.equal(mlxInstalled,true,'MLX install action must use runtime install API')
 await mlxCard.getByRole('button',{name:'MLX 실행기 재설치'}).click()
 assert.equal(mlxRepairs,1,'installed MLX must allow force-repair without LM Studio')
 assert.equal(await page.getByText('외부 로컬 서버 연결 (고급) · Ollama / LM Studio 등').count(),1)
 await page.getByRole('button',{name:'닫기'}).last().click()
 // Missing llama.cpp must preserve selected model and guide straight to setup.
 forceMissingEngine=true
 await page.getByRole('button',{name:'대화 상대 선택'}).click()
 await picker.getByRole('tab',{name:'로컬 모델'}).click()
 await picker.getByRole('button',{name:/fixture\/local-4b/}).click()
 await page.waitForURL('**/#/models?tab=local&view=import',{timeout:8000})
 await page.getByText(/실행 이어가기 · Fixture 4B/).waitFor({state:'visible',timeout:8000})
 assert.equal(JSON.parse(await page.evaluate(()=>sessionStorage.getItem('dominion-model-engine-retry'))).id,'fixture-local')
 await page.getByRole('button',{name:'필요한 실행기 설치'}).click()
 assert.equal(llamaInstalled,true,'missing llama.cpp installs through existing runtime API')
 await page.getByRole('button',{name:'닫기'}).last().click()
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
 assert.equal(await sidebar.getByText('더 많은 기능').count(),0,'no Sub4 extra-features grouping')
 for(const section of ['작업 공간','확장','운영']){
  assert.equal(await sidebar.getByText(section,{exact:true}).count(),0,
   'remove redundant settings section heading: '+section)
 }
 assert.equal(await sidebar.getByPlaceholder('설정 찾기').count(),0,'no copied settings search')
 for(const label of ['모델 관리','에이전트','연결 · 채널','메모리','실행 기록']){
  assert.equal(await sidebar.getByRole('button',{name:label,exact:true}).count(),1,'original Sub2 item: '+label)
 }
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
 // The drawer uses a 180 ms transform. Measure its settled position, not
 // a transitional animation frame returned immediately after the click.
 await page.waitForFunction(()=>{
  const rect=document.querySelector('#workspace-settings-sidebar')?.getBoundingClientRect()
  return !!rect && rect.left>=-1 && rect.right<=window.innerWidth+1
 },null,{timeout:6000})
 const rect=await sidebar.boundingBox()
 assert.ok(rect && rect.x>=-1 && rect.width>200 && rect.width<=390,
  'mobile drawer must fit viewport: '+JSON.stringify(rect))
 await page.locator('.workspace-responsive-scrim').click({position:{x:380,y:440}})
 assert.equal(await sidebar.getAttribute('data-collapsed'),'true','scrim dismisses mobile drawer')
 await page.getByRole('button',{name:'닫기'}).last().click()
 assert.equal(await rail.isVisible(),false,'collapsed icon rail must not occupy mobile viewport')
 // Mobile mode tabs keep the drawer open, but explicitly dismissing it
 // leaves the full transcript width available. Switching preserves drafts.
 const mobileChat=page.locator('.page-chat')
 const privateComposer=mobileChat.locator('textarea.conversation-composer-input')
 await privateComposer.fill('전환해도 보존할 일반대화 초안')
 await mobileChat.locator('.conversation-topbar').getByRole('button',{name:'대화 목록 펼치기'}).click()
 await history.waitFor({state:'visible'})
 await history.getByRole('button',{name:'그룹',exact:true}).click()
 await roomHistory.waitFor({state:'visible'})
 const drawer=await roomHistory.boundingBox()
 assert.ok(drawer&&drawer.x>=0&&drawer.width<=232,'group drawer matches mobile chat width')
 await roomHistory.getByRole('button',{name:'채팅',exact:true}).click()
 await history.waitFor({state:'visible'})
 await history.getByRole('button',{name:'대화 목록 접기'}).click()
 await history.waitFor({state:'hidden'})
 assert.equal(await privateComposer.inputValue(),'전환해도 보존할 일반대화 초안')
 await mobileChat.locator('.conversation-topbar').getByRole('button',{name:'대화 목록 펼치기'}).click()
 await history.getByRole('button',{name:'그룹',exact:true}).click()
 await roomHistory.waitFor({state:'visible'})
 await roomHistory.getByRole('button',{name:'그룹 목록 접기'}).click()
 await roomHistory.waitFor({state:'hidden'})
 await page.locator('.page-rooms .conversation-topbar .conversation-mobile-toggle').click()
 await roomHistory.waitFor({state:'visible'})
 await roomHistory.getByRole('button',{name:'채팅',exact:true}).click()
 await history.waitFor({state:'visible'})
 await history.getByRole('button',{name:'대화 목록 접기'}).click()
 await privateComposer.waitFor({state:'visible'})
 assert.equal(await privateComposer.inputValue(),'전환해도 보존할 일반대화 초안')
 assert.deepEqual(pageErrors,[], 'responsive React navigation runtime errors')
 // Exercise the actual message toolbar: copy beside the overflow menu,
 // with reasoning and tokens hidden until selected.
 const inspector=await browser.newPage({viewport:{width:1080,height:760},locale:'ko-KR',
  permissions:['clipboard-read','clipboard-write']})
 const inspectionErrors=[]
 inspector.on('pageerror',error=>inspectionErrors.push(error.message))
 await inspector.route(url=>new URL(url).pathname.startsWith('/api/'),async route=>{
  const path=new URL(route.request().url()).pathname
  if(path==='/api/events'){
   await route.fulfill({status:200,contentType:'text/event-stream',body:': fixture\n\n'})
   return
  }
  let result=[]
  if(path==='/api/health')result={branch:'dominion-sub2',commit:'message-actions'}
  else if(path==='/api/status')result={jobs:0,providers:1,agents:0}
  else if(path==='/api/sessions')result=[{id:'menu-demo',title:'Fixture conversation',
   providerId:'fixture',model:'test-model'}]
  else if(path==='/api/sessions/menu-demo/messages')result=[{
   id:'response-1',role:'assistant',content:'복사할 답변입니다.',
   reasoning:'추론 기록 예시',created_at:'2026-11-23T00:52:00Z',
   metadata:{executionId:'exec-menu-demo',usage:{totalTokens:80,promptTokens:60,completionTokens:20,cachedPromptTokens:10}}
  }]
  else if(path==='/api/sessions/menu-demo/usage')result={totalTokens:80}
  else if(path==='/api/executions/exec-menu-demo')result={
   id:'exec-menu-demo',status:'completed',contextPolicy:'private-context-engine-v1',
   modelCalls:1,toolCalls:0,tokens:80,
   providerRounds:[{round:1,promptTokens:60,completionTokens:20,cachedPromptTokens:10,
    usageExact:true,toolsAdvertised:0,nativeInputTokensEstimated:75}],
   inputContext:{systemTokens:28,historyTokens:15,toolSchemaTokens:0,
    reservedOutputTokens:1024,preAdapterWireProfile:{totalEstimatedTokens:75,
     components:{systemAndPersona:28,ownerAndHistory:45,toolSchemas:0},toolDefinitions:0}}
  }
  else if(path==='/api/providers')result=[{id:'fixture',name:'Fixture',enabled:true,
   kind:'openai',model:'test-model',baseUrl:'http://127.0.0.1/v1'}]
  else if(path==='/api/agents')result=[]
  else if(path==='/api/approvals')result=[]
  else if(path==='/api/activity/teams')result={teams:[],leadJobs:[],remoteWorkers:[],remoteConfigured:false}
  else if(path==='/api/model-library/runtime')result={status:'idle'}
  else if(path==='/api/settings/owner-profile')result={id:'owner',name:'나'}
  else if(path==='/api/policy'||path.startsWith('/api/settings/'))result={}
  await route.fulfill({status:200,contentType:'application/json',body:JSON.stringify(result)})
 })
 await inspector.goto(origin+'/#/chat?session=menu-demo',{waitUntil:'domcontentloaded'})
 const answer=inspector.locator('[data-message][data-role="assistant"]').first()
 await answer.waitFor({state:'visible',timeout:15000}).catch(async error=>{
  console.error('Message toolbar fixture diagnosis',JSON.stringify({url:inspector.url(),errors:inspectionErrors,
   text:(await inspector.locator('body').innerText()).slice(0,3000)}))
  throw error
 })
 assert.equal(await answer.getByText('추론 기록 예시').count(),0,
  'reasoning must not fill the transcript until requested')
 await answer.getByRole('button',{name:'답변 옵션'}).click()
 await answer.getByRole('menuitem',{name:'추론 정보 보기'}).click()
 await answer.getByText('추론 기록 예시').waitFor({state:'visible',timeout:5000})
 await answer.getByRole('button',{name:'답변 옵션'}).click()
 await answer.getByRole('menuitem',{name:'토큰 사용량 보기'}).click()
 const evidence=inspector.getByRole('dialog',{name:'실행·토큰 상세'})
 await evidence.waitFor({state:'visible',timeout:5000})
 await evidence.getByText('요청 구성',{exact:true}).waitFor({state:'visible',timeout:5000})
 await evidence.getByText('모델 토큰 정산',{exact:true}).waitFor({state:'visible',timeout:5000})
 await evidence.getByText('모델 호출별 사용량',{exact:true}).waitFor({state:'visible',timeout:5000})
 assert.match(await evidence.innerText(),/첫 입력[\s\S]*60/)
 assert.match(await evidence.innerText(),/입력 합계[\s\S]*60/)
 await evidence.getByRole('button',{name:'실행·토큰 상세 닫기'}).click()
 await answer.getByRole('button',{name:'답변 복사'}).click()
 await answer.getByRole('button',{name:'답변 복사됨'}).waitFor({state:'visible',timeout:5000})
 assert.equal(await inspector.evaluate(()=>navigator.clipboard.readText()),'복사할 답변입니다.')
 await inspector.locator('.chat-usage-details > summary').waitFor({state:'visible',timeout:5000})
 assert.deepEqual(inspectionErrors,[],'assistant message action errors')
 await inspector.close()
 console.log('PASS: responsive Chat and compact assistant copy/reasoning/token menu')
}finally{
 await browser?.close()
 server.kill('SIGTERM')
}
