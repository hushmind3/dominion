import assert from 'node:assert/strict'
import {spawn} from 'node:child_process'
import {mkdir,stat} from 'node:fs/promises'
import path from 'node:path'
import {fileURLToPath} from 'node:url'
import {chromium} from 'playwright'

// Real React/Vite and Chromium with isolated HTTP fixtures, no paid model.
const root=fileURLToPath(new URL('../../',import.meta.url))
const output=path.join(root,'web/artifacts/chat-visual')
const port=Number(process.env.DOMINION_CHAT_VISUAL_PORT||4387)
const base='http://127.0.0.1:'+port
const server=spawn(process.execPath,['node_modules/vite/bin/vite.js',
 '--host','127.0.0.1','--port',String(port),'--strictPort'],{
 cwd:path.join(root,'web'),stdio:['ignore','pipe','pipe']
})
let logs=''
server.stdout.on('data',d=>{logs+=d.toString()})
server.stderr.on('data',d=>{logs+=d.toString()})
const original=[
 {id:'owner-1',role:'user',session_id:'chat-ui',content:'표와 코드를 읽기 쉽게 보여주세요.'},
 {id:'model-1',role:'assistant',session_id:'chat-ui',content:
  '## 서식 있는 답변\n**굵은 글씨**와 `inline code`\n\n- 첫 항목\n- 두 번째 항목\n\n| 이름 | 값 |\n| --- | --- |\n| 테스트 | 1 |\n\n```ts\nconst answer = 42\n```\n\n[참고](https://example.com) <img src=x onerror=alert(1)>'}
]
const session={id:'chat-ui',title:'기본 채팅 UI 점검',mode:'chat',agentId:null,providerId:'mock-chat',model:'mock-native'}
const provider={id:'mock-chat',name:'Mock Chat',kind:'openai',enabled:true,model:'mock-native',baseUrl:'http://127.0.0.1:9/v1'}
const agent={id:'default',name:'기본값',providerId:'mock-chat',model:'mock-native'}
let messages=structuredClone(original)
const requests=[]
let releaseReply=null
const json=(route,body,status=200)=>route.fulfill({status,contentType:'application/json',body:JSON.stringify(body)})
async function mock(route){
 const req=route.request(),n=new URL(req.url()).pathname,m=req.method()
 if(n==='/api/events')return route.fulfill({status:200,contentType:'text/event-stream',body:':ok\n\n'})
 if(n==='/api/providers')return json(route,[provider])
 if(n==='/api/agents')return json(route,[agent])
 if(n==='/api/approvals')return json(route,[])
 if(n==='/api/sessions'&&m==='GET')return json(route,[session])
 if(n==='/api/sessions/chat-ui/messages')return json(route,messages)
 if(n==='/api/sessions/chat-ui/usage')return json(route,{totalTokens:42,calls:2,promptTokens:30,completionTokens:12})
 if(n==='/api/status')return json(route,{agents:1,providers:1,executions:0,jobs:0})
 if(n==='/api/model-library/runtime')return json(route,{status:'idle'})
 if(n==='/api/model-library/installed')return json(route,[])
 if(n==='/api/chat'&&m==='POST'){
  const body=JSON.parse(req.postData()||'{}')
  requests.push(body)
  assert.equal(body.sessionId,'chat-ui')
  assert.equal(body.mode,'chat')
  assert.equal(body.agentId,'')
  assert.equal(body.toolsEnabled,false)
  assert.equal(body.toolsExecutionEnabled,false)
  assert.equal(body.autoLearning,false)
  assert.equal(body.input,'후속 질문도 같은 채팅으로 보내주세요.')
  // Hold the network response to prove the owner bubble is painted
  // immediately, before the server has returned any conversation delta.
  await new Promise(resolve=>{releaseReply=resolve})
  messages.push({id:'owner-'+(requests.length+1),role:'user',session_id:'chat-ui',content:body.input})
  const reply='### 후속 답변\n**같은 대화**를 이어갑니다.'
  messages.push({id:'model-'+(requests.length+1),role:'assistant',session_id:'chat-ui',content:reply})
  const events=[
   {type:'text',text:'### 후속 답변\n'},
   {type:'text',text:'**같은 대화**를 이어갑니다.'},
   {type:'assistant_saved',messageId:'model-'+(requests.length+1),sessionId:'chat-ui'},
   {type:'done',sessionId:'chat-ui'}
  ]
  return route.fulfill({status:200,contentType:'text/event-stream',body:events.map(x=>'data: '+JSON.stringify(x)+'\n\n').join('')})
 }
 return json(route,[])
}
async function ready(){
 for(let n=0;n<90;n++){
  if(server.exitCode!==null)throw Error('Vite exited: '+logs)
  try{if((await fetch(base)).ok)return}catch{}
  await new Promise(resolve=>setTimeout(resolve,200))
 }
 throw Error('Vite failed: '+logs)
}
let browser
try{
 await mkdir(output,{recursive:true})
 await ready()
 browser=await chromium.launch({headless:true,args:['--no-sandbox']})
 for(const width of [1440,390,320]){
  messages=structuredClone(original)
  const page=await browser.newPage({viewport:{width,height:width===1440?900:760},locale:'ko-KR'})
  const pageErrors=[]
  page.on('pageerror',e=>pageErrors.push(e.message))
  await page.addInitScript(()=>localStorage.setItem('dominion.locale','ko'))
  await page.route('**/api/**',mock)
  await page.goto(base+'/#/chat?session=chat-ui',{waitUntil:'domcontentloaded'})
  await page.locator('[data-role="assistant"] .chat-richtext h2').waitFor()
  assert.equal(await page.locator('[data-role="assistant"] .chat-richtext strong').first().textContent(),'굵은 글씨')
  assert.equal(await page.locator('[data-role="assistant"] .chat-richtext table tbody tr td').last().textContent(),'1')
  assert.match(await page.locator('[data-role="assistant"] .chat-code').textContent(),/const answer = 42/)
  assert.equal(await page.locator('[data-role="assistant"] img').count(),0,'Model HTML must stay literal text')
  const owner=await page.locator('[data-role="user"] .chat-message-inner').first().boundingBox()
  const assistant=await page.locator('[data-role="assistant"] .chat-message-inner').first().boundingBox()
  const transcript=await page.locator('.chat-transcript').boundingBox()
  const alignment=await page.locator('[data-role="user"]').first().evaluate(el=>getComputedStyle(el).justifyContent)
  assert.equal(alignment,'flex-end','Owner row must be right-aligned at '+width)
  assert.ok(owner&&assistant&&transcript,'Chat message geometry missing at '+width)
  // Long user prompts may start LEFT of the assistant text on narrow phones:
  // the bubble's RIGHT edge, not its left edge, proves correct alignment.
  const rightEdge=owner.x+owner.width
  assert.ok(Math.abs(rightEdge-(transcript.x+transcript.width))<55,
   'Owner bubble right edge is detached from the conversation column at '+width)
  const font=await page.locator('[data-role="assistant"] .chat-message-text').first().evaluate(el=>Number.parseFloat(getComputedStyle(el).fontSize))
  assert.ok(font>=15,'Assistant font is too small at '+width+': '+font)
  const overflow=await page.evaluate(()=>document.documentElement.scrollWidth-innerWidth)
  assert.ok(overflow<=2,'Chat overflowed horizontally at '+width+': '+overflow)
  const file=path.join(output,width+'-chat.png')
  await page.screenshot({path:file,animations:'disabled'})
  assert.ok((await stat(file)).size>3000)
  assert.equal(await page.getByRole('combobox',{name:'대화 실행 모드'}).inputValue(),'chat')
  assert.equal(await page.locator('.chat-agent').count(),0)
  await page.locator('.conversation-composer-input').fill('후속 질문도 같은 채팅으로 보내주세요.')
  await page.locator('.conversation-composer-input').press('Enter')
  const pending=page.locator('[data-role="user"][data-pending="true"]')
  await pending.waitFor({timeout:5000})
  assert.equal(await pending.locator('.chat-message-text').textContent(),'후속 질문도 같은 채팅으로 보내주세요.')
  assert.equal(await page.getByText('후속 답변',{exact:true}).count(),0,'Provider response must remain gated')
  await page.waitForFunction(()=>false,null,{timeout:100}).catch(()=>{})
  assert.equal(await pending.count(),1,'Pending owner bubble should not vanish before transcript reload')
  assert.ok(releaseReply,'Mock model endpoint has not received the request')
  releaseReply();releaseReply=null
  await page.getByText('후속 답변',{exact:true}).waitFor({timeout:15000})
  await page.waitForFunction(()=>document.querySelectorAll('[data-role="user"][data-pending="true"]').length===0,null,{timeout:15000})
  assert.equal(await page.locator('[data-role="user"]').count(),2,'Pending owner message must be replaced without duplication')
  assert.ok(await page.locator('[data-role="user"]').count()>=2)
  assert.ok(await page.locator('[data-role="assistant"] .chat-richtext h3').count()>=1)
  assert.deepEqual(pageErrors,[],'React failed at '+width)
  await page.close()
 }
 assert.equal(requests.length,3)
 console.log('Sub3 chat desktop/mobile + Markdown + follow-up fixtures: PASS')
}finally{
 await browser?.close().catch(()=>{})
 server.kill('SIGTERM')
}
